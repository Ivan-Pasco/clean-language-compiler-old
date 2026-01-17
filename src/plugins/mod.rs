/*!
 * Clean Language Compiler - Plugin System
 *
 * This module provides the framework plugin architecture for Clean Frame
 * and other DSL extensions. Plugins transform framework-specific DSL blocks
 * (like `endpoints:`, `data`, `component`) into standard Clean Language AST
 * before the HIR transformation stage.
 *
 * ## Important: Plugins vs Imports
 *
 * **Plugins are NOT imported in source files.** Plugins are declared at the
 * project level in `configuration.cln`:
 *
 * ```clean
 * // configuration.cln - Project configuration file
 * plugins:
 *     frame.web
 *     frame.ui
 *     frame.data
 * ```
 *
 * The `import:` statement is **exclusively for Clean Language modules** (.cln files).
 * If a plugin provides runtime helpers, those helpers may be imported, but the
 * plugin itself is not imported.
 *
 * ## Architecture
 *
 * ```text
 * configuration.cln → [Plugin Loading] → Source → Lexer → Parser → [Plugin Expansion] → HIR → ...
 *                          ↑                                              ↑
 *                   Plugins declared here                          Plugins transform here
 * ```
 *
 * ## Language Server Integration
 *
 * Plugins can provide IDE support by implementing the optional LSP methods:
 *
 * ```text
 * ┌──────────────────────────────────────────────────────────────────┐
 * │                    PLUGIN-AWARE LANGUAGE SERVER                   │
 * ├──────────────────────────────────────────────────────────────────┤
 * │   ┌──────────────┐         ┌──────────────────────┐              │
 * │   │   Plugins    │────────▶│   PluginRegistry     │              │
 * │   │              │         │   - handles: [...]    │              │
 * │   │ - endpoints: │         │   - completions       │              │
 * │   │ - data:      │         │   - hover docs        │              │
 * │   └──────────────┘         └───────────┬──────────┘              │
 * │                                        │                          │
 * │                                        ▼                          │
 * │   ┌────────────────────────────────────────────────────────────┐ │
 * │   │                  Language Server                            │ │
 * │   │   CompletionProvider  ◀──── registry.get_completions()     │ │
 * │   │   HoverProvider       ◀──── registry.get_hover_info()      │ │
 * │   │   SemanticTokens      ◀──── registry.get_keywords()        │ │
 * │   └────────────────────────────────────────────────────────────┘ │
 * └──────────────────────────────────────────────────────────────────┘
 * ```
 *
 * ## Usage
 *
 * Plugins are loaded from `configuration.cln` at the project level:
 *
 * ```ignore
 * use clean_language_compiler::plugins::{PluginRegistry, WasmPluginLoader};
 *
 * // Load plugins from configuration.cln
 * let mut loader = WasmPluginLoader::new()?;
 * let plugin_names = read_plugins_from_config("configuration.cln")?;
 * let registry = loader.load_plugins(&plugin_names)?;
 *
 * // Compile with plugins
 * compile_with_plugins(source, file_path, &registry)?;
 * ```
 */

mod app_config;
mod discovery;
mod expander;
pub mod language_registry;
pub mod plugin_abi;
mod registry;
mod wasm_adapter;
mod wasm_loader;

pub use app_config::AppConfig;
pub use discovery::{DiscoveryError, PluginDiscovery};
pub use expander::PluginExpander;
pub use language_registry::{
    BlockInfo, CompletionSnippet, FunctionInfo, KeywordInfo, LanguageRegistry, TypeInfo,
};
pub use plugin_abi::{
    BridgeFunction, PluginAbi, PluginBridge, PluginCompletionDef, PluginFunctionDef, PluginKeyword,
    PluginLanguage, PluginManifest, PluginTypeDef, PLUGIN_ABI_VERSION,
};
pub use registry::{PluginError, PluginRegistry, PluginRegistryBuilder};
pub use wasm_adapter::WasmPluginAdapter;
pub use wasm_loader::WasmPluginLoader;

use crate::ast::{SourceLocation, Statement};

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

// ============================================================================
// Language Server Protocol (LSP) Integration Types
// ============================================================================

/// Completion item provided by a plugin for IDE autocomplete
#[derive(Debug, Clone)]
pub struct PluginCompletionItem {
    /// The label shown in the completion list
    pub label: String,
    /// The kind of completion (keyword, function, snippet, etc.)
    pub kind: PluginCompletionKind,
    /// Short description shown alongside the label
    pub detail: Option<String>,
    /// Full documentation (supports markdown)
    pub documentation: Option<String>,
    /// Text to insert when completion is accepted (supports snippets with ${1:placeholder})
    pub insert_text: Option<String>,
    /// Whether insert_text is a snippet with placeholders
    pub is_snippet: bool,
}

/// The kind of completion item
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCompletionKind {
    /// A keyword (e.g., `endpoints`, `GET`, `POST`)
    Keyword,
    /// A function or method
    Function,
    /// A code snippet template
    Snippet,
    /// A type or class
    Type,
    /// A property or field
    Property,
    /// A variable
    Variable,
    /// An operator
    Operator,
}

/// Hover information provided by a plugin
#[derive(Debug, Clone)]
pub struct PluginHoverInfo {
    /// The content to display (supports markdown)
    pub content: String,
}

/// Diagnostic/error information provided by a plugin
#[derive(Debug, Clone)]
pub struct PluginDiagnostic {
    /// Error message
    pub message: String,
    /// Severity level
    pub severity: PluginDiagnosticSeverity,
    /// Line number (1-based)
    pub line: usize,
    /// Column number (1-based)
    pub column: usize,
    /// Length of the problematic text
    pub length: usize,
}

/// Severity of a diagnostic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDiagnosticSeverity {
    /// Error - prevents compilation
    Error,
    /// Warning - may indicate a problem
    Warning,
    /// Information - general message
    Info,
    /// Hint - suggestion for improvement
    Hint,
}

/// Context information passed to plugins for LSP operations
#[derive(Debug, Clone)]
pub struct PluginLspContext<'a> {
    /// The block name being edited (e.g., "endpoints")
    pub block_name: &'a str,
    /// Content inside the block
    pub block_content: &'a str,
    /// Current line within the block (0-based)
    pub line: usize,
    /// Current column within the line (0-based)
    pub column: usize,
    /// The word being typed (for completion filtering)
    pub prefix: &'a str,
    /// Stack of nested blocks (innermost last)
    /// E.g., ["data", "User", "validations"] for a nested structure
    pub block_stack: Vec<&'a str>,
    /// File path being edited (for path-based plugin activation)
    pub file_path: Option<&'a std::path::Path>,
    /// The plugin that owns this file/context (if determined)
    pub owning_plugin: Option<&'a str>,
}

impl<'a> PluginLspContext<'a> {
    /// Create a new context with minimal required fields
    pub fn new(block_name: &'a str, block_content: &'a str, prefix: &'a str) -> Self {
        Self {
            block_name,
            block_content,
            line: 0,
            column: 0,
            prefix,
            block_stack: Vec::new(),
            file_path: None,
            owning_plugin: None,
        }
    }

    /// Set the position within the block
    pub fn at_position(mut self, line: usize, column: usize) -> Self {
        self.line = line;
        self.column = column;
        self
    }

    /// Set the block stack
    pub fn with_block_stack(mut self, stack: Vec<&'a str>) -> Self {
        self.block_stack = stack;
        self
    }

    /// Set the file path
    pub fn with_file_path(mut self, path: &'a std::path::Path) -> Self {
        self.file_path = Some(path);
        self
    }

    /// Set the owning plugin
    pub fn with_owning_plugin(mut self, plugin: &'a str) -> Self {
        self.owning_plugin = Some(plugin);
        self
    }

    /// Get the current context (innermost block in the stack)
    pub fn current_context(&self) -> &str {
        self.block_stack.last().copied().unwrap_or(self.block_name)
    }
}

/// Trait for framework plugins that expand DSL blocks into Clean AST
///
/// Plugins register themselves with the compiler and are invoked when
/// their handled block types are encountered during parsing.
///
/// ## Core Methods (Required)
///
/// - `name()` - Plugin identifier
/// - `handles()` - Block types this plugin handles
/// - `expand()` - Transform DSL block to Clean AST
///
/// ## Language Server Methods (Optional)
///
/// Plugins can provide IDE support by implementing these methods:
///
/// - `get_keywords()` - Keywords for syntax highlighting
/// - `get_completions()` - Autocomplete suggestions
/// - `get_hover_info()` - Hover documentation
/// - `get_diagnostics()` - Real-time error checking
///
/// ## Example Implementation
///
/// ```ignore
/// struct MyFrameworkPlugin;
///
/// impl FrameworkPlugin for MyFrameworkPlugin {
///     fn name(&self) -> &'static str {
///         "my.framework"
///     }
///
///     fn handles(&self) -> &'static [&'static str] {
///         &["myblock"]
///     }
///
///     fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
///         // Parse DSL block content and generate Clean AST
///         // ...
///     }
///
///     // Optional: Provide IDE support
///     fn get_keywords(&self) -> &'static [&'static str] {
///         &["myblock", "option1", "option2"]
///     }
///
///     fn get_completions(&self, ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
///         vec![
///             PluginCompletionItem {
///                 label: "option1".to_string(),
///                 kind: PluginCompletionKind::Keyword,
///                 detail: Some("First option".to_string()),
///                 documentation: Some("Detailed description...".to_string()),
///                 insert_text: Some("option1 ${1:value}".to_string()),
///                 is_snippet: true,
///             }
///         ]
///     }
/// }
/// ```
pub trait FrameworkPlugin: Send + Sync {
    // ========================================================================
    // Core Methods (Required)
    // ========================================================================

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

    /// Expands a framework block into a full plugin expansion result
    ///
    /// This method allows plugins to generate not just statements, but also
    /// functions and the start function. The default implementation calls
    /// `expand()` and wraps the result.
    ///
    /// # Arguments
    /// * `block` - The framework block to expand
    ///
    /// # Returns
    /// * `Ok(PluginExpansion)` - The expanded code including statements and functions
    /// * `Err(PluginError)` - If expansion fails
    fn expand_full(&self, block: &FrameworkBlock) -> PluginResult<PluginExpansion> {
        Ok(PluginExpansion {
            statements: self.expand(block)?,
            start_function: None,
            functions: Vec::new(),
        })
    }

    // ========================================================================
    // Validation Methods (Optional)
    // ========================================================================

    /// Optional: Validate block content before expansion
    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        let _ = block;
        Ok(())
    }

    /// Optional: Plugin version for compatibility checks
    fn version(&self) -> &'static str {
        "1.0.0"
    }

    // ========================================================================
    // Language Server Protocol (LSP) Methods (Optional)
    // ========================================================================

    /// Returns keywords for syntax highlighting
    ///
    /// These keywords will be highlighted in the IDE when used inside
    /// blocks handled by this plugin.
    ///
    /// # Example
    /// ```ignore
    /// fn get_keywords(&self) -> &'static [&'static str] {
    ///     &["GET", "POST", "PUT", "DELETE", "PATCH"]
    /// }
    /// ```
    fn get_keywords(&self) -> &'static [&'static str] {
        &[]
    }

    /// Returns completion items for autocomplete
    ///
    /// Called when the user triggers autocomplete inside a block
    /// handled by this plugin.
    ///
    /// # Arguments
    /// * `ctx` - Context about the current editing position
    ///
    /// # Returns
    /// List of completion suggestions
    fn get_completions(&self, ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
        let _ = ctx;
        Vec::new()
    }

    /// Returns hover information for a keyword
    ///
    /// Called when the user hovers over text inside a block
    /// handled by this plugin.
    ///
    /// # Arguments
    /// * `keyword` - The word being hovered over
    ///
    /// # Returns
    /// Optional hover information with markdown content
    fn get_hover_info(&self, keyword: &str) -> Option<PluginHoverInfo> {
        let _ = keyword;
        None
    }

    /// Returns diagnostics for real-time error checking
    ///
    /// Called to validate block content and provide error/warning markers
    /// in the IDE without running full compilation.
    ///
    /// # Arguments
    /// * `content` - The content inside the block
    ///
    /// # Returns
    /// List of diagnostics (errors, warnings, info, hints)
    fn get_diagnostics(&self, content: &str) -> Vec<PluginDiagnostic> {
        let _ = content;
        Vec::new()
    }

    /// Returns a description of this plugin for documentation
    ///
    /// Used to generate help text and documentation.
    fn description(&self) -> &'static str {
        "A Clean Language plugin"
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

/// Result of expanding a framework block
///
/// Plugins can return statements that replace the block inline,
/// and/or a start function that gets added to the program.
#[derive(Debug, Clone, Default)]
pub struct PluginExpansion {
    /// Statements to insert where the framework block was
    pub statements: Vec<Statement>,
    /// Optional start function generated by the plugin
    pub start_function: Option<crate::ast::Function>,
    /// Optional functions to add to the program
    pub functions: Vec<crate::ast::Function>,
}

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

    /// Mock plugin that provides full LSP support for testing
    struct MockLspPlugin;

    impl FrameworkPlugin for MockLspPlugin {
        fn name(&self) -> &'static str {
            "test.lsp"
        }

        fn handles(&self) -> &'static [&'static str] {
            &["testblock"]
        }

        fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            Ok(vec![])
        }

        fn get_keywords(&self) -> &'static [&'static str] {
            &["KEYWORD1", "KEYWORD2", "KEYWORD3"]
        }

        fn get_completions(&self, ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
            vec![PluginCompletionItem {
                label: "KEYWORD1".to_string(),
                kind: PluginCompletionKind::Keyword,
                detail: Some("First keyword".to_string()),
                documentation: Some("Documentation for KEYWORD1".to_string()),
                insert_text: Some(format!("KEYWORD1 {}", ctx.prefix)),
                is_snippet: false,
            }]
        }

        fn get_hover_info(&self, keyword: &str) -> Option<PluginHoverInfo> {
            match keyword {
                "KEYWORD1" => Some(PluginHoverInfo {
                    content: "**KEYWORD1**\n\nThis is the first keyword.".to_string(),
                }),
                _ => None,
            }
        }

        fn get_diagnostics(&self, content: &str) -> Vec<PluginDiagnostic> {
            if content.contains("ERROR") {
                vec![PluginDiagnostic {
                    message: "Found ERROR in content".to_string(),
                    severity: PluginDiagnosticSeverity::Error,
                    line: 1,
                    column: 1,
                    length: 5,
                }]
            } else {
                vec![]
            }
        }

        fn description(&self) -> &'static str {
            "A test plugin with full LSP support"
        }
    }

    #[test]
    fn test_plugin_trait() {
        let plugin = MockPlugin;
        assert_eq!(plugin.name(), "test.mock");
        assert_eq!(plugin.handles(), &["mock"]);
        assert_eq!(plugin.version(), "1.0.0");
        // Default LSP methods should return empty
        assert!(plugin.get_keywords().is_empty());
        assert!(plugin.get_hover_info("test").is_none());
    }

    #[test]
    fn test_plugin_lsp_methods() {
        let plugin = MockLspPlugin;

        // Test keywords
        assert_eq!(plugin.get_keywords(), &["KEYWORD1", "KEYWORD2", "KEYWORD3"]);

        // Test completions
        let ctx = PluginLspContext::new("testblock", "", "test");
        let completions = plugin.get_completions(&ctx);
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, "KEYWORD1");
        assert_eq!(completions[0].kind, PluginCompletionKind::Keyword);

        // Test hover
        let hover = plugin.get_hover_info("KEYWORD1");
        assert!(hover.is_some());
        assert!(hover.unwrap().content.contains("KEYWORD1"));

        // Test diagnostics
        let diags = plugin.get_diagnostics("This has ERROR in it");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, PluginDiagnosticSeverity::Error);

        let no_diags = plugin.get_diagnostics("This is fine");
        assert!(no_diags.is_empty());

        // Test description
        assert!(plugin.description().contains("LSP"));
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
