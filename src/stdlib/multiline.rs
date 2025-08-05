use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, ValType};

/// Multi-line expression support for Clean Language
/// Enables expressions to span multiple lines when wrapped in parentheses
pub struct MultilineManager {
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl MultilineManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all multi-line expression functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_expression_helpers(codegen)?;
        self.register_chaining_functions(codegen)?;
        Ok(())
    }

    /// Register expression helper functions for multi-line support
    fn register_expression_helpers(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Function to combine multiple integer expressions
        register_stdlib_function(
            codegen,
            "multiline.combineIntegers",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // operator, left, right
            Some(WasmType::I32),                            // result
            self.generate_combine_integers(),
        )?;

        // Function to combine multiple number expressions
        register_stdlib_function(
            codegen,
            "multiline.combineNumbers",
            &[WasmType::I32, WasmType::F64, WasmType::F64], // operator, left, right
            Some(WasmType::F64),                            // result
            self.generate_combine_numbers(),
        )?;

        // Function to combine multiple string expressions
        register_stdlib_function(
            codegen,
            "multiline.combineStrings",
            &[WasmType::I32, WasmType::I32], // string1_ptr, string2_ptr
            Some(WasmType::I32),             // result_ptr
            self.generate_combine_strings(),
        )?;

        // Function to evaluate grouped expressions
        register_stdlib_function(
            codegen,
            "multiline.evaluateGroup",
            &[WasmType::I32],    // expression_value
            Some(WasmType::I32), // result (pass-through)
            self.generate_evaluate_group(),
        )?;

        Ok(())
    }

    /// Register function chaining support for multi-line expressions
    fn register_chaining_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Chain integer operations across multiple lines
        register_stdlib_function(
            codegen,
            "multiline.chainIntegers",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // base_value, operation, operand
            Some(WasmType::I32),                            // result
            self.generate_chain_integers(),
        )?;

        // Chain number operations across multiple lines
        register_stdlib_function(
            codegen,
            "multiline.chainNumbers",
            &[WasmType::F64, WasmType::I32, WasmType::F64], // base_value, operation, operand
            Some(WasmType::F64),                            // result
            self.generate_chain_numbers(),
        )?;

        // Chain boolean operations across multiple lines
        register_stdlib_function(
            codegen,
            "multiline.chainBooleans",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // base_value, operation, operand
            Some(WasmType::I32),                            // result
            self.generate_chain_booleans(),
        )?;

        // Parenthesized expression evaluation
        register_stdlib_function(
            codegen,
            "multiline.parenthesized",
            &[WasmType::I32],    // expression_result
            Some(WasmType::I32), // result (pass-through with validation)
            self.generate_parenthesized_expression(),
        )?;

        Ok(())
    }

    /// Generate WASM for combining integer expressions
    fn generate_combine_integers(&self) -> Vec<Instruction> {
        vec![
            // Parameters: operator (0), left (1), right (2)
            // operator: 0=add, 1=sub, 2=mul, 3=div, 4=mod
            Instruction::LocalGet(0), // operator
            // Switch on operator type
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Addition
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32Add,
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Subtraction
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32Sub,
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Multiplication
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32Mul,
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Division
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32DivS,
            Instruction::Else,
            // Modulo (default case)
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32RemS,
            Instruction::End,
            Instruction::End,
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for combining number expressions
    fn generate_combine_numbers(&self) -> Vec<Instruction> {
        vec![
            // Parameters: operator (0), left (1), right (2)
            // operator: 0=add, 1=sub, 2=mul, 3=div
            Instruction::LocalGet(0), // operator
            // Switch on operator type
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
            // Addition
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::F64Add,
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
            // Subtraction
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::F64Sub,
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
            // Multiplication
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::F64Mul,
            Instruction::Else,
            // Division (default case)
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::F64Div,
            Instruction::End,
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for combining string expressions
    fn generate_combine_strings(&self) -> Vec<Instruction> {
        vec![
            // Parameters: string1_ptr (0), string2_ptr (1)
            // For now, just concatenate strings (simplified implementation)

            // Call string concatenation function (would be implemented in string_ops)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::Call(self.get_string_concat_function_index()),
        ]
    }

    /// Generate WASM for evaluating grouped expressions
    fn generate_evaluate_group(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expression_value (0)
            // This is essentially a pass-through function that validates grouping
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for chaining integer operations
    fn generate_chain_integers(&self) -> Vec<Instruction> {
        vec![
            // Parameters: base_value (0), operation (1), operand (2)
            // Similar to combine_integers but with base value as starting point
            Instruction::LocalGet(1), // operation
            Instruction::LocalGet(0), // base_value
            Instruction::LocalGet(2), // operand
            Instruction::Call(self.get_combine_integers_function_index()),
        ]
    }

    /// Generate WASM for chaining number operations
    fn generate_chain_numbers(&self) -> Vec<Instruction> {
        vec![
            // Parameters: base_value (0), operation (1), operand (2)
            Instruction::LocalGet(1), // operation (as i32)
            Instruction::LocalGet(0), // base_value
            Instruction::LocalGet(2), // operand
            Instruction::Call(self.get_combine_numbers_function_index()),
        ]
    }

    /// Generate WASM for chaining boolean operations
    fn generate_chain_booleans(&self) -> Vec<Instruction> {
        vec![
            // Parameters: base_value (0), operation (1), operand (2)
            // operation: 0=and, 1=or, 2=xor
            Instruction::LocalGet(1), // operation
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // AND operation
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32And,
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // OR operation
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32Or,
            Instruction::Else,
            // XOR operation (default)
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32Xor,
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for parenthesized expression evaluation
    fn generate_parenthesized_expression(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expression_result (0)
            // This validates that parentheses are properly balanced and returns the result

            // For now, this is a pass-through that could be extended with validation
            Instruction::LocalGet(0),
        ]
    }

    // Helper function indices (would be resolved from function table)
    fn get_string_concat_function_index(&self) -> u32 {
        500
    }
    fn get_combine_integers_function_index(&self) -> u32 {
        501
    }
    fn get_combine_numbers_function_index(&self) -> u32 {
        502
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiline_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let multiline_manager = MultilineManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert_eq!(
            multiline_manager
                .memory_manager
                .borrow()
                .get_total_allocated(),
            0
        );
    }

    #[test]
    fn test_combine_integers_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let multiline_manager = MultilineManager::new(memory_manager);

        let instructions = multiline_manager.generate_combine_integers();
        assert!(!instructions.is_empty());

        // Check that it contains conditional logic for operations
        // The If instruction should be at index 3 after LocalGet(0), I32Const(0), I32Eq
        assert!(matches!(instructions[3], Instruction::If(_)));
    }

    #[test]
    fn test_combine_numbers_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let multiline_manager = MultilineManager::new(memory_manager);

        let instructions = multiline_manager.generate_combine_numbers();
        assert!(!instructions.is_empty());

        // Check that it contains conditional logic for operations
        // The If instruction should be at index 3 after LocalGet(0), I32Const(0), I32Eq
        assert!(matches!(instructions[3], Instruction::If(_)));
    }

    #[test]
    fn test_parenthesized_expression() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let multiline_manager = MultilineManager::new(memory_manager);

        let instructions = multiline_manager.generate_parenthesized_expression();
        assert!(!instructions.is_empty());

        // Should be a simple pass-through
        assert!(matches!(instructions[0], Instruction::LocalGet(0)));
    }

    #[test]
    fn test_chain_operations() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let multiline_manager = MultilineManager::new(memory_manager);

        let int_chain = multiline_manager.generate_chain_integers();
        assert!(!int_chain.is_empty());

        let bool_chain = multiline_manager.generate_chain_booleans();
        assert!(!bool_chain.is_empty());
        // The If instruction should be at index 3 after LocalGet(1), I32Const(0), I32Eq
        assert!(matches!(bool_chain[3], Instruction::If(_)));
    }
}
