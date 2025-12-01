use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, ValType};

/// Conditional expressions implementation for Clean Language
/// Enables if-then-else ternary-style conditionals in expressions
pub struct ConditionalManager {
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl ConditionalManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all conditional expression functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        tracing::debug!("DEBUG CONDITIONAL: Starting register_functions");
        tracing::debug!("DEBUG CONDITIONAL: Calling register_conditional_functions");
        self.register_conditional_functions(codegen)?;
        tracing::debug!("DEBUG CONDITIONAL: Calling register_comparison_functions");
        self.register_comparison_functions(codegen)?;
        tracing::debug!("DEBUG CONDITIONAL: Calling register_logical_functions");
        self.register_logical_functions(codegen)?;
        tracing::debug!("DEBUG CONDITIONAL: All functions registered");
        Ok(())
    }

    /// Register core conditional expression functions
    fn register_conditional_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Integer conditional: if condition then value1 else value2
        register_stdlib_function(
            codegen,
            "conditional.integer",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // condition, then_value, else_value
            Some(WasmType::I32),                            // result
            self.generate_integer_conditional(),
        )?;

        // Number conditional: if condition then value1 else value2
        register_stdlib_function(
            codegen,
            "conditional.number",
            &[WasmType::I32, WasmType::F64, WasmType::F64], // condition, then_value, else_value
            Some(WasmType::F64),                            // result
            self.generate_number_conditional(),
        )?;

        // String conditional: if condition then string1 else string2
        register_stdlib_function(
            codegen,
            "conditional.string",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // condition, then_ptr, else_ptr
            Some(WasmType::I32),                            // result_ptr
            self.generate_string_conditional(),
        )?;

        // Boolean conditional: if condition then bool1 else bool2
        register_stdlib_function(
            codegen,
            "conditional.boolean",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // condition, then_bool, else_bool
            Some(WasmType::I32),                            // result
            self.generate_boolean_conditional(),
        )?;

        Ok(())
    }

    /// Register comparison functions that return boolean conditions
    fn register_comparison_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        tracing::debug!("DEBUG CONDITIONAL: Registering comparison functions");

        // Integer comparisons
        tracing::debug!("DEBUG CONDITIONAL: Registering compare.integer.equal");
        register_stdlib_function(
            codegen,
            "compare.integer.equal",
            &[WasmType::I32, WasmType::I32], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_integer_equal(),
        )?;
        tracing::debug!("DEBUG CONDITIONAL: Successfully registered compare.integer.equal");

        register_stdlib_function(
            codegen,
            "compare.integer.notEqual",
            &[WasmType::I32, WasmType::I32], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_integer_not_equal(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.integer.lessThan",
            &[WasmType::I32, WasmType::I32], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_integer_less_than(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.integer.greaterThan",
            &[WasmType::I32, WasmType::I32], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_integer_greater_than(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.integer.lessEqual",
            &[WasmType::I32, WasmType::I32], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_integer_less_equal(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.integer.greaterEqual",
            &[WasmType::I32, WasmType::I32], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_integer_greater_equal(),
        )?;

        // Number comparisons
        register_stdlib_function(
            codegen,
            "compare.number.equal",
            &[WasmType::F64, WasmType::F64], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_number_equal(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.number.lessThan",
            &[WasmType::F64, WasmType::F64], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_number_less_than(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.number.greaterThan",
            &[WasmType::F64, WasmType::F64], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_number_greater_than(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.number.greaterEqual",
            &[WasmType::F64, WasmType::F64], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_number_greater_equal(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.number.lessEqual",
            &[WasmType::F64, WasmType::F64], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_number_less_equal(),
        )?;

        register_stdlib_function(
            codegen,
            "compare.number.notEqual",
            &[WasmType::F64, WasmType::F64], // value1, value2
            Some(WasmType::I32),             // boolean result
            self.generate_number_not_equal(),
        )?;

        Ok(())
    }

    /// Register logical functions for combining conditions
    fn register_logical_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Logical AND
        register_stdlib_function(
            codegen,
            "logical.and",
            &[WasmType::I32, WasmType::I32], // condition1, condition2
            Some(WasmType::I32),             // boolean result
            self.generate_logical_and(),
        )?;

        // Logical OR
        register_stdlib_function(
            codegen,
            "logical.or",
            &[WasmType::I32, WasmType::I32], // condition1, condition2
            Some(WasmType::I32),             // boolean result
            self.generate_logical_or(),
        )?;

        // Logical NOT
        register_stdlib_function(
            codegen,
            "logical.not",
            &[WasmType::I32],    // condition
            Some(WasmType::I32), // boolean result
            self.generate_logical_not(),
        )?;

        Ok(())
    }

    /// Generate WASM for integer conditional expression
    fn generate_integer_conditional(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0), then_value (1), else_value (2)
            Instruction::LocalGet(0), // condition
            Instruction::If(BlockType::Result(ValType::I32)),
            // If condition is true, return then_value
            Instruction::LocalGet(1),
            Instruction::Else,
            // If condition is false, return else_value
            Instruction::LocalGet(2),
            Instruction::End,
        ]
    }

    /// Generate WASM for number conditional expression
    fn generate_number_conditional(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0), then_value (1), else_value (2)
            Instruction::LocalGet(0), // condition
            Instruction::If(BlockType::Result(ValType::F64)),
            // If condition is true, return then_value
            Instruction::LocalGet(1),
            Instruction::Else,
            // If condition is false, return else_value
            Instruction::LocalGet(2),
            Instruction::End,
        ]
    }

    /// Generate WASM for string conditional expression
    fn generate_string_conditional(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0), then_ptr (1), else_ptr (2)
            Instruction::LocalGet(0), // condition
            Instruction::If(BlockType::Result(ValType::I32)),
            // If condition is true, return then_ptr
            Instruction::LocalGet(1),
            Instruction::Else,
            // If condition is false, return else_ptr
            Instruction::LocalGet(2),
            Instruction::End,
        ]
    }

    /// Generate WASM for boolean conditional expression
    fn generate_boolean_conditional(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0), then_bool (1), else_bool (2)
            Instruction::LocalGet(0), // condition
            Instruction::If(BlockType::Result(ValType::I32)),
            // If condition is true, return then_bool
            Instruction::LocalGet(1),
            Instruction::Else,
            // If condition is false, return else_bool
            Instruction::LocalGet(2),
            Instruction::End,
        ]
    }

    /// Generate WASM for integer equality comparison
    fn generate_integer_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Eq,
        ]
    }

    /// Generate WASM for integer inequality comparison
    fn generate_integer_not_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Ne,
        ]
    }

    /// Generate WASM for integer less than comparison
    fn generate_integer_less_than(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32LtS,
        ]
    }

    /// Generate WASM for integer greater than comparison
    fn generate_integer_greater_than(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32GtS,
        ]
    }

    /// Generate WASM for integer less than or equal comparison
    fn generate_integer_less_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32LeS,
        ]
    }

    /// Generate WASM for integer greater than or equal comparison
    fn generate_integer_greater_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32GeS,
        ]
    }

    /// Generate WASM for number equality comparison
    fn generate_number_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Eq,
        ]
    }

    /// Generate WASM for number less than comparison
    fn generate_number_less_than(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Lt,
        ]
    }

    /// Generate WASM for number greater than comparison
    fn generate_number_greater_than(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Gt,
        ]
    }

    /// Generate WASM for number greater than or equal comparison
    fn generate_number_greater_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Ge,
        ]
    }

    /// Generate WASM for number less than or equal comparison
    fn generate_number_less_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Le,
        ]
    }

    /// Generate WASM for number not equal comparison
    fn generate_number_not_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1 (0), value2 (1)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Ne,
        ]
    }

    /// Generate WASM for logical AND operation
    fn generate_logical_and(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition1 (0), condition2 (1)
            // Returns: 1 if both conditions are true, 0 otherwise
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32And,
        ]
    }

    /// Generate WASM for logical OR operation
    fn generate_logical_or(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition1 (0), condition2 (1)
            // Returns: 1 if at least one condition is true, 0 otherwise
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Or,
        ]
    }

    /// Generate WASM for logical NOT operation
    fn generate_logical_not(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0)
            // Returns: 1 if condition is false, 0 if condition is true
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conditional_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let conditional_manager = ConditionalManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert_eq!(
            conditional_manager
                .memory_manager
                .borrow()
                .get_total_allocated(),
            0
        );
    }

    #[test]
    fn test_integer_conditional_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let conditional_manager = ConditionalManager::new(memory_manager);

        let instructions = conditional_manager.generate_integer_conditional();
        assert!(!instructions.is_empty());

        // Check that it contains the expected if-else structure
        assert!(matches!(instructions[1], Instruction::If(_)));
        assert!(matches!(instructions[3], Instruction::Else));
        assert!(matches!(instructions[5], Instruction::End));
    }

    #[test]
    fn test_comparison_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let conditional_manager = ConditionalManager::new(memory_manager);

        let eq_instructions = conditional_manager.generate_integer_equal();
        assert!(!eq_instructions.is_empty());
        assert!(matches!(eq_instructions[2], Instruction::I32Eq));

        let lt_instructions = conditional_manager.generate_integer_less_than();
        assert!(!lt_instructions.is_empty());
        assert!(matches!(lt_instructions[2], Instruction::I32LtS));
    }

    #[test]
    fn test_logical_operations() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let conditional_manager = ConditionalManager::new(memory_manager);

        let and_instructions = conditional_manager.generate_logical_and();
        assert!(!and_instructions.is_empty());
        assert!(matches!(and_instructions[2], Instruction::I32And));

        let not_instructions = conditional_manager.generate_logical_not();
        assert!(!not_instructions.is_empty());
        assert!(matches!(not_instructions[1], Instruction::I32Eqz));
    }
}
