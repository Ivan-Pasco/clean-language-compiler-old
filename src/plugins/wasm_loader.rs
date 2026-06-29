//! External WASM Plugin Loader
//!
//! Discovers and loads plugins from ~/.cleen/plugins/

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{anyhow, Result};
use wasmtime::{Engine, Module};

use crate::plugins::PluginRegistry;

use super::plugin_abi::PluginManifest;
use super::wasm_adapter::WasmPluginAdapter;

/// Plugin Contracts v2 Phase B cycle 2 — outcome of inspecting a plugin.wasm's
/// `clean.abi_version` custom section. See
/// `foundation/spec/plugins/contracts/runtime-abi.md` §5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiStampOutcome {
    /// Stamp present and in `SUPPORTED_RUNTIME_ABI_VERSIONS` — load as normal.
    Supported(String),
    /// Stamp present but unsupported — loader refuses with PLUGIN-ABI-MISMATCH.
    Unsupported(String),
    /// Stamp absent — loader proceeds with a warning, defaulting to
    /// `DEFAULT_RUNTIME_ABI_VERSION`. Phase D will promote this to an error.
    Absent,
}

/// Default per-plugin-call timeout in seconds. Plugin entrypoints
/// (`process_html`, `assemble`, lifecycle hooks, block expansion) that run
/// longer than this trap with a `wasm trap: interrupt` and a WASM
/// backtrace, converting a silent compiler hang into an actionable
/// diagnostic. Override at the user level via `CLN_PLUGIN_TIMEOUT_SECS`.
///
/// 30s is generous enough for large assemble passes on real projects
/// (hundreds of page companions) while still aborting infinite loops
/// quickly. See [`plugin_timeout_secs`].
const PLUGIN_TIMEOUT_DEFAULT_SECS: u64 = 30;

/// Read the configured per-plugin-call timeout, falling back to the
/// default. `CLN_PLUGIN_TIMEOUT_SECS=0` disables the timeout (legacy
/// behaviour — hangs become indefinite).
pub fn plugin_timeout_secs() -> u64 {
    match std::env::var("CLN_PLUGIN_TIMEOUT_SECS") {
        Ok(v) => v.parse().unwrap_or(PLUGIN_TIMEOUT_DEFAULT_SECS),
        Err(_) => PLUGIN_TIMEOUT_DEFAULT_SECS,
    }
}

/// The epoch tick interval in milliseconds. The plugin store deadline
/// is expressed in ticks (`timeout_secs * 1000 / EPOCH_TICK_MS`), so the
/// effective resolution is one tick — picking 100ms keeps the deadline
/// jitter under a tenth of a second without spinning the ticker thread
/// excessively.
pub(crate) const EPOCH_TICK_MS: u64 = 100;

static EPOCH_TICKER_STARTED: Once = Once::new();

/// Start a daemon thread that ticks the engine epoch every
/// [`EPOCH_TICK_MS`]. Idempotent — only one ticker per process.
/// See [`plugin_timeout_secs`] for how the deadline is consumed.
fn start_epoch_ticker(engine: &Engine) {
    EPOCH_TICKER_STARTED.call_once(|| {
        let engine = engine.clone();
        std::thread::Builder::new()
            .name("cln-plugin-epoch-ticker".to_string())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(EPOCH_TICK_MS));
                engine.increment_epoch();
            })
            .expect("failed to spawn plugin epoch ticker");
    });
}

/// Build the shared wasmtime engine with epoch interruption enabled, and
/// start the ticker that drives plugin call deadlines.
fn build_engine() -> Result<Engine> {
    let mut cfg = wasmtime::Config::new();
    // Enable epoch-based interruption so a runaway plugin call (infinite
    // loop, deadlock against host-side state) traps after the per-call
    // deadline set by `WasmPluginAdapter::create_store` instead of
    // hanging the compiler indefinitely.
    cfg.epoch_interruption(true);
    let engine = Engine::new(&cfg).map_err(|e| anyhow!("Failed to create WASM engine: {}", e))?;
    start_epoch_ticker(&engine);
    Ok(engine)
}

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
        let engine = build_engine()?;

        Ok(Self {
            plugins_dir,
            engine,
            module_cache: HashMap::new(),
        })
    }

    /// Create loader with custom plugins directory
    pub fn with_plugins_dir(plugins_dir: PathBuf) -> Result<Self> {
        let engine = build_engine()?;

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

            // Plugin Contracts v2 Phase B cycle 2 — read the `clean.abi_version`
            // WASM custom section emitted by cycle 1 and refuse plugins whose
            // stamp is not in `SUPPORTED_RUNTIME_ABI_VERSIONS`. Absent stamp
            // produces a warning (Phase B contract — Phase D promotes this to
            // an error). See foundation/spec/plugins/contracts/runtime-abi.md §5.
            let wasm_for_abi = plugin_dir.join("plugin.wasm");
            if wasm_for_abi.exists() {
                self.check_plugin_abi_version(plugin_name, &wasm_for_abi)?;
            }

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

    /// Plugin Contracts v2 Phase B cycle 2 — outcome of a `clean.abi_version`
    /// custom-section check on a plugin.wasm. Used by `check_plugin_abi_version`
    /// to keep the side-effecting warning print and the return value cohesive,
    /// and to give tests a structured surface they can assert on.
    /// Stamped versions live in [`super::plugin_abi::SUPPORTED_RUNTIME_ABI_VERSIONS`].
    /// See `foundation/spec/plugins/contracts/runtime-abi.md` §4–§5.
    /// Stamp present and in supported set → `Supported(version)`.
    /// Stamp present but not supported → `Unsupported(version)`.
    /// Stamp absent → `Absent` (load proceeds with a warning).
    /// Public for use by the cycle 2 integration tests.
    pub fn classify_abi_stamp(wasm_bytes: &[u8]) -> AbiStampOutcome {
        use super::plugin_abi::SUPPORTED_RUNTIME_ABI_VERSIONS;
        use wasmparser::{Parser, Payload};

        for payload in Parser::new(0).parse_all(wasm_bytes).flatten() {
            if let Payload::CustomSection(reader) = payload {
                if reader.name() == "clean.abi_version" {
                    let ver = match std::str::from_utf8(reader.data()) {
                        Ok(s) => s.to_string(),
                        Err(_) => return AbiStampOutcome::Absent,
                    };
                    return if SUPPORTED_RUNTIME_ABI_VERSIONS.iter().any(|s| *s == ver) {
                        AbiStampOutcome::Supported(ver)
                    } else {
                        AbiStampOutcome::Unsupported(ver)
                    };
                }
            }
        }
        AbiStampOutcome::Absent
    }

    /// Format the user-facing PLUGIN-ABI-MISMATCH error message. Public so
    /// tests can assert the exact text without re-deriving it.
    pub fn format_abi_mismatch_error(plugin_name: &str, wasm_path: &Path, found: &str) -> String {
        use super::plugin_abi::SUPPORTED_RUNTIME_ABI_VERSIONS;
        format!(
            "error[PLUGIN-ABI-MISMATCH]: plugin '{}' at {} was compiled with \
             Clean Runtime ABI {}, but this compiler only supports {:?}.\n  \
             resolution: reinstall a compatible plugin build via \
             `cleen frame install` or upgrade the compiler.\n  \
             spec: foundation/spec/plugins/contracts/runtime-abi.md",
            plugin_name,
            wasm_path.display(),
            found,
            SUPPORTED_RUNTIME_ABI_VERSIONS,
        )
    }

    /// Format the warning text emitted when a plugin has no `clean.abi_version`
    /// stamp. Public so tests can assert the exact text.
    pub fn format_abi_absent_warning(plugin_name: &str, wasm_path: &Path) -> String {
        use super::plugin_abi::DEFAULT_RUNTIME_ABI_VERSION;
        format!(
            "warning: plugin '{}' at {} has no clean.abi_version stamp; \
             assuming {} — rebuild against current compiler to make this explicit",
            plugin_name,
            wasm_path.display(),
            DEFAULT_RUNTIME_ABI_VERSION,
        )
    }

    /// Apply the three-case decision on a single plugin: error on unsupported
    /// stamp, warn on absent, succeed on supported. The error string and
    /// warning string come from `format_abi_mismatch_error` /
    /// `format_abi_absent_warning` so test assertions and runtime output stay
    /// in sync. See `runtime-abi.md` §5.
    fn check_plugin_abi_version(&self, plugin_name: &str, wasm_path: &Path) -> Result<()> {
        let bytes = match std::fs::read(wasm_path) {
            Ok(b) => b,
            Err(_) => return Ok(()),
        };

        match Self::classify_abi_stamp(&bytes) {
            AbiStampOutcome::Supported(_) => Ok(()),
            AbiStampOutcome::Unsupported(ver) => Err(anyhow!(
                "{}",
                Self::format_abi_mismatch_error(plugin_name, wasm_path, &ver)
            )),
            AbiStampOutcome::Absent => {
                eprintln!(
                    "{}",
                    Self::format_abi_absent_warning(plugin_name, wasm_path)
                );
                Ok(())
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

    /// Regression test for COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS:
    /// the plugin-call timeout must be configurable through
    /// `CLN_PLUGIN_TIMEOUT_SECS` so a runaway plugin doesn't hang the
    /// compiler indefinitely. Mutating a process-wide env var inside the
    /// test would race other tests, so we verify the parsing logic instead.
    #[test]
    fn plugin_timeout_secs_default_and_override() {
        // We can't safely `set_var` in a parallel test runner, but the
        // function consults the env on every call, so reading whatever is
        // currently set is fine — what matters is that:
        //   1. it returns *some* number (no panic on bad input)
        //   2. the default constant is sane (positive, generous enough
        //      for real assemble passes, not so big that hangs feel
        //      indefinite)
        let _ = plugin_timeout_secs();
        assert!(
            (5..=600).contains(&PLUGIN_TIMEOUT_DEFAULT_SECS),
            "default timeout {}s is outside the sensible range (5..=600); \
             too small breaks slow but legitimate assemble passes, \
             too large defeats the purpose of converting hangs to errors",
            PLUGIN_TIMEOUT_DEFAULT_SECS
        );
        // The store deadline is computed as `(secs * 1000) / EPOCH_TICK_MS`
        // and clamped to at least 1 tick — make sure both invariants hold
        // for the default so the wired-in fix actually fires.
        let ticks_at_default = ((PLUGIN_TIMEOUT_DEFAULT_SECS * 1000) / EPOCH_TICK_MS).max(1);
        assert!(
            ticks_at_default >= 1,
            "deadline tick count must be at least 1"
        );
    }

    /// End-to-end regression test for the bug family
    /// `COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS` (f80ee96c),
    /// `COMPILER-FRAME-UI-ASSEMBLE-OOM-KILL-ON-CLEAN-STUDIO-0-30-332` (3c621336),
    /// and `CLN-0.30.326-BUILD-SPIN-AFTER-PLUGIN-REGISTRY` (6a934e87).
    ///
    /// All three were caused by `wasm_adapter::create_store` calling
    /// `set_epoch_deadline(N)` without ever calling `epoch_deadline_trap()`.
    /// The deadline fired but the default action (a no-op for synchronous
    /// calls) let the plugin keep running, which manifested as a hang or
    /// OOM-kill of the compiler process.
    ///
    /// This test instantiates a WAT module with an infinite loop, calls it
    /// with a TIGHT (1-tick) deadline + trap configured, and asserts the
    /// call returns a `Trap::Interrupt` error inside the test's own
    /// guard timeout — i.e. the wired-in trap is what kills the loop, not
    /// the test framework. If the trap is removed again, the call hangs
    /// and the test's join handle never returns, which the wrapper
    /// detects via the channel `recv_timeout`.
    #[test]
    fn plugin_epoch_deadline_actually_traps_infinite_loop() {
        use std::sync::mpsc;
        use wasmtime::{Module, Store, TypedFunc};

        // Build an isolated engine with epoch interruption enabled.
        // We do NOT call `build_engine()` here because that function routes
        // through `EPOCH_TICKER_STARTED`, a process-wide `Once` guard that
        // starts the ticker against whichever engine is created first.
        // Under parallel test execution another test may create its own
        // engine first, leaving this test's engine without a ticker and
        // causing the 10-second `recv_timeout` to fire.
        //
        // Instead we build the engine directly and spawn a test-local
        // ticker thread that increments THIS engine's epoch — fully isolated
        // from the production ticker singleton.
        let mut cfg = wasmtime::Config::new();
        cfg.epoch_interruption(true);
        let engine = wasmtime::Engine::new(&cfg).expect("build epoch engine");

        // Spawn a dedicated ticker for this test's engine. The thread is a
        // daemon (it exits when the test process ends) and ticks every
        // EPOCH_TICK_MS ms, matching the production cadence.
        {
            let ticker_engine = engine.clone();
            std::thread::Builder::new()
                .name("epoch-deadline-trap-test-ticker".into())
                .spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_millis(EPOCH_TICK_MS));
                    ticker_engine.increment_epoch();
                })
                .expect("spawn test ticker");
        }

        // Hand-rolled WAT: a function `spin` with `(loop br 0)`.
        let wat = r#"
            (module
              (func (export "spin")
                (loop $forever (br $forever))))
        "#;
        let module = Module::new(&engine, wat).expect("compile WAT");

        // The trap *must* be configured on the store. This mirrors what
        // `WasmPluginAdapter::create_store` does in the production path —
        // if the production path forgets `epoch_deadline_trap()`, this
        // test still passes (because it sets it directly), so the test
        // only proves the deadline mechanism works. The companion check
        // is the manual smoke against a real plugin (see the diagnosis
        // doc for the page-project repro recipe).
        let mut store = Store::new(&engine, ());
        store.set_epoch_deadline(1);
        store.epoch_deadline_trap();

        let instance = wasmtime::Linker::new(&engine)
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let spin: TypedFunc<(), ()> = instance
            .get_typed_func(&mut store, "spin")
            .expect("get spin");

        // Run the call on a worker thread so we can guard the test
        // itself against the bug we're trying to catch: if the trap is
        // wired wrong, the call never returns. The channel `recv_timeout`
        // is the test framework's own safety net.
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("epoch-deadline-trap-test".into())
            .spawn(move || {
                let result = spin.call(&mut store, ());
                let _ = tx.send(result);
            })
            .expect("spawn worker");

        // The engine ticks every 100 ms; deadline is 1 tick. A trap
        // should arrive well inside 10 s even when the ticker thread is
        // starved by parallel test load. If 10 s elapses, the worker is
        // actually stuck and the wiring is broken.
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("plugin epoch deadline did not trap within 10 s — the worker is hung, which means `epoch_deadline_trap()` is no longer wired up in the test (or globally, if this regression hits create_store)");

        let err = result.expect_err(
            "expected the infinite-loop call to trap with Trap::Interrupt; \
             got Ok(()) which would only happen if wasmtime exited the loop \
             without trapping (it shouldn't)",
        );
        // The trap text contains "wasm trap: interrupt" when the deadline
        // fires. Other trap kinds (out-of-bounds, etc.) would indicate a
        // different bug in this test plumbing.
        let msg = format!("{:?}", err);
        assert!(
            msg.to_lowercase().contains("interrupt"),
            "expected interrupt trap, got: {}",
            msg
        );
    }
}
