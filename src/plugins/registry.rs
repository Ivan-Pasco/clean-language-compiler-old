/*!
 * Plugin Registry - Manages framework plugin registration and dispatch
 */

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::{FrameworkBlock, FrameworkPlugin};
use crate::ast::{SourceLocation, Statement};

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
    pub fn handles(&self, block_name: &str) -> bool {
        self.handlers.contains_key(block_name)
    }

    /// Get the plugin that handles a specific block type
    pub fn get_handler(&self, block_name: &str) -> Option<&Arc<dyn FrameworkPlugin>> {
        self.handlers.get(block_name)
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
        let handler =
            self.handlers
                .get(&block.name)
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
}

impl PluginRegistryBuilder {
    /// Create a new empty builder
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
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
}
