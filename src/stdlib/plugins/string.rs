use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::plugin::{
    FunctionCategory, FunctionMetadata, MemoryRequirements, PluginConfig, StdlibPlugin,
};
use std::any::Any;

/// String operations plugin
#[allow(dead_code)]
pub struct StringPlugin {
    encoding: StringEncoding,
    max_length: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringEncoding {
    Utf8,
    Utf16,
    Ascii,
}

impl StringPlugin {
    pub fn new() -> Self {
        Self {
            encoding: StringEncoding::Utf8,
            max_length: None,
        }
    }
}

impl Default for StringPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl StdlibPlugin for StringPlugin {
    fn name(&self) -> &'static str {
        "string"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn description(&self) -> &'static str {
        "String manipulation and processing functions"
    }

    fn dependencies(&self) -> Vec<&'static str> {
        vec!["memory"]
    }

    fn initialize(&mut self, _config: PluginConfig) -> Result<(), CompilerError> {
        Ok(())
    }

    fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // TODO: Implement string functions
        Ok(())
    }

    fn provided_functions(&self) -> Vec<FunctionMetadata> {
        vec![
            FunctionMetadata {
                name: "concat".to_string(),
                signature: "concat(a: string, b: string) -> string".to_string(),
                description: "Concatenate two strings".to_string(),
                category: FunctionCategory::String,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements {
                    heap_bytes: Some(128), // Estimated
                    stack_depth: Some(2),
                    allocates_memory: true,
                },
            },
            FunctionMetadata {
                name: "length".to_string(),
                signature: "length(s: string) -> integer".to_string(),
                description: "Get string length".to_string(),
                category: FunctionCategory::String,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
        ]
    }

    fn priority(&self) -> i32 {
        90 // High priority
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
