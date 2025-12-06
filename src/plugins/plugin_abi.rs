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
}
