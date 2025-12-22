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

/// Block types this plugin handles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHandles {
    pub blocks: Vec<String>,
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
        }
    }
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
    /// Parameter types as strings: "string", "integer", "number", "boolean", "void"
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

/// Bridge section in plugin.toml
/// Contains all functions that the runtime must provide
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginBridge {
    #[serde(default)]
    pub functions: Vec<BridgeFunction>,
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
}
