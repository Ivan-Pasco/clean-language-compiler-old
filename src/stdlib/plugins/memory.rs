use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::plugin::{
    FunctionCategory, FunctionMetadata, MemoryRequirements, PluginConfig, StdlibPlugin,
};
use std::any::Any;

/// Memory management plugin
pub struct MemoryPlugin {
    heap_size: usize,
    gc_enabled: bool,
}

impl MemoryPlugin {
    pub fn new() -> Self {
        Self {
            heap_size: 1024 * 1024, // 1MB default
            gc_enabled: true,
        }
    }
}

impl Default for MemoryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl StdlibPlugin for MemoryPlugin {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn description(&self) -> &'static str {
        "Memory management and allocation functions"
    }

    fn initialize(&mut self, config: PluginConfig) -> Result<(), CompilerError> {
        if let Some(heap_size) = config.settings.get("heap_size") {
            if let Some(size) = heap_size.as_integer() {
                self.heap_size = size as usize;
            }
        }

        if let Some(gc_enabled) = config.settings.get("gc_enabled") {
            if let Some(enabled) = gc_enabled.as_boolean() {
                self.gc_enabled = enabled;
            }
        }

        Ok(())
    }

    fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // TODO: Implement memory management functions
        Ok(())
    }

    fn provided_functions(&self) -> Vec<FunctionMetadata> {
        vec![
            FunctionMetadata {
                name: "allocate".to_string(),
                signature: "allocate(size: integer) -> pointer".to_string(),
                description: "Allocate memory block".to_string(),
                category: FunctionCategory::Memory,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(0), // Variable
                    stack_depth: Some(1),
                    allocates_memory: true,
                },
            },
            FunctionMetadata {
                name: "deallocate".to_string(),
                signature: "deallocate(ptr: pointer) -> void".to_string(),
                description: "Deallocate memory block".to_string(),
                category: FunctionCategory::Memory,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
        ]
    }

    fn priority(&self) -> i32 {
        200 // Highest priority - everything depends on memory
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
