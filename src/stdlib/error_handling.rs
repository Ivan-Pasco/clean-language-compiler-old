use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Error handling implementation for Clean Language
/// Enables onError expression and block forms for comprehensive error management
pub struct ErrorHandlingManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl ErrorHandlingManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all error handling functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_error_expression_functions(codegen)?;
        self.register_error_handling_blocks(codegen)?;
        self.register_error_propagation_functions(codegen)?;
        Ok(())
    }

    /// Register onError expression functions
    fn register_error_expression_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // onError expression for integers: value onError fallback_value
        register_stdlib_function_with_locals(
            codegen,
            "onError.integer",
            &[WasmType::I32, WasmType::I32], // value, fallback_value
            Some(WasmType::I32),             // result
            &[WasmType::I32],                // local for error flag
            self.generate_on_error_integer(),
        )?;

        // onError expression for numbers: value onError fallback_value
        register_stdlib_function_with_locals(
            codegen,
            "onError.number",
            &[WasmType::F64, WasmType::F64], // value, fallback_value
            Some(WasmType::F64),             // result
            &[WasmType::I32],                // local for error flag
            self.generate_on_error_number(),
        )?;

        // onError expression for strings: value onError fallback_value
        register_stdlib_function_with_locals(
            codegen,
            "onError.string",
            &[WasmType::I32, WasmType::I32], // value_ptr, fallback_ptr
            Some(WasmType::I32),             // result_ptr
            &[WasmType::I32],                // local for error flag
            self.generate_on_error_string(),
        )?;

        // onError expression for booleans: value onError fallback_value
        register_stdlib_function_with_locals(
            codegen,
            "onError.boolean",
            &[WasmType::I32, WasmType::I32], // value, fallback_value
            Some(WasmType::I32),             // result
            &[WasmType::I32],                // local for error flag
            self.generate_on_error_boolean(),
        )?;

        Ok(())
    }

    /// Register error handling block functions
    fn register_error_handling_blocks(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Error block execution: try { ... } onError { ... }
        register_stdlib_function_with_locals(
            codegen,
            "errorBlock.execute",
            &[WasmType::I32, WasmType::I32], // try_block_ptr, error_block_ptr
            Some(WasmType::I32),             // result
            &[WasmType::I32, WasmType::I32], // locals for error_occurred, result_value
            self.generate_error_block_execution(),
        )?;

        // Error information capture
        register_stdlib_function_with_locals(
            codegen,
            "errorBlock.captureError",
            &[WasmType::I32],                // error_code
            Some(WasmType::I32),             // error_info_ptr
            &[WasmType::I32, WasmType::I32], // locals for error_ptr, message_ptr
            self.generate_capture_error(),
        )?;

        // Error code checking
        register_stdlib_function_with_locals(
            codegen,
            "errorBlock.hasError",
            &[],                 // no parameters
            Some(WasmType::I32), // boolean result
            &[WasmType::I32],    // local for global error state
            self.generate_has_error(),
        )?;

        // Clear error state
        register_stdlib_function_with_locals(
            codegen,
            "errorBlock.clearError",
            &[],              // no parameters
            None,             // void
            &[WasmType::I32], // local for operation
            self.generate_clear_error(),
        )?;

        Ok(())
    }

    /// Register error propagation functions
    fn register_error_propagation_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Throw/raise error with message
        register_stdlib_function_with_locals(
            codegen,
            "error.throw",
            &[WasmType::I32, WasmType::I32], // error_code, message_ptr
            None,                            // void (doesn't return normally)
            &[WasmType::I32],                // local for error setup
            self.generate_throw_error(),
        )?;

        // Check for specific error type
        register_stdlib_function_with_locals(
            codegen,
            "error.isType",
            &[WasmType::I32],    // expected_error_code
            Some(WasmType::I32), // boolean result
            &[WasmType::I32],    // local for current error code
            self.generate_is_error_type(),
        )?;

        // Get error message
        register_stdlib_function_with_locals(
            codegen,
            "error.getMessage",
            &[],                 // no parameters
            Some(WasmType::I32), // message_ptr
            &[WasmType::I32],    // local for error state
            self.generate_get_error_message(),
        )?;

        // Chain error handling: value1 onError value2 onError value3
        register_stdlib_function_with_locals(
            codegen,
            "error.chain",
            &[WasmType::I32, WasmType::I32], // current_value, fallback_value
            Some(WasmType::I32),             // result
            &[WasmType::I32],                // local for error check
            self.generate_chain_error_handling(),
        )?;

        Ok(())
    }

    /// Generate WASM for onError integer expression
    fn generate_on_error_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), fallback_value (1)
            // Local: error_flag (2)

            // Check if there's a global error state
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(2), // Store error flag
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Error occurred - return fallback value and clear error
            Instruction::Call(self.get_clear_error_function_index()),
            Instruction::LocalGet(1), // fallback_value
            Instruction::Else,
            // No error - return original value
            Instruction::LocalGet(0), // value
            Instruction::End,
        ]
    }

    /// Generate WASM for onError number expression
    fn generate_on_error_number(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), fallback_value (1)
            // Local: error_flag (2)

            // Check if there's a global error state
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(2), // Store error flag
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Result(ValType::F64)),
            // Error occurred - return fallback value and clear error
            Instruction::Call(self.get_clear_error_function_index()),
            Instruction::LocalGet(1), // fallback_value
            Instruction::Else,
            // No error - return original value
            Instruction::LocalGet(0), // value
            Instruction::End,
        ]
    }

    /// Generate WASM for onError string expression
    fn generate_on_error_string(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0), fallback_ptr (1)
            // Local: error_flag (2)

            // Check if there's a global error state
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(2), // Store error flag
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Error occurred - return fallback value and clear error
            Instruction::Call(self.get_clear_error_function_index()),
            Instruction::LocalGet(1), // fallback_ptr
            Instruction::Else,
            // No error - return original value
            Instruction::LocalGet(0), // value_ptr
            Instruction::End,
        ]
    }

    /// Generate WASM for onError boolean expression
    fn generate_on_error_boolean(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), fallback_value (1)
            // Local: error_flag (2)

            // Check if there's a global error state
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(2), // Store error flag
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Error occurred - return fallback value and clear error
            Instruction::Call(self.get_clear_error_function_index()),
            Instruction::LocalGet(1), // fallback_value
            Instruction::Else,
            // No error - return original value
            Instruction::LocalGet(0), // value
            Instruction::End,
        ]
    }

    /// Generate WASM for error block execution
    fn generate_error_block_execution(&self) -> Vec<Instruction> {
        vec![
            // Parameters: try_block_ptr (0), error_block_ptr (1)
            // Locals: error_occurred (2), result_value (3)

            // Clear any existing error state
            Instruction::Call(self.get_clear_error_function_index()),
            // Execute try block (simplified - would call actual block)
            Instruction::LocalGet(0), // try_block_ptr
            Instruction::Call(self.get_execute_block_function_index()),
            Instruction::LocalSet(3), // Store result
            // Check if error occurred during execution
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(2), // error_occurred
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Error occurred - execute error block
            Instruction::LocalGet(1), // error_block_ptr
            Instruction::Call(self.get_execute_block_function_index()),
            Instruction::Else,
            // No error - return try block result
            Instruction::LocalGet(3), // result_value
            Instruction::End,
        ]
    }

    /// Generate WASM for error capture
    fn generate_capture_error(&self) -> Vec<Instruction> {
        vec![
            // Parameters: error_code (0)
            // Locals: error_ptr (1), message_ptr (2)

            // Allocate memory for error information
            Instruction::I32Const(16), // Error info size
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // error_ptr
            // Store error code
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Set global error state
            Instruction::I32Const(1), // Error occurred flag
            Instruction::Call(self.get_set_error_state_function_index()),
            // Return error info pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for error state checking
    fn generate_has_error(&self) -> Vec<Instruction> {
        vec![
            // Local: global_error_state (0)

            // Load global error state (simplified - would access global memory)
            Instruction::I32Const(self.get_error_state_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WASM for clearing error state
    fn generate_clear_error(&self) -> Vec<Instruction> {
        vec![
            // Local: operation (0)

            // Clear global error state
            Instruction::I32Const(self.get_error_state_address()),
            Instruction::I32Const(0), // No error
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WASM for throwing errors
    fn generate_throw_error(&self) -> Vec<Instruction> {
        vec![
            // Parameters: error_code (0), message_ptr (1)
            // Local: error_setup (2)

            // Store error code in global state
            Instruction::I32Const(self.get_error_state_address()),
            Instruction::LocalGet(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store error message pointer
            Instruction::I32Const(self.get_error_message_address()),
            Instruction::LocalGet(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Set error occurred flag
            Instruction::I32Const(self.get_error_flag_address()),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // This would normally jump to error handler or unwind stack
            // For now, we just return (in production would use unreachable or trap)
        ]
    }

    /// Generate WASM for error type checking
    fn generate_is_error_type(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expected_error_code (0)
            // Local: current_error_code (1)

            // Load current error code
            Instruction::I32Const(self.get_error_state_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1),
            // Compare with expected error code
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Eq,
        ]
    }

    /// Generate WASM for getting error message
    fn generate_get_error_message(&self) -> Vec<Instruction> {
        vec![
            // Local: error_state (0)

            // Check if error occurred
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(0),
            Instruction::LocalGet(0),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Error occurred - return message pointer
            Instruction::I32Const(self.get_error_message_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // No error - return null
            Instruction::I32Const(0),
            Instruction::End,
        ]
    }

    /// Generate WASM for chained error handling
    fn generate_chain_error_handling(&self) -> Vec<Instruction> {
        vec![
            // Parameters: current_value (0), fallback_value (1)
            // Local: error_check (2)

            // Check if there's an error for current value
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(2),
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Error occurred - use fallback and clear error
            Instruction::Call(self.get_clear_error_function_index()),
            Instruction::LocalGet(1), // fallback_value
            Instruction::Else,
            // No error - use current value
            Instruction::LocalGet(0), // current_value
            Instruction::End,
        ]
    }

    // Helper function indices and memory addresses
    fn get_has_error_function_index(&self) -> u32 {
        600
    }
    fn get_clear_error_function_index(&self) -> u32 {
        601
    }
    fn get_execute_block_function_index(&self) -> u32 {
        602
    }
    fn get_allocate_function_index(&self) -> u32 {
        603
    }
    fn get_set_error_state_function_index(&self) -> u32 {
        604
    }

    fn get_error_state_address(&self) -> i32 {
        0x1000
    }
    fn get_error_message_address(&self) -> i32 {
        0x1004
    }
    fn get_error_flag_address(&self) -> i32 {
        0x1008
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_handling_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let error_manager = ErrorHandlingManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert!(error_manager.memory_manager.borrow().data.len() > 0);
    }

    #[test]
    fn test_on_error_integer_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let error_manager = ErrorHandlingManager::new(memory_manager);

        let instructions = error_manager.generate_on_error_integer();
        assert!(!instructions.is_empty());

        // Check that it contains conditional logic after: Call, LocalSet, LocalGet
        assert!(matches!(instructions[3], Instruction::If(_)));
    }

    #[test]
    fn test_error_block_execution() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let error_manager = ErrorHandlingManager::new(memory_manager);

        let instructions = error_manager.generate_error_block_execution();
        assert!(!instructions.is_empty());

        // Should contain function calls for block execution after clear error call
        assert!(matches!(instructions[0], Instruction::Call(_)));
    }

    #[test]
    fn test_throw_error_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let error_manager = ErrorHandlingManager::new(memory_manager);

        let instructions = error_manager.generate_throw_error();
        assert!(!instructions.is_empty());

        // Should contain memory store operations for error state
        assert!(matches!(instructions[2], Instruction::I32Store(_)));
    }

    #[test]
    fn test_error_type_checking() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let error_manager = ErrorHandlingManager::new(memory_manager);

        let instructions = error_manager.generate_is_error_type();
        assert!(!instructions.is_empty());

        // Should end with comparison instruction after: I32Const, I32Load, LocalSet, LocalGet, LocalGet
        assert!(matches!(instructions[5], Instruction::I32Eq));
    }
}
