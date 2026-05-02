// Active standard library modules (used by codegen_registration.rs and the active codegen path)
pub mod basic_ops;
pub mod error;
pub mod file_class;
pub mod http_class;
pub mod json_class;
pub mod list_behavior;
pub mod list_class;
pub mod list_ops;
pub mod math_class;
pub mod matrix_ops;
pub mod memory;
pub mod method_style;
pub mod numeric_ops;
pub mod string_class;
pub mod string_ops;
pub mod type_conv;
pub mod validator;

// Re-exports used by codegen_registration, other stdlib modules, and codegen/memory.rs
pub use error::StdlibError;
pub use json_class::JsonClass;
pub use list_ops::ListManager;
pub use math_class::MathClass;
pub use matrix_ops::MatrixOperations;
pub use memory::MemoryManager;
pub use string_ops::StringManager;
pub use string_ops::StringOperations;
pub use type_conv::TypeConvOperations;

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::types::WasmType;
use wasm_encoder::Instruction;

/// Helper function to register stdlib functions
pub(crate) fn register_stdlib_function(
    codegen: &mut CodeGenerator,
    name: &str,
    params: &[WasmType],
    return_type: Option<WasmType>,
    instructions: Vec<Instruction>,
) -> Result<u32, CompilerError> {
    let function_index = codegen.register_function(name, params, return_type, &instructions)?;
    Ok(function_index)
}

/// Enhanced helper function to register stdlib functions with explicit local variable types
pub(crate) fn register_stdlib_function_with_locals(
    codegen: &mut CodeGenerator,
    name: &str,
    params: &[WasmType],
    return_type: Option<WasmType>,
    local_types: &[WasmType],
    instructions: Vec<Instruction>,
) -> Result<u32, CompilerError> {
    let function_index = codegen.register_function_with_locals(
        name,
        params,
        return_type,
        local_types,
        &instructions,
    )?;
    Ok(function_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_allocation() -> Result<(), CompilerError> {
        let mut memory = MemoryManager::new(16, Some(1024));
        assert!(memory.allocate(100, 1).is_ok());
        Ok(())
    }
}
