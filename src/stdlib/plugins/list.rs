use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::plugin::{
    FunctionCategory, FunctionMetadata, MemoryRequirements, PluginConfig, StdlibPlugin,
};
use std::any::Any;

/// List operations plugin
#[allow(dead_code)]
pub struct ListPlugin {
    default_capacity: usize,
    growth_factor: f32,
}

impl ListPlugin {
    pub fn new() -> Self {
        Self {
            default_capacity: 16,
            growth_factor: 2.0,
        }
    }
}

impl Default for ListPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl StdlibPlugin for ListPlugin {
    fn name(&self) -> &'static str {
        "list"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn description(&self) -> &'static str {
        "List and array manipulation functions"
    }

    fn dependencies(&self) -> Vec<&'static str> {
        vec!["memory"]
    }

    fn initialize(&mut self, _config: PluginConfig) -> Result<(), CompilerError> {
        Ok(())
    }

    fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // List functions are registered by builtin_generator.rs:register_list_operations()
        // This plugin provides metadata for IDE tooling and documentation
        // Actual WASM code generation uses array imports (array_push, array_pop, etc.)
        Ok(())
    }

    fn provided_functions(&self) -> Vec<FunctionMetadata> {
        vec![
            FunctionMetadata {
                name: "push".to_string(),
                signature: "push(list: List<T>, item: T) -> void".to_string(),
                description: "Add item to end of list".to_string(),
                category: FunctionCategory::List,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(32),
                    stack_depth: Some(2),
                    allocates_memory: true,
                },
            },
            FunctionMetadata {
                name: "pop".to_string(),
                signature: "pop(list: List<T>) -> T".to_string(),
                description: "Remove and return last item".to_string(),
                category: FunctionCategory::List,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
        ]
    }

    fn priority(&self) -> i32 {
        80 // High priority
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
