use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// String interpolation implementation for Clean Language
/// Enables "Hello {name}!" syntax for embedded expressions
pub struct StringInterpolationManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl StringInterpolationManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all string interpolation functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_interpolation_functions(codegen)?;
        self.register_formatting_functions(codegen)?;
        self.register_builder_functions(codegen)?;
        Ok(())
    }

    /// Register core string interpolation functions
    fn register_interpolation_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Interpolate single value into template
        register_stdlib_function_with_locals(
            codegen,
            "string.interpolate",
            &[WasmType::I32, WasmType::I32], // template_ptr, value_ptr
            Some(WasmType::I32),             // result string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, builder_ptr, placeholder_pos
            self.generate_interpolate_single(),
        )?;

        // Interpolate multiple values into template
        register_stdlib_function_with_locals(
            codegen,
            "string.interpolateMultiple",
            &[WasmType::I32, WasmType::I32], // template_ptr, values_array_ptr
            Some(WasmType::I32),             // result string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, builder_ptr, value_count
            self.generate_interpolate_multiple(),
        )?;

        // Create interpolation template from string literal
        register_stdlib_function_with_locals(
            codegen,
            "string.createTemplate",
            &[WasmType::I32],                // string_literal_ptr
            Some(WasmType::I32),             // template_ptr
            &[WasmType::I32, WasmType::I32], // template_ptr, placeholder_count
            self.generate_create_template(),
        )?;

        // Parse interpolation expressions in braces
        register_stdlib_function_with_locals(
            codegen,
            "string.parseExpressions",
            &[WasmType::I32],    // string_with_expressions_ptr
            Some(WasmType::I32), // expression_list_ptr
            &[WasmType::I32, WasmType::I32, WasmType::I32], // expr_list_ptr, current_pos, brace_depth
            self.generate_parse_expressions(),
        )?;

        Ok(())
    }

    /// Register formatting functions for different types
    fn register_formatting_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Format integer with optional width and padding
        register_stdlib_function_with_locals(
            codegen,
            "string.formatInteger",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // value, width, pad_char
            Some(WasmType::I32),                            // formatted string pointer
            &[WasmType::I32, WasmType::I32],                // result_ptr, digit_count
            self.generate_format_integer(),
        )?;

        // Format number with precision control
        register_stdlib_function_with_locals(
            codegen,
            "string.formatNumber",
            &[WasmType::F64, WasmType::I32], // value, decimal_places
            Some(WasmType::I32),             // formatted string pointer
            &[WasmType::I32, WasmType::I32], // result_ptr, total_chars
            self.generate_format_number(),
        )?;

        // Format boolean with custom true/false strings
        register_stdlib_function_with_locals(
            codegen,
            "string.formatBoolean",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // value, true_str_ptr, false_str_ptr
            Some(WasmType::I32),                            // formatted string pointer
            &[WasmType::I32],                               // selected_str_ptr
            self.generate_format_boolean(),
        )?;

        // Format value with type detection
        register_stdlib_function_with_locals(
            codegen,
            "string.formatValue",
            &[WasmType::I32],                // value_ptr (with type info)
            Some(WasmType::I32),             // formatted string pointer
            &[WasmType::I32, WasmType::I32], // type_id, result_ptr
            self.generate_format_value(),
        )?;

        Ok(())
    }

    /// Register string builder functions for efficient concatenation
    fn register_builder_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Create new string builder
        register_stdlib_function_with_locals(
            codegen,
            "string.createBuilder",
            &[WasmType::I32],    // initial_capacity
            Some(WasmType::I32), // builder pointer
            &[WasmType::I32],    // builder_ptr
            self.generate_create_builder(),
        )?;

        // Append string to builder
        register_stdlib_function_with_locals(
            codegen,
            "string.builderAppend",
            &[WasmType::I32, WasmType::I32], // builder_ptr, string_ptr
            None,                            // void
            &[WasmType::I32, WasmType::I32, WasmType::I32], // current_len, new_len, capacity
            self.generate_builder_append(),
        )?;

        // Append formatted value to builder
        register_stdlib_function_with_locals(
            codegen,
            "string.builderAppendValue",
            &[WasmType::I32, WasmType::I32], // builder_ptr, value_ptr
            None,                            // void
            &[WasmType::I32],                // formatted_str_ptr
            self.generate_builder_append_value(),
        )?;

        // Finalize builder to string
        register_stdlib_function_with_locals(
            codegen,
            "string.builderFinalize",
            &[WasmType::I32],                // builder_ptr
            Some(WasmType::I32),             // final string pointer
            &[WasmType::I32, WasmType::I32], // result_ptr, final_len
            self.generate_builder_finalize(),
        )?;

        Ok(())
    }

    /// Generate WASM for single value interpolation
    fn generate_interpolate_single(&self) -> Vec<Instruction> {
        vec![
            // Parameters: template_ptr (0), value_ptr (1)
            // Locals: result_ptr (2), builder_ptr (3), placeholder_pos (4)

            // Create string builder with estimated capacity
            Instruction::I32Const(256), // Initial capacity
            Instruction::Call(self.get_create_builder_function_index()),
            Instruction::LocalSet(3), // builder_ptr
            // Find placeholder position in template
            Instruction::LocalGet(0),   // template_ptr
            Instruction::I32Const(123), // '{' character
            Instruction::Call(self.get_string_find_char_function_index()),
            Instruction::LocalSet(4), // placeholder_pos
            // Check if placeholder found
            Instruction::LocalGet(4),
            Instruction::I32Const(-1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // No placeholder - return original template
            Instruction::LocalGet(0), // template_ptr
            Instruction::Else,
            // Append text before placeholder
            Instruction::LocalGet(3), // builder_ptr
            Instruction::LocalGet(0), // template_ptr
            Instruction::I32Const(0), // start index
            Instruction::LocalGet(4), // end index (placeholder_pos)
            Instruction::Call(self.get_builder_append_substring_function_index()),
            // Format and append the value
            Instruction::LocalGet(3), // builder_ptr
            Instruction::LocalGet(1), // value_ptr
            Instruction::Call(self.get_builder_append_value_function_index()),
            // Find closing brace
            Instruction::LocalGet(0),   // template_ptr
            Instruction::I32Const(125), // '}' character
            Instruction::LocalGet(4),   // start search from placeholder_pos
            Instruction::Call(self.get_string_find_char_from_function_index()),
            Instruction::LocalTee(4), // Update placeholder_pos to closing brace
            // Append text after closing brace
            Instruction::LocalGet(3), // builder_ptr
            Instruction::LocalGet(0), // template_ptr
            Instruction::LocalGet(4), // start index (after closing brace)
            Instruction::I32Const(1),
            Instruction::I32Add,       // Skip the closing brace
            Instruction::I32Const(-1), // end index (to end of string)
            Instruction::Call(self.get_builder_append_substring_function_index()),
            // Finalize builder to string
            Instruction::LocalGet(3), // builder_ptr
            Instruction::Call(self.get_builder_finalize_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for multiple value interpolation
    fn generate_interpolate_multiple(&self) -> Vec<Instruction> {
        vec![
            // Parameters: template_ptr (0), values_array_ptr (1)
            // Locals: result_ptr (2), builder_ptr (3), value_count (4)

            // Get value count from array header
            Instruction::LocalGet(1), // values_array_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // Load array size
            Instruction::LocalSet(4), // value_count
            // Create string builder with larger capacity for multiple values
            Instruction::I32Const(512), // Initial capacity
            Instruction::Call(self.get_create_builder_function_index()),
            Instruction::LocalSet(3), // builder_ptr
            // Process template with multiple placeholders (simplified loop)
            Instruction::LocalGet(0), // template_ptr
            Instruction::LocalGet(1), // values_array_ptr
            Instruction::LocalGet(4), // value_count
            Instruction::LocalGet(3), // builder_ptr
            Instruction::Call(self.get_process_multiple_placeholders_function_index()),
            // Finalize builder to string
            Instruction::LocalGet(3), // builder_ptr
            Instruction::Call(self.get_builder_finalize_function_index()),
        ]
    }

    /// Generate WASM for creating interpolation template
    fn generate_create_template(&self) -> Vec<Instruction> {
        vec![
            // Parameters: string_literal_ptr (0)
            // Locals: template_ptr (1), placeholder_count (2)

            // Count placeholders in string literal
            Instruction::LocalGet(0), // string_literal_ptr
            Instruction::Call(self.get_count_placeholders_function_index()),
            Instruction::LocalSet(2), // placeholder_count
            // Allocate template structure
            Instruction::LocalGet(2),  // placeholder_count
            Instruction::I32Const(24), // Base template size
            Instruction::I32Mul,
            Instruction::I32Const(32), // Header size
            Instruction::I32Add,
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // template_ptr
            // Initialize template header
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // original string
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2), // placeholder_count
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Parse placeholder positions (would call parsing function)
            Instruction::LocalGet(1), // template_ptr
            Instruction::LocalGet(0), // string_literal_ptr
            Instruction::Call(self.get_parse_placeholder_positions_function_index()),
            // Return template
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for parsing expressions in braces
    fn generate_parse_expressions(&self) -> Vec<Instruction> {
        vec![
            // Parameters: string_with_expressions_ptr (0)
            // Locals: expr_list_ptr (1), current_pos (2), brace_depth (3)

            // Allocate expression list
            Instruction::I32Const(128), // Initial expression list size
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // expr_list_ptr
            // Initialize list header
            Instruction::LocalGet(1),
            Instruction::I32Const(0), // expression count
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Parse expressions (simplified - would implement full parser)
            Instruction::LocalGet(0), // string_with_expressions_ptr
            Instruction::LocalGet(1), // expr_list_ptr
            Instruction::Call(self.get_extract_brace_expressions_function_index()),
            // Return expression list
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for integer formatting
    fn generate_format_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), width (1), pad_char (2)
            // Locals: result_ptr (3), digit_count (4)

            // Convert integer to string
            Instruction::LocalGet(0), // value
            Instruction::Call(self.get_integer_to_string_function_index()),
            Instruction::LocalSet(3), // result_ptr
            // Check if padding needed
            Instruction::LocalGet(1), // width
            Instruction::I32Const(0),
            Instruction::I32GtS,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Apply padding
            Instruction::LocalGet(3), // string_ptr
            Instruction::LocalGet(1), // width
            Instruction::LocalGet(2), // pad_char
            Instruction::Call(self.get_string_pad_function_index()),
            Instruction::Else,
            // No padding needed
            Instruction::LocalGet(3),
            Instruction::End,
        ]
    }

    /// Generate WASM for number formatting
    fn generate_format_number(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), decimal_places (1)
            // Locals: result_ptr (2), total_chars (3)

            // Format number with specified decimal places
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(1), // decimal_places
            Instruction::Call(self.get_number_to_string_precision_function_index()),
        ]
    }

    /// Generate WASM for boolean formatting
    fn generate_format_boolean(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), true_str_ptr (1), false_str_ptr (2)
            // Local: selected_str_ptr (3)
            Instruction::LocalGet(0), // value
            Instruction::If(BlockType::Result(ValType::I32)),
            // Value is true
            Instruction::LocalGet(1), // true_str_ptr
            Instruction::Else,
            // Value is false
            Instruction::LocalGet(2), // false_str_ptr
            Instruction::End,
        ]
    }

    /// Generate WASM for value formatting with type detection
    fn generate_format_value(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Locals: type_id (1), result_ptr (2)

            // Load type ID from value header
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // Type ID at offset 8
            Instruction::LocalSet(1), // type_id
            // Switch on type ID
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Integer type
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Format as integer
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(0),  // No width
            Instruction::I32Const(32), // Space padding
            Instruction::Call(self.get_format_integer_function_index()),
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(2), // Number type
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Format as number
            Instruction::LocalGet(0),
            Instruction::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::I32Const(2), // 2 decimal places
            Instruction::Call(self.get_format_number_function_index()),
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(3), // String type
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Already a string - return as is
            Instruction::LocalGet(0),
            Instruction::Else,
            // Boolean or other type - convert to string
            Instruction::LocalGet(0),
            Instruction::Call(self.get_value_to_string_function_index()),
            Instruction::End,
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for creating string builder
    fn generate_create_builder(&self) -> Vec<Instruction> {
        vec![
            // Parameters: initial_capacity (0)
            // Local: builder_ptr (1)

            // Allocate builder structure (32 bytes header + buffer)
            Instruction::LocalGet(0),  // initial_capacity
            Instruction::I32Const(32), // Header size
            Instruction::I32Add,
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalTee(1), // builder_ptr
            // Initialize builder header
            Instruction::I32Const(0), // current length
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // capacity
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Return builder pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for appending to string builder
    fn generate_builder_append(&self) -> Vec<Instruction> {
        vec![
            // Parameters: builder_ptr (0), string_ptr (1)
            // Locals: current_len (2), new_len (3), capacity (4)

            // Load current length and capacity
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // current_len
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // capacity
            // Calculate new length
            Instruction::LocalGet(2), // current_len
            Instruction::LocalGet(1), // string_ptr
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::I32Add,
            Instruction::LocalSet(3), // new_len
            // Check if we need to grow buffer
            Instruction::LocalGet(3), // new_len
            Instruction::LocalGet(4), // capacity
            Instruction::I32GtU,
            Instruction::If(BlockType::Empty),
            // Grow buffer (simplified - would implement reallocation)
            Instruction::LocalGet(0), // builder_ptr
            Instruction::LocalGet(3), // new_len
            Instruction::I32Const(2),
            Instruction::I32Mul, // Double the needed size
            Instruction::Call(self.get_builder_grow_function_index()),
            Instruction::End,
            // Copy string data to buffer
            Instruction::LocalGet(0),  // builder_ptr
            Instruction::I32Const(32), // Buffer offset
            Instruction::I32Add,
            Instruction::LocalGet(2), // current_len (offset in buffer)
            Instruction::I32Add,
            Instruction::LocalGet(1), // source string
            Instruction::Call(self.get_string_data_ptr_function_index()),
            Instruction::LocalGet(1), // string length
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::Call(self.get_memory_copy_function_index()),
            // Update current length
            Instruction::LocalGet(0),
            Instruction::LocalGet(3), // new_len
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WASM for appending formatted value to builder
    fn generate_builder_append_value(&self) -> Vec<Instruction> {
        vec![
            // Parameters: builder_ptr (0), value_ptr (1)
            // Local: formatted_str_ptr (2)

            // Format the value to string
            Instruction::LocalGet(1), // value_ptr
            Instruction::Call(self.get_format_value_function_index()),
            Instruction::LocalSet(2), // formatted_str_ptr
            // Append formatted string to builder
            Instruction::LocalGet(0), // builder_ptr
            Instruction::LocalGet(2), // formatted_str_ptr
            Instruction::Call(self.get_builder_append_function_index()),
        ]
    }

    /// Generate WASM for finalizing string builder
    fn generate_builder_finalize(&self) -> Vec<Instruction> {
        vec![
            // Parameters: builder_ptr (0)
            // Locals: result_ptr (1), final_len (2)

            // Get final length
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // final_len
            // Allocate final string
            Instruction::LocalGet(2),  // length
            Instruction::I32Const(12), // String header size
            Instruction::I32Add,
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // result_ptr
            // Set string header
            Instruction::LocalGet(1),
            Instruction::LocalGet(2), // length
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Copy data from builder buffer to final string
            Instruction::LocalGet(1),  // result_ptr
            Instruction::I32Const(12), // String data offset
            Instruction::I32Add,
            Instruction::LocalGet(0),  // builder_ptr
            Instruction::I32Const(32), // Builder buffer offset
            Instruction::I32Add,
            Instruction::LocalGet(2), // final_len
            Instruction::Call(self.get_memory_copy_function_index()),
            // Return final string
            Instruction::LocalGet(1),
        ]
    }

    // Helper function indices
    fn get_create_builder_function_index(&self) -> u32 {
        900
    }
    fn get_string_find_char_function_index(&self) -> u32 {
        901
    }
    fn get_string_find_char_from_function_index(&self) -> u32 {
        902
    }
    fn get_builder_append_substring_function_index(&self) -> u32 {
        903
    }
    fn get_builder_append_value_function_index(&self) -> u32 {
        904
    }
    fn get_builder_finalize_function_index(&self) -> u32 {
        905
    }
    fn get_process_multiple_placeholders_function_index(&self) -> u32 {
        906
    }
    fn get_count_placeholders_function_index(&self) -> u32 {
        907
    }
    fn get_allocate_function_index(&self) -> u32 {
        908
    }
    fn get_parse_placeholder_positions_function_index(&self) -> u32 {
        909
    }
    fn get_extract_brace_expressions_function_index(&self) -> u32 {
        910
    }
    fn get_integer_to_string_function_index(&self) -> u32 {
        911
    }
    fn get_string_pad_function_index(&self) -> u32 {
        912
    }
    fn get_number_to_string_precision_function_index(&self) -> u32 {
        913
    }
    fn get_value_to_string_function_index(&self) -> u32 {
        914
    }
    fn get_string_length_function_index(&self) -> u32 {
        915
    }
    fn get_builder_grow_function_index(&self) -> u32 {
        916
    }
    fn get_string_data_ptr_function_index(&self) -> u32 {
        917
    }
    fn get_memory_copy_function_index(&self) -> u32 {
        918
    }
    fn get_builder_append_function_index(&self) -> u32 {
        919
    }
    fn get_format_value_function_index(&self) -> u32 {
        920
    }
    fn get_format_integer_function_index(&self) -> u32 {
        921
    }
    fn get_format_number_function_index(&self) -> u32 {
        922
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_interpolation_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let interpolation_manager = StringInterpolationManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert!(interpolation_manager.memory_manager.borrow().data.len() > 0);
    }

    #[test]
    fn test_interpolate_single_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let interpolation_manager = StringInterpolationManager::new(memory_manager);

        let instructions = interpolation_manager.generate_interpolate_single();
        assert!(!instructions.is_empty());

        // Should contain conditional logic for placeholder handling
        // After: I32Const(256), Call, LocalSet, LocalGet, I32Const(123), Call, LocalSet, LocalGet, I32Const(-1), I32Eq, If
        assert!(matches!(instructions[10], Instruction::If(_)));
    }

    #[test]
    fn test_format_functions_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let interpolation_manager = StringInterpolationManager::new(memory_manager);

        let format_int = interpolation_manager.generate_format_integer();
        assert!(!format_int.is_empty());

        let format_bool = interpolation_manager.generate_format_boolean();
        assert!(!format_bool.is_empty());
        assert!(matches!(format_bool[1], Instruction::If(_)));

        let format_value = interpolation_manager.generate_format_value();
        assert!(!format_value.is_empty());
        // Should contain type switching logic
        // After: LocalGet(0), I32Load, LocalSet(1), LocalGet(1), I32Const(1), I32Eq, If
        assert!(matches!(format_value[6], Instruction::If(_)));
    }

    #[test]
    fn test_string_builder_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let interpolation_manager = StringInterpolationManager::new(memory_manager);

        let create_builder = interpolation_manager.generate_create_builder();
        assert!(!create_builder.is_empty());

        let append_builder = interpolation_manager.generate_builder_append();
        assert!(!append_builder.is_empty());
        // Should contain growth check - looks for the If instruction that checks buffer growth
        let has_if = append_builder
            .iter()
            .any(|inst| matches!(inst, Instruction::If(_)));
        assert!(
            has_if,
            "Builder append should contain If instruction for growth check"
        );

        let finalize_builder = interpolation_manager.generate_builder_finalize();
        assert!(!finalize_builder.is_empty());
    }

    #[test]
    fn test_template_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let interpolation_manager = StringInterpolationManager::new(memory_manager);

        let create_template = interpolation_manager.generate_create_template();
        assert!(!create_template.is_empty());

        // Should allocate memory for template structure
        // After: LocalGet(0), Call, LocalSet(2), LocalGet(2), I32Const(24), I32Mul, I32Const(32), I32Add, Call
        assert!(matches!(create_template[8], Instruction::Call(_)));
    }

    #[test]
    fn test_multiple_interpolation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let interpolation_manager = StringInterpolationManager::new(memory_manager);

        let interpolate_multiple = interpolation_manager.generate_interpolate_multiple();
        assert!(!interpolate_multiple.is_empty());

        // Should load array size first
        assert!(matches!(interpolate_multiple[1], Instruction::I32Load(_)));
    }
}
