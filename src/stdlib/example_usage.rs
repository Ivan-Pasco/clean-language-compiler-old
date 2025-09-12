//! Example usage of the modular plugin-based standard library
//! 
//! This file demonstrates how to use the new plugin system for the Clean Language standard library.

#[allow(unused_imports)]
use crate::stdlib::{
    ModularStandardLibrary, StandardLibraryBuilder, StandardLibraryConfig,
    StdlibPlugin, FunctionCategory, PluginRegistry
};
#[allow(unused_imports)]
use crate::stdlib::plugin::{ConfigValue, PluginConfig};
#[allow(unused_imports)]
use crate::stdlib::plugins::{MathPlugin, StringPlugin, ConsolePlugin};
use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use std::collections::HashMap;

/// Example: Creating a default standard library
#[allow(dead_code)]
pub fn example_basic_usage() -> Result<(), CompilerError> {
    // Create a standard library with default plugins
    let mut stdlib = ModularStandardLibrary::new()?;
    
    // Initialize all plugins
    stdlib.initialize()?;
    
    // List available plugins
    let plugins = stdlib.list_plugins();
    println!("Available plugins: {:?}", plugins);
    
    // Check if specific functions are available
    println!("Has 'add' function: {}", stdlib.has_function("add"));
    println!("Has 'print' function: {}", stdlib.has_function("print"));
    
    // Find which plugin provides a function
    if let Some(provider) = stdlib.find_function_provider("concat") {
        println!("'concat' function is provided by: {}", provider);
    }
    
    // Get functions by category
    let math_functions = stdlib.get_functions_by_category()
        .get(&FunctionCategory::Math)
        .cloned()
        .unwrap_or_default();
    
    println!("Math functions: {}", math_functions.len());
    
    Ok(())
}

/// Example: Using the builder pattern for custom configuration
#[allow(dead_code)]
pub fn example_custom_configuration() -> Result<(), CompilerError> {
    // Configure math plugin to use single precision
    let mut math_config = HashMap::new();
    math_config.insert("precision".to_string(), ConfigValue::String("single".to_string()));
    math_config.insert("enable_advanced".to_string(), ConfigValue::Boolean(false));
    
    // Build a custom standard library
    let stdlib = StandardLibraryBuilder::new()
        .with_debug_mode(true)
        .with_memory_start(32768)
        .with_optimization_level(2)
        .disable_category("console")  // Disable console functions
        .configure_plugin("math", math_config)
        .build()?;
    
    // Check which categories are enabled
    let plugins = stdlib.list_plugins();
    println!("Enabled plugins: {:?}", plugins);
    
    // Verify console is disabled
    assert!(!stdlib.has_function("print"));
    
    // Math functions should still be available but with single precision
    assert!(stdlib.has_function("add"));
    
    Ok(())
}

/// Example: Adding a custom plugin
#[allow(dead_code)]
pub fn example_custom_plugin() -> Result<(), CompilerError> {
    let mut stdlib = ModularStandardLibrary::new()?;
    
    // Create and add a custom plugin
    let custom_plugin = CustomFilePlugin::new();
    stdlib.add_plugin(custom_plugin)?;
    
    // Initialize all plugins including the custom one
    stdlib.initialize()?;
    
    // Check if custom functions are available
    println!("Has custom 'read_file' function: {}", stdlib.has_function("read_file"));
    
    Ok(())
}

/// Example: Plugin registry usage
#[allow(dead_code)]
pub fn example_registry_usage() -> Result<(), CompilerError> {
    let mut registry = PluginRegistry::new();
    
    // Register plugins manually
    registry.register_plugin(MathPlugin::new())?;
    registry.register_plugin(StringPlugin::new())?;
    registry.register_plugin(ConsolePlugin::new())?;
    
    // List plugins in load order (by priority)
    let plugins = registry.plugins_in_order();
    for plugin in plugins {
        println!("Plugin: {} (priority: {})", plugin.name(), plugin.priority());
    }
    
    // Get function mapping
    println!("'add' provided by: {:?}", registry.find_function_provider("add"));
    println!("'concat' provided by: {:?}", registry.find_function_provider("concat"));
    
    Ok(())
}

/// Example: Registering functions with code generator
#[allow(dead_code)]
pub fn example_code_generation(codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
    // Create and initialize standard library
    let mut stdlib = ModularStandardLibrary::new()?;
    stdlib.initialize()?;
    
    // Register all plugin functions with the code generator
    stdlib.register_functions(codegen)?;
    
    println!("Standard library functions registered with code generator");
    
    // Get memory usage estimation
    let estimated_memory = stdlib.estimate_memory_usage();
    println!("Estimated memory usage: {} bytes", estimated_memory);
    
    Ok(())
}

/// Example custom plugin implementation
#[allow(dead_code)]
struct CustomFilePlugin {
    max_file_size: usize,
}

#[allow(dead_code)]
impl CustomFilePlugin {
    fn new() -> Self {
        Self {
            max_file_size: 1024 * 1024, // 1MB
        }
    }
}

#[allow(dead_code)]
impl StdlibPlugin for CustomFilePlugin {
    fn name(&self) -> &'static str {
        "custom_file"
    }
    
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    
    fn description(&self) -> &'static str {
        "Custom file operations plugin"
    }
    
    fn dependencies(&self) -> Vec<&'static str> {
        vec!["memory", "string"]
    }
    
    fn initialize(&mut self, config: PluginConfig) -> Result<(), CompilerError> {
        if let Some(max_size) = config.settings.get("max_file_size") {
            if let Some(size) = max_size.as_integer() {
                self.max_file_size = size as usize;
            }
        }
        Ok(())
    }
    
    fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // In a real implementation, this would register WASM functions
        Ok(())
    }
    
    fn provided_functions(&self) -> Vec<crate::stdlib::plugin::FunctionMetadata> {
        use crate::stdlib::plugin::{FunctionMetadata, FunctionCategory, MemoryRequirements};
        
        vec![
            FunctionMetadata {
                name: "read_file".to_string(),
                signature: "read_file(path: string) -> string".to_string(),
                description: "Read file contents as string".to_string(),
                category: FunctionCategory::File,
                is_builtin: false,
                is_async: true,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(self.max_file_size),
                    stack_depth: Some(3),
                    allocates_memory: true,
                },
            },
            FunctionMetadata {
                name: "write_file".to_string(),
                signature: "write_file(path: string, content: string) -> void".to_string(),
                description: "Write content to file".to_string(),
                category: FunctionCategory::File,
                is_builtin: false,
                is_async: true,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(256),
                    stack_depth: Some(3),
                    allocates_memory: false,
                },
            },
        ]
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    
    fn priority(&self) -> i32 {
        10 // Lower priority custom plugin
    }
}

/// Example: Performance comparison between modular and legacy systems
#[allow(dead_code)]
pub fn example_performance_comparison() -> Result<(), CompilerError> {
    use std::time::Instant;
    
    // Time modular system initialization
    let start = Instant::now();
    let mut modular_stdlib = ModularStandardLibrary::new()?;
    modular_stdlib.initialize()?;
    let modular_time = start.elapsed();
    
    println!("Modular system initialization: {:?}", modular_time);
    
    // Time legacy system initialization
    let start = Instant::now();
    let _legacy_stdlib = crate::stdlib::StandardLibrary::new();
    let legacy_time = start.elapsed();
    
    println!("Legacy system initialization: {:?}", legacy_time);
    
    // Compare memory usage estimates
    let modular_memory = modular_stdlib.estimate_memory_usage();
    println!("Modular system estimated memory: {} bytes", modular_memory);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_basic_usage() {
        assert!(example_basic_usage().is_ok());
    }

    #[test]
    fn test_example_custom_configuration() {
        assert!(example_custom_configuration().is_ok());
    }

    #[test]
    fn test_example_registry_usage() {
        assert!(example_registry_usage().is_ok());
    }

    #[test]
    fn test_custom_plugin_creation() {
        let plugin = CustomFilePlugin::new();
        assert_eq!(plugin.name(), "custom_file");
        assert_eq!(plugin.version(), "1.0.0");
        
        let functions = plugin.provided_functions();
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].name, "read_file");
        assert_eq!(functions[1].name, "write_file");
    }
}