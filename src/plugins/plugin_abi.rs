//! Plugin ABI Definitions
//!
//! Defines the interface between the compiler and WASM plugins

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Plugin manifest (plugin.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginInfo,
    #[serde(default)]
    pub compatibility: PluginCompatibility,
    pub handles: PluginHandles,
    #[serde(default)]
    pub exports: PluginExports,
    /// Per-block dispatch configuration introduced by Plugin Contracts v3
    /// (typed AST emission). Optional; when absent, every block dispatches
    /// through `[exports].expand` using the v1 string-emission ABI, which
    /// preserves backwards compatibility for every plugin that predates v3.
    ///
    /// See foundation/spec/plugins/contracts/typed-emission.md §2.1.
    #[serde(default)]
    pub blocks: HashMap<String, PluginBlockConfig>,
    /// Bridge functions that the plugin expects the runtime to provide
    #[serde(default)]
    pub bridge: PluginBridge,
    /// Language definitions for LSP support (static, no WASM required)
    #[serde(default)]
    pub language: PluginLanguage,
    /// AI context for agent-assisted development
    #[serde(default)]
    pub ai: PluginAiContext,
    /// Path ownership for manifest-driven plugin detection
    #[serde(default)]
    pub paths: PluginPaths,
    /// Enforcement rules for project structure conventions
    #[serde(default)]
    pub enforcement: PluginEnforcement,
    /// Memory budget tier requested by this plugin (MEMORY_POLICY.md §3.1 rule 3)
    #[serde(default)]
    pub memory: PluginMemory,
    /// Build provenance — stamped automatically by `cln compile` when building a plugin
    #[serde(default)]
    pub build: PluginBuildMeta,
    /// Plugin lifecycle slot declarations.
    /// Plugin Contracts v2 — see foundation/spec/plugins/contracts/lifecycle.md.
    #[serde(default)]
    pub lifecycle: PluginLifecycle,
    /// Side-channel artifacts the plugin declares it produces.
    /// Plugin Contracts v2 — see foundation/spec/plugins/contracts/artifacts.md.
    #[serde(default)]
    pub artifacts: Vec<PluginArtifact>,
}

/// Result of resolving the expand dispatch for a single block.
/// See foundation/spec/plugins/contracts/typed-emission.md §2.1 and §6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDispatch {
    /// WASM export name the compiler should call.
    pub export: String,
    /// Expansion ABI version (1 for v1 string emission, 3 for typed emission).
    pub version: u32,
}

impl PluginManifest {
    /// Highest expansion-ABI version this compiler implementation supports.
    /// Bump together with shipping a higher-version code path in `wasm_adapter`.
    pub const HIGHEST_SUPPORTED_EXPANSION_VERSION: u32 = 3;

    /// Returns true when the plugin opts into Plugin Contracts v3
    /// typed AST emission at the plugin level. Per-block opt-outs are
    /// still possible via `[blocks].<name>.version = 1`.
    pub fn opts_into_typed_emission(&self) -> bool {
        self.compatibility
            .expansion_version
            .as_deref()
            .map(|v| v == "3.0.0")
            .unwrap_or(false)
    }

    /// Resolve which WASM export to call and which ABI version to use for
    /// a given block name. The block name should be passed without its
    /// trailing colon (e.g. `"endpoints"`, not `"endpoints:"`).
    ///
    /// Resolution rules (typed-emission.md §6):
    ///   1. If `[blocks].<name>` is present, its `expand`/`version` win;
    ///      missing fields are filled from the plugin-level defaults.
    ///   2. If absent, use `[exports].expand` with version = 3 when the
    ///      plugin opts into typed emission at the plugin level, else 1.
    pub fn resolve_block_dispatch(&self, block_name: &str) -> BlockDispatch {
        let plugin_default_version = if self.opts_into_typed_emission() {
            3
        } else {
            1
        };
        match self.blocks.get(block_name) {
            Some(cfg) => BlockDispatch {
                export: cfg
                    .expand
                    .clone()
                    .unwrap_or_else(|| self.exports.expand.clone()),
                version: cfg.version.unwrap_or(plugin_default_version),
            },
            None => BlockDispatch {
                export: self.exports.expand.clone(),
                version: plugin_default_version,
            },
        }
    }
}

/// Stamped by `cln compile` into plugin.toml after a successful plugin build.
/// Used at load time to detect plugins built with compilers that had known codegen bugs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginBuildMeta {
    /// Compiler version that produced plugin.wasm (e.g. "0.30.154").
    /// Absent on plugins that predate this field.
    #[serde(default)]
    pub built_with_compiler: Option<String>,
}

/// Compiler version below which plugin WASM may be corrupted by known codegen bugs.
/// - 0.30.96: string comparison inversion fix (plugins built before this return
///   wrong results from any `if x == "literal"` check, causing expand_block to
///   emit garbage or nothing).
pub const MINIMUM_SAFE_PLUGIN_COMPILER: &str = "0.30.96";

/// Memory configuration declared in a plugin's `[memory]` section.
///
/// A plugin MAY declare `tier = "canvas"` (or any valid tier name) to raise
/// the default memory budget for projects that use it. The compiler picks
/// the highest tier among all active plugins (MEMORY_POLICY.md §3.1 rule 3).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginMemory {
    /// Memory tier this plugin expects. One of:
    /// "embedded", "minimal", "standard", "heavy", "canvas".
    /// Parsed via `MemoryTier::from_str`; unknown values produce a build error.
    #[serde(default)]
    pub tier: Option<String>,
}

/// Basic plugin information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
}

/// Compatibility requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCompatibility {
    #[serde(default = "default_min_compiler")]
    pub min_compiler_version: String,
    /// Clean Runtime ABI version the plugin's WASM was compiled against.
    /// Plugin Contracts v2 — see foundation/spec/plugins/contracts/runtime-abi.md.
    /// Absent on plugins that predate the versioned ABI; the loader treats absent
    /// as legacy ABI "0.0.0" and applies the hand-maintained stub set for
    /// backwards compatibility.
    #[serde(default)]
    pub abi_version: Option<String>,
    /// Highest expansion ABI version the plugin is built against.
    /// Plugin Contracts v3 — see foundation/spec/plugins/contracts/typed-emission.md §2.1.
    ///
    /// Absent or `"1.0.0"`: plugin uses v1 string emission for every block;
    /// the compiler re-parses the returned source. `"3.0.0"`: plugin opts
    /// into typed AST emission; per-block `version` in `[blocks]` selects
    /// which ABI a specific expand entry point uses.
    ///
    /// The compiler refuses to load a plugin whose declared `expansion_version`
    /// is higher than what the compiler supports (PLUGIN006).
    #[serde(default)]
    pub expansion_version: Option<String>,
    /// SHA-256 (lowercase hex, 64 chars) of the `typed-emission-ops.toml` file the
    /// plugin was compiled against. Validated at load time against `EMISSION_OPS_HASH`.
    ///
    /// Three-case loader (typed-emission.md §3.10 / Layer D step 2):
    ///   - Match   → load silently.
    ///   - Mismatch → refuse with PLUGIN006.
    ///   - Absent  → warn, load (refuse when `--strict-emission-ops` is active).
    ///
    /// Plugins compiled without typed-op bridge support omit this field.
    #[serde(default)]
    pub emission_ops_hash: Option<String>,
}

impl Default for PluginCompatibility {
    fn default() -> Self {
        Self {
            min_compiler_version: default_min_compiler(),
            abi_version: None,
            expansion_version: None,
            emission_ops_hash: None,
        }
    }
}

/// Per-block dispatch configuration (Plugin Contracts v3).
///
/// Allows a single plugin to mix v1 (string emission) and v3 (typed emission)
/// expand entry points on a block-by-block basis. See
/// foundation/spec/plugins/contracts/typed-emission.md §2.1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginBlockConfig {
    /// WASM export name to call for this block's expansion.
    /// When omitted, the compiler falls back to `[exports].expand`.
    #[serde(default)]
    pub expand: Option<String>,
    /// Expansion ABI version for this specific block.
    /// When omitted, defaults to 3 if `[compatibility].expansion_version`
    /// is `"3.0.0"`, otherwise 1. Set explicitly to `1` on a v3 plugin to
    /// opt a specific block out of typed emission during gradual migration.
    #[serde(default)]
    pub version: Option<u32>,
}

fn default_min_compiler() -> String {
    "0.15.0".to_string()
}

/// Block types and expression patterns this plugin handles.
///
/// `blocks` lists top-level block identifiers (e.g., `["data", "endpoints"]`).
///
/// `expressions` lists ORM-style expression patterns that the plugin recognises
/// as expression initiators inside function bodies (e.g., `["*.find:", "*.insert:"]`).
/// Patterns support a leading `*` wildcard that matches any identifier before the `.`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHandles {
    pub blocks: Vec<String>,
    #[serde(default)]
    pub expressions: Vec<String>,
}

/// Exported function names
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginExports {
    #[serde(default = "default_expand")]
    pub expand: String,
    #[serde(default)]
    pub validate: Option<String>,
    #[serde(default)]
    pub get_keywords: Option<String>,
    #[serde(default)]
    pub get_completions: Option<String>,
    #[serde(default)]
    pub get_hover_info: Option<String>,
    #[serde(default)]
    pub get_diagnostics: Option<String>,
    /// WASM export name for the server lifecycle hook.
    /// The function signature is `() -> i32` where the returned pointer is a
    /// length-prefixed JSON string containing a serialized `ServerRegistration`.
    #[serde(default)]
    pub register_server: Option<String>,
    /// WASM export name for the CLI lifecycle hook.
    /// The function signature is `() -> i32` where the returned pointer is a
    /// length-prefixed JSON string containing a serialized `CliRegistration`.
    #[serde(default)]
    pub register_cli: Option<String>,
    /// WASM export name for the data lifecycle hook.
    /// The function signature is `() -> i32` where the returned pointer is a
    /// length-prefixed JSON string containing a serialized `DataRegistration`.
    #[serde(default)]
    pub register_data: Option<String>,
    /// WASM export name for the build lifecycle hook.
    /// The function signature is `() -> i32` where the returned pointer is a
    /// length-prefixed JSON string containing a serialized `BuildRegistration`.
    #[serde(default)]
    pub register_build: Option<String>,
    /// WASM export name for the assemble hook.
    /// Called once before parsing with the full list of source files.
    /// Signature: `(input_ptr: i32, input_len: i32) -> i32` where the returned
    /// pointer is a length-prefixed JSON string containing a serialized `AssembleOutput`.
    #[serde(default)]
    pub assemble: Option<String>,
    /// WASM export name for the HTML-page processor.
    /// Invoked by the compiler when a manifest's `entry:` resolves to a file
    /// with one of the extensions declared in `[html].extensions` (e.g. `.html`).
    /// The plugin transforms the raw HTML into Clean Language source, which the
    /// compiler then parses as the entry module.
    ///
    /// Signature: `(html_ptr, html_len, path_ptr, path_len, registry_ptr,
    /// registry_len, companion_ptr, companion_len) -> i32` where the returned
    /// pointer is a length-prefixed Clean source string.
    #[serde(default)]
    pub process_html: Option<String>,
}

impl Default for PluginExports {
    fn default() -> Self {
        Self {
            expand: default_expand(),
            validate: None,
            get_keywords: None,
            get_completions: None,
            get_hover_info: None,
            get_diagnostics: None,
            register_server: None,
            register_cli: None,
            register_data: None,
            register_build: None,
            assemble: None,
            process_html: None,
        }
    }
}

// ============================================================================
// Plugin Lifecycle Registration Structs
// ============================================================================

/// Server registration returned by the `register_server` lifecycle hook.
///
/// Plugins that provide HTTP server middleware, startup logic, or shutdown
/// cleanup should return this struct (serialised as JSON) from their
/// `register_server` WASM export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerRegistration {
    /// Middleware function names to install on every request.
    #[serde(default)]
    pub middleware: Vec<String>,
    /// Optional startup hook function name called once before the server
    /// begins accepting connections.
    #[serde(default)]
    pub startup: Option<String>,
    /// Optional shutdown hook function name called once after the server
    /// stops accepting connections.
    #[serde(default)]
    pub shutdown: Option<String>,
}

/// CLI registration returned by the `register_cli` lifecycle hook.
///
/// Plugins that expose custom command-line commands should return this struct
/// (serialised as JSON) from their `register_cli` WASM export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CliRegistration {
    /// Commands provided by this plugin.
    #[serde(default)]
    pub commands: Vec<CliCommand>,
}

/// A single CLI command provided by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliCommand {
    /// The command name as it appears on the CLI (e.g., `"db:migrate"`).
    pub name: String,
    /// Human-readable description shown in `--help` output.
    pub description: String,
    /// WASM function name that handles this command.
    pub handler: String,
}

/// Data registration returned by the `register_data` lifecycle hook.
///
/// Plugins that contribute custom data types, validators, or query extensions
/// should return this struct (serialised as JSON) from their `register_data`
/// WASM export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataRegistration {
    /// Custom data type names contributed by this plugin.
    #[serde(default)]
    pub types: Vec<String>,
    /// Validator function names registered by this plugin.
    #[serde(default)]
    pub validators: Vec<String>,
    /// Query extension names registered by this plugin (e.g., custom SQL
    /// dialect extensions or ORM helpers).
    #[serde(default)]
    pub query_extensions: Vec<String>,
}

/// Build registration returned by the `register_build` lifecycle hook.
///
/// Plugins that hook into the build pipeline — e.g., to process assets,
/// run code generators, or perform post-build packaging — should return this
/// struct (serialised as JSON) from their `register_build` WASM export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildRegistration {
    /// Pre-build hook function names called before compilation starts.
    #[serde(default)]
    pub pre_build: Vec<String>,
    /// Post-build hook function names called after compilation finishes.
    #[serde(default)]
    pub post_build: Vec<String>,
    /// Asset processor function names that transform or copy static assets.
    #[serde(default)]
    pub asset_processors: Vec<String>,
}

// ============================================================================
// Plugin Assemble Hook Types
// ============================================================================

/// Input passed to a plugin's `assemble` hook.
///
/// The hook runs once per compilation, after all source files have been
/// discovered but before any parsing. Plugins can inject synthetic sources or
/// transform existing ones.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssembleInput {
    /// All source files in the compilation unit (path + raw content).
    #[serde(default)]
    pub source_files: Vec<AssembleSourceFile>,
    pub project_root: String,
    pub manifest_dir: String,
    pub has_frame_server: bool,
}

/// A single source file passed to the assemble hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembleSourceFile {
    pub path: String,
    pub content: String,
}

/// What the assemble hook returns.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssembleOutput {
    /// New virtual sources to inject into the compilation unit.
    #[serde(default)]
    pub injected_sources: Vec<InjectedSource>,
    /// Transformed versions of existing files (path → new content).
    #[serde(default)]
    pub transformed_sources: Vec<TransformedSource>,
}

/// A synthetic source file injected by a plugin's assemble hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectedSource {
    pub virtual_path: String,
    pub content: String,
}

/// A transformed version of an existing source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformedSource {
    pub path: String,
    pub content: String,
}

fn default_expand() -> String {
    "expand".to_string()
}

/// Bridge function declaration from plugin.toml
/// Defines a function that the plugin expects the runtime to provide
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeFunction {
    /// Function name (e.g., "_db_query")
    pub name: String,
    /// Parameter types as strings: "string", "integer", "number", "boolean", "void", "handler".
    /// v2 (contracts/bridge-host-classes.md §5) accepts tagged forms: "number:f64",
    /// "integer:i64", "string:lp" — propagated through MIR to close CODEGEN_F64.
    pub params: Vec<String>,
    /// Return type as string. Tagged forms accepted per v2 §5.
    pub returns: String,
    /// WASM import module name (defaults to "env")
    #[serde(default = "default_bridge_module")]
    pub module: String,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    /// Whether string parameters should be expanded to (ptr, len) pairs at WASM level
    #[serde(default)]
    pub expand_strings: bool,
    /// Host classes the function is available on.
    /// Plugin Contracts v2 — see foundation/spec/plugins/contracts/bridge-host-classes.md §2.
    /// Allowed values: "all" (default if omitted), "server", "browser", "native".
    /// Absent is treated as `["all"]` with a deprecation warning at plugin load.
    #[serde(default)]
    pub hosts: Option<Vec<String>>,
    /// Whether the server host provides a real implementation or a stub.
    /// v2 §3. None defaults to BridgeImpl::Real on hosts listed in `hosts`.
    #[serde(default)]
    pub server_impl: Option<BridgeImpl>,
    /// Whether the browser host provides a real implementation or a stub. v2 §3.
    #[serde(default)]
    pub browser_impl: Option<BridgeImpl>,
    /// Whether the native host provides a real implementation or a stub. v2 §3.
    #[serde(default)]
    pub native_impl: Option<BridgeImpl>,
    /// Stub behavior when server_impl = Stub. v2 §3.
    #[serde(default)]
    pub server_stub: Option<BridgeStub>,
    /// Stub behavior when browser_impl = Stub. v2 §3.
    #[serde(default)]
    pub browser_stub: Option<BridgeStub>,
    /// Stub behavior when native_impl = Stub. v2 §3.
    #[serde(default)]
    pub native_stub: Option<BridgeStub>,
    /// Module-callback contract for bridges that dispatch back into the WASM module.
    /// v2 §4 — closes SRV001 by giving _ui_render_page a documented purpose.
    #[serde(default)]
    pub callback: Option<BridgeCallback>,
}

fn default_bridge_module() -> String {
    "env".to_string()
}

impl Default for BridgeFunction {
    fn default() -> Self {
        Self {
            name: String::new(),
            params: Vec::new(),
            returns: String::new(),
            module: default_bridge_module(),
            description: None,
            expand_strings: false,
            hosts: None,
            server_impl: None,
            browser_impl: None,
            native_impl: None,
            server_stub: None,
            browser_stub: None,
            native_stub: None,
            callback: None,
        }
    }
}

/// AI context for agent-assisted development
///
/// Provides metadata that AI agents can use to understand what a plugin does,
/// how to use it, and what constraints apply. This section is optional and
/// purely informational — it has no effect on compilation.
///
/// # Example in plugin.toml
///
/// ```toml
/// [ai]
/// description = "HTTP endpoint DSL for Clean Language"
/// examples = ["examples/basic_api.cln", "examples/crud.cln"]
/// constraints = ["All endpoints must have authentication", "Use REST conventions"]
///
/// [[ai.patterns]]
/// name = "versioned-api"
/// use_when = "public API that must stay stable across releases"
/// anti_pattern = "no versioning — a breaking change breaks all clients"
/// example = "patterns/versioned-api.cln"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginAiContext {
    /// Natural-language description of what the plugin does, for AI agents
    #[serde(default)]
    pub description: Option<String>,
    /// Paths to example files demonstrating plugin usage
    #[serde(default)]
    pub examples: Vec<String>,
    /// Constraints or rules that AI agents should follow when generating code for this plugin
    #[serde(default)]
    pub constraints: Vec<String>,
    /// Named architectural patterns with use-when guidance and anti-pattern warnings
    #[serde(default)]
    pub patterns: Vec<PluginPattern>,
}

/// A named architectural pattern declared in a plugin's [ai] section.
///
/// Patterns give AI assistants the knowledge to choose the RIGHT architecture
/// for a problem class — not just write syntactically valid code. Each pattern
/// has a name, a one-sentence use-when guide, an anti-pattern warning, and a
/// path to a complete example file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginPattern {
    /// Kebab-case identifier (e.g. "multilingual-site")
    pub name: String,
    /// One sentence: when an AI should reach for this pattern
    pub use_when: String,
    /// What an AI without guidance would do instead, and precisely why it's wrong
    pub anti_pattern: String,
    /// Path to the example .cln file, relative to the plugin directory
    pub example: String,
}

/// Path ownership for manifest-driven plugin detection
///
/// When `implicit_import` is true, the compiler will automatically activate
/// this plugin for files located in any of the `owns` directories.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginPaths {
    /// Directories this plugin owns (e.g., ["app/backend/", "app/backend/api/"])
    #[serde(default)]
    pub owns: Vec<String>,
    /// Whether to auto-create owned directories when compiling
    #[serde(default)]
    pub auto_create: bool,
    /// File patterns this plugin applies to (e.g., ["*.cln"])
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Whether files in owned paths should implicitly import this plugin
    #[serde(default)]
    pub implicit_import: bool,
}

/// Enforcement rules for project structure conventions
///
/// Plugins can declare rules that the compiler will check during compilation.
/// Rules can emit warnings or errors depending on the `severity` level.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginEnforcement {
    /// "warn" for diagnostic warnings, "error" for compile errors
    #[serde(default = "default_enforcement_severity")]
    pub severity: String,
    /// Functions that should not be called directly (use DSL blocks instead)
    #[serde(default)]
    pub restricted_functions: Vec<RestrictedFunction>,
    /// Blocks required when a file is in a specific folder
    #[serde(default)]
    pub required_blocks: Vec<RequiredBlock>,
    /// Blocks that should only appear in specific folders
    #[serde(default)]
    pub block_folder_rules: Vec<BlockFolderRule>,
}

fn default_enforcement_severity() -> String {
    "warn".to_string()
}

/// A function that plugins restrict from direct use
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestrictedFunction {
    /// The raw bridge function name (e.g., "_http_route")
    pub name: String,
    /// What to use instead (e.g., "endpoints:")
    pub use_instead: String,
    /// Human-readable message explaining the restriction
    #[serde(default)]
    pub message: String,
}

/// A block required when a file is in a specific folder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredBlock {
    /// Folder path pattern (e.g., "app/backend/api/")
    pub folder: String,
    /// Required block name (e.g., "endpoints")
    pub block: String,
    /// Human-readable message
    #[serde(default)]
    pub message: String,
}

/// A rule restricting where a block can appear
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockFolderRule {
    /// Block name (e.g., "endpoints")
    pub block: String,
    /// Allowed folder paths (e.g., ["app/backend/", "app/server/"])
    pub allowed_in: Vec<String>,
    /// Human-readable message
    #[serde(default)]
    pub message: String,
}

/// Bridge section in plugin.toml
/// Contains all functions that the runtime must provide
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginBridge {
    #[serde(default)]
    pub functions: Vec<BridgeFunction>,
}

// ============================================================================
// Language Server Protocol (LSP) Static Definitions
// ============================================================================

/// Language definitions for LSP support in plugin.toml
///
/// This section allows plugins to provide static language definitions
/// that don't require WASM execution. The language server can use these
/// definitions to provide completions, hover documentation, and diagnostics.
///
/// # Example in plugin.toml
///
/// ```toml
/// [language]
/// blocks = ["data"]
/// owns_paths = ["app/data/"]
///
/// [[language.keywords]]
/// name = "find"
/// description = "Query records from a model"
/// context = "expression"
///
/// [[language.types]]
/// name = "Model"
/// description = "Base type for data models"
///
/// [[language.functions]]
/// name = "Data.tx"
/// signature = "Data.tx: block"
/// description = "Execute operations in a transaction"
///
/// [[language.completions]]
/// trigger = "data "
/// insert = "data ${1:ModelName}:\n\t${2:field}: ${3:type}"
/// description = "Create a new data model"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginLanguage {
    /// Block types this language definition applies to
    #[serde(default)]
    pub blocks: Vec<String>,
    /// Keyword definitions with context and documentation
    #[serde(default)]
    pub keywords: Vec<PluginKeyword>,
    /// Type definitions for the plugin's DSL
    #[serde(default)]
    pub types: Vec<PluginTypeDef>,
    /// Function definitions for the plugin's DSL
    #[serde(default)]
    pub functions: Vec<PluginFunctionDef>,
    /// Completion snippets for the plugin's DSL
    #[serde(default)]
    pub completions: Vec<PluginCompletionDef>,
    /// Path patterns that this plugin "owns" (for file-based plugin activation)
    /// E.g., ["app/data/", "app/models/"] means files in these directories
    /// should activate this plugin's language features
    #[serde(default)]
    pub owns_paths: Vec<String>,
}

/// A keyword defined by a plugin for its DSL
///
/// Keywords can be context-sensitive, appearing only in certain locations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginKeyword {
    /// The keyword name (e.g., "find", "where", "order")
    pub name: String,
    /// Human-readable description for hover and documentation
    pub description: String,
    /// The context where this keyword is valid
    /// One of: "expression", "block", "directive", "config", "attribute", "any"
    #[serde(default = "default_keyword_context")]
    pub context: String,
}

/// Default keyword context is "any" (valid everywhere)
fn default_keyword_context() -> String {
    "any".to_string()
}

/// A field on a plugin-declared type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTypeField {
    pub name: String,
    #[serde(rename = "type", default = "default_field_type")]
    pub type_: String,
    #[serde(default)]
    pub description: String,
}

fn default_field_type() -> String {
    "any".to_string()
}

/// A type definition for the plugin's DSL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTypeDef {
    /// The type name (e.g., "Model", "Query", "Relationship")
    pub name: String,
    /// Human-readable description for hover and documentation
    pub description: String,
    /// Optional field declarations so the type checker can resolve member access.
    /// Fields not listed here are implicitly typed as `any`.
    #[serde(default)]
    pub fields: Vec<PluginTypeField>,
}

/// A function definition for the plugin's DSL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFunctionDef {
    /// The function name (e.g., "Data.tx", "db.query", "req.param")
    pub name: String,
    /// Function signature for display (e.g., "Data.tx: block -> Result")
    pub signature: String,
    /// Human-readable description for hover and documentation
    pub description: String,
    /// Bridge function name this language function maps to (e.g., "_db_query").
    ///
    /// When present, the compiler registers `name` as an alias that resolves
    /// to the `maps_to` bridge function at every compilation stage: semantic
    /// analysis, name resolution and code generation.
    ///
    /// When absent the compiler attempts a convention-based derivation:
    ///   `req.param` → `_req_param`  (replace '.' with '_', prepend '_')
    /// If the derived name is not a known bridge function, the language function
    /// is treated as purely informational (LSP only, not callable).
    #[serde(default)]
    pub maps_to: Option<String>,
    /// Name of a plugin-emitted helper function (added to the user module via
    /// `_batch_func` + `_emit_helpers_batch`, or as an `expansion.functions`
    /// contribution during framework-block expansion) that implements this
    /// language function. Use for language APIs whose implementation is a
    /// Clean-language wrapper the plugin generates at expansion time, rather
    /// than a direct bridge call (e.g. `auth.jwt.sign` → the `jwt_sign`
    /// helper the `auth:` block emits, which reads `__jwt_secret` / `__jwt_alg`
    /// before delegating to `_jwt_sign`).
    ///
    /// Resolution precedence: `maps_to` > `maps_to_helper` > convention-derived
    /// bridge alias. Entries with `maps_to_helper` are callable by both the
    /// language name AND the helper name.
    #[serde(default)]
    pub maps_to_helper: Option<String>,
    /// Override the bridge function's parameter type list.
    /// When present, the registered external function uses these param types
    /// instead of the bridge function's declared params. Useful when the
    /// language-level function signature differs (e.g. optional args).
    #[serde(default)]
    pub params: Option<Vec<String>>,
    /// Override the bridge function's return type.
    #[serde(default)]
    pub returns: Option<String>,
    /// Per-parameter default values (as literal strings: "302", "{}", etc.).
    /// Parallel to `params`: index N is the default for param N.
    /// Empty string `""` means the param is required (no default).
    #[serde(default)]
    pub param_defaults: Vec<String>,
}

/// A completion snippet defined by the plugin
///
/// Completions can have triggers (prefixes that activate them) and
/// support VS Code snippet syntax with placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCompletionDef {
    /// The trigger text that activates this completion
    /// E.g., "data " (note trailing space) or "find"
    pub trigger: String,
    /// The text to insert (supports VS Code snippet syntax)
    /// E.g., "data ${1:ModelName}:\n\t${2:field}: ${3:type}"
    pub insert: String,
    /// Optional description shown in completion list
    #[serde(default)]
    pub description: Option<String>,
}

impl BridgeFunction {
    /// Convert a string type from plugin.toml to BuiltinType.
    ///
    /// Accepts base spellings ("string", "integer", "number", "boolean",
    /// "void", "handler", "ptr") AND the v2 tagged forms
    /// ("integer:i64", "string:lp") — both collapse to the same BuiltinType
    /// at the language level. Width information (i32 vs i64) is preserved
    /// separately via [`Self::param_is_i64`] / [`Self::return_is_i64`] so
    /// codegen can select the correct WASM ABI while the Clean-facing
    /// signature stays `integer` (i32).
    pub fn parse_type(type_str: &str) -> crate::builtins::registry::BuiltinType {
        use crate::builtins::registry::BuiltinType;
        // Strip tagged suffix ("integer:i64" → "integer") so language-level
        // typing is unaffected by the raw WASM width tag.
        let base = type_str.split(':').next().unwrap_or(type_str);
        match base.to_lowercase().as_str() {
            "string" => BuiltinType::String,
            "integer" | "int" | "i32" | "i64" | "long" => BuiltinType::Integer,
            "number" | "float" | "f64" => BuiltinType::Number,
            "boolean" | "bool" => BuiltinType::Boolean,
            "void" | "" => BuiltinType::Void,
            "handler" | "callback" => BuiltinType::Handler,
            // "ptr" means a pointer to a Clean string struct {length: u32, data: bytes}.
            // The server's write_string_to_caller always allocates this plain format — NOT a
            // boxed Any tagged-union. Mapping to String ensures no UnboxAnyToI32 is generated.
            "ptr" => BuiltinType::String,
            // "any" is the dynamic type — a pointer to a 12-byte boxed struct
            // `[tag@0:i32][value1@4:i32][value2@8:i32]` per foundation/spec/type-system.md.
            // Bridge functions declaring `params=["any"]` receive the raw boxed
            // pointer; the host implementation reads the tag and dispatches
            // on value1/value2. `returns="any"` symmetrically means the host
            // returns a raw boxed-Any pointer.
            "any" => BuiltinType::Any,
            _ => BuiltinType::Any, // Default to Any for unknown types
        }
    }

    /// Get parameter types as `BuiltinType` values from the builtins registry.
    pub fn get_param_types(&self) -> Vec<crate::builtins::registry::BuiltinType> {
        self.params.iter().map(|p| Self::parse_type(p)).collect()
    }

    /// Get return type as BuiltinType
    pub fn get_return_type(&self) -> crate::builtins::registry::BuiltinType {
        Self::parse_type(&self.returns)
    }

    /// True when the given param's raw WASM ABI is `i64`.
    ///
    /// Recognized spellings: bare `"i64"` / `"long"`, or the v2 tagged form
    /// `"integer:i64"`. All other integer spellings return false — the
    /// wrapper path then falls through to `builtin_type_to_wasm_type` which
    /// maps `Integer → I32`. Reading directly from the plugin/registry
    /// declaration keeps the width knowledge out of codegen (no hardcoded
    /// bridge names — architecture-boundary rule).
    pub fn param_is_i64(&self, idx: usize) -> bool {
        self.params
            .get(idx)
            .map(|s| Self::type_str_is_i64(s))
            .unwrap_or(false)
    }

    /// True when the return type's raw WASM ABI is `i64`. Same recognized
    /// spellings as [`Self::param_is_i64`].
    pub fn return_is_i64(&self) -> bool {
        Self::type_str_is_i64(&self.returns)
    }

    /// True when the declared param type is the raw `ptr` designator.
    ///
    /// `parse_type("ptr")` maps to `BuiltinType::String` so language-level
    /// typing keeps working for bridges returning length-prefixed buffers
    /// (see `parse_type` doc). But `expand_strings=true` bridges must NOT
    /// expand `ptr` params into (ptr, len) pairs — the caller already holds
    /// a single pointer (byte handle from `_req_body_bytes`, etc.). Use this
    /// predicate at wrapper-generation time to keep the raw-import shape
    /// aligned with the host and to skip the +4/i32.load unpack in the
    /// wrapper body. Any tagged suffix (`ptr:i64`) is preserved but only
    /// the base designator drives the match, mirroring `parse_type`.
    pub fn param_is_raw_ptr(&self, idx: usize) -> bool {
        self.params
            .get(idx)
            .map(|s| {
                let base = s.split(':').next().unwrap_or(s);
                base.eq_ignore_ascii_case("ptr")
            })
            .unwrap_or(false)
    }

    fn type_str_is_i64(s: &str) -> bool {
        let lower = s.to_lowercase();
        lower == "i64" || lower == "long" || lower == "integer:i64" || lower == "int:i64"
    }
}

/// Plugin ABI version
pub const PLUGIN_ABI_VERSION: u32 = 1;

// ============================================================================
// Plugin Contracts v2
// See foundation/spec/plugins/contracts/
//
// All fields are additive and #[serde(default)] so v1.0.0 plugins continue to
// load unchanged. A plugin opts into v2 by declaring any [lifecycle] slot, any
// hosts field on a bridge entry, any [[artifacts]] entry, or
// [compatibility].abi_version.
// ============================================================================

/// Plugin contract revision implemented by this compiler.
/// Sections §11+ of plugin-contract.md reference foundation/spec/plugins/contracts/.
pub const PLUGIN_CONTRACT_VERSION: &str = "1.0.0";

/// Clean Runtime ABI versions this compiler can load plugin WASM stamped against.
/// See contracts/runtime-abi.md §5. Missing stamp ("0.0.0") falls back to the
/// hand-maintained stub block in wasm_adapter.rs for backwards compatibility.
pub const SUPPORTED_RUNTIME_ABI_VERSIONS: &[&str] = &["1.0.0"];

/// Clean Runtime ABI version emitted by `cln compile` when building a plugin.
pub const DEFAULT_RUNTIME_ABI_VERSION: &str = "1.0.0";

/// Expansion ABI versions this compiler accepts in a plugin's
/// `[compatibility].expansion_version` field. A plugin declaring a version
/// outside this set is refused at load time with PLUGIN006.
/// See `foundation/spec/plugins/contracts/typed-emission.md` §6.
pub const SUPPORTED_EXPANSION_VERSIONS: &[&str] = &["1.0.0", "3.0.0"];

/// SHA-256 (lowercase hex) of `foundation/spec/plugins/contracts/typed-emission-ops.toml`
/// at the time this compiler was built. Plugins that declare `emission_ops_hash`
/// in `[compatibility]` are validated against this value at load time.
///
/// Three-case loader behaviour (typed-emission.md §3.10 / Layer D step 2):
///   - Match   → load silently; plugin may use `_expr_binop_op` / `_expr_unop_op`.
///   - Mismatch → refuse with PLUGIN006 (actionable reinstall message).
///   - Absent  → warn only; plugin loads but lacks typed-op bridge capability.
///
/// When `--strict-emission-ops` is set, the Absent case is promoted to Refuse.
///
/// The build script (`build.rs`) computes this hash from the raw file bytes.
/// Sentinel "0000…0000" (64 zeros) means the spec TOML was not present at
/// build time and hash validation is disabled.
pub const EMISSION_OPS_HASH: &str = env!("EMISSION_OPS_HASH");

/// Plugin Contracts v2 — shared per-build state.
/// See `foundation/spec/plugins/contracts/lifecycle.md` §2.5.
///
/// `BuildState` is a thread-safe key/value store the compiler owns for the
/// duration of one `cln compile` / `cln build` invocation. Plugins read and
/// write through the `_build_state_set` / `_build_state_get` bridges provided
/// by the plugin sandbox; the final snapshot is published in
/// `dist/build-manifest.json` so hosts can read the state at startup.
///
/// Keys should be plugin-namespaced (`frame.ui:components`,
/// `frame.server:routes`). The `__compiler:` prefix is reserved for compiler
/// internal keys.
pub type BuildState = std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>;

/// Construct a fresh, empty build state for a new compilation pass.
pub fn new_build_state() -> BuildState {
    std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()))
}

/// Plugin Contracts v2 — JSON build context passed to every lifecycle slot
/// call (and every `artifact_emitters` callback).
/// See `foundation/spec/plugins/contracts/lifecycle.md` §2.1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildContext {
    /// Host class the build is producing for.
    /// `"server"`, `"browser"`, or `"native"`.
    pub target: String,
    /// True iff `target == "browser"`. Convenience for plugins that key on
    /// "this is the nested client build."
    pub client_mode: bool,
    /// Path to the `.cln` entry file the build was invoked on, relative to
    /// the project root.
    pub entry_path: String,
    /// Directory artifacts are written into.
    pub output_dir: String,
    /// Path to the main WASM output, relative to `output_dir`.
    pub main_wasm: String,
    /// Compiler version producing the build.
    pub compiler_version: String,
    /// Plugin contract version this compiler implements.
    pub contract_version: String,
    /// Clean runtime ABI version emitted into module imports.
    pub runtime_abi_version: String,
    /// Artifacts produced earlier in the build. Empty for slots that run
    /// before any artifact emission.
    #[serde(default)]
    pub artifacts_so_far: Vec<BuildContextArtifact>,
    /// Snapshot of the compiler-owned build state at the moment this slot
    /// is invoked. Read-only — plugins MUST use `_build_state_set` to write.
    #[serde(default)]
    pub build_state: std::collections::BTreeMap<String, String>,
}

/// Minimal artifact view passed in the build context. Avoids leaking
/// compiler-internal fields like `size_bytes` / `sha256` to plugins.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildContextArtifact {
    pub name: String,
    pub path_relative: String,
    pub purpose: String,
    pub public: bool,
}

impl BuildContext {
    /// Construct a context with the current compiler/contract/abi versions
    /// and sensible defaults. Callers fill in target/entry_path/output_dir.
    pub fn new() -> Self {
        Self {
            target: "server".to_string(),
            client_mode: false,
            entry_path: String::new(),
            output_dir: String::new(),
            main_wasm: String::new(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            contract_version: PLUGIN_CONTRACT_VERSION.to_string(),
            runtime_abi_version: DEFAULT_RUNTIME_ABI_VERSION.to_string(),
            artifacts_so_far: Vec::new(),
            build_state: std::collections::BTreeMap::new(),
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self.client_mode = self.target == "browser";
        self
    }
    pub fn with_entry(mut self, entry: impl Into<String>) -> Self {
        self.entry_path = entry.into();
        self
    }
    pub fn with_output_dir(mut self, dir: impl Into<String>) -> Self {
        self.output_dir = dir.into();
        self
    }
    pub fn with_main_wasm(mut self, path: impl Into<String>) -> Self {
        self.main_wasm = path.into();
        self
    }
    /// Take a snapshot of the current build state into `build_state`.
    /// Called just before passing the context to a slot per
    /// contracts/lifecycle.md §2.1 (the snapshot reflects writes from
    /// earlier expand_block / slot calls in this build).
    pub fn snapshot_build_state(&mut self, state: &BuildState) {
        if let Ok(guard) = state.lock() {
            self.build_state = guard.clone();
        }
    }
}

/// Plugin lifecycle slot declarations.
/// See foundation/spec/plugins/contracts/lifecycle.md.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginLifecycle {
    /// WASM export contributing per-file helper functions and classes.
    /// Replaces the `__preamble` magic block for plugins that opt into v2.
    #[serde(default)]
    pub module_helpers: Option<String>,
    /// If true, BFS reachability treats module_helpers output as roots.
    /// Closes GEN003 for plugins whose helpers are called from generated code
    /// that isn't statically reachable from user entry points.
    #[serde(default)]
    pub module_helpers_are_roots: bool,
    /// WASM export contributing program _start prelude statements.
    #[serde(default)]
    pub program_init: Option<String>,
    /// WASM export contributing client _start body (browser builds only).
    /// Closes HYDRATE_AUTO by replacing the compiler's no-op rewrite.
    #[serde(default)]
    pub client_init: Option<String>,
    /// WASM export contributing server bootstrap body (server builds only).
    #[serde(default)]
    pub server_init: Option<String>,
    /// WASM export contributing per-request handler prelude.
    #[serde(default)]
    pub per_request: Option<String>,
    /// Entry-point shapes per_request applies to.
    /// Default ["http"] when per_request is set. Other values: "sse", "ws".
    #[serde(default)]
    pub per_request_targets: Vec<String>,
    /// WASM export returning dynamic [[artifacts]] entries at build time.
    /// Most plugins declare artifacts statically; this slot is for plugins
    /// that need to compute the set from the build context.
    #[serde(default)]
    pub artifact_emitters: Option<String>,
}

impl PluginLifecycle {
    /// Returns true if the plugin declares any v2 lifecycle slot.
    /// Used to choose between v2 dispatch and the v1.0.0 `__preamble` path.
    pub fn opts_into_v2(&self) -> bool {
        self.module_helpers.is_some()
            || self.program_init.is_some()
            || self.client_init.is_some()
            || self.server_init.is_some()
            || self.per_request.is_some()
            || self.artifact_emitters.is_some()
    }

    /// Returns true if the plugin.toml declared a `[lifecycle]` section
    /// with at least one field set (any slot, or `module_helpers_are_roots`).
    /// Phase D refuses plugins whose plugin.toml has no `[lifecycle]` section
    /// via PLUGIN010 (LegacyV1PluginRejected).
    pub fn has_lifecycle_declaration(&self) -> bool {
        self.opts_into_v2() || self.module_helpers_are_roots
    }
}

/// Build artifact a plugin declares it produces.
/// See foundation/spec/plugins/contracts/artifacts.md §2.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginArtifact {
    /// Conventional file name (e.g. "frontend.wasm", "theme.css").
    /// Must be unique across loaded plugins.
    pub name: String,
    /// Documented purpose. See contracts/artifacts.md §4 for the set.
    /// Values: "client_hydration", "static_asset", "manifest", "data_migration".
    pub purpose: String,
    /// Source for the artifact bytes. Either `emit` or `static_path` is required.
    #[serde(default)]
    pub emit: Option<ArtifactSource>,
    /// Path inside the plugin directory to a precomputed file.
    #[serde(default)]
    pub static_path: Option<String>,
    /// Output path relative to the build directory.
    /// Supports `{output_dir}` substitution.
    pub output_relative: String,
    /// When the artifact must be produced.
    /// Values: "always" (default), "never", "has_client_init",
    /// "has_lifecycle.<slot>", "has_artifact.<name>".
    #[serde(default = "default_required_when")]
    pub required_when: String,
    /// Whether the artifact is served to web clients.
    #[serde(default)]
    pub public: bool,
    /// Cache key hint: "build_input_hash" (default), "plugin_version", "never".
    #[serde(default = "default_cache_hint")]
    pub cache: String,
    /// MIME type. Inferred from extension if absent.
    #[serde(default)]
    pub content_type: Option<String>,
}

fn default_required_when() -> String {
    "always".to_string()
}

fn default_cache_hint() -> String {
    "build_input_hash".to_string()
}

/// How an artifact's bytes are produced.
/// See contracts/artifacts.md §3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArtifactSource {
    /// Plugin exports a WASM function returning the bytes.
    /// The string is the export name.
    Callback(String),
    /// Compiler performs a named build operation to produce the bytes.
    /// Documented values: "client_only_build", "server_only_build", "manifest".
    Module { from_module: String },
}

/// Whether a host class is expected to provide a real implementation or a stub.
/// See contracts/bridge-host-classes.md §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeImpl {
    /// Host MUST provide the real implementation.
    Real,
    /// Host registers a stub. The stub's behavior is declared in the
    /// matching `*_stub` block.
    Stub,
}

/// Stub behavior for a bridge function on hosts that don't provide a real impl.
/// See contracts/bridge-host-classes.md §3.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BridgeStub {
    /// Literal return value as a string ("0", "false", "\"\"", "null").
    /// Mutually exclusive with `no_op`.
    #[serde(default)]
    pub returns: Option<String>,
    /// If true, the stub is void-equivalent (returns nothing).
    #[serde(default)]
    pub no_op: bool,
}

/// Module-callback contract for bridges that dispatch back into the WASM module.
/// See contracts/bridge-host-classes.md §4.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BridgeCallback {
    /// One of the documented purposes:
    /// "component_tag_render", "route_dispatch", "migration_apply", "event_dispatch".
    pub purpose: String,
    /// Which plugin's exports the host should look up.
    pub plugin_target: String,
    /// How the host finds the right export.
    /// Values: "exports_matching", "manifest_lookup", "explicit_argument".
    pub discovery: String,
    /// When discovery is "exports_matching", the symbol pattern
    /// with `{placeholder}` substitution (e.g. "{tagname}_render").
    #[serde(default)]
    pub export_pattern: Option<String>,
    /// What to do when no matching export is found.
    /// Values: "passthrough" (default), "error", "empty".
    #[serde(default = "default_callback_fallback")]
    pub fallback: String,
}

fn default_callback_fallback() -> String {
    "passthrough".to_string()
}

/// Expected WASM exports from a plugin
pub struct PluginAbi;

impl PluginAbi {
    /// Minimum required exports
    pub const REQUIRED_EXPORTS: &'static [&'static str] = &[
        "expand", // (block_ptr: i32, block_len: i32) -> result_ptr: i32
        "memory", // WebAssembly.Memory
    ];

    /// Optional exports
    pub const OPTIONAL_EXPORTS: &'static [&'static str] = &[
        "validate",        // (block_ptr: i32, block_len: i32) -> error_ptr: i32
        "get_keywords",    // () -> keywords_ptr: i32
        "get_completions", // (ctx_ptr: i32, ctx_len: i32) -> completions_ptr: i32
        "get_hover_info",  // (keyword_ptr: i32, keyword_len: i32) -> info_ptr: i32
        "get_diagnostics", // (content_ptr: i32, content_len: i32) -> diags_ptr: i32
        "register_server", // () -> json_ptr: i32  (ServerRegistration JSON)
        "register_cli",    // () -> json_ptr: i32  (CliRegistration JSON)
        "register_data",   // () -> json_ptr: i32  (DataRegistration JSON)
        "register_build",  // () -> json_ptr: i32  (BuildRegistration JSON)
    ];

    /// Validate that a module exports required functions
    pub fn validate_module(exports: &[String]) -> Result<(), String> {
        for required in Self::REQUIRED_EXPORTS {
            if !exports.contains(&required.to_string()) {
                return Err(format!("Plugin missing required export: {}", required));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_deserialization() {
        let toml_str = r#"
            [plugin]
            name = "test.plugin"
            version = "1.0.0"
            description = "Test plugin"
            author = "Test Author"

            [compatibility]
            min_compiler_version = "0.15.0"

            [handles]
            blocks = ["test", "example"]

            [exports]
            expand = "expand"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.plugin.name, "test.plugin");
        assert_eq!(manifest.plugin.version, "1.0.0");
        assert_eq!(manifest.handles.blocks, vec!["test", "example"]);
    }

    #[test]
    fn test_manifest_minimal() {
        let toml_str = r#"
            [plugin]
            name = "minimal.plugin"
            version = "0.1.0"

            [handles]
            blocks = ["minimal"]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.plugin.name, "minimal.plugin");
        assert_eq!(manifest.compatibility.min_compiler_version, "0.15.0");
        assert_eq!(manifest.exports.expand, "expand");
    }

    #[test]
    fn test_validate_module() {
        let exports = vec!["expand".to_string(), "memory".to_string()];
        assert!(PluginAbi::validate_module(&exports).is_ok());

        let missing = vec!["memory".to_string()];
        assert!(PluginAbi::validate_module(&missing).is_err());
    }

    #[test]
    fn test_manifest_with_bridge() {
        let toml_str = r#"
            [plugin]
            name = "frame.data"
            version = "1.0.0"

            [handles]
            blocks = ["model", "query"]

            [bridge]
            functions = [
                { name = "_db_query", params = ["string", "string"], returns = "string", expand_strings = true },
                { name = "_db_execute", params = ["string", "string"], returns = "integer", expand_strings = true },
                { name = "_db_begin", params = [], returns = "string" },
            ]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.plugin.name, "frame.data");
        assert_eq!(manifest.bridge.functions.len(), 3);

        let query_fn = &manifest.bridge.functions[0];
        assert_eq!(query_fn.name, "_db_query");
        assert_eq!(query_fn.params, vec!["string", "string"]);
        assert_eq!(query_fn.returns, "string");
        assert!(query_fn.expand_strings);
        assert_eq!(query_fn.module, "env"); // Default module

        let begin_fn = &manifest.bridge.functions[2];
        assert_eq!(begin_fn.name, "_db_begin");
        assert!(begin_fn.params.is_empty());
        assert!(!begin_fn.expand_strings); // Default is false
    }

    #[test]
    fn test_bridge_function_type_parsing() {
        use crate::builtins::registry::BuiltinType;

        assert_eq!(BridgeFunction::parse_type("string"), BuiltinType::String);
        assert_eq!(BridgeFunction::parse_type("integer"), BuiltinType::Integer);
        assert_eq!(BridgeFunction::parse_type("number"), BuiltinType::Number);
        assert_eq!(BridgeFunction::parse_type("boolean"), BuiltinType::Boolean);
        assert_eq!(BridgeFunction::parse_type("void"), BuiltinType::Void);
        assert_eq!(BridgeFunction::parse_type("i32"), BuiltinType::Integer);
        assert_eq!(BridgeFunction::parse_type("f64"), BuiltinType::Number);
    }

    #[test]
    fn test_manifest_with_language() {
        let toml_str = r#"
            [plugin]
            name = "frame.data"
            version = "1.0.0"

            [handles]
            blocks = ["data"]

            [language]
            blocks = ["data"]
            owns_paths = ["app/data/"]

            [[language.keywords]]
            name = "find"
            description = "Query records from a model"
            context = "expression"

            [[language.keywords]]
            name = "where"
            description = "Filter query conditions"
            context = "block"

            [[language.types]]
            name = "Model"
            description = "Base type for data models"

            [[language.functions]]
            name = "Data.tx"
            signature = "Data.tx: block"
            description = "Execute operations in a transaction"

            [[language.completions]]
            trigger = "data "
            insert = "data ${1:ModelName}:\n\t${2:field}: ${3:type}"
            description = "Create a new data model"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.plugin.name, "frame.data");
        assert_eq!(manifest.language.blocks, vec!["data"]);
        assert_eq!(manifest.language.owns_paths, vec!["app/data/"]);

        // Test keywords
        assert_eq!(manifest.language.keywords.len(), 2);
        assert_eq!(manifest.language.keywords[0].name, "find");
        assert_eq!(manifest.language.keywords[0].context, "expression");
        assert_eq!(manifest.language.keywords[1].name, "where");
        assert_eq!(manifest.language.keywords[1].context, "block");

        // Test types
        assert_eq!(manifest.language.types.len(), 1);
        assert_eq!(manifest.language.types[0].name, "Model");

        // Test functions
        assert_eq!(manifest.language.functions.len(), 1);
        assert_eq!(manifest.language.functions[0].name, "Data.tx");

        // Test completions
        assert_eq!(manifest.language.completions.len(), 1);
        assert_eq!(manifest.language.completions[0].trigger, "data ");
        assert!(manifest.language.completions[0]
            .insert
            .contains("ModelName"));
    }

    #[test]
    fn test_language_defaults() {
        let toml_str = r#"
            [plugin]
            name = "minimal.plugin"
            version = "0.1.0"

            [handles]
            blocks = ["minimal"]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        // Language should be default (empty)
        assert!(manifest.language.blocks.is_empty());
        assert!(manifest.language.keywords.is_empty());
        assert!(manifest.language.types.is_empty());
        assert!(manifest.language.functions.is_empty());
        assert!(manifest.language.completions.is_empty());
        assert!(manifest.language.owns_paths.is_empty());
    }

    #[test]
    fn test_keyword_default_context() {
        let toml_str = r#"
            [plugin]
            name = "test.plugin"
            version = "1.0.0"

            [handles]
            blocks = ["test"]

            [[language.keywords]]
            name = "testkw"
            description = "Test keyword without context"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.language.keywords.len(), 1);
        assert_eq!(manifest.language.keywords[0].context, "any"); // Default context
    }

    #[test]
    fn test_manifest_with_ai_context() {
        let toml_str = r#"
            [plugin]
            name = "frame.web"
            version = "1.0.0"

            [handles]
            blocks = ["endpoints"]

            [ai]
            description = "HTTP endpoint DSL for Clean Language"
            examples = ["examples/basic_api.cln", "examples/crud.cln"]
            constraints = ["All endpoints must have authentication", "Use REST conventions"]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.plugin.name, "frame.web");
        assert_eq!(
            manifest.ai.description.as_deref(),
            Some("HTTP endpoint DSL for Clean Language")
        );
        assert_eq!(manifest.ai.examples.len(), 2);
        assert_eq!(manifest.ai.examples[0], "examples/basic_api.cln");
        assert_eq!(manifest.ai.constraints.len(), 2);
        assert_eq!(
            manifest.ai.constraints[0],
            "All endpoints must have authentication"
        );
    }

    #[test]
    fn test_manifest_with_ai_patterns() {
        let toml_str = r#"
            [plugin]
            name = "frame.data"
            version = "1.0.0"

            [handles]
            blocks = ["data"]

            [[ai.patterns]]
            name = "multilingual-site"
            use_when = "app serves content in more than one language"
            anti_pattern = "storing translations as a JSON column — prevents SQL indexing"
            example = "patterns/multilingual-site.cln"

            [[ai.patterns]]
            name = "soft-delete"
            use_when = "records must be deactivatable without permanent removal"
            anti_pattern = "boolean is_deleted column — loses the deletion timestamp"
            example = "patterns/soft-delete.cln"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.ai.patterns.len(), 2);
        assert_eq!(manifest.ai.patterns[0].name, "multilingual-site");
        assert_eq!(
            manifest.ai.patterns[0].example,
            "patterns/multilingual-site.cln"
        );
        assert!(!manifest.ai.patterns[0].use_when.is_empty());
        assert!(!manifest.ai.patterns[0].anti_pattern.is_empty());
        assert_eq!(manifest.ai.patterns[1].name, "soft-delete");
    }

    #[test]
    fn test_ai_context_defaults() {
        let toml_str = r#"
            [plugin]
            name = "minimal.plugin"
            version = "0.1.0"

            [handles]
            blocks = ["minimal"]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        // AI context should be default (empty)
        assert!(manifest.ai.description.is_none());
        assert!(manifest.ai.examples.is_empty());
        assert!(manifest.ai.constraints.is_empty());
        assert!(manifest.ai.patterns.is_empty());
    }

    #[test]
    fn test_manifest_with_memory_tier() {
        let toml_str = r#"
            [plugin]
            name = "frame.canvas"
            version = "1.0.0"

            [handles]
            blocks = ["scene"]

            [memory]
            tier = "canvas"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.memory.tier, Some("canvas".to_string()));
    }

    #[test]
    fn test_manifest_without_memory_section() {
        let toml_str = r#"
            [plugin]
            name = "frame.data"
            version = "1.0.0"

            [handles]
            blocks = ["data"]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.memory.tier, None);
    }

    #[test]
    fn test_manifest_memory_tier_unknown_string_parses() {
        // The toml parsing itself succeeds for any string —
        // validation happens in PluginRegistry::resolve_plugin_memory_tier()
        let toml_str = r#"
            [plugin]
            name = "bad.plugin"
            version = "1.0.0"

            [handles]
            blocks = ["bad"]

            [memory]
            tier = "gigantic"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.memory.tier, Some("gigantic".to_string()));
        // Validation that "gigantic" is invalid happens at resolve time, not parse time
        assert!(crate::MemoryTier::from_str("gigantic").is_none());
    }

    #[test]
    fn test_plugin_type_fields_inline() {
        let toml_str = r#"
            [plugin]
            name = "frame.server"
            version = "2.3.0"

            [handles]
            blocks = ["endpoints"]

            [language]
            types = [
              { name = "Request", description = "HTTP request context", fields = [{ name = "query", type = "any", description = "Query params" }, { name = "auth", type = "any" }] },
              { name = "Response", description = "HTTP response object" },
            ]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.language.types.len(), 2);
        let request_type = &manifest.language.types[0];
        assert_eq!(request_type.name, "Request");
        assert_eq!(
            request_type.fields.len(),
            2,
            "Request should have 2 fields but got {}; fields: {:?}",
            request_type.fields.len(),
            request_type
                .fields
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
        assert_eq!(request_type.fields[0].name, "query");
        assert_eq!(request_type.fields[0].type_, "any");
        let response_type = &manifest.language.types[1];
        assert_eq!(response_type.name, "Response");
        assert_eq!(response_type.fields.len(), 0);
    }

    // ========================================================================
    // Plugin Contracts v2 (foundation/spec/plugins/contracts/)
    // ========================================================================

    #[test]
    fn test_v1_manifest_loads_with_empty_v2_defaults() {
        // A v1.0.0 plugin.toml must continue to load unchanged.
        // v2 fields default to absent / empty / false.
        let toml_str = r#"
            [plugin]
            name = "legacy.plugin"
            version = "1.0.0"

            [handles]
            blocks = ["legacy"]
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.plugin.name, "legacy.plugin");
        // v2 fields all default
        assert!(manifest.compatibility.abi_version.is_none());
        assert!(!manifest.lifecycle.opts_into_v2());
        assert!(!manifest.lifecycle.module_helpers_are_roots);
        assert!(manifest.artifacts.is_empty());
    }

    #[test]
    fn test_v2_manifest_lifecycle_section_parses() {
        let toml_str = r#"
            [plugin]
            name = "frame.ui"
            version = "2.6.12"

            [compatibility]
            abi_version = "1.0.0"

            [handles]
            blocks = ["component"]

            [lifecycle]
            module_helpers = "emit_ui_helpers"
            module_helpers_are_roots = true
            client_init = "emit_ui_client_init"
            artifact_emitters = "emit_ui_artifacts"
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.compatibility.abi_version.as_deref(), Some("1.0.0"));
        assert!(manifest.lifecycle.opts_into_v2());
        assert!(manifest.lifecycle.module_helpers_are_roots);
        assert_eq!(
            manifest.lifecycle.module_helpers.as_deref(),
            Some("emit_ui_helpers")
        );
        assert_eq!(
            manifest.lifecycle.client_init.as_deref(),
            Some("emit_ui_client_init")
        );
        assert_eq!(
            manifest.lifecycle.artifact_emitters.as_deref(),
            Some("emit_ui_artifacts")
        );
        // Slots not declared default to None
        assert!(manifest.lifecycle.program_init.is_none());
        assert!(manifest.lifecycle.server_init.is_none());
        assert!(manifest.lifecycle.per_request.is_none());
    }

    #[test]
    fn test_v2_manifest_artifacts_section_parses() {
        // The artifact closes BUILD_FRONTEND/SRV004 — frame.ui declares
        // frontend.wasm here and clean-server locates it by manifest, not
        // by CWD coincidence.
        let toml_str = r#"
            [plugin]
            name = "frame.ui"
            version = "2.6.12"

            [handles]
            blocks = ["component"]

            [[artifacts]]
            name = "frontend.wasm"
            purpose = "client_hydration"
            emit = { from_module = "client_only_build" }
            output_relative = "{output_dir}/frontend.wasm"
            required_when = "has_client_init"
            public = true
            content_type = "application/wasm"

            [[artifacts]]
            name = "theme.css"
            purpose = "static_asset"
            emit = "emit_theme_css"
            output_relative = "{output_dir}/theme.css"
            public = true
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.artifacts.len(), 2);

        let frontend = &manifest.artifacts[0];
        assert_eq!(frontend.name, "frontend.wasm");
        assert_eq!(frontend.purpose, "client_hydration");
        assert_eq!(frontend.required_when, "has_client_init");
        assert!(frontend.public);
        assert_eq!(frontend.content_type.as_deref(), Some("application/wasm"));
        match frontend.emit.as_ref().unwrap() {
            ArtifactSource::Module { from_module } => {
                assert_eq!(from_module, "client_only_build");
            }
            _ => panic!("expected Module variant for frontend.wasm emit"),
        }

        let theme = &manifest.artifacts[1];
        assert_eq!(theme.name, "theme.css");
        assert_eq!(theme.purpose, "static_asset");
        // Default required_when is "always"
        assert_eq!(theme.required_when, "always");
        // Default cache is "build_input_hash"
        assert_eq!(theme.cache, "build_input_hash");
        match theme.emit.as_ref().unwrap() {
            ArtifactSource::Callback(name) => assert_eq!(name, "emit_theme_css"),
            _ => panic!("expected Callback variant for theme.css emit"),
        }
    }

    #[test]
    fn test_v2_bridge_function_with_hosts_and_callback() {
        // The callback declaration is how SRV001 is closed — _ui_render_page
        // declares the component_tag_render purpose so clean-server can
        // dispatch back into the WASM module without hardcoded knowledge.
        let toml_str = r#"
            [plugin]
            name = "frame.ui"
            version = "2.6.12"

            [handles]
            blocks = ["component"]

            [[bridge.functions]]
            name = "_ui_render_page"
            params = ["string", "string"]
            returns = "string"
            expand_strings = true
            hosts = ["server"]

            [bridge.functions.callback]
            purpose = "component_tag_render"
            plugin_target = "frame.ui"
            discovery = "exports_matching"
            export_pattern = "{tagname}_render"
            fallback = "passthrough"

            [[bridge.functions]]
            name = "_ui_get_bounds"
            params = ["string"]
            returns = "string"
            expand_strings = true
            hosts = ["browser", "server"]
            browser_impl = "real"
            server_impl = "stub"
            server_stub = { returns = "\"\"" }
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.bridge.functions.len(), 2);

        let render = &manifest.bridge.functions[0];
        assert_eq!(render.name, "_ui_render_page");
        assert_eq!(render.hosts.as_ref().unwrap(), &vec!["server".to_string()]);
        let cb = render.callback.as_ref().expect("callback should parse");
        assert_eq!(cb.purpose, "component_tag_render");
        assert_eq!(cb.discovery, "exports_matching");
        assert_eq!(cb.export_pattern.as_deref(), Some("{tagname}_render"));
        assert_eq!(cb.fallback, "passthrough");

        let bounds = &manifest.bridge.functions[1];
        assert_eq!(
            bounds.hosts.as_ref().unwrap(),
            &vec!["browser".to_string(), "server".to_string()]
        );
        assert_eq!(bounds.browser_impl, Some(BridgeImpl::Real));
        assert_eq!(bounds.server_impl, Some(BridgeImpl::Stub));
        let stub = bounds.server_stub.as_ref().expect("server_stub parses");
        assert_eq!(stub.returns.as_deref(), Some("\"\""));
        assert!(!stub.no_op);
    }

    #[test]
    fn test_v2_bridge_function_defaults_without_v2_fields() {
        // v1.0.0 bridge declarations must keep parsing with v2 fields absent.
        let toml_str = r#"
            [plugin]
            name = "test.plugin"
            version = "1.0.0"

            [handles]
            blocks = ["test"]

            [[bridge.functions]]
            name = "_db_query"
            params = ["string", "string"]
            returns = "string"
            expand_strings = true
        "#;

        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        let f = &manifest.bridge.functions[0];
        assert!(f.hosts.is_none());
        assert!(f.server_impl.is_none());
        assert!(f.browser_impl.is_none());
        assert!(f.callback.is_none());
    }

    // ========================================================================
    // Plugin Contracts v2 §2 — BuildContext and BuildState
    // ========================================================================

    #[test]
    fn test_build_context_default_carries_version_constants() {
        let ctx = BuildContext::new();
        assert_eq!(ctx.contract_version, PLUGIN_CONTRACT_VERSION);
        assert_eq!(ctx.runtime_abi_version, DEFAULT_RUNTIME_ABI_VERSION);
        assert_eq!(ctx.target, "server");
        assert!(!ctx.client_mode);
    }

    #[test]
    fn test_build_context_with_target_derives_client_mode() {
        let ctx = BuildContext::new().with_target("browser");
        assert_eq!(ctx.target, "browser");
        assert!(ctx.client_mode);

        let ctx = BuildContext::new().with_target("server");
        assert_eq!(ctx.target, "server");
        assert!(!ctx.client_mode);

        let ctx = BuildContext::new().with_target("native");
        assert!(!ctx.client_mode);
    }

    #[test]
    fn test_build_context_json_round_trip() {
        // The JSON shape is what plugins parse — a stable round trip ensures
        // the field names match contracts/lifecycle.md §2.1.
        let mut ctx = BuildContext::new()
            .with_target("server")
            .with_entry("app/main.cln")
            .with_output_dir("dist")
            .with_main_wasm("app.wasm");
        ctx.artifacts_so_far.push(BuildContextArtifact {
            name: "frontend.wasm".to_string(),
            path_relative: "frontend.wasm".to_string(),
            purpose: "client_hydration".to_string(),
            public: true,
        });
        ctx.build_state.insert(
            "frame.ui:components".to_string(),
            "[\"app-header\"]".to_string(),
        );

        let json = serde_json::to_string(&ctx).expect("serialize");
        // Verify every documented field appears in the wire form.
        for field in [
            "\"target\"",
            "\"client_mode\"",
            "\"entry_path\"",
            "\"output_dir\"",
            "\"main_wasm\"",
            "\"compiler_version\"",
            "\"contract_version\"",
            "\"runtime_abi_version\"",
            "\"artifacts_so_far\"",
            "\"build_state\"",
        ] {
            assert!(json.contains(field), "missing field {} in {}", field, json);
        }

        let parsed: BuildContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.target, "server");
        assert_eq!(parsed.entry_path, "app/main.cln");
        assert_eq!(parsed.artifacts_so_far.len(), 1);
        assert_eq!(parsed.artifacts_so_far[0].name, "frontend.wasm");
        assert_eq!(
            parsed.build_state.get("frame.ui:components"),
            Some(&"[\"app-header\"]".to_string())
        );
    }

    #[test]
    fn test_build_state_snapshot_reflects_writes() {
        // The BuildState is shared by Arc so concurrent writes are visible
        // through the snapshot helper. This is how `expand_block` writes are
        // observed by later slot calls within one build.
        let state = new_build_state();
        {
            let mut g = state.lock().unwrap();
            g.insert("frame.ui:components".to_string(), "[]".to_string());
            g.insert("frame.server:routes".to_string(), "[]".to_string());
        }

        let mut ctx = BuildContext::new();
        ctx.snapshot_build_state(&state);
        assert_eq!(ctx.build_state.len(), 2);
        assert!(ctx.build_state.contains_key("frame.ui:components"));

        // Subsequent writes don't retroactively appear in the snapshot — the
        // snapshot is a point-in-time copy per contracts/lifecycle.md §2.1.
        state
            .lock()
            .unwrap()
            .insert("frame.auth:roles".to_string(), "[]".to_string());
        assert_eq!(
            ctx.build_state.len(),
            2,
            "snapshot must not see post-snapshot writes"
        );

        // A fresh snapshot picks up the new write.
        let mut ctx2 = BuildContext::new();
        ctx2.snapshot_build_state(&state);
        assert_eq!(ctx2.build_state.len(), 3);
    }

    #[test]
    fn test_build_state_keys_namespaced_by_convention() {
        // The spec says: keys should be plugin-namespaced (frame.ui:foo) and
        // __compiler:* is reserved. This test documents the convention so a
        // future spec lint can check it.
        let state = new_build_state();
        {
            let mut g = state.lock().unwrap();
            g.insert("frame.ui:components".to_string(), "[]".to_string());
            g.insert("__compiler:internal".to_string(), "value".to_string());
        }
        // No structural enforcement yet — just confirm both kinds round-trip.
        let snap: std::collections::BTreeMap<String, String> = state.lock().unwrap().clone();
        assert!(snap.contains_key("frame.ui:components"));
        assert!(snap.contains_key("__compiler:internal"));
    }

    #[test]
    fn test_lifecycle_opts_into_v2() {
        let empty = PluginLifecycle::default();
        assert!(!empty.opts_into_v2());

        let with_helpers = PluginLifecycle {
            module_helpers: Some("emit_helpers".to_string()),
            ..PluginLifecycle::default()
        };
        assert!(with_helpers.opts_into_v2());

        // module_helpers_are_roots alone is NOT a v2 opt-in — it's a
        // modifier on module_helpers. Without module_helpers it does nothing.
        let only_roots_flag = PluginLifecycle {
            module_helpers_are_roots: true,
            ..PluginLifecycle::default()
        };
        assert!(!only_roots_flag.opts_into_v2());
    }

    // ------------------------------------------------------------------
    // Plugin Contracts v3 — typed-emission dispatch
    // See foundation/spec/plugins/contracts/typed-emission.md §2.1, §6.
    // ------------------------------------------------------------------

    #[test]
    fn test_v3_manifest_absent_defaults_to_v1_for_all_blocks() {
        let toml_str = r#"
            [plugin]
            name = "legacy.plugin"
            version = "1.0.0"

            [handles]
            blocks = ["legacy"]
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert!(manifest.compatibility.expansion_version.is_none());
        assert!(!manifest.opts_into_typed_emission());
        assert!(manifest.blocks.is_empty());

        let dispatch = manifest.resolve_block_dispatch("legacy");
        assert_eq!(dispatch.export, "expand");
        assert_eq!(dispatch.version, 1);
    }

    #[test]
    fn test_v3_plugin_level_optin_defaults_blocks_to_v3() {
        let toml_str = r#"
            [plugin]
            name = "frame.server"
            version = "3.0.0"

            [compatibility]
            expansion_version = "3.0.0"

            [handles]
            blocks = ["endpoints"]
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(
            manifest.compatibility.expansion_version.as_deref(),
            Some("3.0.0")
        );
        assert!(manifest.opts_into_typed_emission());

        // Block has no entry in [blocks] table -> defaults from plugin level (3).
        let dispatch = manifest.resolve_block_dispatch("endpoints");
        assert_eq!(dispatch.export, "expand");
        assert_eq!(dispatch.version, 3);
    }

    #[test]
    fn test_v3_per_block_table_overrides_export_and_version() {
        let toml_str = r#"
            [plugin]
            name = "frame.server"
            version = "3.0.0"

            [compatibility]
            expansion_version = "3.0.0"

            [handles]
            blocks = ["endpoints", "data"]

            [blocks]
            endpoints = { expand = "expand_endpoints_typed", version = 3 }
            data      = { expand = "expand_data_v1_string",  version = 1 }
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert!(manifest.opts_into_typed_emission());
        assert_eq!(manifest.blocks.len(), 2);

        let endpoints = manifest.resolve_block_dispatch("endpoints");
        assert_eq!(endpoints.export, "expand_endpoints_typed");
        assert_eq!(endpoints.version, 3);

        // Per-block opt-out: plugin is v3 but `data` stays on v1 string emission.
        let data = manifest.resolve_block_dispatch("data");
        assert_eq!(data.export, "expand_data_v1_string");
        assert_eq!(data.version, 1);
    }

    #[test]
    fn test_v3_block_with_only_version_field_inherits_export() {
        // A v3 plugin can declare a per-block version-only override without
        // restating the export name.
        let toml_str = r#"
            [plugin]
            name = "p"
            version = "1.0.0"

            [compatibility]
            expansion_version = "3.0.0"

            [handles]
            blocks = ["a"]

            [blocks]
            a = { version = 1 }
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        let dispatch = manifest.resolve_block_dispatch("a");
        assert_eq!(dispatch.export, "expand"); // inherited from [exports].expand
        assert_eq!(dispatch.version, 1);
    }

    #[test]
    fn test_v3_v1_plugin_with_blocks_entry_still_uses_v1() {
        // A v1 plugin (no expansion_version) that happens to declare a
        // [blocks] entry for an alternate export name still defaults the
        // version to 1 because the plugin has not opted into v3.
        let toml_str = r#"
            [plugin]
            name = "p"
            version = "1.0.0"

            [handles]
            blocks = ["a"]

            [blocks]
            a = { expand = "expand_a_custom" }
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();

        assert!(!manifest.opts_into_typed_emission());
        let dispatch = manifest.resolve_block_dispatch("a");
        assert_eq!(dispatch.export, "expand_a_custom");
        assert_eq!(dispatch.version, 1);
    }
}
