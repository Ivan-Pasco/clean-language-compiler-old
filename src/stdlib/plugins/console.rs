use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::plugin::{
    FunctionCategory, FunctionMetadata, MemoryRequirements, PluginConfig, StdlibPlugin,
};
use std::any::Any;

/// Console I/O plugin
#[allow(dead_code)]
pub struct ConsolePlugin {
    buffer_size: usize,
    async_mode: bool,
}

impl ConsolePlugin {
    pub fn new() -> Self {
        Self {
            buffer_size: 1024,
            async_mode: false,
        }
    }
}

impl Default for ConsolePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl StdlibPlugin for ConsolePlugin {
    fn name(&self) -> &'static str {
        "console"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn description(&self) -> &'static str {
        "Console input/output operations"
    }

    fn dependencies(&self) -> Vec<&'static str> {
        vec!["string", "memory"]
    }

    fn initialize(&mut self, _config: PluginConfig) -> Result<(), CompilerError> {
        Ok(())
    }

    fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Console functions are registered by builtin_generator.rs:register_console_operations()
        // This plugin provides metadata for IDE tooling and documentation
        // Actual WASM code generation uses the print/printl imports
        Ok(())
    }

    fn provided_functions(&self) -> Vec<FunctionMetadata> {
        vec![
            FunctionMetadata {
                name: "print".to_string(),
                signature: "print(message: string) -> void".to_string(),
                description: "Print message to console".to_string(),
                category: FunctionCategory::Console,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(64),
                    stack_depth: Some(1),
                    allocates_memory: false,
                },
            },
            FunctionMetadata {
                name: "println".to_string(),
                signature: "println(message: string) -> void".to_string(),
                description: "Print message with newline".to_string(),
                category: FunctionCategory::Console,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(64),
                    stack_depth: Some(1),
                    allocates_memory: false,
                },
            },
        ]
    }

    fn priority(&self) -> i32 {
        50 // Medium priority
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
