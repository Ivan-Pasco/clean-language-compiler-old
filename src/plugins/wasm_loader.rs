//! External WASM Plugin Loader
//!
//! Discovers and loads plugins from ~/.cleen/plugins/

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use wasmtime::{Engine, Module};

use crate::plugins::PluginRegistry;

use super::plugin_abi::PluginManifest;
use super::wasm_adapter::WasmPluginAdapter;

/// Loads external WASM plugins from the filesystem
pub struct WasmPluginLoader {
    /// Path to plugins directory (typically ~/.cleen/plugins/)
    plugins_dir: PathBuf,
    /// Wasmtime engine for compiling WASM modules
    engine: Engine,
    /// Cache of loaded plugin modules
    module_cache: HashMap<String, Module>,
}

impl WasmPluginLoader {
    /// Create a new plugin loader
    pub fn new() -> Result<Self> {
        let plugins_dir = Self::get_plugins_dir()?;
        let engine = Engine::default();

        Ok(Self {
            plugins_dir,
            engine,
            module_cache: HashMap::new(),
        })
    }

    /// Create loader with custom plugins directory
    pub fn with_plugins_dir(plugins_dir: PathBuf) -> Result<Self> {
        let engine = Engine::default();

        Ok(Self {
            plugins_dir,
            engine,
            module_cache: HashMap::new(),
        })
    }

    /// Get the default plugins directory (~/.cleen/plugins/)
    fn get_plugins_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        Ok(home.join(".cleen").join("plugins"))
    }

    /// Load plugins declared in configuration.cln
    ///
    /// Plugins are declared at the project level in configuration.cln, NOT imported
    /// in source files. The `import:` statement is exclusively for Clean Language modules.
    ///
    /// # Arguments
    /// * `plugin_names` - List of plugin names from configuration.cln (e.g., ["frame.web", "frame.data"])
    ///
    /// # Returns
    /// A PluginRegistry with all requested plugins loaded, including bridge functions
    pub fn load_plugins(&mut self, plugin_names: &[String]) -> Result<PluginRegistry> {
        let mut builder = PluginRegistry::builder();

        for plugin_name in plugin_names {
            // Find plugin directory and load manifest first
            let plugin_dir = self.find_plugin_dir(plugin_name)?;
            let manifest_path = plugin_dir.join("plugin.toml");
            let manifest = self.load_manifest(&manifest_path)?;

            // Add bridge functions from the manifest
            if !manifest.bridge.functions.is_empty() {
                tracing::info!(
                    plugin = plugin_name,
                    bridge_function_count = manifest.bridge.functions.len(),
                    "Loading bridge functions from plugin"
                );
                builder = builder.add_bridge_functions(&manifest.bridge);
            }

            // Register default lifecycle entries based on what the manifest declares.
            // The actual values are populated below once the adapter is instantiated.
            builder = builder.add_registration(plugin_name, &manifest);

            // Store the full manifest so language_to_bridge_map() can resolve
            // dot-notation aliases (e.g. "req.query" → "_req_query").
            builder = builder.add_manifest(plugin_name.clone(), manifest.clone());

            // Load the plugin adapter
            let wasm_path = plugin_dir.join("plugin.wasm");
            let module = self.load_wasm_module(&wasm_path)?;
            let adapter = WasmPluginAdapter::new(
                plugin_name.to_string(),
                manifest,
                module,
                self.engine.clone(),
            )?;

            // Call the lifecycle hooks and update the registrations that were
            // added above with the actual values returned by the WASM module.
            if let Some(server_reg) = adapter.call_register_server() {
                tracing::info!(
                    plugin = plugin_name,
                    middleware_count = server_reg.middleware.len(),
                    "Plugin registered server lifecycle hooks"
                );
                // Replace the default entry written by add_registration with the
                // real one.  Linear search is acceptable here — plugin counts are small.
                builder = builder.update_server_registration(plugin_name, server_reg);
            }

            if let Some(cli_reg) = adapter.call_register_cli() {
                tracing::info!(
                    plugin = plugin_name,
                    command_count = cli_reg.commands.len(),
                    "Plugin registered CLI commands"
                );
                builder = builder.update_cli_registration(plugin_name, cli_reg);
            }

            if let Some(data_reg) = adapter.call_register_data() {
                tracing::info!(
                    plugin = plugin_name,
                    type_count = data_reg.types.len(),
                    "Plugin registered data lifecycle hooks"
                );
                builder = builder.update_data_registration(plugin_name, data_reg);
            }

            if let Some(build_reg) = adapter.call_register_build() {
                tracing::info!(
                    plugin = plugin_name,
                    pre_build_count = build_reg.pre_build.len(),
                    post_build_count = build_reg.post_build.len(),
                    "Plugin registered build lifecycle hooks"
                );
                builder = builder.update_build_registration(plugin_name, build_reg);
            }

            builder = builder.add(adapter);
        }

        builder
            .build()
            .map_err(|e| anyhow!("Failed to build plugin registry: {}", e))
    }

    /// Find the plugin directory, using the latest version if not specified
    fn find_plugin_dir(&self, name: &str) -> Result<PathBuf> {
        let plugin_base = self.plugins_dir.join(name);

        if !plugin_base.exists() {
            return Err(anyhow!(
                "Plugin '{}' not found. Install with: cleen plugin add {}",
                name,
                name
            ));
        }

        // Find the latest version
        let mut versions: Vec<_> = std::fs::read_dir(&plugin_base)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        if versions.is_empty() {
            return Err(anyhow!("Plugin '{}' has no installed versions", name));
        }

        // Sort by version (semantic versioning)
        versions.sort_by(|a, b| {
            let a_name = a.file_name().to_string_lossy().to_string();
            let b_name = b.file_name().to_string_lossy().to_string();
            Self::compare_versions(&a_name, &b_name)
        });

        Ok(versions.last().unwrap().path())
    }

    /// Compare semantic versions
    fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
        let parse = |s: &str| -> (u32, u32, u32) {
            let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();
            (
                parts.first().copied().unwrap_or(0),
                parts.get(1).copied().unwrap_or(0),
                parts.get(2).copied().unwrap_or(0),
            )
        };
        parse(a).cmp(&parse(b))
    }

    /// Load plugin manifest from TOML file
    fn load_manifest(&self, path: &Path) -> Result<PluginManifest> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read plugin manifest {}: {}", path.display(), e))?;

        toml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse plugin manifest {}: {}", path.display(), e))
    }

    /// Load and compile WASM module
    fn load_wasm_module(&mut self, path: &Path) -> Result<Module> {
        let path_str = path.to_string_lossy().to_string();

        eprintln!(
            "[Plugin Loader] Loading WASM module from: {}",
            path.display()
        );

        // Check cache first
        if let Some(module) = self.module_cache.get(&path_str) {
            eprintln!("[Plugin Loader] Using cached module");
            return Ok(module.clone());
        }

        // Load and compile
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| anyhow!("Failed to read WASM file {}: {}", path.display(), e))?;

        eprintln!("[Plugin Loader] WASM file size: {} bytes", wasm_bytes.len());

        let module = Module::new(&self.engine, &wasm_bytes)
            .map_err(|e| anyhow!("Failed to compile WASM module {}: {}", path.display(), e))?;

        eprintln!(
            "[Plugin Loader] Module compiled, exports count: {}",
            module.exports().count()
        );

        // Cache the compiled module
        self.module_cache.insert(path_str, module.clone());

        Ok(module)
    }

    /// List all installed plugins
    pub fn list_installed_plugins(&self) -> Result<Vec<(String, String)>> {
        let mut plugins = Vec::new();

        if !self.plugins_dir.exists() {
            return Ok(plugins);
        }

        for entry in std::fs::read_dir(&self.plugins_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();

                // Find latest version
                if let Ok(plugin_dir) = self.find_plugin_dir(&name) {
                    let version = plugin_dir
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    plugins.push((name, version));
                }
            }
        }

        Ok(plugins)
    }

    /// Get the engine for creating stores
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Check if a plugin is installed
    pub fn is_plugin_installed(&self, name: &str) -> bool {
        self.plugins_dir.join(name).exists()
    }
}

impl Default for WasmPluginLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create default WasmPluginLoader")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_version_comparison() {
        assert_eq!(
            WasmPluginLoader::compare_versions("1.0.0", "1.0.1"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            WasmPluginLoader::compare_versions("1.2.0", "1.1.9"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            WasmPluginLoader::compare_versions("2.0.0", "1.9.9"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            WasmPluginLoader::compare_versions("1.0.0", "1.0.0"),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn test_loader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf());
        assert!(loader.is_ok());
    }

    #[test]
    fn test_list_empty_plugins() {
        let temp_dir = TempDir::new().unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();
        let plugins = loader.list_installed_plugins().unwrap();
        assert!(plugins.is_empty());
    }
}
