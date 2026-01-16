/*!
 * Plugin Registry - Manages framework plugin registration and dispatch
 */

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::{
    FrameworkBlock, FrameworkPlugin, PluginCompletionItem, PluginDiagnostic, PluginHoverInfo,
    PluginLspContext,
};
use crate::ast::{SourceLocation, Statement};
use crate::plugins::plugin_abi::BridgeFunction;

/// Error type for plugin operations
#[derive(Debug, Clone)]
pub enum PluginError {
    /// No plugin registered for the given block type
    UnknownBlockType {
        block_name: String,
        location: Option<SourceLocation>,
    },
    /// Plugin expansion failed
    ExpansionFailed {
        plugin_name: String,
        block_name: String,
        message: String,
        location: Option<SourceLocation>,
    },
    /// Plugin validation failed
    ValidationFailed {
        plugin_name: String,
        message: String,
        location: Option<SourceLocation>,
    },
    /// Plugin registration conflict
    RegistrationConflict {
        block_name: String,
        existing_plugin: String,
        new_plugin: String,
    },
    /// Parse error in DSL content
    ParseError {
        message: String,
        line: usize,
        column: usize,
        location: Option<SourceLocation>,
    },
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::UnknownBlockType {
                block_name,
                location,
            } => {
                write!(f, "No plugin registered for block type '{}:'", block_name)?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            PluginError::ExpansionFailed {
                plugin_name,
                block_name,
                message,
                location,
            } => {
                write!(
                    f,
                    "Plugin '{}' failed to expand '{}:': {}",
                    plugin_name, block_name, message
                )?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            PluginError::ValidationFailed {
                plugin_name,
                message,
                location,
            } => {
                write!(f, "Plugin '{}' validation failed: {}", plugin_name, message)?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            PluginError::RegistrationConflict {
                block_name,
                existing_plugin,
                new_plugin,
            } => {
                write!(
                    f,
                    "Registration conflict for '{}:': already handled by '{}', cannot register '{}'",
                    block_name, existing_plugin, new_plugin
                )
            }
            PluginError::ParseError {
                message,
                line,
                column,
                location,
            } => {
                write!(
                    f,
                    "Parse error in DSL content at {}:{}: {}",
                    line, column, message
                )?;
                if let Some(loc) = location {
                    write!(f, " (block at {})", loc)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PluginError {}

/// Registry for framework plugins
///
/// Maintains a mapping from block identifiers to plugin handlers.
/// When a FrameworkBlock is encountered, the registry dispatches
/// to the appropriate plugin for expansion.
///
/// **Immutable after creation** - plugins cannot be added/removed after building.
/// This prevents plugin injection mid-compilation.
///
/// # Example
///
/// ```ignore
/// use my_framework_plugins::MyPlugin;
///
/// let registry = PluginRegistry::builder()
///     .add(MyPlugin::new())
///     .build()
///     .expect("Failed to build registry");
/// ```
pub struct PluginRegistry {
    /// Map from block identifier (e.g., "endpoints") to plugin
    handlers: HashMap<String, Arc<dyn FrameworkPlugin>>,
    /// Track plugin names for debugging
    registered_plugins: Vec<String>,
    /// Bridge functions from all loaded plugins
    /// These are functions that plugins expect the runtime to provide (e.g., _db_query)
    bridge_functions: Vec<BridgeFunction>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("Default registry should build")
    }
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    ///
    /// **Deprecated**: Use `PluginRegistry::builder()` instead.
    /// This method is kept for backward compatibility.
    #[deprecated(since = "0.13.1", note = "Use PluginRegistry::builder() instead")]
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            registered_plugins: Vec::new(),
            bridge_functions: Vec::new(),
        }
    }

    /// Create a builder for constructing an immutable plugin registry
    ///
    /// # Example
    ///
    /// ```ignore
    /// use my_framework_plugins::MyPlugin;
    ///
    /// let registry = PluginRegistry::builder()
    ///     .add(MyPlugin::new())
    ///     .build()?;
    /// ```
    pub fn builder() -> PluginRegistryBuilder {
        PluginRegistryBuilder::new()
    }

    /// Register a plugin with the registry
    ///
    /// **Deprecated**: Use the builder pattern instead.
    /// This method is kept for backward compatibility only.
    ///
    /// # Arguments
    /// * `plugin` - The plugin to register
    ///
    /// # Returns
    /// * `Ok(())` if registration succeeds
    /// * `Err(PluginError::RegistrationConflict)` if a handler already exists
    #[deprecated(
        since = "0.13.1",
        note = "Use PluginRegistry::builder().add(plugin).build()"
    )]
    pub fn register(&mut self, plugin: Arc<dyn FrameworkPlugin>) -> Result<(), PluginError> {
        let plugin_name = plugin.name().to_string();

        for block_name in plugin.handles() {
            if let Some(existing) = self.handlers.get(*block_name) {
                return Err(PluginError::RegistrationConflict {
                    block_name: block_name.to_string(),
                    existing_plugin: existing.name().to_string(),
                    new_plugin: plugin_name,
                });
            }
        }

        // Register all handlers
        for block_name in plugin.handles() {
            self.handlers
                .insert(block_name.to_string(), Arc::clone(&plugin));
        }

        self.registered_plugins.push(plugin_name);
        Ok(())
    }

    /// Check if a block type is handled by a registered plugin
    ///
    /// Extracts the block type (first word) from names like "screen RegistrationForm"
    /// before checking the handler registry.
    pub fn handles(&self, block_name: &str) -> bool {
        // Extract block type (first word) from names like "screen RegistrationForm"
        let block_type = block_name.split_whitespace().next().unwrap_or(block_name);
        self.handlers.contains_key(block_type)
    }

    /// Get the plugin that handles a specific block type
    ///
    /// Extracts the block type (first word) from names like "screen RegistrationForm"
    /// before looking up the handler.
    pub fn get_handler(&self, block_name: &str) -> Option<&Arc<dyn FrameworkPlugin>> {
        let block_type = block_name.split_whitespace().next().unwrap_or(block_name);
        self.handlers.get(block_type)
    }

    /// Expand a framework block using the appropriate plugin
    ///
    /// # Arguments
    /// * `block` - The framework block to expand
    ///
    /// # Returns
    /// * `Ok(Vec<Statement>)` - The expanded statements
    /// * `Err(PluginError)` - If no handler exists or expansion fails
    pub fn expand(&self, block: &FrameworkBlock) -> Result<Vec<Statement>, PluginError> {
        // Extract block type (first word) from names like "screen RegistrationForm"
        let block_type = block.name.split_whitespace().next().unwrap_or(&block.name);
        let handler =
            self.handlers
                .get(block_type)
                .ok_or_else(|| PluginError::UnknownBlockType {
                    block_name: block.name.clone(),
                    location: block.location.clone(),
                })?;

        // Validate first
        handler
            .validate(block)
            .map_err(|e| PluginError::ValidationFailed {
                plugin_name: handler.name().to_string(),
                message: e.to_string(),
                location: block.location.clone(),
            })?;

        // Then expand
        handler
            .expand(block)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: handler.name().to_string(),
                block_name: block.name.clone(),
                message: e.to_string(),
                location: block.location.clone(),
            })
    }

    /// Expand a framework block using the appropriate plugin (full version)
    ///
    /// Returns a PluginExpansion that can include start functions and other definitions.
    pub fn expand_full(
        &self,
        block: &FrameworkBlock,
    ) -> Result<super::PluginExpansion, PluginError> {
        // Extract block type (first word) from names like "screen RegistrationForm"
        let block_type = block.name.split_whitespace().next().unwrap_or(&block.name);
        let handler =
            self.handlers
                .get(block_type)
                .ok_or_else(|| PluginError::UnknownBlockType {
                    block_name: block.name.clone(),
                    location: block.location.clone(),
                })?;

        // Validate first
        handler
            .validate(block)
            .map_err(|e| PluginError::ValidationFailed {
                plugin_name: handler.name().to_string(),
                message: e.to_string(),
                location: block.location.clone(),
            })?;

        // Then expand with full result
        handler
            .expand_full(block)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: handler.name().to_string(),
                block_name: block.name.clone(),
                message: e.to_string(),
                location: block.location.clone(),
            })
    }

    /// Get list of registered plugin names
    pub fn registered_plugins(&self) -> &[String] {
        &self.registered_plugins
    }

    /// Get list of handled block types
    pub fn handled_block_types(&self) -> Vec<&str> {
        self.handlers.keys().map(|s| s.as_str()).collect()
    }

    /// Check if the registry has any plugins registered
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Get the number of registered handlers
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Get all bridge functions from loaded plugins
    ///
    /// Bridge functions are runtime-provided functions declared in plugin.toml [bridge] sections.
    /// These functions (e.g., _db_query, _db_execute) need to be registered with the compiler
    /// so that code calling them can be type-checked and compiled correctly.
    ///
    /// # Example
    /// ```ignore
    /// let registry = loader.load_plugins(&["frame.data"])?;
    /// for func in registry.bridge_functions() {
    ///     // Register _db_query, _db_execute, etc.
    ///     builtin_registry.register_bridge_function(func);
    /// }
    /// ```
    pub fn bridge_functions(&self) -> &[BridgeFunction] {
        &self.bridge_functions
    }

    /// Check if a function name is a bridge function
    pub fn is_bridge_function(&self, name: &str) -> bool {
        self.bridge_functions.iter().any(|f| f.name == name)
    }

    /// Get a bridge function by name
    pub fn get_bridge_function(&self, name: &str) -> Option<&BridgeFunction> {
        self.bridge_functions.iter().find(|f| f.name == name)
    }

    // ========================================================================
    // Language Server Protocol (LSP) Aggregation Methods
    // ========================================================================

    /// Get all keywords from all registered plugins
    ///
    /// Returns a combined list of keywords from all plugins for syntax highlighting.
    /// Also includes the block names themselves (e.g., "endpoints", "data").
    ///
    /// # Example
    /// ```ignore
    /// let registry = PluginRegistry::builder()
    ///     .add(WebPlugin::new())
    ///     .build()?;
    ///
    /// let keywords = registry.get_all_keywords();
    /// // Returns: ["endpoints", "GET", "POST", "PUT", "DELETE", ...]
    /// ```
    pub fn get_all_keywords(&self) -> Vec<&str> {
        let mut keywords: Vec<&str> = Vec::new();

        // Add block names as keywords
        for block_name in self.handlers.keys() {
            keywords.push(block_name.as_str());
        }

        // Add keywords from each unique plugin
        let mut seen_plugins = std::collections::HashSet::new();
        for plugin in self.handlers.values() {
            if seen_plugins.insert(plugin.name()) {
                keywords.extend(plugin.get_keywords());
            }
        }

        keywords
    }

    /// Get all block keywords that plugins handle
    ///
    /// Returns just the block handler names (e.g., "data", "endpoints", "screen")
    /// without the additional DSL keywords. This is used by the parser to recognize
    /// plugin-defined keywords that don't require colons.
    ///
    /// # Returns
    /// A vector of block keyword strings that plugins handle
    ///
    /// # Example
    /// ```ignore
    /// let registry = PluginRegistry::builder()
    ///     .add(DataPlugin::new())  // Handles "data"
    ///     .add(HttpPlugin::new())  // Handles "endpoints"
    ///     .build()?;
    ///
    /// let keywords = registry.get_all_block_keywords();
    /// // Returns: ["data", "endpoints"]
    /// ```
    pub fn get_all_block_keywords(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    /// Get completions for a specific block type
    ///
    /// Delegates to the plugin that handles the given block type.
    ///
    /// # Arguments
    /// * `block_name` - The block type (e.g., "endpoints")
    /// * `ctx` - Context about the current editing position
    ///
    /// # Returns
    /// List of completion items, or empty if no plugin handles the block
    pub fn get_completions(
        &self,
        block_name: &str,
        ctx: &PluginLspContext,
    ) -> Vec<PluginCompletionItem> {
        if let Some(plugin) = self.handlers.get(block_name) {
            plugin.get_completions(ctx)
        } else {
            Vec::new()
        }
    }

    /// Get completions for all plugins (for top-level completion)
    ///
    /// Returns block-level completions (the block names themselves).
    ///
    /// # Returns
    /// Completion items for all registered block types
    pub fn get_block_completions(&self) -> Vec<PluginCompletionItem> {
        let mut completions = Vec::new();
        let mut seen_plugins = std::collections::HashSet::new();

        for (block_name, plugin) in &self.handlers {
            if seen_plugins.insert(plugin.name()) {
                completions.push(PluginCompletionItem {
                    label: format!("{}:", block_name),
                    kind: super::PluginCompletionKind::Keyword,
                    detail: Some(format!("Plugin: {}", plugin.name())),
                    documentation: Some(plugin.description().to_string()),
                    insert_text: Some(format!("{}:\n\t${{1:content}}", block_name)),
                    is_snippet: true,
                });
            }
        }

        completions
    }

    /// Get hover information for a keyword
    ///
    /// First checks if the keyword is a block name, then delegates to plugins.
    ///
    /// # Arguments
    /// * `keyword` - The word being hovered over
    /// * `block_name` - Optional: the block type context
    ///
    /// # Returns
    /// Hover information if found
    pub fn get_hover_info(
        &self,
        keyword: &str,
        block_name: Option<&str>,
    ) -> Option<PluginHoverInfo> {
        // Check if hovering over a block name
        if let Some(plugin) = self.handlers.get(keyword) {
            return Some(PluginHoverInfo {
                content: format!(
                    "**{}:** Block\n\n{}\n\n*Plugin: {} v{}*",
                    keyword,
                    plugin.description(),
                    plugin.name(),
                    plugin.version()
                ),
            });
        }

        // If we're inside a block, delegate to that plugin
        if let Some(block) = block_name {
            if let Some(plugin) = self.handlers.get(block) {
                return plugin.get_hover_info(keyword);
            }
        }

        // Try all plugins (for keywords that might appear outside their blocks)
        for plugin in self.handlers.values() {
            if let Some(info) = plugin.get_hover_info(keyword) {
                return Some(info);
            }
        }

        None
    }

    /// Get diagnostics for a block's content
    ///
    /// Delegates to the plugin that handles the given block type.
    ///
    /// # Arguments
    /// * `block_name` - The block type (e.g., "endpoints")
    /// * `content` - The content inside the block
    ///
    /// # Returns
    /// List of diagnostics (errors, warnings, etc.)
    pub fn get_diagnostics(&self, block_name: &str, content: &str) -> Vec<PluginDiagnostic> {
        if let Some(plugin) = self.handlers.get(block_name) {
            plugin.get_diagnostics(content)
        } else {
            Vec::new()
        }
    }

    /// Get all plugin descriptions for documentation
    ///
    /// Returns a list of (name, block_types, description) for all registered plugins.
    pub fn get_plugin_info(&self) -> Vec<(&str, Vec<&str>, &str)> {
        let mut info = Vec::new();
        let mut seen_plugins = std::collections::HashSet::new();

        for plugin in self.handlers.values() {
            if seen_plugins.insert(plugin.name()) {
                info.push((
                    plugin.name(),
                    plugin.handles().to_vec(),
                    plugin.description(),
                ));
            }
        }

        info
    }
}

impl fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("registered_plugins", &self.registered_plugins)
            .field("handlers", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Builder for creating an immutable PluginRegistry
///
/// This builder allows plugins to be added fluently, then builds
/// an immutable registry that cannot be modified after creation.
///
/// # Example
///
/// ```ignore
/// use my_framework_plugins::MyPlugin;
///
/// let registry = PluginRegistryBuilder::new()
///     .add(MyPlugin::new())
///     .build()?;
/// ```
pub struct PluginRegistryBuilder {
    plugins: Vec<Arc<dyn FrameworkPlugin>>,
    bridge_functions: Vec<BridgeFunction>,
}

impl PluginRegistryBuilder {
    /// Create a new empty builder
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            bridge_functions: Vec::new(),
        }
    }

    /// Add a plugin to the registry
    ///
    /// Plugins are validated for conflicts when `build()` is called.
    ///
    /// # Arguments
    /// * `plugin` - The plugin to add
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add(mut self, plugin: impl FrameworkPlugin + 'static) -> Self {
        self.plugins.push(Arc::new(plugin));
        self
    }

    /// Add a plugin via Arc (for when you already have an Arc)
    ///
    /// # Arguments
    /// * `plugin` - The plugin Arc to add
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_arc(mut self, plugin: Arc<dyn FrameworkPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// Add bridge functions from a plugin manifest
    ///
    /// Bridge functions are runtime-provided functions declared in plugin.toml [bridge] sections.
    ///
    /// # Arguments
    /// * `bridge` - The bridge section from a plugin manifest
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_bridge_functions(
        mut self,
        bridge: &crate::plugins::plugin_abi::PluginBridge,
    ) -> Self {
        self.bridge_functions.extend(bridge.functions.clone());
        self
    }

    /// Add a single bridge function
    ///
    /// # Arguments
    /// * `func` - The bridge function to add
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_bridge_function(mut self, func: BridgeFunction) -> Self {
        self.bridge_functions.push(func);
        self
    }

    /// Build the immutable plugin registry
    ///
    /// This validates that there are no block type conflicts between plugins.
    ///
    /// # Returns
    /// * `Ok(PluginRegistry)` - The immutable registry
    /// * `Err(PluginError::RegistrationConflict)` - If plugins conflict
    ///
    /// # Example
    ///
    /// ```ignore
    /// use my_framework_plugins::MyPlugin;
    ///
    /// let registry = PluginRegistry::builder()
    ///     .add(MyPlugin::new())
    ///     .build()?;
    /// ```
    pub fn build(self) -> Result<PluginRegistry, PluginError> {
        let mut handlers = HashMap::new();
        let mut registered_plugins = Vec::new();

        // Validate and register all plugins
        for plugin in self.plugins {
            let plugin_name = plugin.name().to_string();

            // Check for conflicts first
            for block_name in plugin.handles() {
                if let Some(existing) = handlers.get(*block_name) {
                    let existing_plugin: &Arc<dyn FrameworkPlugin> = existing;
                    return Err(PluginError::RegistrationConflict {
                        block_name: block_name.to_string(),
                        existing_plugin: existing_plugin.name().to_string(),
                        new_plugin: plugin_name,
                    });
                }
            }

            // Register all handlers for this plugin
            for block_name in plugin.handles() {
                handlers.insert(block_name.to_string(), Arc::clone(&plugin));
            }

            registered_plugins.push(plugin_name);
        }

        Ok(PluginRegistry {
            handlers,
            registered_plugins,
            bridge_functions: self.bridge_functions,
        })
    }
}

impl Default for PluginRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::PluginResult;

    struct TestPlugin {
        name: &'static str,
        handles: &'static [&'static str],
    }

    impl FrameworkPlugin for TestPlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        fn handles(&self) -> &'static [&'static str] {
            self.handles
        }

        fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            // Simple test: return empty expansion
            let _ = block;
            Ok(vec![])
        }
    }

    #[test]
    fn test_register_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin {
            name: "test.plugin",
            handles: &["test"],
        });

        assert!(registry.register(plugin).is_ok());
        assert!(registry.handles("test"));
        assert!(!registry.handles("unknown"));
    }

    #[test]
    fn test_registration_conflict() {
        let mut registry = PluginRegistry::new();

        let plugin1 = Arc::new(TestPlugin {
            name: "plugin1",
            handles: &["test"],
        });
        let plugin2 = Arc::new(TestPlugin {
            name: "plugin2",
            handles: &["test"],
        });

        assert!(registry.register(plugin1).is_ok());

        let result = registry.register(plugin2);
        assert!(matches!(
            result,
            Err(PluginError::RegistrationConflict { .. })
        ));
    }

    #[test]
    fn test_expand_unknown_block() {
        let registry = PluginRegistry::new();
        let block = FrameworkBlock {
            name: "unknown".to_string(),
            content: "content".to_string(),
            attributes: vec![],
            location: None,
        };

        let result = registry.expand(&block);
        assert!(matches!(result, Err(PluginError::UnknownBlockType { .. })));
    }

    #[test]
    fn test_bridge_function_registration() {
        use crate::plugins::plugin_abi::{BridgeFunction, PluginBridge};

        // Create bridge functions like they would come from plugin.toml
        let bridge = PluginBridge {
            functions: vec![
                BridgeFunction {
                    name: "_db_query".to_string(),
                    params: vec!["string".to_string(), "string".to_string()],
                    returns: "string".to_string(),
                    module: "env".to_string(),
                    description: Some("Execute SELECT query".to_string()),
                    expand_strings: true,
                },
                BridgeFunction {
                    name: "_db_execute".to_string(),
                    params: vec!["string".to_string(), "string".to_string()],
                    returns: "integer".to_string(),
                    module: "env".to_string(),
                    description: Some("Execute INSERT/UPDATE/DELETE".to_string()),
                    expand_strings: true,
                },
            ],
        };

        // Build registry with bridge functions
        let registry = PluginRegistryBuilder::new()
            .add_bridge_functions(&bridge)
            .build()
            .expect("Failed to build registry");

        // Verify bridge functions are registered
        assert_eq!(registry.bridge_functions().len(), 2);
        assert!(registry.is_bridge_function("_db_query"));
        assert!(registry.is_bridge_function("_db_execute"));
        assert!(!registry.is_bridge_function("_unknown"));

        // Verify the function properties
        let db_query = registry.get_bridge_function("_db_query").unwrap();
        assert_eq!(db_query.name, "_db_query");
        assert_eq!(db_query.params.len(), 2);
        assert_eq!(db_query.returns, "string");
        assert!(db_query.expand_strings);
    }
}
