use crate::recipes::search_recipe::retrieve_recipe_file;
use goose::agents::extension::ExtensionConfig;
use goose::recipe::build_recipe::build_recipe_from_template;
use goose::recipe::Recipe;
use std::collections::HashSet;

/// Represents a secret requirement discovered from a recipe extension
#[derive(Debug, Clone, PartialEq)]
pub struct SecretRequirement {
    /// The environment variable name (e.g., "GITHUB_TOKEN")
    pub key: String,
    /// The name of the extension that requires this secret
    pub extension_name: String,
}

impl SecretRequirement {
    pub fn new(extension_name: String, key: String) -> Self {
        Self {
            key,
            extension_name,
        }
    }

    /// Returns a human-readable description of what this secret is for
    pub fn description(&self) -> String {
        format!("Required by {} extension", self.extension_name)
    }
}

/// Discovers all secrets required by MCP extensions in a recipe and its sub-recipes
///
/// This function recursively scans the recipe and all its sub-recipes for extensions
/// and collects their declared env_keys, creating SecretRequirement structs for each
/// unique environment variable.
///
/// # Arguments
/// * `recipe` - The recipe to analyze for secret requirements
///
/// # Returns
/// A vector of SecretRequirement objects, deduplicated by key name
pub fn discover_recipe_secrets(recipe: &Recipe) -> Vec<SecretRequirement> {
    let mut visited_recipes = HashSet::new();
    discover_recipe_secrets_recursive(recipe, &mut visited_recipes)
}

/// Internal recursive function (depth-first search) to discover secrets nested in sub-recipes
/// This is future-proofing for a time when we have more than one-level of sub-recipe nesting
fn discover_recipe_secrets_recursive(
    recipe: &Recipe,
    visited_recipes: &mut HashSet<String>,
) -> Vec<SecretRequirement> {
    let mut secrets: Vec<SecretRequirement> = Vec::new();
    let mut seen_keys = HashSet::new();

    if let Some(extensions) = &recipe.extensions {
        for ext in extensions {
            let (extension_name, env_keys) = match ext {
                ExtensionConfig::Sse { name, env_keys, .. } => (name, env_keys),
                ExtensionConfig::Stdio { name, env_keys, .. } => (name, env_keys),
                ExtensionConfig::StreamableHttp { name, env_keys, .. } => (name, env_keys),
                ExtensionConfig::Builtin { name, .. } => (name, &Vec::new()),
                ExtensionConfig::Frontend { name, .. } => (name, &Vec::new()),
            };

            for key in env_keys {
                if seen_keys.insert(key.clone()) {
                    let secret_req = SecretRequirement::new(extension_name.clone(), key.clone());
                    secrets.push(secret_req);
                }
            }
        }
    }

    if let Some(sub_recipes) = &recipe.sub_recipes {
        for sub_recipe in sub_recipes {
            if visited_recipes.contains(&sub_recipe.path) {
                continue;
            }
            visited_recipes.insert(sub_recipe.path.clone());

            match load_sub_recipe(&sub_recipe.path) {
                Ok(loaded_recipe) => {
                    let sub_secrets =
                        discover_recipe_secrets_recursive(&loaded_recipe, visited_recipes);
                    for sub_secret in sub_secrets {
                        if seen_keys.insert(sub_secret.key.clone()) {
                            secrets.push(sub_secret);
                        }
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }
    }

    secrets
}

/// Loads a recipe from a file path for sub-recipe secret discovery
fn load_sub_recipe(recipe_path: &str) -> Result<Recipe, Box<dyn std::error::Error>> {
    let recipe_file = retrieve_recipe_file(recipe_path)?;
    let recipe = build_recipe_from_template(
        recipe_file,
        vec![],
        None::<fn(&str, &str) -> anyhow::Result<String>>,
    )?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose::agents::extension::{Envs, ExtensionConfig};
    use goose::recipe::Recipe;
    use std::collections::HashMap;

    fn create_test_recipe_with_extensions() -> Recipe {
        Recipe {
            version: "1.0.0".to_string(),
            title: "Test Recipe".to_string(),
            description: "A test recipe with MCP extensions".to_string(),
            instructions: Some("Test instructions".to_string()),
            prompt: None,
            extensions: Some(vec![
                ExtensionConfig::Sse {
                    name: "github-mcp".to_string(),
                    uri: "sse://example.com".to_string(),
                    envs: Envs::new(HashMap::new()),
                    env_keys: vec!["GITHUB_TOKEN".to_string(), "GITHUB_API_URL".to_string()],
                    description: None,
                    timeout: None,
                    bundled: None,
                },
                ExtensionConfig::Stdio {
                    name: "slack-mcp".to_string(),
                    cmd: "slack-mcp".to_string(),
                    args: vec![],
                    envs: Envs::new(HashMap::new()),
                    env_keys: vec!["SLACK_TOKEN".to_string()],
                    timeout: None,
                    description: None,
                    bundled: None,
                },
                ExtensionConfig::Builtin {
                    name: "builtin-ext".to_string(),
                    display_name: None,
                    description: None,
                    timeout: None,
                    bundled: None,
                },
            ]),
            context: None,
            settings: None,
            activities: None,
            author: None,
            parameters: None,
            response: None,
            sub_recipes: None,
            retry: None,
        }
    }

    #[test]
    fn test_discover_recipe_secrets() {
        let recipe = create_test_recipe_with_extensions();
        let secrets = discover_recipe_secrets(&recipe);

        assert_eq!(secrets.len(), 3);

        let github_token = secrets.iter().find(|s| s.key == "GITHUB_TOKEN").unwrap();
        assert_eq!(github_token.key, "GITHUB_TOKEN");
        assert_eq!(github_token.extension_name, "github-mcp");
        assert_eq!(
            github_token.description(),
            "Required by github-mcp extension"
        );

        let github_api = secrets.iter().find(|s| s.key == "GITHUB_API_URL").unwrap();
        assert_eq!(github_api.key, "GITHUB_API_URL");
        assert_eq!(github_api.extension_name, "github-mcp");

        let slack_token = secrets.iter().find(|s| s.key == "SLACK_TOKEN").unwrap();
        assert_eq!(slack_token.key, "SLACK_TOKEN");
        assert_eq!(slack_token.extension_name, "slack-mcp");
    }

    #[test]
    fn test_discover_recipe_secrets_empty_recipe() {
        let recipe = Recipe {
            version: "1.0.0".to_string(),
            title: "Empty Recipe".to_string(),
            description: "A recipe with no extensions".to_string(),
            instructions: Some("Test instructions".to_string()),
            prompt: None,
            extensions: None,
            context: None,
            settings: None,
            activities: None,
            author: None,
            parameters: None,
            response: None,
            sub_recipes: None,
            retry: None,
        };

        let secrets = discover_recipe_secrets(&recipe);
        assert_eq!(secrets.len(), 0);
    }

    #[test]
    fn test_discover_recipe_secrets_deduplication() {
        let recipe = Recipe {
            version: "1.0.0".to_string(),
            title: "Test Recipe".to_string(),
            description: "A test recipe with duplicate secrets".to_string(),
            instructions: Some("Test instructions".to_string()),
            prompt: None,
            extensions: Some(vec![
                ExtensionConfig::Sse {
                    name: "service-a".to_string(),
                    uri: "sse://example.com".to_string(),
                    envs: Envs::new(HashMap::new()),
                    env_keys: vec!["API_KEY".to_string()],
                    description: None,
                    timeout: None,
                    bundled: None,
                },
                ExtensionConfig::Stdio {
                    name: "service-b".to_string(),
                    cmd: "service-b".to_string(),
                    args: vec![],
                    envs: Envs::new(HashMap::new()),
                    env_keys: vec!["API_KEY".to_string()], // Same original key, different extension
                    timeout: None,
                    description: None,
                    bundled: None,
                },
            ]),
            context: None,
            settings: None,
            activities: None,
            author: None,
            parameters: None,
            response: None,
            sub_recipes: None,
            retry: None,
        };

        let secrets = discover_recipe_secrets(&recipe);
        assert_eq!(secrets.len(), 1);

        let api_key = secrets.iter().find(|s| s.key == "API_KEY").unwrap();
        assert_eq!(api_key.key, "API_KEY");
        assert!(api_key.extension_name == "service-a" || api_key.extension_name == "service-b");
    }

    #[test]
    fn test_secret_requirement_creation() {
        let req = SecretRequirement::new("test-ext".to_string(), "API_TOKEN".to_string());

        assert_eq!(req.key, "API_TOKEN");
        assert_eq!(req.extension_name, "test-ext");
        assert_eq!(req.description(), "Required by test-ext extension");
    }

    #[test]
    fn test_discover_recipe_secrets_with_sub_recipes() {
        use goose::recipe::SubRecipe;

        let recipe = Recipe {
            version: "1.0.0".to_string(),
            title: "Parent Recipe".to_string(),
            description: "A recipe with sub-recipes".to_string(),
            instructions: Some("Test instructions".to_string()),
            prompt: None,
            extensions: Some(vec![ExtensionConfig::Sse {
                name: "parent-ext".to_string(),
                uri: "sse://parent.com".to_string(),
                envs: Envs::new(HashMap::new()),
                env_keys: vec!["PARENT_TOKEN".to_string()],
                description: None,
                timeout: None,
                bundled: None,
            }]),
            sub_recipes: Some(vec![SubRecipe {
                name: "child-recipe".to_string(),
                path: "path/to/child.yaml".to_string(),
                values: None,
                sequential_when_repeated: false,
                description: None,
            }]),
            context: None,
            settings: None,
            activities: None,
            author: None,
            parameters: None,
            response: None,
            retry: None,
        };

        let secrets = discover_recipe_secrets(&recipe);

        assert_eq!(secrets.len(), 1);

        let parent_secret = secrets.iter().find(|s| s.key == "PARENT_TOKEN").unwrap();
        assert_eq!(parent_secret.extension_name, "parent-ext");
    }

    /// Integration tests - these test the full workflow within recipe context
    mod integration_tests {
        use super::*;
        use crate::recipes::recipe::collect_missing_secrets;
        use goose::config::Config;
        use goose::recipe::SubRecipe;
        use serde_json::Value;

        fn create_test_recipe_with_mcp_extensions() -> Recipe {
            Recipe {
                version: "1.0.0".to_string(),
                title: "MCP Test Recipe".to_string(),
                description: "A recipe for testing MCP secret management".to_string(),
                instructions: Some("Test instructions".to_string()),
                prompt: None,
                extensions: Some(vec![
                    ExtensionConfig::Sse {
                        name: "github-mcp".to_string(),
                        uri: "sse://github.mcp.example.com".to_string(),
                        envs: Envs::new(HashMap::new()),
                        env_keys: vec!["GITHUB_TOKEN".to_string()],
                        description: Some("GitHub MCP extension".to_string()),
                        timeout: Some(30),
                        bundled: Some(false),
                    },
                    ExtensionConfig::Stdio {
                        name: "slack-mcp".to_string(),
                        cmd: "slack-mcp".to_string(),
                        args: vec!["--server".to_string()],
                        envs: Envs::new(HashMap::new()),
                        env_keys: vec!["SLACK_TOKEN".to_string(), "SLACK_WORKSPACE".to_string()],
                        timeout: Some(60),
                        description: Some("Slack MCP extension".to_string()),
                        bundled: Some(false),
                    },
                ]),
                context: None,
                settings: None,
                activities: None,
                author: None,
                parameters: None,
                response: None,
                sub_recipes: None,
                retry: None,
            }
        }

        #[test]
        fn test_recipe_secret_discovery_integration() {
            let recipe = create_test_recipe_with_mcp_extensions();
            let secrets = discover_recipe_secrets(&recipe);

            assert_eq!(secrets.len(), 3);

            let github_secret = secrets
                .iter()
                .find(|s| s.extension_name == "github-mcp")
                .unwrap();
            assert_eq!(github_secret.key, "GITHUB_TOKEN");

            let slack_secrets: Vec<_> = secrets
                .iter()
                .filter(|s| s.extension_name == "slack-mcp")
                .collect();
            assert_eq!(slack_secrets.len(), 2);

            let slack_token = slack_secrets
                .iter()
                .find(|s| s.key == "SLACK_TOKEN")
                .unwrap();
            assert_eq!(slack_token.key, "SLACK_TOKEN");

            let slack_workspace = slack_secrets
                .iter()
                .find(|s| s.key == "SLACK_WORKSPACE")
                .unwrap();
            assert_eq!(slack_workspace.key, "SLACK_WORKSPACE");
        }

        #[test]
        fn test_collect_missing_secrets_empty_list() {
            let requirements: Vec<SecretRequirement> = vec![];
            let result = collect_missing_secrets(&requirements);
            assert!(result.is_ok(), "Empty requirements should succeed");
        }

        #[test]
        fn test_collect_missing_secrets_with_existing_secrets() {
            let config = Config::global();

            config
                .set_secret("API_KEY", Value::String("existing_key".to_string()))
                .unwrap();

            let requirements = vec![SecretRequirement::new(
                "test-ext".to_string(),
                "API_KEY".to_string(),
            )];

            let result = collect_missing_secrets(&requirements);
            assert!(result.is_ok(), "Should succeed when secrets already exist");
        }

        #[test]
        fn test_recipe_without_extensions() {
            let recipe = Recipe {
                version: "1.0.0".to_string(),
                title: "Simple Recipe".to_string(),
                description: "A recipe without MCP extensions".to_string(),
                instructions: Some("Test instructions".to_string()),
                prompt: None,
                extensions: None, // No extensions
                context: None,
                settings: None,
                activities: None,
                author: None,
                parameters: None,
                response: None,
                sub_recipes: None,
                retry: None,
            };

            let secrets = discover_recipe_secrets(&recipe);
            assert_eq!(
                secrets.len(),
                0,
                "Recipe without extensions should have no secrets"
            );
        }

        #[test]
        fn test_secret_requirement_description_in_recipe_context() {
            let req = SecretRequirement::new("github-mcp".to_string(), "GITHUB_TOKEN".to_string());

            assert_eq!(req.description(), "Required by github-mcp extension");
            assert!(
                req.description().contains("github-mcp"),
                "Description should mention extension name for recipe context"
            );
        }

        #[test]
        fn test_recipe_with_sub_recipes_integration() {
            let recipe = Recipe {
                version: "1.0.0".to_string(),
                title: "Main Recipe with Sub-recipes".to_string(),
                description: "A recipe that includes other recipes".to_string(),
                instructions: Some("Main recipe instructions".to_string()),
                prompt: None,
                extensions: Some(vec![ExtensionConfig::Sse {
                    name: "main-github".to_string(),
                    uri: "sse://github.mcp.example.com".to_string(),
                    envs: Envs::new(HashMap::new()),
                    env_keys: vec!["MAIN_GITHUB_TOKEN".to_string()],
                    description: Some("Main GitHub MCP extension".to_string()),
                    timeout: Some(30),
                    bundled: Some(false),
                }]),
                sub_recipes: Some(vec![
                    SubRecipe {
                        name: "sub-recipe-1".to_string(),
                        path: "sub-recipes/github-setup.yaml".to_string(),
                        values: Some(HashMap::from([(
                            "repo_name".to_string(),
                            "test-repo".to_string(),
                        )])),
                        sequential_when_repeated: false,
                        description: Some("GitHub setup sub-recipe".to_string()),
                    },
                    SubRecipe {
                        name: "sub-recipe-2".to_string(),
                        path: "sub-recipes/slack-setup.yaml".to_string(),
                        values: None,
                        sequential_when_repeated: true,
                        description: Some("Slack setup sub-recipe".to_string()),
                    },
                ]),
                context: None,
                settings: None,
                activities: None,
                author: None,
                parameters: None,
                response: None,
                retry: None,
            };

            let secrets = discover_recipe_secrets(&recipe);

            // Sub-recipe secrets won't be found in integration test since files don't exist
            // But the function should handle this gracefully
            assert_eq!(secrets.len(), 1);

            let main_secret = secrets
                .iter()
                .find(|s| s.key == "MAIN_GITHUB_TOKEN")
                .unwrap();
            assert_eq!(main_secret.extension_name, "main-github");
        }
    }
}
