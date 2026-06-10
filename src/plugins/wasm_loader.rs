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
    /// Plugin Contracts v2 — load plugins and share a per-build state
    /// container across all of them. The container threads through each
    /// `WasmPluginAdapter` so the `_build_state_set` / `_build_state_get`
    /// bridge stubs operate on a single keystore. The same container is
    /// stored on the returned `PluginRegistry` (via `set_build_state`) so the
    /// build orchestrator can snapshot it for the manifest at end of build.
    /// See `contracts/lifecycle.md` §2.5.
    pub fn load_plugins_with_build_state(
        &mut self,
        plugin_names: &[String],
        build_state: super::BuildState,
    ) -> Result<PluginRegistry> {
        let mut registry = self.load_plugins_inner(plugin_names, Some(build_state.clone()))?;
        registry.set_build_state(build_state);
        Ok(registry)
    }

    /// Backward-compatible plugin loader — creates a fresh, isolated build
    /// state for the returned registry. Callers that want to surface build
    /// state into the manifest should use `load_plugins_with_build_state`.
    pub fn load_plugins(&mut self, plugin_names: &[String]) -> Result<PluginRegistry> {
        self.load_plugins_inner(plugin_names, None)
    }

    fn load_plugins_inner(
        &mut self,
        plugin_names: &[String],
        shared_state: Option<super::BuildState>,
    ) -> Result<PluginRegistry> {
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

            // Check for root-level plugin.wasm that differs from the active version
            self.check_plugin_version_mismatch(plugin_name, &plugin_dir);

            // Warn if this plugin was compiled with a compiler known to have codegen bugs.
            self.check_plugin_build_compatibility(plugin_name, &manifest, &plugin_dir);

            // Bridge-only plugins (handles.blocks is empty and no plugin.wasm exists) provide
            // only bridge function declarations and keywords — no WASM module is needed.
            let wasm_path = plugin_dir.join("plugin.wasm");
            let is_bridge_only = manifest.handles.blocks.is_empty() && !wasm_path.exists();
            if is_bridge_only {
                tracing::debug!(
                    plugin = plugin_name,
                    "Bridge-only plugin: skipping WASM load (no blocks, no plugin.wasm)"
                );
                continue;
            }

            // Load the plugin adapter
            let module = self.load_wasm_module(&wasm_path)?;
            let mut adapter = WasmPluginAdapter::new(
                plugin_name.to_string(),
                manifest,
                module,
                self.engine.clone(),
            )?;
            // Plugin Contracts v2 — if a shared build state was supplied,
            // wire it into the adapter so its `_build_state_*` bridge stubs
            // operate on the registry-wide keystore. Re-linking is required
            // because the linker captures the Arc by clone at setup time.
            // See contracts/lifecycle.md §2.5.
            if let Some(ref shared) = shared_state {
                adapter.set_build_state(shared.clone())?;
            }

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

    /// Find the plugin directory, respecting .active-version pin if present
    fn find_plugin_dir(&self, name: &str) -> Result<PathBuf> {
        let plugin_base = self.plugins_dir.join(name);

        if !plugin_base.exists() {
            return Err(anyhow!(
                "Plugin '{}' not found. Install with: cleen plugin add {}",
                name,
                name
            ));
        }

        // Honour the pinned version written by `cleen plugin use`
        let active_version_file = plugin_base.join(".active-version");
        if let Ok(pinned) = std::fs::read_to_string(&active_version_file) {
            let pinned = pinned.trim().to_string();
            if !pinned.is_empty() {
                let pinned_dir = plugin_base.join(&pinned);
                if pinned_dir.is_dir() && pinned_dir.join("plugin.wasm").exists() {
                    return Ok(pinned_dir);
                }
                // Pinned version directory missing — fall through to highest semver
                eprintln!(
                    "warning: plugin '{}' pinned version '{}' not found on disk; falling back to latest",
                    name, pinned
                );
            }
        }

        // Fall back to highest semver directory
        let mut versions: Vec<_> = std::fs::read_dir(&plugin_base)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        if versions.is_empty() {
            return Err(anyhow!("Plugin '{}' has no installed versions", name));
        }

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

    /// Warn if the root-level plugin.wasm differs from the active versioned copy.
    /// This catches cases where `cleen frame install` updated the root file but the
    /// versioned directory still has a stale binary.
    fn check_plugin_version_mismatch(&self, plugin_name: &str, version_dir: &Path) {
        let plugin_base = self.plugins_dir.join(plugin_name);
        let root_wasm = plugin_base.join("plugin.wasm");
        let versioned_wasm = version_dir.join("plugin.wasm");

        if !root_wasm.exists() || !versioned_wasm.exists() {
            return;
        }

        let root_meta = match std::fs::metadata(&root_wasm) {
            Ok(m) => m,
            Err(_) => return,
        };
        let versioned_meta = match std::fs::metadata(&versioned_wasm) {
            Ok(m) => m,
            Err(_) => return,
        };

        if root_meta.len() != versioned_meta.len() {
            let version = version_dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            eprintln!(
                "warning: plugin '{}' root plugin.wasm ({} bytes) differs from active version {} ({} bytes)",
                plugin_name,
                root_meta.len(),
                version,
                versioned_meta.len()
            );
            eprintln!(
                "hint: run `cleen plugin use {} <version>` or reinstall the plugin",
                plugin_name
            );
        }
    }

    /// Warn when a plugin was compiled with a compiler known to have codegen bugs.
    ///
    /// Plugins compiled before 0.30.96 have the string-comparison inversion bug:
    /// any `if x == "literal"` check inside the plugin always evaluates to the wrong
    /// branch, causing expand_block to emit garbage output instead of valid Clean code.
    /// Plugins without a [build] stamp are also suspect and trigger a warning.
    fn check_plugin_build_compatibility(
        &self,
        plugin_name: &str,
        manifest: &super::plugin_abi::PluginManifest,
        plugin_dir: &Path,
    ) {
        use super::plugin_abi::MINIMUM_SAFE_PLUGIN_COMPILER;
        let rebuild_hint = format!(
            "hint: rebuild with `bash {}/build.sh` then copy plugin.wasm to {}",
            plugin_dir.display(),
            plugin_dir.display(),
        );

        match &manifest.build.built_with_compiler {
            None => {
                eprintln!(
                    "warning[{}]: plugin has no build stamp — it may have been compiled \
                     with a compiler that has known codegen bugs.",
                    plugin_name
                );
                eprintln!("{}", rebuild_hint);
            }
            Some(ver) => {
                if Self::version_less_than(ver, MINIMUM_SAFE_PLUGIN_COMPILER) {
                    eprintln!(
                        "warning[{}]: plugin was built with compiler {} which has known \
                         codegen bugs (minimum safe: {}). Output may be corrupted.",
                        plugin_name, ver, MINIMUM_SAFE_PLUGIN_COMPILER
                    );
                    eprintln!("{}", rebuild_hint);
                }
            }
        }
    }

    /// Returns true if `a` is strictly less than `b` as a semantic version triple.
    fn version_less_than(a: &str, b: &str) -> bool {
        let parse = |s: &str| -> (u32, u32, u32) {
            let p: Vec<u32> = s.split('.').filter_map(|x| x.parse().ok()).collect();
            (
                *p.first().unwrap_or(&0),
                *p.get(1).unwrap_or(&0),
                *p.get(2).unwrap_or(&0),
            )
        };
        parse(a) < parse(b)
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

    #[test]
    fn test_version_mismatch_warning_no_panic() {
        // Should not panic when root wasm doesn't exist
        let temp_dir = TempDir::new().unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();
        let version_dir = temp_dir.path().join("test.plugin").join("1.0.0");
        std::fs::create_dir_all(&version_dir).unwrap();
        loader.check_plugin_version_mismatch("test.plugin", &version_dir);
    }

    #[test]
    fn test_version_mismatch_detects_size_difference() {
        let temp_dir = TempDir::new().unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();

        let plugin_dir = temp_dir.path().join("test.plugin");
        let version_dir = plugin_dir.join("1.0.0");
        std::fs::create_dir_all(&version_dir).unwrap();

        // Root wasm: 100 bytes, versioned wasm: 50 bytes
        std::fs::write(plugin_dir.join("plugin.wasm"), vec![0u8; 100]).unwrap();
        std::fs::write(version_dir.join("plugin.wasm"), vec![0u8; 50]).unwrap();

        // This should print a warning (no panic, no error)
        loader.check_plugin_version_mismatch("test.plugin", &version_dir);
    }

    #[test]
    fn test_version_mismatch_no_warning_when_same_size() {
        let temp_dir = TempDir::new().unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();

        let plugin_dir = temp_dir.path().join("test.plugin");
        let version_dir = plugin_dir.join("1.0.0");
        std::fs::create_dir_all(&version_dir).unwrap();

        // Same size — no warning expected
        std::fs::write(plugin_dir.join("plugin.wasm"), vec![0u8; 100]).unwrap();
        std::fs::write(version_dir.join("plugin.wasm"), vec![0u8; 100]).unwrap();

        loader.check_plugin_version_mismatch("test.plugin", &version_dir);
    }
}
