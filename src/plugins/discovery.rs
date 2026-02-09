/*!
 * Plugin Discovery - Finds and loads plugin manifests from global and project directories
 *
 * This module provides functionality to discover plugins from:
 * - Global plugins directory: ~/.cleen/plugins/
 * - Project plugins directory: <project>/plugins/
 * - Plugins declared in app.cln
 */

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::plugin_abi::PluginManifest;

/// Error type for plugin discovery operations
#[derive(Debug)]
pub enum DiscoveryError {
    /// Failed to read a directory
    IoError { path: PathBuf, message: String },
    /// Failed to parse a plugin.toml file
    ParseError { path: PathBuf, message: String },
    /// Plugin not found
    PluginNotFound { name: String },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryError::IoError { path, message } => {
                write!(f, "Failed to read '{}': {}", path.display(), message)
            }
            DiscoveryError::ParseError { path, message } => {
                write!(f, "Failed to parse '{}': {}", path.display(), message)
            }
            DiscoveryError::PluginNotFound { name } => {
                write!(f, "Plugin not found: {}", name)
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Plugin discovery service
///
/// Discovers plugin manifests from multiple locations and provides
/// methods to load specific plugins by name.
///
/// # Example
///
/// ```ignore
/// let discovery = PluginDiscovery::new();
/// let manifests = discovery.discover_all()?;
/// for (name, manifest) in &manifests {
///     println!("Found plugin: {}", name);
/// }
/// ```
pub struct PluginDiscovery {
    /// Global plugins directory (~/.cleen/plugins/)
    global_dir: PathBuf,
    /// Project plugins directory (<project>/plugins/)
    project_dir: Option<PathBuf>,
}

impl PluginDiscovery {
    /// Create a new plugin discovery service with default paths
    ///
    /// Uses:
    /// - Global: ~/.cleen/plugins/
    /// - Project: None (set with `with_project_dir`)
    pub fn new() -> Self {
        let global_dir = dirs::home_dir()
            .map(|h| h.join(".cleen").join("plugins"))
            .unwrap_or_else(|| PathBuf::from(".cleen/plugins"));

        Self {
            global_dir,
            project_dir: None,
        }
    }

    /// Set the global plugins directory
    pub fn with_global_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.global_dir = dir.into();
        self
    }

    /// Set the project plugins directory
    pub fn with_project_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_dir = Some(dir.into());
        self
    }

    /// Discover all plugins from global and project directories
    ///
    /// Returns a map of plugin name to manifest. Project plugins
    /// take precedence over global plugins with the same name.
    ///
    /// # Returns
    /// * `Ok(HashMap<String, PluginManifest>)` - Map of plugin name to manifest
    /// * `Err(DiscoveryError)` - If discovery fails
    pub fn discover_all(&self) -> Result<HashMap<String, PluginManifest>, DiscoveryError> {
        let mut manifests = HashMap::new();

        // Load global plugins first
        if self.global_dir.exists() {
            self.discover_from_dir(&self.global_dir, &mut manifests)?;
        }

        // Project plugins override global plugins
        if let Some(ref project_dir) = self.project_dir {
            if project_dir.exists() {
                self.discover_from_dir(project_dir, &mut manifests)?;
            }
        }

        Ok(manifests)
    }

    /// Discover plugins from a specific directory
    ///
    /// Each subdirectory should contain a plugin.toml file.
    fn discover_from_dir(
        &self,
        dir: &Path,
        manifests: &mut HashMap<String, PluginManifest>,
    ) -> Result<(), DiscoveryError> {
        let entries = fs::read_dir(dir).map_err(|e| DiscoveryError::IoError {
            path: dir.to_path_buf(),
            message: e.to_string(),
        })?;

        for entry in entries.flatten() {
            let path = entry.path();

            // Check for plugin.toml directly in directory
            if path.is_dir() {
                let manifest_path = path.join("plugin.toml");
                if manifest_path.exists() {
                    if let Ok(manifest) = self.load_manifest(&manifest_path) {
                        manifests.insert(manifest.plugin.name.clone(), manifest);
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single plugin manifest from a file
    pub fn load_manifest(&self, path: &Path) -> Result<PluginManifest, DiscoveryError> {
        let content = fs::read_to_string(path).map_err(|e| DiscoveryError::IoError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        toml::from_str(&content).map_err(|e| DiscoveryError::ParseError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Load a specific plugin by name
    ///
    /// Searches in project directory first, then global directory.
    ///
    /// # Arguments
    /// * `name` - Plugin name (e.g., "frame.data")
    ///
    /// # Returns
    /// * `Ok(PluginManifest)` - The loaded manifest
    /// * `Err(DiscoveryError::PluginNotFound)` - If plugin doesn't exist
    pub fn load_plugin(&self, name: &str) -> Result<PluginManifest, DiscoveryError> {
        // Convert plugin name to directory name (e.g., "frame.data" -> "frame.data" or "frame-data")
        let dir_names = [name.to_string(), name.replace('.', "-")];

        // Check project directory first
        if let Some(ref project_dir) = self.project_dir {
            for dir_name in &dir_names {
                let manifest_path = project_dir.join(dir_name).join("plugin.toml");
                if manifest_path.exists() {
                    return self.load_manifest(&manifest_path);
                }
            }
        }

        // Check global directory
        for dir_name in &dir_names {
            let manifest_path = self.global_dir.join(dir_name).join("plugin.toml");
            if manifest_path.exists() {
                return self.load_manifest(&manifest_path);
            }
        }

        Err(DiscoveryError::PluginNotFound {
            name: name.to_string(),
        })
    }

    /// Load multiple plugins by name
    ///
    /// # Arguments
    /// * `names` - Plugin names to load
    ///
    /// # Returns
    /// * `Ok(Vec<(String, PluginManifest)>)` - Successfully loaded plugins
    /// * Skips plugins that aren't found (logs warning)
    pub fn load_plugins(&self, names: &[String]) -> Vec<(String, PluginManifest)> {
        let mut result = Vec::new();

        for name in names {
            match self.load_plugin(name) {
                Ok(manifest) => {
                    result.push((name.clone(), manifest));
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugin '{}': {}", name, e);
                }
            }
        }

        result
    }

    /// Get all plugin manifest paths from a directory
    ///
    /// Useful for file watching - returns paths to all plugin.toml files.
    pub fn get_manifest_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Global directory
        if self.global_dir.exists() {
            if let Ok(entries) = fs::read_dir(&self.global_dir) {
                for entry in entries.flatten() {
                    let manifest_path = entry.path().join("plugin.toml");
                    if manifest_path.exists() {
                        paths.push(manifest_path);
                    }
                }
            }
        }

        // Project directory
        if let Some(ref project_dir) = self.project_dir {
            if project_dir.exists() {
                if let Ok(entries) = fs::read_dir(project_dir) {
                    for entry in entries.flatten() {
                        let manifest_path = entry.path().join("plugin.toml");
                        if manifest_path.exists() {
                            paths.push(manifest_path);
                        }
                    }
                }
            }
        }

        paths
    }
}

impl Default for PluginDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_plugin(dir: &Path, name: &str) -> PathBuf {
        let plugin_dir = dir.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();

        let manifest_path = plugin_dir.join("plugin.toml");
        let content = format!(
            r#"
[plugin]
name = "{}"
version = "1.0.0"

[handles]
blocks = ["test"]

[language]
blocks = ["test"]

[[language.keywords]]
name = "testkw"
description = "A test keyword"
"#,
            name
        );

        fs::File::create(&manifest_path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        plugin_dir
    }

    #[test]
    fn test_discover_from_dir() {
        let temp = TempDir::new().unwrap();
        create_test_plugin(temp.path(), "test.plugin1");
        create_test_plugin(temp.path(), "test.plugin2");

        let discovery = PluginDiscovery::new().with_global_dir(temp.path());

        let manifests = discovery.discover_all().unwrap();

        assert_eq!(manifests.len(), 2);
        assert!(manifests.contains_key("test.plugin1"));
        assert!(manifests.contains_key("test.plugin2"));
    }

    #[test]
    fn test_project_overrides_global() {
        let global_temp = TempDir::new().unwrap();
        let project_temp = TempDir::new().unwrap();

        // Create same plugin in both directories
        create_test_plugin(global_temp.path(), "shared.plugin");
        let project_plugin = create_test_plugin(project_temp.path(), "shared.plugin");

        // Modify the project version to have different content
        let manifest_path = project_plugin.join("plugin.toml");
        let content = r#"
[plugin]
name = "shared.plugin"
version = "2.0.0"
description = "Project version"

[handles]
blocks = ["projectblock"]
"#;
        fs::write(&manifest_path, content).unwrap();

        let discovery = PluginDiscovery::new()
            .with_global_dir(global_temp.path())
            .with_project_dir(project_temp.path());

        let manifests = discovery.discover_all().unwrap();

        assert_eq!(manifests.len(), 1);
        let manifest = manifests.get("shared.plugin").unwrap();
        assert_eq!(manifest.plugin.version, "2.0.0"); // Project version wins
    }

    #[test]
    fn test_load_specific_plugin() {
        let temp = TempDir::new().unwrap();
        create_test_plugin(temp.path(), "my.plugin");

        let discovery = PluginDiscovery::new().with_global_dir(temp.path());

        let manifest = discovery.load_plugin("my.plugin").unwrap();
        assert_eq!(manifest.plugin.name, "my.plugin");
    }

    #[test]
    fn test_plugin_not_found() {
        let temp = TempDir::new().unwrap();
        let discovery = PluginDiscovery::new().with_global_dir(temp.path());

        let result = discovery.load_plugin("nonexistent.plugin");
        assert!(matches!(result, Err(DiscoveryError::PluginNotFound { .. })));
    }

    #[test]
    fn test_get_manifest_paths() {
        let temp = TempDir::new().unwrap();
        create_test_plugin(temp.path(), "plugin1");
        create_test_plugin(temp.path(), "plugin2");

        let discovery = PluginDiscovery::new().with_global_dir(temp.path());

        let paths = discovery.get_manifest_paths();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|p| p.ends_with("plugin.toml")));
    }
}
