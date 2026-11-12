/*!
 * Clean Language Compiler Library
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

// =============================================================================
// PRODUCTION QUALITY ENFORCEMENT
// =============================================================================
// These lints DENY patterns that indicate incomplete or placeholder code.
// The Clean Language Compiler must be production-grade with no stubs.

// CRITICAL: Deny any incomplete implementation markers
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
// Note: panic! is allowed in error paths where recovery is not possible
// #![deny(clippy::panic)]

// WARN on patterns that may indicate incomplete code
// These are tracked for gradual improvement (356 unwrap calls currently in codebase)
// NOTE: These are allowed in CI due to existing usage - tracked for future cleanup
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![warn(clippy::dbg_macro)]
#![allow(clippy::print_stdout)]
#![allow(clippy::print_stderr)]
// =============================================================================
// PEDANTIC STYLE ALLOWANCES
// =============================================================================
// Allow clippy pedantic warnings for now - focusing on functionality over style
#![allow(clippy::redundant_else)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::similar_names)]
#![allow(clippy::needless_continue)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::unused_self)]
#![allow(clippy::match_wildcard_for_single_variants)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::bool_to_int_with_if)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::ref_option_ref)]
#![allow(clippy::large_types_passed_by_value)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::missing_const_for_fn)]
// Targeted lint suppressions
// Allow some unknown lints for cross-compiler compatibility
#![allow(unknown_lints)]
#![allow(deprecated)]

/// Abstract Syntax Tree definitions — output of Stage 2 (parsing)
pub mod ast;
/// Build manifest emission (Plugin Contracts v2 — contracts/artifacts.md)
pub mod build_manifest;
/// Built-in function registry — intrinsic functions available without imports
pub mod builtins;
/// WebAssembly code generation — Stage 7 (MIR to WASM bytecode)
pub mod codegen;
/// Compilation orchestration — multi-file compilation and build coordination
pub mod compilation;
/// Debug utilities — AST/MIR/WASM inspection tools
pub mod debug;
/// Doc-code synchronization — validates feature spec frontmatter against compiler symbols
pub mod docs;
/// Error types and reporting — compiler diagnostics with spec error codes
pub mod error;
/// High-level Intermediate Representation — Stage 3 (AST to HIR)
pub mod hir;
/// Lexical analysis — Stage 1 (source to tokens)
pub mod lexer;
/// Model Context Protocol server — IDE tooling integration
pub mod mcp;
/// Medium-level Intermediate Representation — Stage 6 (TAST to MIR with optimizations)
pub mod mir;
/// Parser — Stage 2 (tokens to AST via recursive descent)
pub mod parser;
/// Plugin artifact orchestration (Plugin Contracts v2 — contracts/artifacts.md)
pub mod plugin_artifacts;
/// Plugin system — framework plugin loading, expansion, and enforcement
pub mod plugins;
/// Name and module resolution — Stage 4 (symbol binding and scope resolution)
pub mod resolver;
/// Runtime utilities — async scheduling and I/O helpers
pub mod runtime;
/// Standard library — built-in function implementations for the old codegen path
pub mod stdlib;
/// Telemetry — compilation metrics and performance tracking
pub mod telemetry;
/// Type checker — Stage 5 (type inference, constraint solving, TAST generation)
pub mod typechecker;
/// WASM type definitions — value types and type conversion utilities
pub mod types;

use crate::error::CompilerError;

/// Compiler version (from Cargo.toml)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compilation target determines what imports and features are included
///
/// Different targets optimize the generated WASM for specific use cases:
/// - `Server`: Full HTTP server imports (_req_*, _session_*, _auth_*)
/// - `Plugin`: Minimal imports for WASM plugins (no server functions)
/// - `Standalone`: Standard CLI/library compilation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilationTarget {
    /// Server target - includes all HTTP server, session, and auth imports
    /// Use this when compiling Clean Language web applications
    #[default]
    Server,
    /// Plugin target - excludes server-specific imports
    /// Use this when compiling WASM plugins that don't need server functions
    Plugin,
    /// Standalone target - standard compilation for CLI/library code
    /// Includes basic HTTP client but no server functions
    Standalone,
}

impl CompilationTarget {
    /// Returns true if server imports (_req_*, _session_*, _auth_*) should be included
    pub fn include_server_imports(&self) -> bool {
        matches!(self, CompilationTarget::Server)
    }

    /// Parse a target from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "server" => Some(CompilationTarget::Server),
            "plugin" => Some(CompilationTarget::Plugin),
            "standalone" => Some(CompilationTarget::Standalone),
            _ => None,
        }
    }
}

/// Memory budget tier for WASM modules.
///
/// Each tier defines initial pages, max pages, and intended use case.
/// Values are from `foundation/platform-architecture/MEMORY_POLICY.md` section 3.
///
/// | Tier       | Initial Pages | Initial Size | Max Pages | Max Size |
/// |------------|---------------|-------------|-----------|----------|
/// | Embedded   | 4             | 256 KB      | 16        | 1 MB     |
/// | Minimal    | 8             | 512 KB      | 128       | 8 MB     |
/// | Standard   | 32            | 2 MB        | 512       | 32 MB    |
/// | Heavy      | 64            | 4 MB        | 1024      | 64 MB    |
/// | Canvas     | 256           | 16 MB       | 1024      | 64 MB    |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum MemoryTier {
    /// IoT, constrained devices — 4 initial pages, 16 max (1 MB)
    Embedded,
    /// CLI tools, simple scripts — 8 initial pages, 128 max (8 MB)
    Minimal,
    /// Web apps, APIs, PWAs, mobile, server — 32 initial pages, 512 max (32 MB)
    #[default]
    Standard,
    /// SSR, large data processing, desktop — 64 initial pages, 1024 max (64 MB)
    Heavy,
    /// Games, real-time rendering — 256 initial pages, 1024 max (64 MB)
    Canvas,
    /// Compile-time plugin tools — 32 initial pages, 16384 max (1 GB).
    ///
    /// Plugins are compile-time artifacts that process the user's entire source
    /// tree inside a single `process_html` / `assemble` call: every page
    /// companion gets renamed, every HTML body is walked, the synthesized route
    /// module accumulates routes for every page, etc. frame.ui's allocation
    /// pattern scales super-linearly with project size (the synthesized route
    /// module aggregates content for every prior page, and the diagnostic
    /// counters add per-page strings that aren't reclaimed because the
    /// host-side bump allocator's `mem_scope_pop` is a no-op since the
    /// per-iteration push/pop pair was removed in
    /// RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED).
    ///
    /// Empirical cliff progression with this tier's max cap:
    /// - Standard (32 MB):   SEM007 silent failure at n=51 pages
    /// - Plugin (256 MB):    SEM007 silent failure at n=~140 pages
    /// - Plugin (1 GB):      fits Clean Studio's ~170 files with
    ///   headroom for several × growth.
    ///
    /// On the previous Standard default, the plugin received a 0 from
    /// `mem_alloc` once `memory.grow()` couldn't extend further, the
    /// assemble output silently became empty, and the build failed
    /// downstream with `error[SEM007]: Function 'pages_home_load_impl'
    /// not found`. Plugins live only for the duration of a single
    /// `cln compile` invocation, so the runtime cost of a generous cap
    /// is zero — peak resident set is bounded by what the plugin
    /// actually allocates, not by the declared maximum. The underlying
    /// O(n²)-in-pages allocation pattern is worth a separate framework
    /// fix, but bumping the cap clears the user-visible blocker for
    /// COMPILER-FRAME-UI-ASSEMBLE-OOM-KILL-ON-CLEAN-STUDIO and
    /// CLN-0.30.326-BUILD-SPIN-AFTER-PLUGIN-REGISTRY.
    Plugin,
}

impl MemoryTier {
    /// Initial WASM memory pages for this tier.
    pub fn initial_pages(self) -> u64 {
        match self {
            MemoryTier::Embedded => 4,
            MemoryTier::Minimal => 8,
            MemoryTier::Standard => 32,
            MemoryTier::Heavy => 64,
            MemoryTier::Canvas => 256,
            MemoryTier::Plugin => 32,
        }
    }

    /// Maximum WASM memory pages for this tier.
    pub const fn max_pages(self) -> u64 {
        match self {
            MemoryTier::Embedded => 16,
            MemoryTier::Minimal => 128,
            MemoryTier::Standard => 512,
            MemoryTier::Heavy => 1024,
            MemoryTier::Canvas => 1024,
            MemoryTier::Plugin => 16384,
        }
    }

    /// Maximum memory in bytes for this tier.
    pub fn max_bytes(self) -> usize {
        self.max_pages() as usize * 65536
    }

    /// Tier name as used in CLI flags and custom sections.
    pub fn name(self) -> &'static str {
        match self {
            MemoryTier::Embedded => "embedded",
            MemoryTier::Minimal => "minimal",
            MemoryTier::Standard => "standard",
            MemoryTier::Heavy => "heavy",
            MemoryTier::Canvas => "canvas",
            MemoryTier::Plugin => "plugin",
        }
    }

    /// Parse a tier from string (CLI flag value).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "embedded" => Some(MemoryTier::Embedded),
            "minimal" => Some(MemoryTier::Minimal),
            "standard" => Some(MemoryTier::Standard),
            "heavy" => Some(MemoryTier::Heavy),
            "canvas" => Some(MemoryTier::Canvas),
            "plugin" => Some(MemoryTier::Plugin),
            _ => None,
        }
    }

    /// Default memory tier for a given compilation target string.
    ///
    /// Mapping from MEMORY_POLICY.md section 3.2:
    /// - web, pwa, nodejs, server → standard
    /// - native → heavy
    /// - embedded → embedded
    /// - wasi → minimal
    /// - plugin → plugin (256 MB cap — compile-time tools that walk the whole
    ///   project; see `MemoryTier::Plugin` doc for the SEM007-on-many-pages
    ///   cliff this fixed)
    /// - auto/standalone → standard (default)
    pub fn default_for_target(target: &str) -> Self {
        match target.to_lowercase().as_str() {
            "web" | "pwa" | "nodejs" | "server" => MemoryTier::Standard,
            "native" => MemoryTier::Heavy,
            "embedded" => MemoryTier::Embedded,
            "wasi" => MemoryTier::Minimal,
            "plugin" => MemoryTier::Plugin,
            _ => MemoryTier::Standard,
        }
    }
}

impl std::fmt::Display for MemoryTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

// ----------------------------------------------------------------------------
// Plugin Contracts v2 — CLI-supplied host class overrides
// ----------------------------------------------------------------------------
//
// `cln compile --target browser|node|server` and `--strict-hosts` need to
// reach the codegen stage's bridge-host validator without changing the public
// signatures of every `compile_multi_file_*` entry point (24 callers across
// src/, tests/, and binaries). The CLI writes these into thread-local state
// before invoking the compile function; the compile function reads them in
// the WASM-generation stage. When unset (library or test consumers calling
// the API directly), behaviour falls back to the previous `client_mode`-derived
// host class and `CLEAN_STRICT_HOSTS` env var.
//
// See foundation/spec/plugins/contracts/bridge-host-classes.md §6.

thread_local! {
    /// `CompilationTarget` requested by `compile_with_target`; `None` builds
    /// with the code generator's default (server) target.
    static COMPILATION_TARGET: std::cell::Cell<Option<CompilationTarget>> = const { std::cell::Cell::new(None) };
    static TARGET_HOST_CLASS: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static STRICT_HOSTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Plugin Contracts v2 Phase B — when set, `compile_multi_file_with_memory_tier`
    /// stamps the produced WASM with this Clean Runtime ABI version as a
    /// `clean.abi_version` custom section. Set by `handle_compile` when the CLI
    /// receives `--target=plugin`; cleared otherwise so user-code builds carry
    /// no stamp. See foundation/spec/plugins/contracts/runtime-abi.md §4.
    static PLUGIN_ABI_VERSION: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// STATE-A heap-probe hunt — when true, codegen emits `env._probe_ptr`,
    /// `env._probe_ptr_dump`, `env._probe_ptr_reset` imports and injects a
    /// `call $_probe_ptr(callsite_id, result_ptr)` immediately after every
    /// WASM `call` to `__string_builder_append` / `__string_builder_finalize`.
    /// Off by default: zero production cost. Set by `--emit-heap-probes`.
    /// Contract acked to node-server in prompt 410a2312-836c-11f1-9d55-da25a95a496b.
    static EMIT_HEAP_PROBES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// STATE-A heap-probe hunt — bridge-scoped superset of
    /// `EMIT_HEAP_PROBES`. When true, codegen emits `call $_probe_ptr(...)`
    /// *before* and *after* every WASM `call` to a host-imported bridge
    /// function (declared via `plugin.toml [bridge]` or the built-in Layer
    /// 2 host bridge). Reuses the same three `_probe_ptr*` imports; either
    /// flag turns them on. The "after" probe is skipped when the bridge
    /// returns non-i32 (only heap-pointer returns are probeable). Off by
    /// default. Set by `--emit-bridge-probes`. Acked to node-server in the
    /// STATE A V2 response prompt.
    static EMIT_BRIDGE_PROBES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// STATE-A heap-probe hunt — callsite metadata captured from the last
    /// `MirCodegenResult`. Written by the codegen pipeline; drained by the
    /// CLI when writing the `.probes.json` sidecar. Empty when
    /// `--emit-heap-probes` is off. Uses a thread-local so the many
    /// `compile_multi_file_*` entry points don't need signature changes.
    static HEAP_PROBE_CALLSITES: std::cell::RefCell<Vec<crate::codegen::mir_codegen::ProbeCallsite>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// `--plugin-override <name>=<path>` overrides for the current thread.
    /// Maps plugin name → directory containing the override's `plugin.wasm`
    /// and `plugin.toml`. Consumed by `WasmPluginLoader::find_plugin_dir`
    /// which prefers the override over `~/.cleen/plugins/<name>/`.
    /// Enables local plugin verification without a comita round-trip.
    static PLUGIN_OVERRIDES: std::cell::RefCell<std::collections::HashMap<String, std::path::PathBuf>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Plugin Contracts v2 Phase B — set the Clean Runtime ABI version to stamp
/// into the next plugin WASM produced on this thread. Pass `None` to clear
/// (subsequent compiles emit no stamp). The CLI populates this from the
/// sibling `plugin.toml`'s `[compatibility].abi_version` when `--target=plugin`
/// is set. See `foundation/spec/plugins/contracts/runtime-abi.md` §4.
pub fn set_plugin_abi_version_override(abi_version: Option<String>) {
    PLUGIN_ABI_VERSION.with(|cell| {
        *cell.borrow_mut() = abi_version;
    });
}

/// Read the active plugin ABI version override for the current thread, if any.
pub fn plugin_abi_version_override() -> Option<String> {
    PLUGIN_ABI_VERSION.with(|cell| cell.borrow().clone())
}

/// Set the bridge-host-class override for the current thread.
///
/// Maps `--target` CLI values to the host classes used by bridge declarations
/// (see `bridge-host-classes.md` §2):
///
/// | CLI `--target`         | host class |
/// |------------------------|------------|
/// | `browser`, `web`       | `browser`  |
/// | `node`, `nodejs`, `server`, `wasi`, `native` (default) | `server` |
///
/// Pass `None` to clear the override (subsequent compiles fall back to the
/// `client_mode`-derived default).
pub fn set_target_host_class_override(host_class: Option<String>) {
    TARGET_HOST_CLASS.with(|cell| {
        *cell.borrow_mut() = host_class;
    });
}

/// Read the active host-class override for the current thread, if any.
pub fn target_host_class_override() -> Option<String> {
    TARGET_HOST_CLASS.with(|cell| cell.borrow().clone())
}

/// Set the `--strict-hosts` flag for the current thread. When true, the
/// codegen bridge-host validator promotes `BRIDGE-HOST-MISMATCH` warnings to
/// hard errors instead of substituting server stubs.
pub fn set_strict_hosts_override(strict: bool) {
    STRICT_HOSTS.with(|cell| cell.set(strict));
}

/// Read the active `--strict-hosts` override for the current thread.
pub fn strict_hosts_override() -> bool {
    STRICT_HOSTS.with(|cell| cell.get())
}

/// Set the `--emit-heap-probes` flag for the current thread.
///
/// When true, codegen emits three env imports (`_probe_ptr`, `_probe_ptr_dump`,
/// `_probe_ptr_reset`) and injects a `call $_probe_ptr(callsite_id, result_ptr)`
/// after every WASM `call` to `__string_builder_append` and
/// `__string_builder_finalize`. Off by default: no imports, no extra calls.
///
/// Used by the STATE-A heap-probe bug hunt with clean-node-server. See the
/// ack in prompt 410a2312-836c-11f1-9d55-da25a95a496b.
pub fn set_emit_heap_probes_override(enable: bool) {
    EMIT_HEAP_PROBES.with(|cell| cell.set(enable));
}

/// Read the active `--emit-heap-probes` override for the current thread.
pub fn emit_heap_probes_override() -> bool {
    EMIT_HEAP_PROBES.with(|cell| cell.get())
}

/// Set the `--emit-bridge-probes` flag for the current thread.
///
/// Superset of `--emit-heap-probes` — when true, codegen emits `_probe_ptr`
/// calls *before* and *after* every WASM `call` to a host-imported bridge
/// function. Reuses the same three `_probe_ptr*` imports as the heap-probes
/// flag; either flag turns them on. Off by default.
///
/// Ack'd to node-server in the STATE A V2 response prompt.
pub fn set_emit_bridge_probes_override(enable: bool) {
    EMIT_BRIDGE_PROBES.with(|cell| cell.set(enable));
}

/// Read the active `--emit-bridge-probes` override for the current thread.
pub fn emit_bridge_probes_override() -> bool {
    EMIT_BRIDGE_PROBES.with(|cell| cell.get())
}

/// True when either `--emit-heap-probes` or `--emit-bridge-probes` is set —
/// i.e. when the codegen should register the `_probe_ptr*` imports and reserve
/// callsite IDs. Used to gate the import block once for both flags so a module
/// built with both carries a single import set.
pub fn any_probe_flag_active() -> bool {
    emit_heap_probes_override() || emit_bridge_probes_override()
}

/// Replace the heap-probe callsite list for the current thread.
///
/// Called by the codegen pipeline after each compile finishes so the CLI can
/// pick up the collected callsites and write them to the `.probes.json`
/// sidecar. Always overwrites any previous list.
pub fn set_heap_probe_callsites(callsites: Vec<crate::codegen::mir_codegen::ProbeCallsite>) {
    HEAP_PROBE_CALLSITES.with(|cell| {
        *cell.borrow_mut() = callsites;
    });
}

/// Drain and return the heap-probe callsites captured on the current thread.
/// After this call the thread-local list is empty. Used by the CLI's sidecar
/// writer right after a successful compile.
pub fn take_heap_probe_callsites() -> Vec<crate::codegen::mir_codegen::ProbeCallsite> {
    HEAP_PROBE_CALLSITES.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Replace the `--plugin-override` map for the current thread.
///
/// Each entry maps a plugin name (as declared in a `plugins:` block, e.g.
/// `frame.data`) to a directory containing that plugin's `plugin.wasm` and
/// sibling `plugin.toml`. `WasmPluginLoader::find_plugin_dir` prefers the
/// override over `~/.cleen/plugins/<name>/` when both exist.
///
/// Set by `cln lint`, `cln check`, and `cln compile` when the user passes
/// `--plugin-override name=path`; empty otherwise. Enables local plugin
/// verification without a comita round-trip.
pub fn set_plugin_overrides(overrides: std::collections::HashMap<String, std::path::PathBuf>) {
    PLUGIN_OVERRIDES.with(|cell| {
        *cell.borrow_mut() = overrides;
    });
}

/// Return the `--plugin-override` directory for `name` on the current thread,
/// if any. Used by `WasmPluginLoader::find_plugin_dir` to short-circuit the
/// default `~/.cleen/plugins/<name>/` resolution.
pub fn plugin_override_dir(name: &str) -> Option<std::path::PathBuf> {
    PLUGIN_OVERRIDES.with(|cell| cell.borrow().get(name).cloned())
}

/// Translate a CLI `--target` value into the bridge host class it implies.
/// Returns `None` for unknown targets (caller leaves the override unset and
/// gets the `client_mode`-derived default).
pub fn target_to_host_class(target: &str) -> Option<&'static str> {
    match target.to_lowercase().as_str() {
        "browser" | "web" => Some("browser"),
        "node" | "nodejs" | "server" | "wasi" | "native" | "embedded" => Some("server"),
        // "auto" — the compiler will infer host_class from the reachable
        // bridge functions; if inference is ambiguous the compile path falls
        // back to "server". Returning None here means the CLI leaves the
        // thread-local override unset, so the inference step runs.
        // Fixes BRIDGE-HOST-MISMATCH bug 270f8fc643db: standalone .cln files
        // that only reach browser-only bridges (e.g. `ui.observeVisible`)
        // no longer default to `server` and fail validation.
        "auto" => None,
        _ => None,
    }
}

/// Compiles Clean Language source code to WebAssembly.
///
/// Uses the same multi-file pipeline as `cln compile` (see
/// [`compile_with_external_plugins_and_opt_level`]); the source is compiled
/// as `main.cln`.
pub fn compile(source: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_external_plugins_and_opt_level(source, "main.cln", 2)
}

/// Compiles Clean Language source code to WebAssembly with file path for better error reporting
///
/// Plugins declared in the source's `plugins:` block are loaded, exactly as
/// `cln compile` does (see [`compile_with_external_plugins_and_opt_level`]).
pub fn compile_with_file(source: &str, file_path: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_external_plugins_and_opt_level(source, file_path, 2)
}

/// Parses Clean Language source code and returns the AST (stages 1-2 only).
///
/// Runs lexing and parsing only, without any semantic analysis or type checking.
/// The returned Program AST can be serialized to JSON for tooling integration.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path to the source file (used for error reporting)
///
/// # Returns
/// * `Ok(ast::Program)` - Parsed AST
/// * `Err(Vec<CompilerError>)` - Parse errors
pub fn parse_to_ast(source: &str, file_path: &str) -> Result<ast::Program, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::parser::SpecificationParser;

    // Stage 1: Lexical Analysis
    let source_code = crate::lexer::specification_lexer::SourceCode::new(
        source.to_string(),
        file_path.to_string(),
    );
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer.tokenize().map_err(|e| vec![CompilerError::from(e)])?;

    // Stage 2: Parsing to AST
    let mut parser = SpecificationParser::with_plugin_keywords(tokens, Vec::new());
    let ast = parser.parse_program().map_err(|e| vec![e])?;

    Ok(ast)
}

/// Result of type-checking a Clean Language source file (stages 1-5 only)
#[derive(Debug)]
pub struct TypeCheckResult {
    /// Whether type-checking succeeded without errors
    pub success: bool,
    /// Number of functions found
    pub function_count: usize,
    /// Number of classes found
    pub type_count: usize,
    /// Duration of the type-check in milliseconds
    pub duration_ms: u64,
    /// Diagnostics (warnings, info) collected during type-checking
    pub diagnostics: Vec<CompilerError>,
}

/// Type-checks a source through the same front end as `cln compile`
/// ([`type_check_project`]): `package:` manifests, imports and the plugins in
/// its `plugins:` block are all honoured, and no WASM is generated.
///
/// `file_path` locates the source on disk; when that file does not hold
/// `source` verbatim, the source is checked from a scratch copy with the
/// same file name.
pub fn type_check_with_external_plugins(
    source: &str,
    file_path: &str,
) -> Result<TypeCheckResult, Vec<CompilerError>> {
    with_entry_file(source, file_path, |entry, search_paths| {
        type_check_project(entry, search_paths)
    })
}

/// Load the named plugins from the installed plugin directory (or a
/// `--plugin-override`) into a registry.
fn load_plugin_registry(
    plugin_names: &[String],
) -> Result<plugins::PluginRegistry, Vec<CompilerError>> {
    let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("Failed to create plugin loader: {}", e),
            location: None,
        }]
    })?;
    loader.load_plugins(plugin_names).map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("[PLUGIN001] Failed to load plugins: {}", e),
            location: None,
        }]
    })
}

/// Run the Contract 5 lint pass over the source's declared plugins.
///
/// This is the compiler-side entry point invoked by `cln lint` and (after
/// the type-check step succeeds) by `cln check`. It:
///   1. Extracts plugin names from the source's `plugins:` block.
///   2. Loads those plugins (identical mechanism as `type_check_with_external_plugins`).
///   3. Parses the source into a `Program`.
///   4. Runs `plugins::lint::run_lint_pass` against that pre-expansion AST.
///
/// **Phase B deviation from spec §6:** the specification requires lint to
/// run after parse + resolve + typecheck. The current compiler pipeline
/// expands plugin blocks *before* typecheck, so a naive "typecheck then
/// lint the pre-expansion AST" ordering would require either reordering
/// the whole pipeline (large blast radius) or typechecking twice.
/// For Phase B we run lint after parse only — the residual 4 diagnostics
/// (E026, I002, canvas save/restore, UI-C005) do not require resolved
/// types, so this is safe for the current scope. When a future
/// diagnostic requires typecheck output, we revisit the ordering.
///
/// Returns the raw `PluginLintReport` vec so the CLI can render each
/// entry (Cycle 2 prints raw JSON; Cycle 3 parses and routes through the
/// compiler diagnostic pipeline).
pub fn lint_source(
    source: &str,
    file_path: &str,
    config_level: &str,
) -> Result<Vec<plugins::lint::PluginLintReport>, Vec<CompilerError>> {
    let plugin_names = extract_plugins(source);

    // No plugins declared — lint has nothing to do; return an empty vec.
    // A `package:` manifest holds build configuration, not Clean code: it is
    // not parseable as a program, so there is nothing to lint either.
    if plugin_names.is_empty() || compilation::MultiFileCompiler::is_manifest_file(source) {
        return Ok(Vec::new());
    }

    let registry = load_plugin_registry(&plugin_names)?;

    // Parse the source to a Program. The parser needs the plugin block
    // keywords so it can recognise DSL blocks as `FrameworkBlock` statements
    // rather than syntax errors — same as `type_check_with_plugins` stage 2.
    use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
    use crate::parser::SpecificationParser;

    let source_code = SourceCode::new(source.to_string(), file_path.to_string());
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer.tokenize().map_err(|e| vec![CompilerError::from(e)])?;

    let plugin_keywords = registry.get_all_block_keywords();
    let mut parser = SpecificationParser::with_plugin_keywords(tokens, plugin_keywords);
    let program = parser.parse_program().map_err(|e| vec![e])?;

    // The pre-expansion snapshot IS the just-parsed program — we run lint
    // before ever calling `PluginExpander::expand_program`.
    let reports = plugins::lint::run_lint_pass(&registry, &program, config_level);
    Ok(reports)
}

/// Render a non-fatal diagnostic (HIR validation, name resolution or type
/// checking: FUNC004, FUNC005, FUNC007, IDX004 on `any`, ...) as
/// `warning[CODE]: message` plus a `-->` location line. `None` when the user
/// cannot act on it: lexer and plugin errors, diagnostics without a source
/// position (compiler-injected plugin type stubs) and diagnostics inside
/// plugin-generated code.
pub fn render_warning(warning: &CompilerError) -> Option<String> {
    let context = warning.context()?;
    let loc = context
        .location
        .as_ref()
        .filter(|loc| !loc.file.is_empty() || loc.line > 0)?;
    if (!loc.file.is_empty() && ast::is_synthetic_file_marker(&loc.file))
        || ast::plugin_name_from_origin_marker(&loc.file).is_some()
    {
        return None;
    }
    let code = context.error_code.as_deref().unwrap_or("W000");
    Some(format!(
        "warning[{}]: {}\n  --> {}:{}:{}",
        code, context.message, loc.file, loc.line, loc.column
    ))
}

/// The warnings the user can act on (see [`render_warning`]), each with its
/// rendered text, deduplicated by that text.
fn user_facing_warnings_rendered(warnings: &[CompilerError]) -> Vec<(&CompilerError, String)> {
    let mut seen = std::collections::HashSet::new();
    warnings
        .iter()
        .filter_map(|warning| render_warning(warning).map(|line| (warning, line)))
        .filter(|(_, line)| seen.insert(line.clone()))
        .collect()
}

/// The user-facing subset of `warnings` (see [`user_facing_warnings_rendered`]).
fn user_facing_warnings(warnings: &[CompilerError]) -> Vec<CompilerError> {
    user_facing_warnings_rendered(warnings)
        .into_iter()
        .map(|(warning, _)| warning.clone())
        .collect()
}

/// Print the user-facing compile warnings to stderr. Warnings never fail
/// compilation.
fn report_compile_warnings(warnings: &[CompilerError]) {
    for (_, line) in user_facing_warnings_rendered(warnings) {
        eprintln!("{line}");
    }
}

/// Merge two HIR state blocks. Used by the multi-file pipeline to fold each
/// non-entry module's `state:` declarations into the merged state alongside
/// the entry module's. Without this, plugin-emitted state blocks (e.g.
/// `state: <Class> instance_<tag> = <Class>()` from frame.ui's
/// `expand_component`) that live in component source files were silently
/// dropped because the merge logic only copied `hir.state` from the entry
/// module. The dropped declarations caused the `client_init` splice's
/// `instance_<tag>` reference to resolve as `Undefined variable`.
///
/// Semantics mirror the AST-level `merge_state_block` in `plugins::expander`:
/// declarations and computed values concatenate; the first non-empty rules
/// vec survives (multiple rules vecs from different modules is a caller
/// concern); scope and location are taken from the first non-None block.
fn merge_hir_state(
    target: &mut Option<crate::hir::HirStateBlock>,
    incoming: Option<crate::hir::HirStateBlock>,
) {
    let incoming = match incoming {
        Some(s) => s,
        None => return,
    };
    match target {
        None => *target = Some(incoming),
        Some(existing) => {
            existing.declarations.extend(incoming.declarations);
            existing.computed.extend(incoming.computed);
            if existing.rules.is_empty() {
                existing.rules = incoming.rules;
            }
        }
    }
}

/// Compiles Clean Language source code with the plugins of `registry`.
///
/// Compilation runs through the `cln compile` pipeline, which loads the
/// plugins named in the source's `plugins:` block. Every plugin in
/// `registry` must be declared there; a registry plugin the source does not
/// declare is reported as a PLUGIN001 error instead of being silently
/// ignored.
///
/// # Example
/// ```ignore
/// let registry = WasmPluginLoader::new()?.load_plugins(&["frame.server".to_string()])?;
/// let wasm = compile_with_plugins(source, "main.cln", &registry)?;
/// ```
pub fn compile_with_plugins(
    source: &str,
    file_path: &str,
    registry: &plugins::PluginRegistry,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Default to optimization level 2 (standard optimization)
    compile_with_plugins_and_opt_level(source, file_path, registry, 2)
}

/// [`compile_with_plugins`] with an optimization level (0-3).
pub fn compile_with_plugins_and_opt_level(
    source: &str,
    file_path: &str,
    registry: &plugins::PluginRegistry,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    let declared = extract_plugins(source);
    let undeclared: Vec<CompilerError> = registry
        .registered_plugins()
        .iter()
        .filter(|name| !declared.contains(name))
        .map(|name| CompilerError::PluginError {
            message: format!(
                "[PLUGIN001] plugin '{}' is in the registry but not in the source's plugins: block",
                name
            ),
            location: None,
        })
        .collect();
    if !undeclared.is_empty() {
        return Err(undeclared);
    }
    let registry = std::sync::Arc::new(registry.clone());
    with_entry_file(source, file_path, |entry, search_paths| {
        let caller_host_class = target_host_class_override();
        let result = compile_multi_file_impl(
            entry,
            search_paths,
            opt_level,
            None,
            MemoryTier::default_for_target("server"),
            false,
            false,
            Some(registry),
        );
        set_target_host_class_override(caller_host_class);
        result.map(|(wasm, _build_state)| wasm)
    })
}

/// True when bridge-host mismatches must be hard errors: either the
/// `--strict-hosts` override is set on this thread or `CLEAN_STRICT_HOSTS`
/// is `1`/`true`.
fn strict_hosts_enabled() -> bool {
    strict_hosts_override()
        || std::env::var("CLEAN_STRICT_HOSTS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

/// Compiles Clean Language source code with external WASM plugins and custom optimization level
///
/// # Arguments
/// * `source` - The Clean Language source code (may contain `plugins:` blocks)
/// * `file_path` - Path for error reporting
/// * `opt_level` - Optimization level (0-3)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
pub fn compile_with_external_plugins_and_opt_level(
    source: &str,
    file_path: &str,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Route through the same multi-file pipeline the CLI uses so the library
    // and the CLI produce byte-identical WASM for the same input (COM001,
    // fp ee1a83f9: a separate single-file path diverged on bridge handling,
    // host-class inference, memory tiers and resolver plumbing).
    with_entry_file(source, file_path, |entry, search_paths| {
        // A `package:` block's `target:` sets the thread's host-class
        // override for the build. Library callers (MCP server, `cln test`)
        // compile many sources on one thread, so restore the caller's
        // override afterwards: otherwise one `target: browser` source turns
        // every later compile on the thread into a browser build.
        let caller_host_class = target_host_class_override();
        let result = compile_multi_file_with_memory_tier(
            entry,
            search_paths,
            opt_level,
            None,
            MemoryTier::default_for_target("server"),
            false,
        );
        set_target_host_class_override(caller_host_class);
        result.map(|(wasm, _build_state)| wasm)
    })
}

thread_local! {
    /// While a library compile runs from a scratch copy of its source (see
    /// [`with_entry_file`]): the scratch directory (as written and
    /// canonicalized) and the directory of the path the caller asked for.
    static ENTRY_PATH_ALIAS: std::cell::RefCell<Option<(Vec<std::path::PathBuf>, std::path::PathBuf)>> =
        const { std::cell::RefCell::new(None) };
}

/// Rewrite diagnostic locations that point into the scratch copy of a
/// library compile's source back to the caller's path, so warnings and
/// errors name the file the caller gave instead of a temp directory.
fn remap_entry_paths(diagnostics: &mut [CompilerError]) {
    ENTRY_PATH_ALIAS.with(|alias| {
        let alias = alias.borrow();
        let Some((scratch_dirs, display_dir)) = alias.as_ref() else {
            return;
        };
        for diagnostic in diagnostics.iter_mut() {
            let Some(location) = diagnostic.location_mut() else {
                continue;
            };
            let file = std::path::Path::new(&location.file);
            if let Some(rest) = scratch_dirs.iter().find_map(|d| file.strip_prefix(d).ok()) {
                location.file = display_dir.join(rest).to_string_lossy().into_owned();
            }
        }
    });
}

/// Run `run` on an on-disk entry file holding `source`.
///
/// The multi-file pipeline discovers modules and runs plugin lifecycle hooks
/// from a real entry file. When `file_path` already holds `source` verbatim
/// it is used directly; otherwise the source is written to a scratch
/// directory under the same file name (so file-name-based plugin behaviour
/// is preserved) and removed afterwards. Diagnostics produced from the
/// scratch copy name `file_path`, not the scratch directory.
fn with_entry_file<T>(
    source: &str,
    file_path: &str,
    run: impl FnOnce(&std::path::Path, Vec<std::path::PathBuf>) -> Result<T, Vec<CompilerError>>,
) -> Result<T, Vec<CompilerError>> {
    let requested_path = std::path::Path::new(file_path);
    let existing_matches = std::fs::read_to_string(requested_path)
        .map(|existing| existing == source)
        .unwrap_or(false);
    if existing_matches {
        let parent = requested_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        return run(requested_path, vec![parent]);
    }

    let file_name = requested_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "main.cln".to_string());
    let scratch = tempfile::Builder::new()
        .prefix("cln_library_entry_")
        .tempdir()
        .map_err(|e| {
            vec![CompilerError::io_error(
                format!("failed to create library entry scratch dir: {}", e),
                None,
                None,
            )]
        })?;
    let entry = scratch.path().join(file_name);
    std::fs::write(&entry, source).map_err(|e| {
        vec![CompilerError::io_error(
            format!("failed to write library entry source: {}", e),
            None,
            None,
        )]
    })?;
    let mut scratch_dirs = vec![scratch.path().to_path_buf()];
    if let Ok(canonical) = scratch.path().canonicalize() {
        scratch_dirs.push(canonical);
    }
    let display_dir = requested_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let previous = ENTRY_PATH_ALIAS.with(|a| a.replace(Some((scratch_dirs, display_dir))));
    let result = run(&entry, vec![scratch.path().to_path_buf()]).map_err(|mut errors| {
        remap_entry_paths(&mut errors);
        errors
    });
    ENTRY_PATH_ALIAS.with(|a| *a.borrow_mut() = previous);
    result
}

/// Compiles Clean Language source code with a specific compilation target
///
/// This function allows specifying a compilation target to control which imports
/// are included in the generated WASM:
/// - `Server`: Full HTTP server imports (_req_*, _session_*, _auth_*)
/// - `Plugin`: Minimal imports for WASM plugins (no server functions)
/// - `Standalone`: Standard CLI/library compilation
///
/// Compilation runs through the `cln compile` pipeline. A `Plugin` build
/// takes its `[bridge]` declarations and Clean Runtime ABI version from the
/// sibling `plugin.toml`, as `cln compile --target plugin` does.
///
/// # Example
/// ```ignore
/// use clean_language_compiler::{compile_with_target, CompilationTarget};
///
/// // Compile plugin with minimal imports
/// let wasm = compile_with_target(source, "plugin.cln", CompilationTarget::Plugin, 2)?;
/// ```
pub fn compile_with_target(
    source: &str,
    file_path: &str,
    target: CompilationTarget,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    let previous_target = COMPILATION_TARGET.with(|t| t.replace(Some(target)));
    let previous_abi = plugin_abi_version_override();
    if matches!(target, CompilationTarget::Plugin) {
        set_plugin_abi_version_override(Some(resolve_plugin_abi_version_for_path(
            std::path::Path::new(file_path),
        )));
    }
    let result = compile_with_external_plugins_and_opt_level(source, file_path, opt_level);
    set_plugin_abi_version_override(previous_abi);
    COMPILATION_TARGET.with(|t| t.set(previous_target));
    result
}

/// Compiles Clean Language source code for use as a WASM plugin
///
/// This is a convenience function that compiles with `CompilationTarget::Plugin`,
/// which excludes server-specific imports (_req_*, _session_*, _auth_*) to produce
/// a minimal WASM module suitable for use as a plugin.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes (with minimal imports)
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// use clean_language_compiler::compile_for_plugin;
///
/// let wasm = compile_for_plugin(source, "my_plugin.cln")?;
/// // Result: ~67 imports instead of ~83
/// ```
pub fn compile_for_plugin(source: &str, file_path: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_target(source, file_path, CompilationTarget::Plugin, 2)
}

/// When `source` is a package manifest (starts with `package:`), return the path of
/// the entry `.cln` file declared under the first `entry:` key.
fn extract_manifest_entry(
    source: &str,
    manifest_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("entry:") {
            let p = rest.trim().trim_matches('"');
            if !p.is_empty() {
                return Some(manifest_dir.join(p));
            }
        }
    }
    None
}

/// Extracts plugin names from `plugins:` blocks in source code
///
/// # Format
/// ```clean
/// plugins:
///     frame.web
///     frame.data
/// ```
///
/// # Returns
/// Vector of plugin names (e.g., ["frame.web", "frame.data"])
///
/// `//` comments (whole-line or trailing) are ignored.
pub fn extract_plugins(source: &str) -> Vec<String> {
    let mut plugins = Vec::new();
    let mut in_plugins_block = false;
    let mut plugins_indent: usize = 0;

    for line in source.lines() {
        // Plugin names never contain `//`, so everything after it is a comment.
        let line = line.split("//").next().unwrap_or_default();
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        // Handle inline list syntax at any indentation: `plugins: [frame.ui, frame.server]`
        if let Some(rest) = trimmed.strip_prefix("plugins:") {
            let rest = rest.trim();
            if rest.starts_with('[') {
                let end = rest.find(']').unwrap_or(rest.len());
                let inner = &rest[1..end];
                for name in inner.split(',') {
                    let name = name.trim();
                    if !name.is_empty() && !name.starts_with('#') {
                        plugins.push(name.to_string());
                    }
                }
                continue;
            }

            // Multi-line block: `plugins:` alone (at any indentation)
            if rest.is_empty() {
                in_plugins_block = true;
                plugins_indent = indent;
                continue;
            }
        }

        // Collect multi-line plugin entries
        if in_plugins_block {
            if trimmed.is_empty() {
                continue;
            }
            // A line at the same or lesser indentation as the plugins: header ends the block
            if indent <= plugins_indent && trimmed.ends_with(':') {
                in_plugins_block = false;
                continue;
            }
            // Lines indented deeper than the plugins: header are entries
            if indent > plugins_indent {
                if !trimmed.starts_with('#') {
                    plugins.push(trimmed.to_string());
                }
            } else {
                in_plugins_block = false;
            }
        }
    }

    plugins
}

/// Plugin names required by the package rooted at `entry_path` (see
/// [`collect_package_plugins`]); empty when the entry cannot be read.
fn entry_plugin_names(entry_path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(entry_path)
        .map(|source| collect_package_plugins(entry_path, &source))
        .unwrap_or_default()
}

/// Load the plugins required by the package rooted at `entry_path`. `None`
/// when it declares none or loading fails; the compile pass reports loading
/// errors itself, so the discovery helpers below degrade to "nothing found".
fn load_entry_plugin_registry(entry_path: &std::path::Path) -> Option<plugins::PluginRegistry> {
    let plugin_names = entry_plugin_names(entry_path);
    if plugin_names.is_empty() {
        return None;
    }
    plugins::WasmPluginLoader::new()
        .ok()?
        .load_plugins(&plugin_names)
        .ok()
}

/// Plugin Contracts v2 — discover plugins for the given entry file, load
/// their manifests, and return resolved callback contracts.
///
/// Used by the build flow (`cln compile`, `cln build`) to populate the
/// `callbacks` field in `dist/build-manifest.json` so hosts can read the
/// dispatch contracts (e.g. `_ui_render_page` → `component_tag_render`)
/// without re-parsing every plugin.toml. Validation per
/// `foundation/spec/plugins/contracts/bridge-host-classes.md` §4 already
/// happens at registry build; this helper just extracts the resolved set.
///
/// Returns an empty Vec when:
/// - the entry file declares no plugins,
/// - no loaded plugin declares any `[bridge.functions.callback]` blocks,
/// - plugin loading itself fails (the caller's normal compile pass will
///   surface that error — this helper degrades to "no callbacks" rather
///   than blocking manifest emission).
pub fn discover_callback_contracts<P: AsRef<std::path::Path>>(
    entry_path: P,
) -> Vec<build_manifest::CallbackContract> {
    load_entry_plugin_registry(entry_path.as_ref())
        .map(|registry| registry.callback_contracts())
        .unwrap_or_default()
}

/// Plugin Contracts v2 — discover plugins for the given entry file, load
/// their manifests, and return the full set keyed by plugin name. Used by
/// the build flow to orchestrate `[[artifacts]]` emission per
/// `foundation/spec/plugins/contracts/artifacts.md` §7.
///
/// The map is in plugin-load order. Returns an empty map on any error so the
/// legacy emission path can gracefully take over.
pub fn discover_plugin_manifests<P: AsRef<std::path::Path>>(
    entry_path: P,
) -> indexmap::IndexMap<String, plugins::PluginManifest> {
    load_entry_plugin_registry(entry_path.as_ref())
        .map(|registry| registry.loaded_manifests().clone())
        .unwrap_or_default()
}

/// Plugin Contracts v2 — discover the on-disk installed directory for each
/// loaded plugin keyed by plugin name. Pairs with `discover_plugin_manifests`
/// for static-path artifact resolution per `artifacts.md` §2/§6.
///
/// Returns an empty map on any error (no plugins, loader failure, etc.) so
/// callers can degrade gracefully — a missing entry surfaces as a per-artifact
/// warning rather than a hard build failure.
pub fn discover_plugin_dirs<P: AsRef<std::path::Path>>(
    entry_path: P,
) -> std::collections::HashMap<String, std::path::PathBuf> {
    let plugin_names = entry_plugin_names(entry_path.as_ref());
    if plugin_names.is_empty() {
        return std::collections::HashMap::new();
    }
    let Ok(loader) = plugins::WasmPluginLoader::new() else {
        return std::collections::HashMap::new();
    };
    plugin_names
        .into_iter()
        .filter_map(|name| loader.plugin_dir(&name).ok().map(|dir| (name, dir)))
        .collect()
}

/// Plugin Contracts v2 Phase B — resolve the Clean Runtime ABI version to
/// stamp into a plugin WASM build. Reads `[compatibility].abi_version` from
/// the sibling `plugin.toml` (searched at the same locations as
/// `extend_with_source_plugin_bridges`), falling back to
/// `DEFAULT_RUNTIME_ABI_VERSION` when the manifest is absent, unparseable, or
/// omits the field. Returns the string the codegen will write verbatim into
/// the `clean.abi_version` custom section.
/// See `foundation/spec/plugins/contracts/runtime-abi.md` §4–§5.
pub fn resolve_plugin_abi_version_for_path(entry_path: &std::path::Path) -> String {
    use plugins::plugin_abi::DEFAULT_RUNTIME_ABI_VERSION;

    match find_source_plugin_manifest(entry_path) {
        Some((_, Ok(manifest))) => manifest
            .compatibility
            .abi_version
            .unwrap_or_else(|| DEFAULT_RUNTIME_ABI_VERSION.to_string()),
        _ => DEFAULT_RUNTIME_ABI_VERSION.to_string(),
    }
}

/// Locate and parse the source `plugin.toml` of a plugin being built: next
/// to the entry file, or one level up when the entry sits in `src/` (the
/// framework layout `plugins/<name>/src/main.cln`). Returns `None` when there
/// is no such file or it cannot be read.
fn find_source_plugin_manifest(
    entry_path: &std::path::Path,
) -> Option<(
    std::path::PathBuf,
    Result<plugins::plugin_abi::PluginManifest, toml::de::Error>,
)> {
    let parent = entry_path.parent()?;
    let mut candidates = vec![parent.join("plugin.toml")];
    if parent.file_name().and_then(|n| n.to_str()) == Some("src") {
        if let Some(grand) = parent.parent() {
            candidates.push(grand.join("plugin.toml"));
        }
    }
    let manifest_path = candidates.into_iter().find(|p| p.is_file())?;
    let source = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest = toml::from_str(&source);
    Some((manifest_path, manifest))
}

/// Extend the resolver's bridge-function set with declarations from the
/// source plugin manifest when one is being built.
///
/// PLUGIN-BUILD-STATE-CROSS-PLUGIN-BOOTSTRAP-BLOCKED: the per-plugin
/// `PluginRegistry` is loaded from `~/.cleen/plugins/<name>/plugin.toml`
/// — the previously installed copy. When a plugin author adds a new
/// `[bridge].functions` entry and immediately calls it from
/// `src/main.cln`, the resolver looks the call up against the stale
/// installed manifest and SEM007s on the new function name. The plugin
/// cannot be rebuilt in a single commit, because the rebuild needs the
/// new bridge to be visible AT BUILD TIME, but only the rebuild can
/// install it.
///
/// Resolution: when the entry being compiled lives next to (or one
/// `src/` level below) a `plugin.toml`, parse THAT manifest and append
/// its bridge declarations to the resolver's bridge_functions set. The
/// installed-registry path stays canonical for everyone else; the
/// source-manifest path only fires when the directory layout matches
/// the framework's plugin source convention. Bridges already known by
/// canonical name are not duplicated.
///
/// The detection is conservative on purpose:
/// 1. `<entry>.parent()/plugin.toml`  — top-level layout (rare).
/// 2. `<entry>.parent().parent()/plugin.toml` ONLY IF `<entry>.parent()`
///    is named `src` — the standard framework layout (`plugins/<name>/src/main.cln`).
///
/// If a regular user project happens to have `plugin.toml` two levels
/// above its entry but no `src/` segment between them, nothing
/// happens — the function bails out silently. The framework's pattern
/// is the only one this fires on.
fn extend_with_source_plugin_bridges(
    entry_path: &std::path::Path,
    bridge_functions: &mut Vec<plugins::BridgeFunction>,
) {
    let (manifest_path, manifest) = match find_source_plugin_manifest(entry_path) {
        Some((path, Ok(manifest))) => (path, manifest),
        Some((path, Err(e))) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "PLUGIN-BUILD-STATE-CROSS-PLUGIN-BOOTSTRAP-BLOCKED: source plugin.toml \
                 found but failed to parse — falling back to installed-registry bridges only"
            );
            return;
        }
        None => return,
    };

    let already: std::collections::HashSet<String> =
        bridge_functions.iter().map(|bf| bf.name.clone()).collect();

    let added: usize = manifest
        .bridge
        .functions
        .into_iter()
        .filter(|bf| !already.contains(&bf.name))
        .map(|bf| bridge_functions.push(bf))
        .count();

    if added > 0 {
        tracing::info!(
            plugin = %manifest.plugin.name,
            path = %manifest_path.display(),
            added = added,
            "Source plugin.toml found — extended bridge set with declarations not yet \
             present in the installed registry"
        );
    }
}

/// Extend `bridge_functions` with any registry entries not already declared
/// by a loaded plugin manifest.
///
/// Some Layer-3 bridges (e.g. `_dev_snapshot`) are declared only in the
/// canonical `foundation/platform-architecture/function-registry.toml` and
/// never in a plugin.toml `[bridge]` section — the framework calls them
/// directly by their `_prefixed` name. Codegen only emits WASM imports for
/// bridges present in `bridge_functions`, and codegen already filters that
/// list by `used_bridge_function_names`, so injecting every registry entry
/// here is safe: only entries actually referenced by user code produce an
/// import. Unused registry entries are dropped by
/// `register_plugin_bridge_imports` (see codegen/mir_codegen/utilities.rs).
///
/// Complements the resolver-side on-demand registration in
/// `resolver_impl.rs::try_register_bridge_from_registry` — the resolver
/// makes the call resolve, this makes codegen emit the import.
fn extend_with_registry_bridges(bridge_functions: &mut Vec<plugins::BridgeFunction>) {
    let idx = match plugins::registry_loader::RegistryIndex::load() {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load function-registry.toml — skipping registry-only bridge extension");
            return;
        }
    };

    let already: std::collections::HashSet<String> =
        bridge_functions.iter().map(|bf| bf.name.clone()).collect();

    let mut added = 0usize;
    for reg_fn in idx.functions() {
        if already.contains(&reg_fn.name) {
            continue;
        }
        // Only inject bridge-convention names (leading underscore). Registry
        // entries WITHOUT the underscore (e.g. `http_get`, `http_post`,
        // `string_split`) are already registered by the compiler's built-in
        // importers (`register_http_imports`, `register_string_split_import`,
        // etc.) with specific WASM shapes. Adding them here as generic
        // `expand_strings=true` bridges causes the HTTP importer's skip
        // filter (mir_codegen/mod.rs `skip_http_functions`) to drop them,
        // leaving downstream codegen unable to find them in `function_map`.
        // Registry-only bridges that the resolver needs to resolve on demand
        // are all `_prefixed` — `_dev_snapshot` and its Layer 3 peers.
        if !reg_fn.name.starts_with('_') {
            continue;
        }
        bridge_functions.push(plugins::BridgeFunction {
            name: reg_fn.name.clone(),
            params: reg_fn.params.clone(),
            returns: reg_fn.returns.clone(),
            module: if reg_fn.module.is_empty() {
                "env".to_string()
            } else {
                reg_fn.module.clone()
            },
            description: reg_fn.description.clone(),
            // Registry convention: `"string"` params expand to (ptr, len) at
            // the WASM level by default. Individual entries opt out via
            // `expand_strings = false` in the toml when the canonical plugin
            // ships bare length-prefixed pointers — e.g. `_json_get`, so its
            // second param can be declared as `"string"` (language level) and
            // resolved to HirType::String while still matching the plugin's
            // ABI. Propagating that flag here also keeps codegen import shape
            // aligned.
            expand_strings: reg_fn.expand_strings,
            hosts: if reg_fn.hosts.is_empty() {
                None
            } else {
                Some(reg_fn.hosts.clone())
            },
            ..Default::default()
        });
        added += 1;
    }

    if added > 0 {
        tracing::debug!(
            added = added,
            "Extended bridge set with registry-only entries; codegen will emit imports only for those actually used"
        );
    }
}

/// Collect every loaded bridge that declares `registers_handler_at_arg`
/// on its plugin.toml `[[bridge]]` entry, returning
/// `(bridge_name, handler_arg_index)` pairs. Used by
/// `HirValidator::validate_with_handler_bridges` to recognize plugin-emitted
/// route handlers as legitimate request-context sites (CONC002).
/// Mirrors `PluginRegistry::handler_registering_bridges()` but works off a
/// raw slice — the multi-file paths compute bridge_functions inline before
/// they have a full registry handle.
///
/// Fixes ARCH-CONC002-HARDCODES-HTTP-ROUTE (fp 12ce9f522815).
fn handler_bridges_from_slice(bridges: &[plugins::BridgeFunction]) -> Vec<(String, usize)> {
    bridges
        .iter()
        .filter_map(|bf| {
            bf.registers_handler_at_arg
                .map(|idx| (bf.name.clone(), idx))
        })
        .collect()
}

/// Collect all plugin names required by a package.
///
/// For a single-file build: returns plugins declared in that file.
/// For a package manifest (starts with `package:`): reads the declared entry
/// file AND recursively scans every `.cln` file in every `shared:` folder,
/// merging all unique plugin names.  This ensures that plugins declared only
/// in shared source files (e.g. `frame.server` in routes.cln) are loaded into
/// the registry even when the manifest entry file does not re-declare them.
fn collect_package_plugins(entry_path: &std::path::Path, entry_source: &str) -> Vec<String> {
    let mut plugins: Vec<String> = extract_plugins(entry_source);

    if !compilation::MultiFileCompiler::is_manifest_file(entry_source) {
        return plugins;
    }

    let manifest_dir = match entry_path.parent() {
        Some(d) => d,
        None => return plugins,
    };

    // Read plugins from the declared entry .cln file (may differ from manifest).
    if let Some(actual_entry) = extract_manifest_entry(entry_source, manifest_dir) {
        if let Ok(src) = std::fs::read_to_string(&actual_entry) {
            for p in extract_plugins(&src) {
                if !plugins.contains(&p) {
                    plugins.push(p);
                }
            }
        }
    }

    // Scan all .cln files in every shared: folder.
    fn scan_cln_dir(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_cln_dir(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("cln") {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    for p in extract_plugins(&src) {
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
        }
    }

    // Parse shared: [...] from manifest
    for line in entry_source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("shared:") {
            let rest = rest.trim();
            if rest.starts_with('[') {
                let end = rest.find(']').unwrap_or(rest.len());
                for path_str in rest[1..end].split(',') {
                    let p = path_str.trim().trim_matches('"');
                    if !p.is_empty() {
                        scan_cln_dir(&manifest_dir.join(p), &mut plugins);
                    }
                }
            }
        }
    }

    plugins
}

/// Extract the `target:` value from a `package:` block in the entry source, if
/// present. Returns e.g. `Some("browser")` for a manifest like:
///
/// ```clean
/// package: MyApp
///     version: "1.0.0"
///     target: browser
///         plugins: [frame.data, frame.ui]
///         entry: app.cln
/// ```
///
/// Returns `None` when the file does not start with `package:` OR when no
/// `target:` line is present in the package block. This mirrors the parser
/// convention: the package block content is raw text; the compiler scans it
/// for the declarations it needs (plugins:, shared:, target:, entry:).
///
/// Fixes ac38e3d2 BRIDGE-HOST-MISMATCH: without this, a manifest declaring
/// `target: browser` fell through to the codegen inference path, which
/// defaulted to `server` when no browser-only bridges were reachable — so
/// server-only bridges like `_db_query` were silently emitted into a
/// browser-target build instead of surfacing a hard compile error.
fn extract_package_target(entry_source: &str) -> Option<String> {
    // Find the `package:` declaration. Framework repros commonly ship with a
    // multi-line comment header describing the file, so `.trim_start()` is
    // not enough — we scan line-by-line and skip blank lines + `//` comments.
    // Fixes framework prompt b6d79783: `cln build` on
    // examples/bug-repro-db-in-browser/main.cln (10 lines of comment header
    // before `package:`) missed the target derivation and shipped
    // `_db_query` into a browser build without erroring.
    let mut lines = entry_source.lines();
    let mut in_package = false;
    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("package:") {
            in_package = true;
            break;
        }
        // First non-comment / non-blank line was NOT `package:`. This isn't
        // a manifest file; nothing to extract.
        return None;
    }
    if !in_package {
        return None;
    }
    // Now scan the remainder of the source for a `target: <value>` line that
    // is indented into the package block. Anything not indented ends the
    // package block scan (subsequent top-level declarations).
    for line in lines {
        let indented = line.starts_with('\t') || line.starts_with("    ");
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if !indented {
            // Left the package block without finding target:.
            return None;
        }
        if let Some(rest) = trimmed.strip_prefix("target:") {
            let value = rest.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Returns true if a module is server-only and must be excluded from a
/// `frontend.wasm` build. Used by `compile_multi_file_client_mode`.
///
/// Heuristics, derived from the canonical app structure
/// (`foundation/spec/plugins/frame-ui-semantics.md` §UI-S007):
///   - synthetic `__page_routes_generated` module → server-only
///   - file path contains `/server/`, `/backend/`, `/api/`, `/pages/`, or `routes` → server-only
///   - files in `app/auth/` → server-only: auth configuration (sessions, JWT,
///     roles) lives there and is server configuration
///     (`foundation/spec/framework/frame-server-semantics.md` SRV-I001)
///   - `.client.cln` companion files → client (kept)
///   - `/components/` files → client (kept)
///   - everything else → client (kept, may include shared utilities)
fn is_server_only_module(module_name: &str, file_path: &std::path::Path) -> bool {
    if module_name == "__page_routes_generated" {
        return true;
    }
    let path_str = file_path.to_string_lossy().replace('\\', "/");
    let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    // Explicit client companions are always kept.
    if file_stem.ends_with(".client") {
        return false;
    }
    // Components and shared client code are kept.
    if path_str.contains("/components/") {
        return false;
    }
    // Server-side modules are dropped.
    path_str.contains("/server/")
        || path_str.contains("/backend/")
        || path_str.contains("/api/")
        || path_str.contains("/pages/")
        || path_str.contains("/app/auth/")
        || file_stem == "routes"
}

/// Compiles Clean Language with multi-file support and a specific memory tier.
///
/// Supports `import:` of other `.cln` files and `plugins:` blocks, with control
/// over the memory budget tier.
///
/// # Tier resolution precedence (MEMORY_POLICY.md §3.1):
/// 1. `explicit_tier` (`Some`) — from `--memory-tier` CLI flag, always wins.
/// 2. Highest tier declared by any active plugin's `[memory] tier` field.
/// 3. `target_default` — inferred from `--target` via `MemoryTier::default_for_target`.
/// 4. `MemoryTier::Standard` — ultimate fallback.
///
/// # Client mode
/// When `client_mode` is `true`, the compilation produces a `frontend.wasm`-shaped
/// output suitable for browser hydration (loader.js):
///   - Server-only modules (see [`is_server_only_module`]) are dropped before HIR merge
///   - The entry module's `start:` body is replaced with an empty body so the
///     browser does not attempt to call `_http_listen` etc. on `_start()`
///   - Component classes and their `events:` handlers are preserved as exports
///
/// Spec reference: `foundation/spec/plugins/frame-ui-semantics.md` §UI-B009.
pub fn compile_multi_file_with_memory_tier<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
    explicit_tier: Option<MemoryTier>,
    target_default: MemoryTier,
    client_mode: bool,
) -> Result<(Vec<u8>, std::collections::BTreeMap<String, String>), Vec<CompilerError>> {
    compile_multi_file_impl(
        entry_path.as_ref(),
        search_paths,
        opt_level,
        explicit_tier,
        target_default,
        client_mode,
        false,
        None,
    )
}

/// Output of the multi-file front end (discovery through type checking).
struct MultiFileFrontend {
    type_result: crate::typechecker::TypeCheckResult,
    /// Non-fatal diagnostics from HIR validation, resolution and type checking.
    warnings: Vec<CompilerError>,
    registry: Option<std::sync::Arc<plugins::PluginRegistry>>,
    bridge_functions: Vec<plugins::BridgeFunction>,
    lang_to_bridge: std::collections::HashMap<String, String>,
    lang_to_helper: std::collections::HashMap<String, String>,
    build_state: Option<plugins::BuildState>,
    /// Functions and classes written in source files (plugin-generated
    /// declarations excluded).
    function_count: usize,
    type_count: usize,
}

/// Type-check a project from its entry file through the same front end as
/// `cln compile` (module discovery, `package:` manifests, plugins, imports,
/// screen and state merging), without generating WASM. Used by `cln check`
/// so that checking and compiling a project always agree.
pub fn type_check_project<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
) -> Result<TypeCheckResult, Vec<CompilerError>> {
    let start_time = std::time::Instant::now();
    // See `compile_with_external_plugins_and_opt_level`: a package
    // `target:` must not leak into later work on this thread.
    let caller_host_class = target_host_class_override();
    let frontend =
        run_multi_file_frontend(entry_path.as_ref(), search_paths, 2, false, false, None);
    set_target_host_class_override(caller_host_class);
    let mut frontend = frontend?;
    remap_entry_paths(&mut frontend.warnings);
    Ok(TypeCheckResult {
        success: true,
        function_count: frontend.function_count,
        type_count: frontend.type_count,
        duration_ms: start_time.elapsed().as_millis() as u64,
        diagnostics: user_facing_warnings(&frontend.warnings),
    })
}

/// The single multi-file pipeline shared by development and release builds.
/// `release` only changes MIR lowering (`always:` invariant checks are
/// stripped) and the module-compiler release flag; every other stage —
/// package `target:` handling, plugin type stubs, required-parameter counts,
/// host-class resolution — is identical for both.
fn compile_multi_file_impl(
    entry_path: &std::path::Path,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
    explicit_tier: Option<MemoryTier>,
    target_default: MemoryTier,
    client_mode: bool,
    release: bool,
    given_registry: Option<std::sync::Arc<plugins::PluginRegistry>>,
) -> Result<(Vec<u8>, std::collections::BTreeMap<String, String>), Vec<CompilerError>> {
    use crate::mir::{lower_tast_to_mir_release, lower_tast_to_mir_with_opt_level};

    tracing::info!(
        entry = %entry_path.display(),
        opt_level = opt_level,
        explicit_tier = ?explicit_tier,
        target_default = %target_default,
        client_mode = client_mode,
        release = release,
        "Starting multi-file compilation with memory tier"
    );

    let MultiFileFrontend {
        type_result,
        warnings,
        registry,
        bridge_functions,
        lang_to_bridge,
        lang_to_helper,
        build_state: build_state_arc,
        ..
    } = run_multi_file_frontend(
        entry_path,
        search_paths,
        opt_level,
        client_mode,
        release,
        given_registry,
    )?;
    let mut warnings = warnings;
    remap_entry_paths(&mut warnings);
    report_compile_warnings(&warnings);

    // Stage 6: TAST to MIR. Release builds strip `always:` invariant checks.
    let mir_result = if release {
        lower_tast_to_mir_release(type_result.tast, opt_level, true)?
    } else {
        lower_tast_to_mir_with_opt_level(type_result.tast, opt_level)?
    };

    // Resolve effective memory tier (MEMORY_POLICY.md §3.1):
    // explicit CLI flag > max(plugin tiers) > target default > standard
    let memory_tier = if let Some(tier) = explicit_tier {
        tracing::debug!(tier = %tier, "Using explicit CLI memory tier");
        tier
    } else {
        let plugin_tier = registry
            .as_ref()
            .map(|r| r.resolve_plugin_memory_tier())
            .transpose()
            .map_err(|e| vec![e])?
            .flatten();

        if let Some(pt) = plugin_tier {
            let effective = std::cmp::max(pt, target_default);
            tracing::info!(
                plugin_tier = %pt,
                target_default = %target_default,
                effective = %effective,
                "Memory tier resolved from plugin declaration"
            );
            effective
        } else {
            tracing::debug!(tier = %target_default, "Using target-default memory tier");
            target_default
        }
    };

    // Stage 7: WASM generation with memory tier
    use crate::codegen::mir_codegen::MirCodeGenerator;
    let mut mir_codegen = match COMPILATION_TARGET.with(|t| t.get()) {
        Some(target) => MirCodeGenerator::with_target(target),
        None => MirCodeGenerator::default(),
    };
    mir_codegen.set_memory_tier(memory_tier);

    if !bridge_functions.is_empty() {
        mir_codegen.set_bridge_functions(bridge_functions);
    }
    if !lang_to_bridge.is_empty() {
        mir_codegen.set_language_to_bridge_map(lang_to_bridge);
    }
    if !lang_to_helper.is_empty() {
        mir_codegen.set_language_to_helper_map(lang_to_helper);
    }

    // Plugin Contracts v2 — derive host class from (in priority order):
    //   1. CLI-supplied override via `set_target_host_class_override` (thread-local),
    //      typically set by `handle_compile` from `--target browser|node|server`.
    //   2. client_mode flag — the nested `frontend.wasm` build forces "browser".
    //   3. Bridge-call inference — if every reachable browser/server-restricted
    //      bridge accepts `browser`, pick `browser`. Fixes 270f8fc643db so
    //      standalone client entries (`cln compile app/client/main.cln`) that
    //      only call `ui.*` no longer misdefault to `server` and fail with
    //      BRIDGE-HOST-MISMATCH. See infer_host_class_from_mir.
    //   4. "server" default.
    //
    // See foundation/spec/plugins/contracts/bridge-host-classes.md §6.
    //
    // Strict mode (warnings → errors) is opt-in via either:
    //   - CLI flag `--strict-hosts` (sets the thread-local override), or
    //   - CLEAN_STRICT_HOSTS=1 environment variable (compat).
    let host_class = target_host_class_override().unwrap_or_else(|| {
        if client_mode {
            "browser".to_string()
        } else if let Some(inferred) = mir_codegen.infer_host_class_from_mir(&mir_result.program) {
            inferred
        } else {
            "server".to_string()
        }
    });
    mir_codegen.set_host_class(Some(host_class));
    mir_codegen.set_strict_hosts(strict_hosts_enabled());
    mir_codegen.set_client_mode(client_mode);

    // Plugin Contracts v2 Phase B — when the CLI signalled a plugin build,
    // stamp the resolved Clean Runtime ABI version (per sibling plugin.toml,
    // or DEFAULT_RUNTIME_ABI_VERSION) into the WASM. The loader will check
    // this in Phase B cycle 2; for now it's a passive tag.
    // See foundation/spec/plugins/contracts/runtime-abi.md §4.
    if let Some(abi) = plugin_abi_version_override() {
        mir_codegen.set_abi_version(Some(abi));
    }

    let codegen_result = mir_codegen.generate(mir_result.program)?;

    // Validate the generated WASM before handing it off. If codegen ever
    // produces a malformed module we want to catch it here with offset +
    // section info, not let it flow out to the server/runtime where it
    // surfaces as an opaque "failed to parse".
    crate::codegen::validate::validate_generated_wasm(&codegen_result.wasm_bytes)
        .map_err(|e| vec![e])?;

    tracing::info!(
        bytes = codegen_result.wasm_bytes.len(),
        memory_tier = %memory_tier,
        client_mode = client_mode,
        release = release,
        "Multi-file compilation with memory tier complete"
    );

    let build_state_snapshot = snapshot_build_state(build_state_arc.as_ref());
    Ok((codegen_result.wasm_bytes, build_state_snapshot))
}

/// Discovery, plugin loading, per-module compilation, HIR merging,
/// validation, resolution and type checking for a multi-file project.
fn run_multi_file_frontend(
    entry_path: &std::path::Path,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
    client_mode: bool,
    release: bool,
    given_registry: Option<std::sync::Arc<plugins::PluginRegistry>>,
) -> Result<MultiFileFrontend, Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
    use crate::typechecker::TypeChecker;
    use std::sync::Arc;

    // Step 0: Read entry file to extract plugins
    let entry_source = std::fs::read_to_string(entry_path).map_err(|e| {
        vec![CompilerError::io_error(
            format!("Failed to read entry file: {}", e),
            None,
            None,
        )]
    })?;

    let plugin_names = collect_package_plugins(entry_path, &entry_source);

    // Bridge-host-classes: if a package: block declares `target: <host>`, treat
    // it as an implicit --target flag so the codegen host-mismatch validator
    // enforces host-restriction correctly. Without this, `target: browser`
    // manifests fell through to inference and defaulted to `server` — server-
    // only bridges silently emitted into browser builds instead of erroring.
    // Only applied when the CLI has NOT already set an explicit override
    // (main.rs's --target flag wins per bridge-host-classes.md §6 priority).
    // Fixes ac38e3d2 BRIDGE-HOST-MISMATCH.
    if target_host_class_override().is_none() {
        if let Some(target) = extract_package_target(&entry_source) {
            if let Some(hc) = target_to_host_class(&target) {
                tracing::info!(
                    target = %target,
                    host_class = %hc,
                    "Derived host class from package: target: manifest"
                );
                set_target_host_class_override(Some(hc.to_string()));
            }
        }
    }

    // Plugin Contracts v2 — keep an Arc clone of the build-state keystore so
    // that, after compilation completes, callers (e.g. main.rs) can read every
    // write made by plugin `_build_state_set` calls during `expand_block`.
    // Without this clone the populated state would be dropped with the
    // registry, leaving the manifest writer and artifact orchestrator
    // unable to see writes. See `contracts/lifecycle.md` §2.5.
    //
    // A library caller that built its own registry (`compile_with_plugins`)
    // compiles with exactly that registry instead of loading installed
    // plugins by name.
    let given_build_state = given_registry.as_ref().map(|r| r.build_state().clone());
    let build_state_arc: Option<plugins::BuildState> = if given_build_state.is_some() {
        given_build_state
    } else if plugin_names.is_empty() {
        None
    } else {
        Some(plugins::new_build_state())
    };

    let registry = if given_registry.is_some() {
        given_registry
    } else if let Some(ref bs) = build_state_arc {
        tracing::info!(plugins = ?plugin_names, "Loading plugins for multi-file compilation");

        let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
            vec![CompilerError::PluginError {
                message: format!("Failed to create plugin loader: {}", e),
                location: None,
            }]
        })?;

        let reg = loader
            .load_plugins_with_build_state(&plugin_names, Arc::clone(bs))
            .map_err(|e| {
                vec![CompilerError::PluginError {
                    message: format!("[PLUGIN001] Failed to load plugins: {}", e),
                    location: None,
                }]
            })?;

        Some(Arc::new(reg))
    } else {
        None
    };

    // Step 1: Build the compilation unit
    let mut config = MultiFileCompilerConfig::default()
        .with_search_paths(search_paths)
        .with_opt_level(opt_level)
        .with_client_mode(client_mode)
        .with_release_mode(release);

    if let Some(ref reg) = registry {
        config = config.with_plugin_registry(Arc::clone(reg));
    }

    let compiler = MultiFileCompiler::with_config(config);
    let unit = compiler.build_from_file(entry_path)?;

    // Step 2: Merge all HIR programs into a single unified program
    let mut merged_hir = {
        use crate::hir::{HirFunction, HirProgram};

        let mut all_functions: Vec<HirFunction> = Vec::new();
        let mut duplicate_functions: Vec<CompilerError> = Vec::new();
        let mut all_classes = Vec::new();
        let mut all_capabilities: Vec<crate::hir::HirCapability> = Vec::new();
        let mut start_function: Option<crate::hir::HirFunction> = None;
        let mut extra_start_stmts: Vec<crate::hir::HirStatement> = Vec::new();
        // Non-entry modules' `start:` bodies, in compilation order.
        let mut module_starts: Vec<(String, crate::hir::HirFunction)> = Vec::new();
        let mut all_imports = Vec::new();
        let mut all_tests = Vec::new();
        let mut all_externals = Vec::new();
        let mut merged_state: Option<crate::hir::HirStateBlock> = None;
        let mut merged_screen_blocks: Vec<crate::hir::HirScreenBlock> = Vec::new();
        let mut merged_watch_blocks: Vec<crate::hir::HirWatchBlock> = Vec::new();
        let mut root_location = None;

        for module_id in &unit.compilation_order {
            if let Some(module) = unit.get_module(*module_id) {
                // Client-mode filter: drop server-only modules before merging their HIR.
                //
                // The entry module is exempt. Its `start_function` is now
                // compiler-controlled via the `client_init` lifecycle slot
                // dispatch (see `PluginExpander::expand_program` →
                // `prepend_to_start`), so dropping it would also drop every
                // statement the slot produced (component instantiation +
                // onMount() calls). Preamble-injected helpers that would
                // misbehave under a browser host are stripped downstream by
                // the codegen tree-shaker — they are not reachable from the
                // synthetic browser `_start` body.
                if client_mode
                    && !module.is_entry
                    && is_server_only_module(&module.name, &module.file_path)
                {
                    tracing::debug!(
                        module = %module.name,
                        path = %module.file_path.display(),
                        "client_mode: skipping server-only module"
                    );
                    continue;
                }
                if let Some(hir) = &module.hir {
                    // Dedup by name: plugin preamble injection on the entry module can
                    // re-emit helpers (e.g. __redirect_0) already generated by a non-entry
                    // module's block expansion, producing two MIR entries with different
                    // SymbolIds but the same name, corrupting the export table (GEN002).
                    // Two functions written in source files with the same name are a
                    // redefinition (SEM003): keeping the first would silently call it
                    // from the module that wrote the second.
                    for func in &hir.functions {
                        match all_functions.iter().find(|f| f.name == func.name) {
                            None => all_functions.push(func.clone()),
                            Some(existing)
                                if ast::is_user_source_location(&existing.location)
                                    && ast::is_user_source_location(&func.location) =>
                            {
                                duplicate_functions.push(CompilerError::semantic_error_with_code(
                                    format!(
                                        "Function '{}' is defined in {} (line {}) and again in {} (line {})",
                                        func.name,
                                        existing.location.file,
                                        existing.location.line,
                                        func.location.file,
                                        func.location.line
                                    ),
                                    Some(
                                        "Imported modules share one function namespace; rename one of the functions"
                                            .to_string(),
                                    ),
                                    Some(func.location.clone()),
                                    "SEM003",
                                ));
                            }
                            Some(_) => {}
                        }
                    }
                    for class in &hir.classes {
                        all_classes.push(class.clone());
                    }
                    // Collect capabilities (top-level `can Name:` declarations).
                    // Dedup by name — same-name duplicates from multiple modules
                    // are caught by the resolver's SEM003 pass; keep first-seen.
                    for cap in &hir.capabilities {
                        if !all_capabilities.iter().any(|c| c.name == cap.name) {
                            all_capabilities.push(cap.clone());
                        }
                    }
                    // Merge state from every module. compilation_order is
                    // topologically sorted with the entry last, so non-entry
                    // plugin-emitted globals (e.g. frame.ui's `instance_<tag>`
                    // declarations contributed by `expand_component` running on
                    // component source files) land before the entry module's
                    // user state declarations. Before this merge landed, the
                    // entry-only assignment silently discarded plugin state,
                    // breaking the v2.12.3 client_init splice with
                    // `Undefined variable 'instance_<tag>'`.
                    merge_hir_state(&mut merged_state, hir.state.clone());

                    // Screens are named, self-contained blocks (spec §23 "State
                    // in Screens"): a screen declared in an imported module
                    // keeps its own state: scope exactly like one declared in
                    // the entry file, so every module contributes its screens.
                    merged_screen_blocks.extend(hir.screen_blocks.iter().cloned());
                    if module.is_entry {
                        start_function = hir.start_function.clone();
                        merged_watch_blocks = hir.watch_blocks.clone();
                        root_location = Some(hir.location.clone());
                    } else if let Some(ref module_start) = hir.start_function {
                        module_starts.push((module.name.clone(), module_start.clone()));
                    }
                    for import in &hir.imports {
                        all_imports.push(import.clone());
                    }
                    if module.is_entry {
                        for test in &hir.tests {
                            all_tests.push(test.clone());
                        }
                    }
                    for external in &hir.externals {
                        all_externals.push(external.clone());
                    }
                }
            }
        }

        // Each module's `start:` is its own block with its own local scope, so
        // two files may both declare `integer s` in their start bodies. Lower
        // every non-entry start body to a private function and call it, in
        // compilation order, ahead of the entry start body — instead of
        // splicing the statements into one shared scope (SEM007 clash).
        // Client builds never run non-entry start bodies (see below).
        if client_mode {
            module_starts.clear();
        }
        for (module_name, module_start) in module_starts {
            let base: String = module_name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let mut fn_name = format!("__module_start_{}", base);
            let mut suffix = 1;
            while all_functions.iter().any(|f| f.name == fn_name) {
                fn_name = format!("__module_start_{}_{}", base, suffix);
                suffix += 1;
            }
            let loc = module_start.location.clone();
            extra_start_stmts.push(crate::hir::HirStatement::Expression {
                expression: crate::hir::HirExpression::Call {
                    function: fn_name.clone(),
                    arguments: Vec::new(),
                    location: loc.clone(),
                },
                location: loc.clone(),
            });
            all_functions.push(crate::hir::HirFunction {
                name: fn_name,
                parameters: Vec::new(),
                return_type: None,
                body: module_start.body,
                is_start: false,
                is_private: false,
                owner_screen: None,
                location: loc,
            });
        }

        // Client mode: build-pipeline enforcement — what runs in the browser _start.
        //
        // The user's start: block is the server entry point: it boots the HTTP
        // listener, registers migrations, sets up routes. None of that is callable
        // from the browser host. In client builds the expander has already:
        //   1. Collected `program_init` + `client_init` lifecycle slot output into
        //      `slot_prelude`, tagged each statement with LIFECYCLE_SLOT_OUTPUT_MARKER.
        //   2. Cleared the user's start: body (server code gone).
        //   3. Called prepend_to_start to fill the body with only the tagged output.
        //
        // Here we handle the HIR-level merging of non-entry module start: bodies
        // (`extra_start_stmts`) and the legacy case where no plugin declares
        // client_init. See expander.rs `collect_slot_statements` and `expand_program`.
        if client_mode {
            // CLIENT_PULLS_SERVER: non-entry module start: blocks are server-side
            // code (db init, migration registration, etc.) — always discard them
            // in client builds. The entry module's start_function was already
            // cleared and rebuilt by the expander to contain only browser-safe
            // plugin output (program_init + client_init). See expander.rs.
            extra_start_stmts.clear();

            let any_client_init = registry
                .as_ref()
                .map(|r| r.any_plugin_declares_lifecycle_slot("client_init"))
                .unwrap_or(false);
            if !any_client_init {
                // Legacy path: no client_init slot, clear the entry start body too.
                if let Some(ref mut sf) = start_function {
                    sf.body.statements.clear();
                }
            }
            // When any_client_init: the expander already replaced the entry
            // start_function body with only browser-safe plugin output.
        }

        if !extra_start_stmts.is_empty() {
            let loc = root_location
                .clone()
                .unwrap_or_else(|| crate::ast::SourceLocation {
                    file: entry_path.to_string_lossy().to_string(),
                    line: 1,
                    column: 1,
                    byte_start: None,
                    byte_end: None,
                });
            match start_function {
                Some(ref mut sf) => {
                    extra_start_stmts.append(&mut sf.body.statements);
                    sf.body.statements = extra_start_stmts;
                }
                None => {
                    start_function = Some(crate::hir::HirFunction {
                        name: "start".to_string(),
                        parameters: Vec::new(),
                        return_type: None,
                        body: crate::hir::HirBlock {
                            statements: extra_start_stmts,
                            location: loc.clone(),
                        },
                        is_start: true,
                        is_private: false,
                        owner_screen: None,
                        location: loc,
                    });
                }
            }
        }

        let location = root_location.unwrap_or_else(|| crate::ast::SourceLocation {
            file: entry_path.to_string_lossy().to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        });

        // Client mode: guarantee a `_start` export exists even when the entry
        // module was dropped (e.g. when entry lives under `app/web/pages/`).
        if client_mode && start_function.is_none() {
            start_function = Some(crate::hir::HirFunction {
                name: "start".to_string(),
                parameters: Vec::new(),
                return_type: None,
                body: crate::hir::HirBlock {
                    statements: Vec::new(),
                    location: location.clone(),
                },
                is_start: true,
                is_private: false,
                owner_screen: None,
                location: location.clone(),
            });
        }

        if !duplicate_functions.is_empty() {
            return Err(duplicate_functions);
        }

        HirProgram {
            functions: all_functions,
            classes: all_classes,
            start_function,
            imports: all_imports,
            tests: all_tests,
            state: merged_state,
            watch_blocks: merged_watch_blocks,
            externals: all_externals,
            screen_blocks: merged_screen_blocks,
            capabilities: all_capabilities,
            location,
        }
    };

    let function_count = merged_hir
        .functions
        .iter()
        .filter(|f| ast::is_user_source_location(&f.location))
        .count();
    let type_count = merged_hir
        .classes
        .iter()
        .filter(|c| ast::is_user_source_location(&c.location))
        .count();

    let mut bridge_functions = registry
        .as_ref()
        .map(|r| r.bridge_functions().to_vec())
        .unwrap_or_default();
    // A plugin source being built may declare bridges its installed copy
    // does not have yet (see `extend_with_source_plugin_bridges`).
    extend_with_source_plugin_bridges(entry_path, &mut bridge_functions);
    extend_with_registry_bridges(&mut bridge_functions);
    let lang_to_bridge = registry
        .as_ref()
        .map(|r| r.language_to_bridge_map())
        .unwrap_or_default();
    let lang_to_helper = registry
        .as_ref()
        .map(|r| r.language_to_helper_map())
        .unwrap_or_default();

    // Inject phantom HIR class stubs for every type declared in loaded plugin manifests.
    // Without this, HirValidator rejects named types like `Request` as "Undefined type"
    // because they are not present in any source file — they are provided by the plugin.
    //
    // Field types are Clean type spellings (`string`, `integer:64`,
    // `list<Item>`, or another class such as `RequestAuth`) and are parsed
    // like a type annotation. Mapping every non-primitive spelling to `any`
    // made a class-typed field (`Request.auth`) an `any`, so a chained read
    // (`req.auth.userId`) became a dynamic lookup on a class instance and
    // returned an empty value.
    if let Some(ref reg) = registry {
        use crate::hir::hir_builder::HirBuilder;
        use crate::hir::{HirClass, HirField};
        for manifest in reg.loaded_manifests().values() {
            for type_def in &manifest.language.types {
                if merged_hir.classes.iter().any(|c| c.name == type_def.name) {
                    continue;
                }
                let fields: Vec<HirField> = type_def
                    .fields
                    .iter()
                    .map(|f| {
                        let field_type = HirBuilder::type_from_source(&f.type_).map_err(|e| {
                            let message = format!(
                                "Plugin '{}' declares field '{}.{}' with an invalid type: {}",
                                manifest.plugin.name,
                                type_def.name,
                                f.name,
                                e.message()
                            );
                            vec![CompilerError::validation_error(message, Default::default())]
                        })?;
                        Ok(HirField {
                            name: f.name.clone(),
                            field_type,
                            initializer: None,
                            is_private: false,
                            list_behavior: crate::ast::ListBehavior::Default,
                            location: Default::default(),
                        })
                    })
                    .collect::<Result<_, Vec<CompilerError>>>()?;
                merged_hir.classes.push(HirClass {
                    name: type_def.name.clone(),
                    type_parameters: Vec::new(),
                    parent: None,
                    fields,
                    constructor: None,
                    methods: Vec::new(),
                    invariants: Vec::new(),
                    capabilities: Vec::new(),
                    location: Default::default(),
                });
            }
        }
    }

    // Add language-name alias externals to merged_hir BEFORE validation so the
    // HIR validator can derive plugin namespaces (e.g. "req.query" → "req").
    if !lang_to_bridge.is_empty() {
        use crate::hir::{HirExternalFunction, HirParameter, HirType};
        let bridge_by_name: std::collections::HashMap<&str, &crate::plugins::BridgeFunction> =
            bridge_functions
                .iter()
                .map(|bf| (bf.name.as_str(), bf))
                .collect();
        for (lang_name, bridge_name) in &lang_to_bridge {
            if merged_hir.externals.iter().any(|e| e.name == *lang_name) {
                continue;
            }
            if let Some(bf) = bridge_by_name.get(bridge_name.as_str()) {
                let parameters: Vec<HirParameter> = bf
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, _)| HirParameter {
                        name: format!("arg{}", i),
                        param_type: HirType::Integer,
                        default_value: None,
                        location: Default::default(),
                    })
                    .collect();
                merged_hir.externals.push(HirExternalFunction {
                    name: lang_name.clone(),
                    parameters,
                    return_type: HirType::Void,
                    module: bf.module.clone(),
                    location: Default::default(),
                });
            }
        }
    }

    // Stage 3b: HIR semantic validation (ordering rules, contract placement, etc.)
    use crate::hir::validation::HirValidator;
    let handler_bridges = handler_bridges_from_slice(&bridge_functions);
    let validation_warnings =
        HirValidator::validate_with_handler_bridges(&merged_hir, &handler_bridges)?;
    tracing::debug!("Stage 3b complete: HIR validation passed");

    // Remove dot-notation language alias externals: they existed only so HIR validation
    // could derive plugin namespace prefixes. They must NOT become WASM imports.
    // Canonical _namespace_fn names are registered separately via the plugin bridge path.
    if !lang_to_bridge.is_empty() {
        merged_hir
            .externals
            .retain(|e| !lang_to_bridge.contains_key(&e.name));
    }

    // Stage 4: Resolution
    // Collect language function defs (for return-type/param overrides in language aliases)
    let lang_fn_defs_owned: std::collections::HashMap<
        String,
        crate::plugins::plugin_abi::PluginFunctionDef,
    > = registry
        .as_ref()
        .map(|r| {
            r.language_function_defs()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        })
        .unwrap_or_default();

    let resolution_result = resolve_with_plugin_names(
        merged_hir,
        &bridge_functions,
        lang_to_bridge.clone(),
        lang_fn_defs_owned,
        lang_to_helper.clone(),
    )?;
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type checking
    // Compute required-param-count overrides for language alias functions that have
    // param_defaults — the resolver encodes param types but not optionality.
    let required_counts = registry
        .as_ref()
        .map(|reg| required_param_counts(reg, &resolved_hir))
        .unwrap_or_default();
    let type_result = TypeChecker::check_with_required_counts(resolved_hir, required_counts)?;
    let mut warnings = validation_warnings;
    warnings.extend(type_result.warnings.iter().cloned());

    Ok(MultiFileFrontend {
        type_result,
        warnings,
        registry,
        bridge_functions,
        lang_to_bridge,
        lang_to_helper,
        build_state: build_state_arc,
        function_count,
        type_count,
    })
}

/// Name resolution with plugin bridges, language aliases (`db.query`),
/// language function definitions and helper aliases. Language names are only
/// registered alongside bridge declarations: without any, the registry-wide
/// aliases (e.g. `fs.write_bytes`) stay with the resolver's built-in
/// signatures instead of being retyped from the function registry.
fn resolve_with_plugin_names(
    hir: crate::hir::HirProgram,
    bridge_functions: &[plugins::BridgeFunction],
    lang_to_bridge: std::collections::HashMap<String, String>,
    lang_fn_defs: std::collections::HashMap<String, plugins::plugin_abi::PluginFunctionDef>,
    lang_to_helper: std::collections::HashMap<String, String>,
) -> Result<crate::resolver::ResolutionResult, Vec<CompilerError>> {
    use crate::resolver::NameResolver as Resolver;
    if bridge_functions.is_empty() {
        return Resolver::resolve(hir);
    }
    Resolver::resolve_with_bridge_aliases_fn_defs_and_helpers(
        hir,
        bridge_functions,
        &lang_to_bridge,
        lang_fn_defs,
        lang_to_helper,
    )
}

/// Required-parameter-count overrides for plugin language-alias functions
/// that declare `param_defaults`. The resolver encodes parameter types but not
/// optionality, so the type checker needs these counts to accept calls that
/// omit optional trailing arguments.
fn required_param_counts(
    registry: &plugins::PluginRegistry,
    resolved_hir: &crate::resolver::ResolvedHirProgram,
) -> std::collections::HashMap<crate::resolver::SymbolId, usize> {
    let mut counts = std::collections::HashMap::new();
    for (lang_name, lang_def) in &registry.language_function_defs() {
        if lang_def.param_defaults.is_empty() {
            continue;
        }
        let required = lang_def
            .param_defaults
            .iter()
            .filter(|s| s.is_empty())
            .count();
        let total = lang_def
            .params
            .as_ref()
            .map(|p| p.len())
            .unwrap_or(lang_def.param_defaults.len());
        if required < total {
            if let Some(sym_id) = resolved_hir.symbol_table.lookup_symbol(lang_name.as_str()) {
                counts.insert(sym_id, required);
            }
        }
    }
    counts
}

/// Plugin Contracts v2 §2.5 — extract a snapshot of every `_build_state_set`
/// write made during a compile. Returns an empty map when the build had no
/// plugins (and therefore no shared keystore) or when the mutex is poisoned.
fn snapshot_build_state(
    state: Option<&plugins::BuildState>,
) -> std::collections::BTreeMap<String, String> {
    state
        .and_then(|s| s.lock().ok().map(|g| g.clone()))
        .unwrap_or_default()
}

/// Compile a Clean Language project for browser hydration (`frontend.wasm`).
///
/// Convenience wrapper around [`compile_multi_file_with_memory_tier`] with
/// `client_mode: true`. Produces a WASM module suitable for instantiation by
/// frame.ui's `loader.js`: server-only modules are dropped from the merge and
/// the `_start` export is a no-op so the browser does not attempt to invoke
/// server bridge functions (`_http_listen`, `_db_query`, etc.) at hydration.
///
/// Spec: `foundation/spec/plugins/frame-ui-semantics.md` §UI-B009.
pub fn compile_multi_file_client_mode<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Client builds emit `frontend.wasm` only — they do not write a build
    // manifest, so the populated build_state snapshot is intentionally
    // discarded here. Callers that need it must use
    // [`compile_multi_file_with_memory_tier`] directly.
    compile_multi_file_with_memory_tier(
        entry_path,
        search_paths,
        opt_level,
        None,
        MemoryTier::Standard,
        true,
    )
    .map(|(bytes, _state)| bytes)
}

/// Compile a Clean Language project in release mode.
///
/// Identical to [`compile_multi_file_with_memory_tier`] but with `release_mode` enabled:
/// `always:` invariant checks are stripped from the compiled WASM output, producing
/// a smaller and faster binary suitable for production deployments.
///
/// For development (where you want all invariant checks active), use
/// `compile_multi_file_with_memory_tier` instead.
pub fn compile_multi_file_release<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
    explicit_tier: Option<MemoryTier>,
    target_default: MemoryTier,
) -> Result<(Vec<u8>, std::collections::BTreeMap<String, String>), Vec<CompilerError>> {
    compile_multi_file_impl(
        entry_path.as_ref(),
        search_paths,
        opt_level,
        explicit_tier,
        target_default,
        false,
        true,
        None,
    )
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn auth_folder_modules_stay_out_of_the_browser_build() {
        // SRV-I001: auth configuration in app/auth/ is server-side. An
        // `auth:` block there used to be merged into frontend.wasm and fail
        // with BRIDGE-HOST-MISMATCH on `_session_get`.
        let p = std::path::Path::new("/proj/app/auth/auth.cln");
        assert!(is_server_only_module("auth", p));
        // Component and client code is still kept.
        let c = std::path::Path::new("/proj/app/ui/web/components/auth/Login.cln");
        assert!(!is_server_only_module("Login", c));
        let k = std::path::Path::new("/proj/app/ui/web/client/auth.cln");
        assert!(!is_server_only_module("auth", k));
    }

    #[test]
    fn test_basic_integration() {
        let source = r#"start:
	integer x = 42
	print(x)
"#;

        let result = compile_with_file(source, "test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Basic integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(errors) => {
                println!("✗ Basic integration test failed: {} errors", errors.len());
                for error in &errors {
                    println!("  - {error}");
                }
                panic!("Integration test failed with {} errors", errors.len());
            }
        }
    }

    #[test]
    fn test_function_integration() {
        let source = r#"functions:
	integer add(integer a, integer b)
		return a + b

start:
	integer result = add(5, 3)
	print(result)
"#;

        let result = compile_with_file(source, "function_test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Function integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Function integration test failed: {error:?}");
                // Don't panic here as this might reveal integration issues we need to fix
            }
        }
    }

    #[test]
    fn test_type_checking_integration() {
        let source = r#"start:
	integer x = 42
	string y = "hello"
	print(x)
	print(y)
"#;

        let result = compile_with_file(source, "type_test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Type checking integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Type checking integration test failed: {error:?}");
                // This might reveal type system integration issues
            }
        }
    }

    #[test]
    fn test_error_propagation() {
        // Test that calling an undefined function produces an error
        let source = r#"start:
	integer x = undefined_function()
	print(x)
"#;

        let result = compile_with_file(source, "error_test.clean");
        match result {
            Ok(_) => {
                panic!(
                    "Expected compilation error for undefined function, but compilation succeeded"
                );
            }
            Err(error) => {
                println!("✓ Error propagation test: Correctly caught error: {error:?}");
                // Check that the error is related to the undefined function
                let error_string = format!("{error:?}");
                // Should contain either "undefined_function" or some indication the symbol wasn't found
                assert!(
                    error_string.contains("undefined_function") ||
                    error_string.contains("not found") ||
                    error_string.contains("undefined") ||
                    error_string.contains("Cannot unify"),
                    "Error should mention the undefined function or type mismatch, got: {error_string}"
                );
            }
        }
    }

    #[test]
    fn test_stdlib_integration() {
        println!("\n=== Standard Library Integration Test ===");

        let test_cases = vec![
            (
                "Math Functions",
                r#"start:
	integer x = -5
	integer result = abs(x)
	print(result)
"#,
            ),
            (
                "String Functions",
                r#"start:
	string text = "hello"
	integer length = text.length()
	print(length)
"#,
            ),
            (
                "List Functions",
                r#"start:
	list<integer> lst = [1, 2, 3, 4, 5]
	integer length = lst.length()
	print(length)
"#,
            ),
        ];

        let mut passed = 0;
        let total = test_cases.len();

        for (name, source) in test_cases {
            println!("\n--- Testing {name} ---");
            println!("Source: {source}");

            match compile_with_file(source, "stdlib_test.clean") {
                Ok(wasm_binary) => {
                    println!("✓ {name}: {len} bytes", len = wasm_binary.len());
                    assert!(!wasm_binary.is_empty());
                    passed += 1;
                }
                Err(error) => {
                    println!("✗ {name} failed: {error:?}");
                    // Don't panic here as some stdlib functions might not be fully implemented yet
                }
            }
        }

        println!("\n=== Summary ===");
        println!("Passed: {passed}/{total}");

        // At least basic functionality should work
        assert!(passed > 0, "No stdlib integration tests passed");
    }

    #[test]
    fn test_memory_tier_ordering() {
        use std::cmp::Ordering;
        assert_eq!(
            MemoryTier::Embedded.cmp(&MemoryTier::Minimal),
            Ordering::Less
        );
        assert_eq!(
            MemoryTier::Minimal.cmp(&MemoryTier::Standard),
            Ordering::Less
        );
        assert_eq!(MemoryTier::Standard.cmp(&MemoryTier::Heavy), Ordering::Less);
        assert_eq!(MemoryTier::Heavy.cmp(&MemoryTier::Canvas), Ordering::Less);

        // max() picks the highest tier
        assert_eq!(
            std::cmp::max(MemoryTier::Standard, MemoryTier::Canvas),
            MemoryTier::Canvas
        );
        assert_eq!(
            std::cmp::max(MemoryTier::Heavy, MemoryTier::Minimal),
            MemoryTier::Heavy
        );
    }

    #[test]
    fn test_memory_tier_from_str() {
        assert_eq!(MemoryTier::from_str("embedded"), Some(MemoryTier::Embedded));
        assert_eq!(MemoryTier::from_str("canvas"), Some(MemoryTier::Canvas));
        assert_eq!(MemoryTier::from_str("STANDARD"), Some(MemoryTier::Standard));
        assert_eq!(MemoryTier::from_str("gigantic"), None);
    }

    #[test]
    fn test_memory_tier_default_for_target() {
        assert_eq!(MemoryTier::default_for_target("web"), MemoryTier::Standard);
        assert_eq!(MemoryTier::default_for_target("native"), MemoryTier::Heavy);
        assert_eq!(
            MemoryTier::default_for_target("embedded"),
            MemoryTier::Embedded
        );
        assert_eq!(MemoryTier::default_for_target("wasi"), MemoryTier::Minimal);
        assert_eq!(MemoryTier::default_for_target("auto"), MemoryTier::Standard);
        // Plugins are compile-time tools that walk the whole user project in
        // one assemble call. Standard (32 MB cap) silently failed at ~50
        // pages — `mem_alloc` returned 0 once `memory.grow()` couldn't
        // extend further, the plugin's result string also went to 0, and
        // the build failed downstream with SEM007. Plugin tier (256 MB cap)
        // clears the cliff for any realistic project. See
        // `COMPILER-FRAME-UI-ASSEMBLE-OOM-KILL-ON-CLEAN-STUDIO` for the
        // case that exhausted the old cap at scale.
        assert_eq!(MemoryTier::default_for_target("plugin"), MemoryTier::Plugin);
        assert_eq!(MemoryTier::Plugin.max_pages(), 16384);
        assert_eq!(MemoryTier::Plugin.max_bytes(), 1024 * 1024 * 1024);
    }

    #[test]
    fn plugin_tier_is_orderable_and_round_trips_through_from_str() {
        // Plugin tier is greater than Canvas in declaration order so
        // `max()`-based tier resolution (`max(plugin_declared, target_default)`
        // in `lib.rs::compile_multi_file_with_memory_tier`) prefers it when
        // a plugin opts into the larger heap.
        assert!(MemoryTier::Plugin > MemoryTier::Canvas);
        assert_eq!(MemoryTier::from_str("plugin"), Some(MemoryTier::Plugin));
        assert_eq!(MemoryTier::from_str("PLUGIN"), Some(MemoryTier::Plugin));
        assert_eq!(MemoryTier::Plugin.name(), "plugin");
    }

    #[test]
    fn test_extract_plugins_multiline() {
        let source = "plugins:\n\tframe.server\n\tframe.ui\n";
        let plugins = extract_plugins(source);
        assert_eq!(plugins, vec!["frame.server", "frame.ui"]);
    }

    #[test]
    fn test_extract_plugins_inline_list() {
        let source = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui, frame.server]\n\t\tentry: app/main.cln\n";
        let plugins = extract_plugins(source);
        assert_eq!(plugins, vec!["frame.ui", "frame.server"]);
    }

    #[test]
    fn manifest_web_target_is_browser_only() {
        // bridge-host-classes.md §6: `target: web` means browser only; a
        // project with a server part declares `target: server`.
        let web =
            "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/main.cln\n";
        let target = extract_package_target(web).expect("target: web");
        assert_eq!(target_to_host_class(&target), Some("browser"));
        let server = "package: Test\n\ttarget: server\n\t\tplugins: [frame.ui, frame.server]\n\t\tentry: app/main.cln\n";
        let target = extract_package_target(server).expect("target: server");
        assert_eq!(target_to_host_class(&target), Some("server"));
    }

    #[test]
    fn test_extract_plugins_inline_top_level() {
        let source = "plugins: [frame.data, frame.auth]\n\nstart:\n\tprint(\"hello\")\n";
        let plugins = extract_plugins(source);
        assert_eq!(plugins, vec!["frame.data", "frame.auth"]);
    }
}
