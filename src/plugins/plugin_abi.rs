//! Plugin ABI Definitions
//!
//! Defines the interface between the compiler and WASM plugins

use serde::{Deserialize, Serialize};

/// Plugin manifest (plugin.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginInfo,
    #[serde(default)]
    pub compatibility: PluginCompatibility,
    pub handles: PluginHandles,
    #[serde(default)]
    pub exports: PluginExports,
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
}

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
}

impl Default for PluginCompatibility {
    fn default() -> Self {
        Self {
            min_compiler_version: default_min_compiler(),
        }
    }
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

fn default_expand() -> String {
    "expand".to_string()
}

/// Bridge function declaration from plugin.toml
/// Defines a function that the plugin expects the runtime to provide
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeFunction {
    /// Function name (e.g., "_db_query")
    pub name: String,
    /// Parameter types as strings: "string", "integer", "number", "boolean", "void", "handler"
    pub params: Vec<String>,
    /// Return type as string
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
}

fn default_bridge_module() -> String {
    "env".to_string()
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

/// A type definition for the plugin's DSL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTypeDef {
    /// The type name (e.g., "Model", "Query", "Relationship")
    pub name: String,
    /// Human-readable description for hover and documentation
    pub description: String,
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
    /// Convert a string type from plugin.toml to BuiltinType
    pub fn parse_type(type_str: &str) -> crate::builtins::registry::BuiltinType {
        use crate::builtins::registry::BuiltinType;
        match type_str.to_lowercase().as_str() {
            "string" => BuiltinType::String,
            "integer" | "int" | "i32" => BuiltinType::Integer,
            "number" | "float" | "f64" => BuiltinType::Number,
            "boolean" | "bool" => BuiltinType::Boolean,
            "void" | "" => BuiltinType::Void,
            "handler" | "callback" => BuiltinType::Handler,
            _ => BuiltinType::Any, // Default to Any for unknown types
        }
    }

    /// Get parameter types as BuiltinTypes
    pub fn get_param_types(&self) -> Vec<crate::builtins::registry::BuiltinType> {
        self.params.iter().map(|p| Self::parse_type(p)).collect()
    }

    /// Get return type as BuiltinType
    pub fn get_return_type(&self) -> crate::builtins::registry::BuiltinType {
        Self::parse_type(&self.returns)
    }

    /// Convert to BuiltinFunction for registry registration
    pub fn to_builtin_function(&self) -> crate::builtins::registry::BuiltinFunction {
        use crate::builtins::registry::{BuiltinCategory, BuiltinFunction};
        BuiltinFunction::new(
            &self.name,
            self.get_param_types(),
            self.get_return_type(),
            BuiltinCategory::Http, // Use Http category for plugin bridge functions
        )
        .with_wasm_import(&self.module, &self.name)
    }
}

/// Plugin ABI version
pub const PLUGIN_ABI_VERSION: u32 = 1;

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
}
