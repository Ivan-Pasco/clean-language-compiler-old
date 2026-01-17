/*!
 * App Configuration Parser - Extracts plugin declarations from app.cln
 *
 * This module parses the Clean Language configuration file (app.cln or configuration.cln)
 * to extract the list of plugins that should be loaded for a project.
 */

use std::fs;
use std::path::Path;

/// Application configuration parsed from app.cln
///
/// Contains project-level settings including plugin declarations.
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    /// Plugins declared in the plugins: block
    pub plugins: Vec<String>,
    /// Application name (if specified)
    pub name: Option<String>,
    /// Application version (if specified)
    pub version: Option<String>,
}

impl AppConfig {
    /// Create a new empty app configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse app configuration from Clean Language content
    ///
    /// Extracts the plugins: block and other relevant settings.
    ///
    /// # Example app.cln format
    ///
    /// ```text
    /// plugins:
    ///     frame.web
    ///     frame.ui
    ///     frame.data
    ///
    /// name = "my-app"
    /// version = "1.0.0"
    /// ```
    ///
    /// # Arguments
    /// * `content` - The content of the app.cln file
    ///
    /// # Returns
    /// * `AppConfig` with parsed values
    pub fn parse(content: &str) -> Self {
        let mut config = AppConfig::new();

        let mut in_plugins_block = false;
        let mut current_indent = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Check for plugins: block start
            if trimmed == "plugins:" || trimmed.starts_with("plugins:") {
                in_plugins_block = true;
                // Calculate the base indentation for this block
                current_indent = line.len() - line.trim_start().len();
                continue;
            }

            // If in plugins block, parse indented plugin names
            if in_plugins_block {
                let line_indent = line.len() - line.trim_start().len();

                // If this line has less or equal indentation, we've left the block
                if !line.starts_with('\t')
                    && !line.starts_with("    ")
                    && line_indent <= current_indent
                {
                    in_plugins_block = false;
                } else if !trimmed.is_empty() {
                    // This is a plugin name
                    config.plugins.push(trimmed.to_string());
                    continue;
                }
            }

            // Parse name = "value" style assignments
            if let Some((key, value)) = parse_assignment(trimmed) {
                match key.as_str() {
                    "name" => config.name = Some(value),
                    "version" => config.version = Some(value),
                    _ => {}
                }
            }
        }

        config
    }

    /// Load and parse configuration from a file path
    ///
    /// # Arguments
    /// * `path` - Path to the configuration file (app.cln or configuration.cln)
    ///
    /// # Returns
    /// * `Ok(AppConfig)` - Parsed configuration
    /// * `Err(String)` - Error message if file cannot be read
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;

        Ok(Self::parse(&content))
    }

    /// Find and load the configuration file from a project directory
    ///
    /// Searches for (in order):
    /// 1. app.cln
    /// 2. configuration.cln
    ///
    /// # Arguments
    /// * `project_dir` - The project directory to search in
    ///
    /// # Returns
    /// * `Some(AppConfig)` - If a config file was found and parsed
    /// * `None` - If no config file exists
    pub fn find_and_load(project_dir: &Path) -> Option<Self> {
        let config_files = ["app.cln", "configuration.cln"];

        for file in &config_files {
            let path = project_dir.join(file);
            if path.exists() {
                if let Ok(config) = Self::load(&path) {
                    return Some(config);
                }
            }
        }

        None
    }
}

/// Parse a key = "value" style assignment
fn parse_assignment(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }

    let key = parts[0].trim().to_string();
    let value = parts[1].trim();

    // Remove quotes from value
    let value = if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    };

    Some((key, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plugins_block() {
        let content = r#"
plugins:
    frame.web
    frame.ui
    frame.data

name = "my-app"
"#;

        let config = AppConfig::parse(content);

        assert_eq!(config.plugins.len(), 3);
        assert!(config.plugins.contains(&"frame.web".to_string()));
        assert!(config.plugins.contains(&"frame.ui".to_string()));
        assert!(config.plugins.contains(&"frame.data".to_string()));
    }

    #[test]
    fn test_parse_name_and_version() {
        let content = r#"
name = "test-app"
version = "2.0.0"
"#;

        let config = AppConfig::parse(content);

        assert_eq!(config.name, Some("test-app".to_string()));
        assert_eq!(config.version, Some("2.0.0".to_string()));
    }

    #[test]
    fn test_parse_with_comments() {
        let content = r#"
// This is a comment
plugins:
    // Comment in plugins
    frame.web
    frame.data

# Hash comment
name = "app"
"#;

        let config = AppConfig::parse(content);

        assert_eq!(config.plugins.len(), 2);
        assert_eq!(config.name, Some("app".to_string()));
    }

    #[test]
    fn test_parse_empty_content() {
        let config = AppConfig::parse("");

        assert!(config.plugins.is_empty());
        assert!(config.name.is_none());
        assert!(config.version.is_none());
    }

    #[test]
    fn test_parse_single_quotes() {
        let content = r#"
name = 'single-quoted'
version = "double-quoted"
"#;

        let config = AppConfig::parse(content);

        assert_eq!(config.name, Some("single-quoted".to_string()));
        assert_eq!(config.version, Some("double-quoted".to_string()));
    }

    #[test]
    fn test_plugins_with_spaces() {
        let content = r#"
plugins:
    frame.web

    frame.data
"#;

        let config = AppConfig::parse(content);

        assert_eq!(config.plugins.len(), 2);
    }

    #[test]
    fn test_plugins_block_ends_at_unindented_line() {
        let content = r#"
plugins:
    frame.web
    frame.data
name = "app"
version = "1.0"
"#;

        let config = AppConfig::parse(content);

        assert_eq!(config.plugins.len(), 2);
        assert_eq!(config.name, Some("app".to_string()));
        assert_eq!(config.version, Some("1.0".to_string()));
    }
}
