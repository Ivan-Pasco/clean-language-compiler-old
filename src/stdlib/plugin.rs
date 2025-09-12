use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use std::any::Any;
use std::collections::HashMap;

/// Trait that all standard library plugins must implement
pub trait StdlibPlugin: Send + Sync {
    /// Unique name identifier for this plugin
    fn name(&self) -> &'static str;

    /// Version of this plugin
    fn version(&self) -> &'static str;

    /// Description of what this plugin provides
    fn description(&self) -> &'static str;

    /// List of dependencies this plugin requires
    fn dependencies(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Initialize the plugin with the given configuration
    fn initialize(&mut self, config: PluginConfig) -> Result<(), CompilerError>;

    /// Register functions provided by this plugin into the code generator
    fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError>;

    /// Get metadata about functions provided by this plugin
    fn provided_functions(&self) -> Vec<FunctionMetadata>;

    /// Check if this plugin provides the given function
    fn provides_function(&self, name: &str) -> bool {
        self.provided_functions().iter().any(|f| f.name == name)
    }

    /// Called when the plugin is being unloaded
    fn cleanup(&mut self) -> Result<(), CompilerError> {
        Ok(())
    }

    /// Get configuration schema for this plugin
    fn config_schema(&self) -> PluginConfigSchema {
        PluginConfigSchema::default()
    }

    /// Get plugin as Any trait object for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get mutable plugin as Any trait object for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Get plugin priority for load order (higher = loaded first)
    fn priority(&self) -> i32 {
        0
    }

    /// Check if plugin is enabled
    fn is_enabled(&self) -> bool {
        true
    }
}

/// Metadata describing a function provided by a plugin
#[derive(Debug, Clone)]
pub struct FunctionMetadata {
    pub name: String,
    pub signature: String,
    pub description: String,
    pub category: FunctionCategory,
    pub is_builtin: bool,
    pub is_async: bool,
    pub memory_requirements: MemoryRequirements,
}

/// Categories of functions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionCategory {
    Math,
    String,
    List,
    Matrix,
    IO,
    Network,
    Memory,
    Console,
    File,
    Type,
    Async,
    Test,
    Error,
    Other,
}

/// Memory requirements for a function
#[derive(Debug, Clone)]
pub struct MemoryRequirements {
    pub heap_bytes: Option<usize>,
    pub stack_depth: Option<usize>,
    pub allocates_memory: bool,
}

impl Default for MemoryRequirements {
    fn default() -> Self {
        Self {
            heap_bytes: None,
            stack_depth: None,
            allocates_memory: false,
        }
    }
}

/// Configuration passed to plugins during initialization
#[derive(Debug, Clone)]
pub struct PluginConfig {
    pub name: String,
    pub settings: HashMap<String, ConfigValue>,
    pub memory_start: usize,
    pub debug_mode: bool,
    pub optimization_level: u8,
}

/// Configuration value types
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<ConfigValue>),
    Map(HashMap<String, ConfigValue>),
}

impl ConfigValue {
    pub fn as_string(&self) -> Option<&str> {
        match self {
            ConfigValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            ConfigValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            ConfigValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            ConfigValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }
}

/// Schema for plugin configuration
#[derive(Debug, Clone)]
pub struct PluginConfigSchema {
    pub settings: HashMap<String, ConfigSetting>,
}

impl Default for PluginConfigSchema {
    fn default() -> Self {
        Self {
            settings: HashMap::new(),
        }
    }
}

/// Configuration setting definition
#[derive(Debug, Clone)]
pub struct ConfigSetting {
    pub name: String,
    pub description: String,
    pub setting_type: ConfigSettingType,
    pub default_value: Option<ConfigValue>,
    pub required: bool,
    pub validation: Option<ValidationRule>,
}

/// Types of configuration settings
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigSettingType {
    String,
    Integer,
    Float,
    Boolean,
    List(Box<ConfigSettingType>),
    Map,
}

/// Validation rules for configuration
#[derive(Debug, Clone)]
pub enum ValidationRule {
    Range { min: f64, max: f64 },
    MinLength(usize),
    MaxLength(usize),
    Regex(String),
    OneOf(Vec<ConfigValue>),
    Custom(String), // Custom validation function name
}

/// Plugin registry that manages all loaded plugins
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn StdlibPlugin>>,
    load_order: Vec<String>,
    function_map: HashMap<String, String>, // function_name -> plugin_name
    initialized: bool,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            load_order: Vec::new(),
            function_map: HashMap::new(),
            initialized: false,
        }
    }

    /// Register a plugin with the registry
    pub fn register_plugin<P: StdlibPlugin + 'static>(
        &mut self,
        mut plugin: P,
    ) -> Result<(), CompilerError> {
        let name = plugin.name().to_string();

        // Check for conflicts
        if self.plugins.contains_key(&name) {
            return Err(CompilerError::runtime_error(
                format!("Plugin '{}' is already registered", name),
                None,
                None,
            ));
        }

        // Check dependencies
        self.validate_dependencies(&plugin)?;

        // Initialize with default config
        let config = PluginConfig {
            name: name.clone(),
            settings: HashMap::new(),
            memory_start: 65536, // Default WASM memory start
            debug_mode: false,
            optimization_level: 0,
        };

        plugin.initialize(config)?;

        // Register functions in mapping
        for func in plugin.provided_functions() {
            if self.function_map.contains_key(&func.name) {
                return Err(CompilerError::runtime_error(
                    format!(
                        "Function '{}' conflicts with existing function from plugin '{}'",
                        func.name, self.function_map[&func.name]
                    ),
                    None,
                    None,
                ));
            }
            self.function_map.insert(func.name, name.clone());
        }

        // Add to load order based on priority
        let priority = plugin.priority();
        let insert_pos = self
            .load_order
            .iter()
            .position(|plugin_name| self.plugins[plugin_name].priority() < priority)
            .unwrap_or(self.load_order.len());

        self.load_order.insert(insert_pos, name.clone());
        self.plugins.insert(name, Box::new(plugin));

        Ok(())
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<&dyn StdlibPlugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    /// Initialize a plugin by name with config
    pub fn initialize_plugin(
        &mut self,
        name: &str,
        config: PluginConfig,
    ) -> Result<(), CompilerError> {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.initialize(config)?;
        }
        Ok(())
    }

    /// Find which plugin provides a specific function
    pub fn find_function_provider(&self, function_name: &str) -> Option<&str> {
        self.function_map.get(function_name).map(|s| s.as_str())
    }

    /// List all registered plugins
    pub fn list_plugins(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Get plugins in load order
    pub fn plugins_in_order(&self) -> Vec<&dyn StdlibPlugin> {
        self.load_order
            .iter()
            .filter_map(|name| self.plugins.get(name).map(|p| p.as_ref()))
            .collect()
    }

    /// Register all plugin functions with the code generator
    pub fn register_all_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        for plugin in self.plugins_in_order() {
            if plugin.is_enabled() {
                plugin.register_functions(codegen)?;
            }
        }
        Ok(())
    }

    /// Get all provided functions grouped by category
    pub fn get_functions_by_category(&self) -> HashMap<FunctionCategory, Vec<FunctionMetadata>> {
        let mut categories = HashMap::new();

        for plugin in self.plugins.values() {
            if plugin.is_enabled() {
                for func in plugin.provided_functions() {
                    categories
                        .entry(func.category.clone())
                        .or_insert_with(Vec::new)
                        .push(func);
                }
            }
        }

        categories
    }

    /// Validate plugin dependencies
    fn validate_dependencies(&self, plugin: &dyn StdlibPlugin) -> Result<(), CompilerError> {
        for dep in plugin.dependencies() {
            if !self.plugins.contains_key(dep) {
                return Err(CompilerError::runtime_error(
                    format!(
                        "Plugin '{}' depends on '{}' which is not registered",
                        plugin.name(),
                        dep
                    ),
                    None,
                    None,
                ));
            }
        }
        Ok(())
    }

    /// Unload a plugin
    pub fn unload_plugin(&mut self, name: &str) -> Result<(), CompilerError> {
        if let Some(mut plugin) = self.plugins.remove(name) {
            plugin.cleanup()?;

            // Remove from load order
            self.load_order.retain(|n| n != name);

            // Remove function mappings
            self.function_map
                .retain(|_, plugin_name| plugin_name != name);

            Ok(())
        } else {
            Err(CompilerError::runtime_error(
                format!("Plugin '{}' is not registered", name),
                None,
                None,
            ))
        }
    }

    /// Cleanup all plugins
    pub fn cleanup_all(&mut self) -> Result<(), CompilerError> {
        let mut errors = Vec::new();

        for (name, mut plugin) in self.plugins.drain() {
            if let Err(e) = plugin.cleanup() {
                errors.push(format!("Failed to cleanup plugin '{}': {}", name, e));
            }
        }

        self.load_order.clear();
        self.function_map.clear();
        self.initialized = false;

        if !errors.is_empty() {
            return Err(CompilerError::runtime_error(
                format!("Plugin cleanup errors: {}", errors.join("; ")),
                None,
                None,
            ));
        }

        Ok(())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Macro to simplify plugin registration
#[macro_export]
macro_rules! register_plugin {
    ($registry:expr, $plugin:expr) => {
        $registry.register_plugin($plugin).map_err(|e| {
            eprintln!("Failed to register plugin '{}': {}", $plugin.name(), e);
            e
        })?;
    };
}

/// Helper trait for easy plugin casting
pub trait PluginCast {
    fn downcast_ref<T: StdlibPlugin + 'static>(&self) -> Option<&T>;
    fn downcast_mut<T: StdlibPlugin + 'static>(&mut self) -> Option<&mut T>;
}

impl PluginCast for dyn StdlibPlugin {
    fn downcast_ref<T: StdlibPlugin + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    fn downcast_mut<T: StdlibPlugin + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct TestPlugin {
        name: String,
        initialized: Arc<AtomicBool>,
    }

    impl TestPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                initialized: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl StdlibPlugin for TestPlugin {
        fn name(&self) -> &'static str {
            // For testing, we'll use a static string
            "test_plugin"
        }

        fn version(&self) -> &'static str {
            "1.0.0"
        }

        fn description(&self) -> &'static str {
            "Test plugin for unit testing"
        }

        fn initialize(&mut self, _config: PluginConfig) -> Result<(), CompilerError> {
            self.initialized.store(true, Ordering::Relaxed);
            Ok(())
        }

        fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
            Ok(())
        }

        fn provided_functions(&self) -> Vec<FunctionMetadata> {
            vec![FunctionMetadata {
                name: "test_func".to_string(),
                signature: "test_func() -> void".to_string(),
                description: "Test function".to_string(),
                category: FunctionCategory::Test,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            }]
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_plugin_registry_creation() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.list_plugins().len(), 0);
    }

    #[test]
    fn test_plugin_registration() {
        let mut registry = PluginRegistry::new();
        let plugin = TestPlugin::new("test");

        assert!(registry.register_plugin(plugin).is_ok());
        assert_eq!(registry.list_plugins().len(), 1);
        assert!(registry.list_plugins().contains(&"test_plugin"));
    }

    #[test]
    fn test_function_provider_lookup() {
        let mut registry = PluginRegistry::new();
        let plugin = TestPlugin::new("test");

        registry.register_plugin(plugin).unwrap();

        assert_eq!(
            registry.find_function_provider("test_func"),
            Some("test_plugin")
        );
        assert_eq!(registry.find_function_provider("nonexistent"), None);
    }

    #[test]
    fn test_config_value_accessors() {
        let string_val = ConfigValue::String("test".to_string());
        let int_val = ConfigValue::Integer(42);
        let float_val = ConfigValue::Float(3.14);
        let bool_val = ConfigValue::Boolean(true);

        assert_eq!(string_val.as_string(), Some("test"));
        assert_eq!(int_val.as_integer(), Some(42));
        assert_eq!(float_val.as_float(), Some(3.14));
        assert_eq!(bool_val.as_boolean(), Some(true));

        // Cross-type access should return None
        assert_eq!(string_val.as_integer(), None);
        assert_eq!(int_val.as_string(), None);
    }
}
