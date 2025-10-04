use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::plugin::{ConfigValue, PluginConfig, PluginRegistry};
use crate::stdlib::plugins::{ConsolePlugin, ListPlugin, MathPlugin, MemoryPlugin, StringPlugin};
use std::collections::HashMap;

/// Modular standard library using the plugin system
pub struct ModularStandardLibrary {
    registry: PluginRegistry,
    config: StandardLibraryConfig,
    initialized: bool,
}

/// Configuration for the standard library
#[derive(Debug, Clone)]
pub struct StandardLibraryConfig {
    pub memory_start: usize,
    pub debug_mode: bool,
    pub optimization_level: u8,
    pub enabled_categories: Vec<String>,
    pub plugin_configs: HashMap<String, HashMap<String, ConfigValue>>,
}

impl Default for StandardLibraryConfig {
    fn default() -> Self {
        Self {
            memory_start: 65536, // Default WASM memory start
            debug_mode: false,
            optimization_level: 0,
            enabled_categories: vec![
                "math".to_string(),
                "string".to_string(),
                "list".to_string(),
                "console".to_string(),
                "memory".to_string(),
            ],
            plugin_configs: HashMap::new(),
        }
    }
}

impl ModularStandardLibrary {
    /// Create a new modular standard library with default plugins
    pub fn new() -> Result<Self, CompilerError> {
        let mut lib = Self {
            registry: PluginRegistry::new(),
            config: StandardLibraryConfig::default(),
            initialized: false,
        };

        lib.register_default_plugins()?;
        Ok(lib)
    }

    /// Create a new modular standard library with custom configuration
    pub fn with_config(config: StandardLibraryConfig) -> Result<Self, CompilerError> {
        let mut lib = Self {
            registry: PluginRegistry::new(),
            config,
            initialized: false,
        };

        lib.register_default_plugins()?;
        Ok(lib)
    }

    /// Register all default plugins
    fn register_default_plugins(&mut self) -> Result<(), CompilerError> {
        // Register plugins in dependency order

        // Memory plugin first (no dependencies)
        if self
            .config
            .enabled_categories
            .contains(&"memory".to_string())
        {
            let plugin = MemoryPlugin::new();
            self.registry.register_plugin(plugin)?;
        }

        // String plugin (depends on memory)
        if self
            .config
            .enabled_categories
            .contains(&"string".to_string())
        {
            let plugin = StringPlugin::new();
            self.registry.register_plugin(plugin)?;
        }

        // List plugin (depends on memory)
        if self.config.enabled_categories.contains(&"list".to_string()) {
            let plugin = ListPlugin::new();
            self.registry.register_plugin(plugin)?;
        }

        // Math plugin (depends on memory)
        if self.config.enabled_categories.contains(&"math".to_string()) {
            let plugin = MathPlugin::new();
            self.registry.register_plugin(plugin)?;
        }

        // Console plugin (depends on string, memory)
        if self
            .config
            .enabled_categories
            .contains(&"console".to_string())
        {
            let plugin = ConsolePlugin::new();
            self.registry.register_plugin(plugin)?;
        }

        Ok(())
    }

    /// Initialize all registered plugins
    pub fn initialize(&mut self) -> Result<(), CompilerError> {
        if self.initialized {
            return Ok(());
        }

        let plugin_names: Vec<String> = self
            .registry
            .list_plugins()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        for plugin_name in plugin_names {
            let config = self.create_plugin_config(&plugin_name);
            self.registry.initialize_plugin(&plugin_name, config)?;
        }

        self.initialized = true;
        Ok(())
    }

    /// Create configuration for a specific plugin
    fn create_plugin_config(&self, plugin_name: &str) -> PluginConfig {
        let plugin_settings = self
            .config
            .plugin_configs
            .get(plugin_name)
            .cloned()
            .unwrap_or_default();

        PluginConfig {
            name: plugin_name.to_string(),
            settings: plugin_settings,
            memory_start: self.config.memory_start,
            debug_mode: self.config.debug_mode,
            optimization_level: self.config.optimization_level,
        }
    }

    /// Register all plugin functions with the code generator
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        if !self.initialized {
            return Err(CompilerError::runtime_error(
                "Standard library not initialized".to_string(),
                None,
                None,
            ));
        }

        self.registry.register_all_functions(codegen)
    }

    /// Add a custom plugin to the library
    pub fn add_plugin<P: crate::stdlib::plugin::StdlibPlugin + 'static>(
        &mut self,
        plugin: P,
    ) -> Result<(), CompilerError> {
        self.registry.register_plugin(plugin)
    }

    /// Remove a plugin from the library
    pub fn remove_plugin(&mut self, plugin_name: &str) -> Result<(), CompilerError> {
        self.registry.unload_plugin(plugin_name)
    }

    /// List all available plugins
    pub fn list_plugins(&self) -> Vec<&str> {
        self.registry.list_plugins()
    }

    /// Get plugin information
    pub fn get_plugin_info(&self, plugin_name: &str) -> Option<PluginInfo> {
        self.registry
            .get_plugin(plugin_name)
            .map(|plugin| PluginInfo {
                name: plugin.name().to_string(),
                version: plugin.version().to_string(),
                description: plugin.description().to_string(),
                dependencies: plugin
                    .dependencies()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                functions: plugin.provided_functions(),
                priority: plugin.priority(),
                is_enabled: plugin.is_enabled(),
            })
    }

    /// Find which plugin provides a specific function
    pub fn find_function_provider(&self, function_name: &str) -> Option<&str> {
        self.registry.find_function_provider(function_name)
    }

    /// Get functions by category
    pub fn get_functions_by_category(
        &self,
    ) -> HashMap<
        crate::stdlib::plugin::FunctionCategory,
        Vec<crate::stdlib::plugin::FunctionMetadata>,
    > {
        self.registry.get_functions_by_category()
    }

    /// Get configuration for a plugin
    pub fn get_plugin_config(&self, plugin_name: &str) -> Option<PluginConfig> {
        if self.registry.list_plugins().contains(&plugin_name) {
            Some(self.create_plugin_config(plugin_name))
        } else {
            None
        }
    }

    /// Update plugin configuration
    pub fn update_plugin_config(
        &mut self,
        plugin_name: &str,
        settings: HashMap<String, ConfigValue>,
    ) -> Result<(), CompilerError> {
        self.config
            .plugin_configs
            .insert(plugin_name.to_string(), settings);

        // Re-initialize the plugin if already initialized
        if self.initialized {
            let config = self.create_plugin_config(plugin_name);
            self.registry.initialize_plugin(plugin_name, config)?;
        }

        Ok(())
    }

    /// Check if a function is available
    pub fn has_function(&self, function_name: &str) -> bool {
        self.registry
            .find_function_provider(function_name)
            .is_some()
    }

    /// Get memory requirements for a function
    pub fn get_function_memory_requirements(
        &self,
        function_name: &str,
    ) -> Option<crate::stdlib::plugin::MemoryRequirements> {
        if let Some(provider) = self.registry.find_function_provider(function_name) {
            if let Some(plugin) = self.registry.get_plugin(provider) {
                for func in plugin.provided_functions() {
                    if func.name == function_name {
                        return Some(func.memory_requirements);
                    }
                }
            }
        }
        None
    }

    /// Get total estimated memory usage
    pub fn estimate_memory_usage(&self) -> usize {
        let mut total = 0;

        for plugin in self.registry.plugins_in_order() {
            for func in plugin.provided_functions() {
                if let Some(heap_bytes) = func.memory_requirements.heap_bytes {
                    total += heap_bytes;
                }
            }
        }

        total
    }

    /// Cleanup all plugins
    pub fn cleanup(&mut self) -> Result<(), CompilerError> {
        self.registry.cleanup_all()?;
        self.initialized = false;
        Ok(())
    }
}

impl Default for ModularStandardLibrary {
    fn default() -> Self {
        Self::new().expect("Failed to create default standard library")
    }
}

/// Information about a plugin
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub functions: Vec<crate::stdlib::plugin::FunctionMetadata>,
    pub priority: i32,
    pub is_enabled: bool,
}

/// Builder for creating customized standard library configurations
pub struct StandardLibraryBuilder {
    config: StandardLibraryConfig,
}

impl StandardLibraryBuilder {
    pub fn new() -> Self {
        Self {
            config: StandardLibraryConfig::default(),
        }
    }

    pub fn with_memory_start(mut self, start: usize) -> Self {
        self.config.memory_start = start;
        self
    }

    pub fn with_debug_mode(mut self, debug: bool) -> Self {
        self.config.debug_mode = debug;
        self
    }

    pub fn with_optimization_level(mut self, level: u8) -> Self {
        self.config.optimization_level = level;
        self
    }

    pub fn enable_category(mut self, category: &str) -> Self {
        if !self
            .config
            .enabled_categories
            .contains(&category.to_string())
        {
            self.config.enabled_categories.push(category.to_string());
        }
        self
    }

    pub fn disable_category(mut self, category: &str) -> Self {
        self.config.enabled_categories.retain(|c| c != category);
        self
    }

    pub fn configure_plugin(
        mut self,
        plugin_name: &str,
        settings: HashMap<String, ConfigValue>,
    ) -> Self {
        self.config
            .plugin_configs
            .insert(plugin_name.to_string(), settings);
        self
    }

    pub fn build(self) -> Result<ModularStandardLibrary, CompilerError> {
        let mut lib = ModularStandardLibrary::with_config(self.config)?;
        lib.initialize()?;
        Ok(lib)
    }
}

impl Default for StandardLibraryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modular_stdlib_creation() {
        let stdlib = ModularStandardLibrary::new().unwrap();
        let plugins = stdlib.list_plugins();

        assert!(plugins.contains(&"memory"));
        assert!(plugins.contains(&"math"));
        assert!(plugins.contains(&"string"));
        assert!(plugins.contains(&"list"));
        assert!(plugins.contains(&"console"));
    }

    #[test]
    fn test_function_provider_lookup() {
        let stdlib = ModularStandardLibrary::new().unwrap();

        assert_eq!(stdlib.find_function_provider("add"), Some("math"));
        assert_eq!(stdlib.find_function_provider("concat"), Some("string"));
        assert_eq!(stdlib.find_function_provider("push"), Some("list"));
        assert_eq!(stdlib.find_function_provider("print"), Some("console"));
    }

    #[test]
    fn test_stdlib_builder() {
        let stdlib = StandardLibraryBuilder::new()
            .with_debug_mode(true)
            .with_memory_start(32768)
            .disable_category("console")
            .build()
            .unwrap();

        let plugins = stdlib.list_plugins();
        assert!(!plugins.contains(&"console"));
        assert!(plugins.contains(&"math"));
    }

    #[test]
    fn test_plugin_config_update() {
        let mut stdlib = ModularStandardLibrary::new().unwrap();

        let mut math_config = HashMap::new();
        math_config.insert(
            "precision".to_string(),
            ConfigValue::String("single".to_string()),
        );

        assert!(stdlib.update_plugin_config("math", math_config).is_ok());
    }

    #[test]
    fn test_function_availability() {
        let stdlib = ModularStandardLibrary::new().unwrap();

        assert!(stdlib.has_function("add"));
        assert!(stdlib.has_function("concat"));
        assert!(!stdlib.has_function("nonexistent_function"));
    }

    #[test]
    fn test_memory_estimation() {
        let stdlib = ModularStandardLibrary::new().unwrap();
        let estimated = stdlib.estimate_memory_usage();

        // Should have some memory estimate from plugins
        assert!(estimated > 0);
    }
}
