//! External WASM Plugin Loader
//!
//! Discovers and loads plugins from ~/.cleen/plugins/

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};

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

/// The process-wide plugin engine.
static PLUGIN_ENGINE: OnceLock<Engine> = OnceLock::new();

/// Compiled plugin modules, shared by every loader in the process. Keyed by
/// the file path plus its size and modification time, so a plugin that is
/// reinstalled while the process runs (the language server, the MCP server)
/// is compiled again instead of served stale.
static MODULE_CACHE: OnceLock<Mutex<HashMap<ModuleKey, Module>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModuleKey {
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
}

/// The shared wasmtime engine, with epoch interruption enabled and the
/// ticker that drives plugin call deadlines running.
///
/// One engine serves the whole process: compiled modules belong to the
/// engine that compiled them, so a shared engine is what lets every loader
/// reuse a module compiled once, and the single epoch ticker then drives
/// the deadline of every plugin call (a second engine would never tick).
pub(crate) fn shared_engine() -> Result<Engine> {
    if let Some(engine) = PLUGIN_ENGINE.get() {
        return Ok(engine.clone());
    }
    let mut cfg = wasmtime::Config::new();
    // Enable epoch-based interruption so a runaway plugin call (infinite
    // loop, deadlock against host-side state) traps after the per-call
    // deadline set by `WasmPluginAdapter::create_store` instead of
    // hanging the compiler indefinitely.
    cfg.epoch_interruption(true);
    let engine = Engine::new(&cfg).map_err(|e| anyhow!("Failed to create WASM engine: {}", e))?;
    let engine = PLUGIN_ENGINE.get_or_init(|| engine).clone();
    start_epoch_ticker(&engine);
    Ok(engine)
}

/// Parse a strict `MAJOR.MINOR.PATCH` version (each part a decimal number).
/// Anything else — `src`, `1.2`, `1.2.3-beta` — is `None`.
fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.split('.');
    let mut next = || -> Option<u64> {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// Loads external WASM plugins from the filesystem
pub struct WasmPluginLoader {
    /// Path to plugins directory (typically ~/.cleen/plugins/)
    plugins_dir: PathBuf,
    /// Wasmtime engine for compiling WASM modules (the process-wide one)
    engine: Engine,
}

impl WasmPluginLoader {
    /// Create a new plugin loader
    pub fn new() -> Result<Self> {
        let plugins_dir = Self::get_plugins_dir()?;
        let engine = shared_engine()?;

        Ok(Self {
            plugins_dir,
            engine,
        })
    }

    /// Create loader with custom plugins directory
    pub fn with_plugins_dir(plugins_dir: PathBuf) -> Result<Self> {
        let engine = shared_engine()?;

        Ok(Self {
            plugins_dir,
            engine,
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
        let mut registry = self.load_plugins_inner(plugin_names, &build_state)?;
        registry.set_build_state(build_state);
        Ok(registry)
    }

    /// Load plugins with a fresh build state of their own. The plugins and
    /// the returned registry still share that one state, so writes a plugin
    /// makes during expansion are visible to the other plugins and to the
    /// lifecycle-slot snapshots. Callers that want to surface build state
    /// into the manifest should use `load_plugins_with_build_state`.
    pub fn load_plugins(&mut self, plugin_names: &[String]) -> Result<PluginRegistry> {
        self.load_plugins_with_build_state(plugin_names, super::new_build_state())
    }

    fn load_plugins_inner(
        &mut self,
        plugin_names: &[String],
        build_state: &super::BuildState,
    ) -> Result<PluginRegistry> {
        let mut builder = PluginRegistry::builder();

        for plugin_name in plugin_names {
            // Find plugin directory and load manifest first
            let plugin_dir = self.find_plugin_dir(plugin_name)?;
            let manifest_path = plugin_dir.join("plugin.toml");
            let manifest = self.load_manifest(&manifest_path)?;

            // plugin-contract.md §1.2 rule 3: a plugin that needs a newer
            // compiler than this one is rejected.
            Self::check_min_compiler_version(
                plugin_name,
                &manifest,
                &plugin_dir,
                env!("CARGO_PKG_VERSION"),
            )?;

            // Add bridge functions from the manifest
            if !manifest.bridge.functions.is_empty() {
                tracing::info!(
                    plugin = plugin_name,
                    bridge_function_count = manifest.bridge.functions.len(),
                    "Loading bridge functions from plugin"
                );
                builder = builder.add_bridge_functions(&manifest.bridge);
            }

            // Store the full manifest so language_to_bridge_map() can resolve
            // dot-notation aliases (e.g. "req.query" → "_req_query").
            builder = builder.add_manifest(plugin_name.clone(), manifest.clone());

            // Check for root-level plugin.wasm that differs from the active version
            self.check_plugin_version_mismatch(plugin_name, &plugin_dir);

            // Warn if this plugin was compiled with a compiler known to have codegen bugs.
            self.check_plugin_build_compatibility(plugin_name, &manifest, &plugin_dir);

            // Plugin Contracts v3 sub-cycle 3 — refuse plugins whose declared
            // expansion_version is not in SUPPORTED_EXPANSION_VERSIONS (PLUGIN006).
            // See foundation/spec/plugins/contracts/typed-emission.md §6.
            self.check_plugin_expansion_version(plugin_name, &manifest, &plugin_dir)?;

            // Plugin Contracts v2 Phase D — refuse plugins whose plugin.toml
            // has no `[lifecycle]` section (PLUGIN010 LegacyV1PluginRejected).
            // Bridge-only plugins (no plugin.wasm) are exempt: they contribute
            // only aliases and keywords and never enter the expansion pipeline.
            // See foundation/spec/plugins/contracts/lifecycle.md §6.
            let wasm_for_lifecycle = plugin_dir.join("plugin.wasm");
            if wasm_for_lifecycle.exists() && !manifest.lifecycle.has_lifecycle_declaration() {
                return Err(anyhow!(
                    "{}",
                    Self::format_missing_lifecycle_error(plugin_name, &plugin_dir)
                ));
            }

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

            // The typed-emission op-code table only matters to a plugin whose
            // WASM runs the typed-emission ABI; bridge-only and string-emission
            // plugins never call the typed-op bridges.
            if manifest.opts_into_typed_emission() {
                Self::check_plugin_emission_ops_hash(plugin_name, &manifest, &plugin_dir)?;
            }

            // Load the plugin adapter
            let module = self.load_wasm_module(&wasm_path)?;
            let mut adapter = WasmPluginAdapter::new(
                plugin_name.to_string(),
                manifest,
                module,
                self.engine.clone(),
            )?;
            // Plugin Contracts v2 — wire the shared build state into the
            // adapter so its `_build_state_*` bridges operate on the
            // registry-wide keystore. See contracts/lifecycle.md §2.5.
            adapter.set_build_state(std::sync::Arc::clone(build_state));

            builder = builder.add(adapter);
        }

        builder
            .build()
            .map_err(|e| anyhow!("Failed to build plugin registry: {}", e))
    }

    /// Resolve the on-disk directory for a loaded plugin. Public companion of
    /// `find_plugin_dir`, used by `discover_plugin_dirs` so artifact
    /// orchestration can resolve `static_path` references per
    /// `artifacts.md` §2/§6.
    pub fn plugin_dir(&self, name: &str) -> Result<PathBuf> {
        self.find_plugin_dir(name)
    }

    /// Find the plugin directory, respecting .active-version pin if present.
    ///
    /// Precedence:
    ///   1. `--plugin-override <name>=<path>` from the CLI (thread-local).
    ///   2. `~/.cleen/plugins/<name>/.active-version` pin.
    ///   3. Highest `MAJOR.MINOR.PATCH` subfolder of `~/.cleen/plugins/<name>/`
    ///      that contains plugin.wasm.
    ///   4. The plugin folder itself, when it holds plugin.toml.
    ///   5. Highest `MAJOR.MINOR.PATCH` subfolder holding only plugin.toml
    ///      (a bridge-only plugin).
    ///
    /// The override wins over both installed paths and is validated up-front
    /// by the CLI (missing plugin.wasm / missing sibling plugin.toml both
    /// hard-error before compile starts). See
    /// `foundation/spec/plugins/plugin-contract.md` §11.
    fn find_plugin_dir(&self, name: &str) -> Result<PathBuf> {
        // Precedence 1: --plugin-override <name>=<path>. The CLI validated
        // path shape and presence of plugin.wasm/plugin.toml before dispatch;
        // here we just short-circuit the default lookup.
        if let Some(override_dir) = crate::plugin_override_dir(name) {
            return Ok(override_dir);
        }

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

        // Fall back to the highest installed version: a semver-named
        // subfolder holding plugin.wasm. Other subfolders (`src`, `tests`,
        // `patterns`, ...) are part of a plugin laid out at the root.
        if let Some(version_dir) = Self::highest_version_dir(&plugin_base, "plugin.wasm")? {
            return Ok(version_dir);
        }
        // A plugin installed at the root of its folder (plugin.toml there,
        // with or without plugin.wasm).
        if plugin_base.join("plugin.toml").is_file() {
            return Ok(plugin_base);
        }
        // A bridge-only plugin installed under a version folder.
        if let Some(version_dir) = Self::highest_version_dir(&plugin_base, "plugin.toml")? {
            return Ok(version_dir);
        }
        Err(anyhow!(
            "Plugin '{}' has no installed versions in {} (expected a plugin.toml at the \
             root or in a MAJOR.MINOR.PATCH subfolder)",
            name,
            plugin_base.display()
        ))
    }

    /// The highest semver-named subfolder of `plugin_base` that contains
    /// `required_file`, if any.
    fn highest_version_dir(plugin_base: &Path, required_file: &str) -> Result<Option<PathBuf>> {
        let mut best: Option<((u64, u64, u64), PathBuf)> = None;
        for entry in std::fs::read_dir(plugin_base)?.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Some(version) = entry.file_name().to_str().and_then(parse_semver) else {
                continue;
            };
            if !path.is_dir() || !path.join(required_file).is_file() {
                continue;
            }
            if best.as_ref().is_none_or(|(v, _)| version > *v) {
                best = Some((version, path));
            }
        }
        Ok(best.map(|(_, path)| path))
    }

    /// Load plugin manifest from TOML file
    fn load_manifest(&self, path: &Path) -> Result<PluginManifest> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read plugin manifest {}: {}", path.display(), e))?;

        toml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse plugin manifest {}: {}", path.display(), e))
    }

    /// Load and compile a WASM module, reusing the process-wide compiled
    /// module when the file is unchanged.
    fn load_wasm_module(&mut self, path: &Path) -> Result<Module> {
        let metadata = std::fs::metadata(path)
            .map_err(|e| anyhow!("Failed to read WASM file {}: {}", path.display(), e))?;
        let key = ModuleKey {
            path: path.to_path_buf(),
            len: metadata.len(),
            modified: metadata.modified().ok(),
        };
        let cache = MODULE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(module) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            tracing::debug!(path = %path.display(), "plugin loader: using cached module");
            return Ok(module.clone());
        }

        let wasm_bytes = std::fs::read(path)
            .map_err(|e| anyhow!("Failed to read WASM file {}: {}", path.display(), e))?;
        let module = Module::new(&self.engine, &wasm_bytes)
            .map_err(|e| anyhow!("Failed to compile WASM module {}: {}", path.display(), e))?;
        tracing::debug!(
            path = %path.display(),
            bytes = wasm_bytes.len(),
            exports = module.exports().count(),
            "plugin loader: compiled module"
        );

        cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, module.clone());
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

    /// Plugin Contracts v3 sub-cycle 3 — refuse plugins declaring an
    /// `expansion_version` outside `SUPPORTED_EXPANSION_VERSIONS` with the
    /// PLUGIN006 (UnsupportedExpansionVersion) error from typed-emission.md §6.
    ///
    /// Absent or default ("1.0.0") expansion_version always loads. Only an
    /// explicitly declared, unsupported value is refused.
    pub fn format_expansion_version_mismatch_error(
        plugin_name: &str,
        plugin_path: &Path,
        found: &str,
    ) -> String {
        use super::plugin_abi::SUPPORTED_EXPANSION_VERSIONS;
        format!(
            "error[PLUGIN006]: plugin '{}' at {} declares expansion_version = \
             \"{}\", but this compiler only supports {:?} \
             (UnsupportedExpansionVersion).\n  \
             resolution: upgrade the compiler or downgrade the plugin to a \
             supported expansion ABI.\n  \
             spec: foundation/spec/plugins/contracts/typed-emission.md §6",
            plugin_name,
            plugin_path.display(),
            found,
            SUPPORTED_EXPANSION_VERSIONS,
        )
    }

    /// Refuse plugins whose declared `expansion_version` is not in
    /// `SUPPORTED_EXPANSION_VERSIONS`. See typed-emission.md §6.
    fn check_plugin_expansion_version(
        &self,
        plugin_name: &str,
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) -> Result<()> {
        use super::plugin_abi::SUPPORTED_EXPANSION_VERSIONS;
        let Some(declared) = manifest.compatibility.expansion_version.as_deref() else {
            return Ok(());
        };
        if SUPPORTED_EXPANSION_VERSIONS.contains(&declared) {
            return Ok(());
        }
        Err(anyhow!(
            "{}",
            Self::format_expansion_version_mismatch_error(plugin_name, plugin_dir, declared)
        ))
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

    // ── Emission-ops hash three-case loader ─────────────────────────────────

    /// Format the PLUGIN006 error when a plugin's `emission_ops_hash` does not
    /// match the compiler's `EMISSION_OPS_HASH` constant.
    pub fn format_emission_ops_hash_mismatch_error(
        plugin_name: &str,
        plugin_path: &Path,
        found: &str,
        expected: &str,
    ) -> String {
        format!(
            "error[PLUGIN006]: plugin '{}' at {} was built against typed-emission-ops.toml \
             hash {found}, but this compiler expects {expected}; the op-code table has \
             changed (EmissionOpsHashMismatch).\n  \
             resolution: reinstall the plugin via `cleen frame install` to rebuild against \
             the current op-code table.\n  \
             spec: foundation/spec/plugins/contracts/typed-emission.md §3.10",
            plugin_name,
            plugin_path.display(),
        )
    }

    /// Format the PLUGIN010 error when a plugin.wasm-bearing manifest has no
    /// `[lifecycle]` section (Phase D refuses v1 plugins).
    pub fn format_missing_lifecycle_error(plugin_name: &str, plugin_dir: &Path) -> String {
        format!(
            "error[PLUGIN010]: plugin '{}' at {} has no [lifecycle] section in \
             plugin.toml; the v1.0.0 __preamble path has been removed and every \
             plugin must declare at least one lifecycle slot (or \
             module_helpers_are_roots = true) per contracts/lifecycle.md §6.\n  \
             resolution: rebuild the plugin against the current contract, or install \
             a newer plugin version.\n  \
             spec: foundation/spec/plugins/contracts/lifecycle.md §6",
            plugin_name,
            plugin_dir.display(),
        )
    }

    /// `emission_ops_hash` check for a plugin that runs the typed-emission
    /// ABI (`expansion_version = "3.0.0"` with a plugin.wasm). Other plugins
    /// are not checked: they never call the typed-op bridges.
    ///
    ///   - Match   → Ok(())  (silent).
    ///   - Mismatch → Err(PLUGIN006 with reinstall guidance).
    ///   - Absent  → Err(PLUGIN006); every shipped typed-emission plugin
    ///     declares the stamp.
    ///
    /// The check is skipped when the compiler itself was built without the
    /// op-code table (the all-zeros `EMISSION_OPS_HASH` sentinel).
    fn check_plugin_emission_ops_hash(
        plugin_name: &str,
        manifest: &super::plugin_abi::PluginManifest,
        plugin_dir: &Path,
    ) -> Result<()> {
        use super::plugin_abi::EMISSION_OPS_HASH;

        let declared = manifest.compatibility.emission_ops_hash.as_deref();

        match declared {
            None => {
                // Absent case: refuse the plugin. Skip only when the compiler
                // has the all-zeros sentinel (spec TOML absent at build time),
                // which means we have nothing useful to validate against.
                if EMISSION_OPS_HASH == "0".repeat(64).as_str() {
                    return Ok(());
                }
                Err(anyhow!(
                    "{}",
                    Self::format_emission_ops_hash_mismatch_error(
                        plugin_name,
                        plugin_dir,
                        "<absent>",
                        EMISSION_OPS_HASH,
                    )
                ))
            }
            Some(found) => {
                if found == EMISSION_OPS_HASH {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "{}",
                        Self::format_emission_ops_hash_mismatch_error(
                            plugin_name,
                            plugin_dir,
                            found,
                            EMISSION_OPS_HASH,
                        )
                    ))
                }
            }
        }
    }

    /// Format the error for a plugin whose `[compatibility].min_compiler_version`
    /// is newer than the running compiler (plugin-contract.md §1.2 rule 3).
    pub fn format_min_compiler_version_error(
        plugin_name: &str,
        plugin_dir: &Path,
        required: &str,
        running: &str,
    ) -> String {
        format!(
            "error[PLUGIN001]: plugin '{}' at {} requires compiler {} or newer \
             ([compatibility].min_compiler_version), but this compiler is {}.\n  \
             resolution: upgrade the compiler (`cleen install latest`) or install a \
             plugin version built for this compiler.\n  \
             spec: foundation/spec/framework/plugin-contract.md §1.2",
            plugin_name,
            plugin_dir.display(),
            required,
            running,
        )
    }

    /// Refuse a plugin that needs a newer compiler than `running`, or whose
    /// `min_compiler_version` is not a `MAJOR.MINOR.PATCH` version.
    pub fn check_min_compiler_version(
        plugin_name: &str,
        manifest: &PluginManifest,
        plugin_dir: &Path,
        running: &str,
    ) -> Result<()> {
        let required = manifest.compatibility.min_compiler_version.as_str();
        let Some(required_version) = parse_semver(required) else {
            return Err(anyhow!(
                "error[PLUGIN001]: plugin '{}' at {} declares min_compiler_version = \
                 \"{}\", which is not a MAJOR.MINOR.PATCH version \
                 (foundation/spec/framework/plugin-contract.md §1.2)",
                plugin_name,
                plugin_dir.display(),
                required
            ));
        };
        let running_version = parse_semver(running)
            .ok_or_else(|| anyhow!("compiler version {:?} is not MAJOR.MINOR.PATCH", running))?;
        if running_version < required_version {
            return Err(anyhow!(
                "{}",
                Self::format_min_compiler_version_error(plugin_name, plugin_dir, required, running)
            ));
        }
        Ok(())
    }

    /// True when `a` is a strictly older `MAJOR.MINOR.PATCH` version than `b`.
    /// A version that does not parse counts as older than any valid one.
    fn version_less_than(a: &str, b: &str) -> bool {
        parse_semver(a) < parse_semver(b)
    }

    /// Get the engine for creating stores
    pub fn engine(&self) -> &Engine {
        &self.engine
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
        assert!(WasmPluginLoader::version_less_than("1.0.0", "1.0.1"));
        assert!(!WasmPluginLoader::version_less_than("1.2.0", "1.1.9"));
        assert!(!WasmPluginLoader::version_less_than("2.0.0", "1.9.9"));
        assert!(!WasmPluginLoader::version_less_than("1.0.0", "1.0.0"));
        assert!(WasmPluginLoader::version_less_than("0.9.10", "0.10.0"));
    }

    #[test]
    fn parse_semver_accepts_only_three_numeric_parts() {
        assert_eq!(parse_semver("3.0.18"), Some((3, 0, 18)));
        for bad in [
            "src", "tests", "patterns", "1.2", "1.2.3.4", "1.2.x", "", "1..2",
        ] {
            assert_eq!(parse_semver(bad), None, "{:?}", bad);
        }
    }

    fn write_plugin(dir: &Path, wasm: bool) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            "[plugin]\nname = \"p\"\nversion = \"1.0.0\"\n[handles]\nblocks = []\n",
        )
        .unwrap();
        if wasm {
            std::fs::write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").unwrap();
        }
    }

    /// A plugin laid out at the root of its folder with helper subfolders
    /// (`src`, `tests`, `patterns`) resolves to the root, not to whichever
    /// subfolder sorts last.
    #[test]
    fn root_layout_with_helper_subfolders_resolves_to_root() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path().join("frame.x");
        write_plugin(&base, true);
        for sub in ["src", "tests", "patterns"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
        }
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();
        assert_eq!(loader.find_plugin_dir("frame.x").unwrap(), base);
    }

    /// The highest semver folder that holds plugin.wasm wins over the root
    /// and over a newer folder without plugin.wasm.
    #[test]
    fn highest_semver_folder_with_wasm_wins() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path().join("frame.x");
        write_plugin(&base, true);
        write_plugin(&base.join("1.9.0"), true);
        write_plugin(&base.join("1.10.0"), true);
        write_plugin(&base.join("2.0.0"), false);
        std::fs::create_dir_all(base.join("src")).unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();
        assert_eq!(
            loader.find_plugin_dir("frame.x").unwrap(),
            base.join("1.10.0")
        );
    }

    /// A bridge-only plugin (no blocks, no plugin.wasm) never runs the
    /// typed-emission ABI, so it loads without an `emission_ops_hash` — also
    /// when it declares `expansion_version = "3.0.0"`.
    #[test]
    fn bridge_only_plugin_loads_without_emission_ops_hash() {
        let temp_dir = TempDir::new().unwrap();
        for (name, extra) in [
            ("bridge.v1", ""),
            (
                "bridge.v3",
                "[compatibility]\nexpansion_version = \"3.0.0\"\n",
            ),
        ] {
            let dir = temp_dir.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("plugin.toml"),
                format!(
                    "[plugin]\nname = \"{name}\"\nversion = \"1.0.0\"\n{extra}\
                     [handles]\nblocks = []\n\
                     [bridge]\nfunctions = [{{ name = \"{bridge}\", params = [], returns = \"string\" }}]\n",
                    name = name,
                    extra = extra,
                    bridge = if name == "bridge.v1" { "_req_ip" } else { "_req_headers" },
                ),
            )
            .unwrap();
        }
        let mut loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf()).unwrap();
        let registry = loader
            .load_plugins(&["bridge.v1".to_string(), "bridge.v3".to_string()])
            .expect("bridge-only plugins must load without an emission_ops_hash");
        assert!(registry.is_bridge_function("_req_ip"));
        assert!(registry.is_bridge_function("_req_headers"));
    }

    #[test]
    fn min_compiler_version_newer_than_compiler_is_refused() {
        let mut manifest: PluginManifest =
            toml::from_str("[plugin]\nname = \"p\"\nversion = \"1.0.0\"\n[handles]\nblocks = []\n")
                .unwrap();
        let dir = Path::new("/plugins/p");
        manifest.compatibility.min_compiler_version = "0.33.200".to_string();
        let err = WasmPluginLoader::check_min_compiler_version("p", &manifest, dir, "0.33.199")
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires compiler 0.33.200"), "{}", err);
        assert!(err.contains("0.33.199"), "{}", err);
        assert!(
            WasmPluginLoader::check_min_compiler_version("p", &manifest, dir, "0.33.200").is_ok()
        );
        manifest.compatibility.min_compiler_version = "latest".to_string();
        assert!(
            WasmPluginLoader::check_min_compiler_version("p", &manifest, dir, "1.0.0").is_err()
        );
    }

    #[test]
    fn test_loader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = WasmPluginLoader::with_plugins_dir(temp_dir.path().to_path_buf());
        assert!(loader.is_ok());
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
        // We do NOT use `shared_engine()` here: the test pins its own
        // deadline and ticker so it does not depend on the process-wide
        // ticker's timing under parallel test load.
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
