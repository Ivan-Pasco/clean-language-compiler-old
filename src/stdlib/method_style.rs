use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use wasm_encoder::{Instruction, MemArg, BlockType, ValType};
use crate::stdlib::register_stdlib_function;
use std::rc::Rc;
use std::cell::RefCell;
use crate::stdlib::memory::MemoryManager;

/// Method-style syntax implementation for Clean Language
/// Enables value.method() calls instead of only function(value) calls
pub struct MethodStyleManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl MethodStyleManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all method-style functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_utility_methods(codegen)?;
        self.register_validation_methods(codegen)?;
        self.register_conversion_methods(codegen)?;
        self.register_boundary_methods(codegen)?;
        Ok(())
    }

    /// Register utility functions that work on any value
    fn register_utility_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register utility methods for all types
        let types = ["integer", "number", "string", "boolean", "value"];
        
        for type_name in &types {
            // Length methods for strings and lists
            let length_name = format!("{}.length", type_name);
            register_stdlib_function(
                codegen,
                &length_name,
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // length
                self.generate_value_length()
            )?;

            // Check if value is defined (not null/undefined)
            register_stdlib_function(
                codegen,
                &format!("{}.isDefined", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_defined()
            )?;

            register_stdlib_function(
                codegen,
                &format!("{}.isNotDefined", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_not_defined()
            )?;

            // Check if value is empty
            register_stdlib_function(
                codegen,
                &format!("{}.isEmpty", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_empty()
            )?;

            register_stdlib_function(
                codegen,
                &format!("{}.isNotEmpty", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_not_empty()
            )?;
        }

        Ok(())
    }

    /// Register validation methods for assertions
    fn register_validation_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register validation methods for all types
        let types = ["integer", "number", "string", "boolean", "value"];
        
        for type_name in &types {
            // Assert that condition is true
            register_stdlib_function(
                codegen,
                &format!("{}.mustBeTrue", type_name),
                &[WasmType::I32, WasmType::I32], // value pointer, condition
                None, // void
                self.generate_must_be_true()
            )?;

            // Assert that condition is false
            register_stdlib_function(
                codegen,
                &format!("{}.mustBeFalse", type_name),
                &[WasmType::I32, WasmType::I32], // value pointer, condition
                None, // void
                self.generate_must_be_false()
            )?;

            // Assert that two values are equal
            register_stdlib_function(
                codegen,
                &format!("{}.mustBeEqual", type_name),
                &[WasmType::I32, WasmType::I32], // value1 pointer, value2 pointer
                None, // void
                self.generate_must_be_equal()
            )?;

            // Assert that two values are not equal
            register_stdlib_function(
                codegen,
                &format!("{}.mustNotBeEqual", type_name),
                &[WasmType::I32, WasmType::I32], // value1 pointer, value2 pointer
                None, // void
                self.generate_must_not_be_equal()
            )?;
        }

        Ok(())
    }

    /// Register type conversion methods
    fn register_conversion_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register conversion methods for all types
        let types = ["integer", "number", "string", "boolean", "value"];
        
        for type_name in &types {
            // String conversion
            register_stdlib_function(
                codegen,
                &format!("{}.toString", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // string pointer
                self.generate_value_to_string()
            )?;

            // Integer conversion
            register_stdlib_function(
                codegen,
                &format!("{}.toInteger", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // integer value
                self.generate_value_to_integer()
            )?;

            // Number conversion - handle both I32 and F64 inputs
            register_stdlib_function(
                codegen,
                &format!("{}.toNumber", type_name),
                &[WasmType::I32], // value pointer (for most types)
                Some(WasmType::F64), // number value
                self.generate_value_to_number_from_ptr()
            )?;

            // Boolean conversion
            register_stdlib_function(
                codegen,
                &format!("{}.toBoolean", type_name),
                &[WasmType::I32], // value pointer
                Some(WasmType::I32), // boolean value
                self.generate_value_to_boolean()
            )?;
        }

        // Special case for number.toNumber that takes F64 directly
        register_stdlib_function(
            codegen,
            "number.toNumber",
            &[WasmType::F64], // value (f64)
            Some(WasmType::F64), // number value
            self.generate_value_to_number()
        )?;

        Ok(())
    }

    /// Register boundary/range methods
    fn register_boundary_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Keep integer value within bounds
        register_stdlib_function(
            codegen,
            "integer.keepBetween",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // value, min, max
            Some(WasmType::I32), // clamped value
            self.generate_integer_keep_between()
        )?;

        // Keep number value within bounds
        register_stdlib_function(
            codegen,
            "number.keepBetween",
            &[WasmType::F64, WasmType::F64, WasmType::F64], // value, min, max
            Some(WasmType::F64), // clamped value
            self.generate_number_keep_between()
        )?;

        Ok(())
    }

    /// Generate WASM for value.length() method
    fn generate_value_length(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: length of value
            
            // Load type ID from value header
            Instruction::LocalGet(0), // value_ptr
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }), // Type ID at offset 8
            Instruction::LocalSet(1), // type_id
            
            // Check if it's a string (type ID 3)
            Instruction::LocalGet(1),
            Instruction::I32Const(3), // String type ID
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // String length - load from string header
                Instruction::LocalGet(0),
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::Else,
                // Check if it's a list (type ID 4)
                Instruction::LocalGet(1),
                Instruction::I32Const(4), // List type ID
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // List length - load from list header
                    Instruction::LocalGet(0),
                    Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
                Instruction::Else,
                    // Default - return 1 for simple values
                    Instruction::I32Const(1),
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.isDefined() method
    fn generate_value_is_defined(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if defined (not null), 0 if null
            Instruction::LocalGet(0),
            Instruction::I32Const(0),
            Instruction::I32Ne,
        ]
    }

    /// Generate WASM for value.isNotDefined() method
    fn generate_value_is_not_defined(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if null, 0 if defined
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
        ]
    }

    /// Generate WASM for value.isEmpty() method
    fn generate_value_is_empty(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if empty, 0 if not empty
            
            // Check if pointer is null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::I32Const(1), // Null is empty
            Instruction::Else,
                // Call length and check if 0
                Instruction::LocalGet(0),
                Instruction::Call(self.get_value_length_function_index()),
                Instruction::I32Eqz,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.isNotEmpty() method
    fn generate_value_is_not_empty(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if not empty, 0 if empty
            
            // Check if pointer is null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::I32Const(0), // Null is empty, so not-empty is false
            Instruction::Else,
                // Call length and check if > 0
                Instruction::LocalGet(0),
                Instruction::Call(self.get_value_length_function_index()),
                Instruction::I32Const(0),
                Instruction::I32GtU,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.mustBeTrue() method
    fn generate_must_be_true(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0), condition (1)
            // Asserts that condition is true
            
            Instruction::LocalGet(1), // condition
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
                // Condition is false - trigger error
                // For now, just return (in production would throw exception)
                Instruction::Unreachable,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.mustBeFalse() method
    fn generate_must_be_false(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0), condition (1)
            // Asserts that condition is false
            
            Instruction::LocalGet(1), // condition
            Instruction::If(BlockType::Empty),
                // Condition is true - trigger error
                Instruction::Unreachable,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.mustBeEqual() method
    fn generate_must_be_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1_ptr (0), value2_ptr (1)
            // Asserts that values are equal
            
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Ne,
            Instruction::If(BlockType::Empty),
                // Values are not equal - trigger error
                Instruction::Unreachable,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.mustNotBeEqual() method
    fn generate_must_not_be_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value1_ptr (0), value2_ptr (1)
            // Asserts that values are not equal
            
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
                // Values are equal - trigger error
                Instruction::Unreachable,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.toString() method
    fn generate_value_to_string(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: string pointer
            
            // For now, return a simple string representation
            // In production, would check type and convert appropriately
            Instruction::LocalGet(0),
            Instruction::Call(self.get_to_string_function_index()),
        ]
    }

    /// Generate WASM for value.toInteger() method
    fn generate_value_to_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: integer value
            
            // Load value assuming it's stored as i32
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
        ]
    }

    /// Generate WASM for value.toNumber() method
    fn generate_value_to_number(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (f64)
            // Returns: number value (same as input for numbers)
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for value.toNumber() method from pointer
    fn generate_value_to_number_from_ptr(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: number value (f64)
            
            // Load value from pointer and convert to f64
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::F64ConvertI32S, // Convert i32 to f64
        ]
    }

    /// Generate WASM for value.toBoolean() method
    fn generate_value_to_boolean(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: boolean value (1 or 0)
            
            // Convert value to boolean
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::I32Const(0),
            Instruction::I32Ne, // 0 = false, non-zero = true
        ]
    }

    /// Generate WASM for integer.keepBetween() method
    fn generate_integer_keep_between(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), min (1), max (2)
            // Returns: clamped value
            
            // Clamp to minimum
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(1), // min
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(1), // min
            Instruction::I32LtS,     // value < min
            Instruction::Select,      // Choose min if value < min, else value
            
            // Clamp to maximum
            Instruction::LocalGet(2), // max
            Instruction::LocalTee(3), // Store in local 3, keep on stack
            Instruction::LocalGet(3), // max again
            Instruction::I32GtS,     // clamped_value > max
            Instruction::Select,      // Choose max if > max, else clamped_value
        ]
    }

    /// Generate WASM for number.keepBetween() method
    fn generate_number_keep_between(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), min (1), max (2)
            // Returns: clamped value
            
            // Clamp to minimum
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(1), // min
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(1), // min
            Instruction::F64Lt,       // value < min
            Instruction::Select,      // Choose min if value < min, else value
            
            // Clamp to maximum
            Instruction::LocalGet(2), // max
            Instruction::LocalTee(3), // Store in local 3, keep on stack
            Instruction::LocalGet(3), // max again
            Instruction::F64Gt,       // clamped_value > max
            Instruction::Select,      // Choose max if > max, else clamped_value
        ]
    }

    // Helper function indices (would be resolved from function table)
    fn get_value_length_function_index(&self) -> u32 { 400 }
    fn get_to_string_function_index(&self) -> u32 { 401 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_style_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let method_manager = MethodStyleManager::new(memory_manager.clone());
        
        // Test that manager is created successfully
        assert!(method_manager.memory_manager.borrow().data.len() > 0);
    }
}