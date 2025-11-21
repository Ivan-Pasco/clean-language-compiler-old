/*!
 * Clean Language Compiler - Plugin System
 *
 * This module provides the framework plugin architecture for Clean Frame
 * and other DSL extensions. Plugins transform framework-specific DSL blocks
 * (like `endpoints:`, `data`, `component`) into standard Clean Language AST
 * before the HIR transformation stage.
 *
 * ## Architecture
 *
 * ```text
 * Source → Lexer → Parser → [Plugin Expansion] → HIR → Resolver → TypeChecker → MIR → WASM
 *                              ↑
 *                      Plugins transform here
 * ```
 *
 * ## Usage
 *
 * ```ignore
 * use clean_language_compiler::plugins::{PluginRegistry, FrameworkPlugin};
 *
 * // Create registry and register plugins
 * let mut registry = PluginRegistry::new();
 * registry.register(Arc::new(WebPlugin::new()));
 *
 * // Expand framework blocks in AST
 * let expanded_ast = registry.expand_program(ast)?;
 * ```
 */

mod expander;
pub mod frame_web;
mod registry;

pub use expander::PluginExpander;
pub use frame_web::WebPlugin;
pub use registry::{PluginError, PluginRegistry};

use crate::ast::{SourceLocation, Statement};

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

/// Trait for framework plugins that expand DSL blocks into Clean AST
///
/// Plugins register themselves with the compiler and are invoked when
/// their handled block types are encountered during parsing.
///
/// ## Example Implementation
///
/// ```ignore
/// struct WebPlugin;
///
/// impl FrameworkPlugin for WebPlugin {
///     fn name(&self) -> &'static str {
///         "frame.web"
///     }
///
///     fn handles(&self) -> &'static [&'static str] {
///         &["endpoints"]
///     }
///
///     fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
///         // Parse endpoints: block content and generate Clean AST
///         // ...
///     }
/// }
/// ```
pub trait FrameworkPlugin: Send + Sync {
    /// Returns the plugin name (e.g., "frame.web", "frame.data")
    fn name(&self) -> &'static str;

    /// Returns the block identifiers this plugin handles (e.g., ["endpoints"])
    fn handles(&self) -> &'static [&'static str];

    /// Expands a framework block into standard Clean Language statements
    ///
    /// # Arguments
    /// * `block` - The framework block to expand
    ///
    /// # Returns
    /// * `Ok(Vec<Statement>)` - The expanded Clean Language statements
    /// * `Err(PluginError)` - If expansion fails
    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>>;

    /// Optional: Validate block content before expansion
    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        let _ = block;
        Ok(())
    }

    /// Optional: Plugin version for compatibility checks
    fn version(&self) -> &'static str {
        "1.0.0"
    }
}

/// Helper struct for creating framework blocks programmatically
#[derive(Debug, Clone)]
pub struct FrameworkBlockBuilder {
    name: String,
    content: String,
    attributes: Vec<FrameworkAttribute>,
    location: Option<SourceLocation>,
}

impl FrameworkBlockBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: String::new(),
            attributes: Vec::new(),
            location: None,
        }
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn attribute(mut self, name: impl Into<String>, value: Option<String>) -> Self {
        self.attributes.push(FrameworkAttribute {
            name: name.into(),
            value,
            location: None,
        });
        self
    }

    pub fn location(mut self, loc: SourceLocation) -> Self {
        self.location = Some(loc);
        self
    }

    pub fn build(self) -> FrameworkBlock {
        FrameworkBlock {
            name: self.name,
            content: self.content,
            attributes: self.attributes,
            location: self.location,
        }
    }
}

/// Re-export FrameworkBlock from AST for convenience
pub use crate::ast::{FrameworkAttribute, FrameworkBlock};

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPlugin;

    impl FrameworkPlugin for MockPlugin {
        fn name(&self) -> &'static str {
            "test.mock"
        }

        fn handles(&self) -> &'static [&'static str] {
            &["mock"]
        }

        fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_plugin_trait() {
        let plugin = MockPlugin;
        assert_eq!(plugin.name(), "test.mock");
        assert_eq!(plugin.handles(), &["mock"]);
        assert_eq!(plugin.version(), "1.0.0");
    }

    #[test]
    fn test_framework_block_builder() {
        let block = FrameworkBlockBuilder::new("endpoints")
            .content("GET /users -> listUsers")
            .attribute("version", Some("v1".to_string()))
            .build();

        assert_eq!(block.name, "endpoints");
        assert_eq!(block.content, "GET /users -> listUsers");
        assert_eq!(block.attributes.len(), 1);
    }
}
