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
pub struct PluginRegistry {
    /// Map from block identifier (e.g., "endpoints") to plugin
    handlers: HashMap<String, Arc<dyn FrameworkPlugin>>,
    /// Track plugin names for debugging
    registered_plugins: Vec<String>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            registered_plugins: Vec::new(),
        }
    }

    /// Register a plugin with the registry
    ///
    /// # Arguments
    /// * `plugin` - The plugin to register
    ///
    /// # Returns
    /// * `Ok(())` if registration succeeds
    /// * `Err(PluginError::RegistrationConflict)` if a handler already exists
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
