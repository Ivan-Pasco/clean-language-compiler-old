use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use wasm_encoder::{Instruction, BlockType, ValType, MemArg};
use crate::stdlib::register_stdlib_function_with_locals;
use std::rc::Rc;
use std::cell::RefCell;
use crate::stdlib::memory::MemoryManager;

/// Extended numeric literals implementation for Clean Language
/// Enables 0xff (hex), 0b1010 (binary), 0o777 (octal) literals
pub struct NumericLiteralsManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl NumericLiteralsManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all numeric literal functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_hexadecimal_functions(codegen)?;
        self.register_binary_functions(codegen)?;
        self.register_octal_functions(codegen)?;
        self.register_literal_validation_functions(codegen)?;
        Ok(())
    }

    /// Register hexadecimal literal functions
    fn register_hexadecimal_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Parse hexadecimal string literal to integer
        register_stdlib_function_with_locals(
            codegen,
            "hex.parseInteger",
            &[WasmType::I32], // hex_string_ptr
            Some(WasmType::I32), // parsed integer value
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result, digit_value, char_value
            self.generate_parse_hex_integer()
        )?;

        // Parse hexadecimal string literal to 64-bit integer
        register_stdlib_function_with_locals(
            codegen,
            "hex.parseInteger64",
            &[WasmType::I32], // hex_string_ptr
            Some(WasmType::I64), // parsed 64-bit integer value
            &[WasmType::I64, WasmType::I32, WasmType::I32], // result, digit_value, char_value
            self.generate_parse_hex_integer_64()
        )?;

        // Validate hexadecimal literal format
        register_stdlib_function_with_locals(
            codegen,
            "hex.validate",
            &[WasmType::I32], // hex_string_ptr
            Some(WasmType::I32), // 1 if valid, 0 if invalid
            &[WasmType::I32, WasmType::I32, WasmType::I32], // length, index, char_value
            self.generate_validate_hex()
        )?;

        // Convert integer to hexadecimal string
        register_stdlib_function_with_locals(
            codegen,
            "hex.toString",
            &[WasmType::I32, WasmType::I32], // value, uppercase (1 = uppercase, 0 = lowercase)
            Some(WasmType::I32), // hex string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, digit, remainder
            self.generate_integer_to_hex()
        )?;

        Ok(())
    }

    /// Register binary literal functions
    fn register_binary_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Parse binary string literal to integer
        register_stdlib_function_with_locals(
            codegen,
            "binary.parseInteger",
            &[WasmType::I32], // binary_string_ptr
            Some(WasmType::I32), // parsed integer value
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result, bit_value, char_value
            self.generate_parse_binary_integer()
        )?;

        // Parse binary string literal to 64-bit integer
        register_stdlib_function_with_locals(
            codegen,
            "binary.parseInteger64",
            &[WasmType::I32], // binary_string_ptr
            Some(WasmType::I64), // parsed 64-bit integer value
            &[WasmType::I64, WasmType::I32, WasmType::I32], // result, bit_value, char_value
            self.generate_parse_binary_integer_64()
        )?;

        // Validate binary literal format
        register_stdlib_function_with_locals(
            codegen,
            "binary.validate",
            &[WasmType::I32], // binary_string_ptr
            Some(WasmType::I32), // 1 if valid, 0 if invalid
            &[WasmType::I32, WasmType::I32, WasmType::I32], // length, index, char_value
            self.generate_validate_binary()
        )?;

        // Convert integer to binary string
        register_stdlib_function_with_locals(
            codegen,
            "binary.toString",
            &[WasmType::I32], // value
            Some(WasmType::I32), // binary string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, bit, remainder
            self.generate_integer_to_binary()
        )?;

        Ok(())
    }

    /// Register octal literal functions
    fn register_octal_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Parse octal string literal to integer
        register_stdlib_function_with_locals(
            codegen,
            "octal.parseInteger",
            &[WasmType::I32], // octal_string_ptr
            Some(WasmType::I32), // parsed integer value
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result, digit_value, char_value
            self.generate_parse_octal_integer()
        )?;

        // Parse octal string literal to 64-bit integer
        register_stdlib_function_with_locals(
            codegen,
            "octal.parseInteger64",
            &[WasmType::I32], // octal_string_ptr
            Some(WasmType::I64), // parsed 64-bit integer value
            &[WasmType::I64, WasmType::I32, WasmType::I32], // result, digit_value, char_value
            self.generate_parse_octal_integer_64()
        )?;

        // Validate octal literal format
        register_stdlib_function_with_locals(
            codegen,
            "octal.validate",
            &[WasmType::I32], // octal_string_ptr
            Some(WasmType::I32), // 1 if valid, 0 if invalid
            &[WasmType::I32, WasmType::I32, WasmType::I32], // length, index, char_value
            self.generate_validate_octal()
        )?;

        // Convert integer to octal string
        register_stdlib_function_with_locals(
            codegen,
            "octal.toString",
            &[WasmType::I32], // value
            Some(WasmType::I32), // octal string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, digit, remainder
            self.generate_integer_to_octal()
        )?;

        Ok(())
    }

    /// Register literal validation and utility functions
    fn register_literal_validation_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Detect numeric literal type from string
        register_stdlib_function_with_locals(
            codegen,
            "literal.detectType",
            &[WasmType::I32], // literal_string_ptr
            Some(WasmType::I32), // type (1=hex, 2=binary, 3=octal, 0=decimal)
            &[WasmType::I32, WasmType::I32], // prefix_char1, prefix_char2
            self.generate_detect_literal_type()
        )?;

        // Parse any extended literal to integer
        register_stdlib_function_with_locals(
            codegen,
            "literal.parseInteger",
            &[WasmType::I32], // literal_string_ptr
            Some(WasmType::I32), // parsed integer value
            &[WasmType::I32, WasmType::I32], // literal_type, result
            self.generate_parse_any_literal()
        )?;

        // Parse any extended literal to 64-bit integer
        register_stdlib_function_with_locals(
            codegen,
            "literal.parseInteger64",
            &[WasmType::I32], // literal_string_ptr
            Some(WasmType::I64), // parsed 64-bit integer value
            &[WasmType::I32, WasmType::I64], // literal_type, result
            self.generate_parse_any_literal_64()
        )?;

        // Validate any extended literal format
        register_stdlib_function_with_locals(
            codegen,
            "literal.validate",
            &[WasmType::I32], // literal_string_ptr
            Some(WasmType::I32), // 1 if valid, 0 if invalid
            &[WasmType::I32, WasmType::I32], // literal_type, is_valid
            self.generate_validate_any_literal()
        )?;

        // Get numeric base for literal type
        register_stdlib_function_with_locals(
            codegen,
            "literal.getBase",
            &[WasmType::I32], // literal_type
            Some(WasmType::I32), // numeric base (2, 8, 10, 16)
            &[WasmType::I32], // base
            self.generate_get_literal_base()
        )?;

        Ok(())
    }

    /// Generate WASM for parsing hexadecimal integer
    fn generate_parse_hex_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: hex_string_ptr (0)
            // Locals: result (1), digit_value (2), char_value (3)
            
            // Initialize result
            Instruction::I32Const(0),
            Instruction::LocalSet(1), // result = 0
            
            // Skip "0x" prefix (call helper to parse hex digits)
            Instruction::LocalGet(0), // hex_string_ptr
            Instruction::I32Const(2), // Skip first 2 characters ("0x")
            Instruction::I32Add,
            Instruction::Call(self.get_parse_hex_digits_function_index()),
        ]
    }

    /// Generate WASM for parsing hexadecimal 64-bit integer
    fn generate_parse_hex_integer_64(&self) -> Vec<Instruction> {
        vec![
            // Parameters: hex_string_ptr (0)
            // Locals: result (1), digit_value (2), char_value (3)
            
            // Initialize result
            Instruction::I64Const(0),
            Instruction::LocalSet(1), // result = 0
            
            // Skip "0x" prefix (call helper to parse hex digits)
            Instruction::LocalGet(0), // hex_string_ptr
            Instruction::I32Const(2), // Skip first 2 characters ("0x")
            Instruction::I32Add,
            Instruction::Call(self.get_parse_hex_digits_64_function_index()),
        ]
    }

    /// Generate WASM for validating hexadecimal format
    fn generate_validate_hex(&self) -> Vec<Instruction> {
        vec![
            // Parameters: hex_string_ptr (0)
            // Locals: length (1), index (2), char_value (3)
            
            // Get string length
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::LocalSet(1), // length
            
            // Check minimum length (at least "0x1")
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32LtU, // length < 3
            Instruction::If(BlockType::Result(ValType::I32)),
                // Too short
                Instruction::I32Const(0), // Invalid
            Instruction::Else,
                // Check "0x" prefix
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }),
                Instruction::I32Const(48), // '0'
                Instruction::I32Eq,
                
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 1, align: 0, memory_index: 0 }),
                Instruction::I32Const(120), // 'x' (or 88 for 'X')
                Instruction::I32Or,
                Instruction::I32Const(88), // 'X'
                Instruction::I32Eq,
                Instruction::I32Or,
                
                Instruction::I32And, // Both conditions must be true
                
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Valid prefix - check hex digits
                    Instruction::LocalGet(0),
                    Instruction::I32Const(2), // Start after "0x"
                    Instruction::Call(self.get_validate_hex_digits_function_index()),
                Instruction::Else,
                    // Invalid prefix
                    Instruction::I32Const(0),
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for converting integer to hexadecimal string
    fn generate_integer_to_hex(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), uppercase (1)
            // Locals: result_ptr (2), digit (3), remainder (4)
            
            // Handle zero case
            Instruction::LocalGet(0), // value
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Return "0x0"
                Instruction::Call(self.get_hex_zero_string_function_index()),
            Instruction::Else,
                // Convert non-zero value
                Instruction::LocalGet(0), // value
                Instruction::LocalGet(1), // uppercase
                Instruction::Call(self.get_convert_to_hex_string_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for parsing binary integer
    fn generate_parse_binary_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: binary_string_ptr (0)
            // Locals: result (1), bit_value (2), char_value (3)
            
            // Initialize result
            Instruction::I32Const(0),
            Instruction::LocalSet(1), // result = 0
            
            // Skip "0b" prefix (call helper to parse binary digits)
            Instruction::LocalGet(0), // binary_string_ptr
            Instruction::I32Const(2), // Skip first 2 characters ("0b")
            Instruction::I32Add,
            Instruction::Call(self.get_parse_binary_digits_function_index()),
        ]
    }

    /// Generate WASM for parsing binary 64-bit integer
    fn generate_parse_binary_integer_64(&self) -> Vec<Instruction> {
        vec![
            // Parameters: binary_string_ptr (0)
            // Locals: result (1), bit_value (2), char_value (3)
            
            // Initialize result
            Instruction::I64Const(0),
            Instruction::LocalSet(1), // result = 0
            
            // Skip "0b" prefix (call helper to parse binary digits)
            Instruction::LocalGet(0), // binary_string_ptr
            Instruction::I32Const(2), // Skip first 2 characters ("0b")
            Instruction::I32Add,
            Instruction::Call(self.get_parse_binary_digits_64_function_index()),
        ]
    }

    /// Generate WASM for validating binary format
    fn generate_validate_binary(&self) -> Vec<Instruction> {
        vec![
            // Parameters: binary_string_ptr (0)
            // Locals: length (1), index (2), char_value (3)
            
            // Get string length
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::LocalSet(1), // length
            
            // Check minimum length (at least "0b1")
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32LtU, // length < 3
            Instruction::If(BlockType::Result(ValType::I32)),
                // Too short
                Instruction::I32Const(0), // Invalid
            Instruction::Else,
                // Check "0b" prefix
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }),
                Instruction::I32Const(48), // '0'
                Instruction::I32Eq,
                
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 1, align: 0, memory_index: 0 }),
                Instruction::I32Const(98), // 'b' (or 66 for 'B')
                Instruction::I32Or,
                Instruction::I32Const(66), // 'B'
                Instruction::I32Eq,
                Instruction::I32Or,
                
                Instruction::I32And, // Both conditions must be true
                
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Valid prefix - check binary digits
                    Instruction::LocalGet(0),
                    Instruction::I32Const(2), // Start after "0b"
                    Instruction::Call(self.get_validate_binary_digits_function_index()),
                Instruction::Else,
                    // Invalid prefix
                    Instruction::I32Const(0),
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for converting integer to binary string
    fn generate_integer_to_binary(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0)
            // Locals: result_ptr (1), bit (2), remainder (3)
            
            // Handle zero case
            Instruction::LocalGet(0), // value
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Return "0b0"
                Instruction::Call(self.get_binary_zero_string_function_index()),
            Instruction::Else,
                // Convert non-zero value
                Instruction::LocalGet(0), // value
                Instruction::Call(self.get_convert_to_binary_string_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for parsing octal integer
    fn generate_parse_octal_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: octal_string_ptr (0)
            // Locals: result (1), digit_value (2), char_value (3)
            
            // Initialize result
            Instruction::I32Const(0),
            Instruction::LocalSet(1), // result = 0
            
            // Skip "0o" prefix (call helper to parse octal digits)
            Instruction::LocalGet(0), // octal_string_ptr
            Instruction::I32Const(2), // Skip first 2 characters ("0o")
            Instruction::I32Add,
            Instruction::Call(self.get_parse_octal_digits_function_index()),
        ]
    }

    /// Generate WASM for parsing octal 64-bit integer
    fn generate_parse_octal_integer_64(&self) -> Vec<Instruction> {
        vec![
            // Parameters: octal_string_ptr (0)
            // Locals: result (1), digit_value (2), char_value (3)
            
            // Initialize result
            Instruction::I64Const(0),
            Instruction::LocalSet(1), // result = 0
            
            // Skip "0o" prefix (call helper to parse octal digits)
            Instruction::LocalGet(0), // octal_string_ptr
            Instruction::I32Const(2), // Skip first 2 characters ("0o")
            Instruction::I32Add,
            Instruction::Call(self.get_parse_octal_digits_64_function_index()),
        ]
    }

    /// Generate WASM for validating octal format
    fn generate_validate_octal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: octal_string_ptr (0)
            // Locals: length (1), index (2), char_value (3)
            
            // Get string length
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::LocalSet(1), // length
            
            // Check minimum length (at least "0o1")
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32LtU, // length < 3
            Instruction::If(BlockType::Result(ValType::I32)),
                // Too short
                Instruction::I32Const(0), // Invalid
            Instruction::Else,
                // Check "0o" prefix
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }),
                Instruction::I32Const(48), // '0'
                Instruction::I32Eq,
                
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 1, align: 0, memory_index: 0 }),
                Instruction::I32Const(111), // 'o' (or 79 for 'O')
                Instruction::I32Or,
                Instruction::I32Const(79), // 'O'
                Instruction::I32Eq,
                Instruction::I32Or,
                
                Instruction::I32And, // Both conditions must be true
                
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Valid prefix - check octal digits
                    Instruction::LocalGet(0),
                    Instruction::I32Const(2), // Start after "0o"
                    Instruction::Call(self.get_validate_octal_digits_function_index()),
                Instruction::Else,
                    // Invalid prefix
                    Instruction::I32Const(0),
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for converting integer to octal string
    fn generate_integer_to_octal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0)
            // Locals: result_ptr (1), digit (2), remainder (3)
            
            // Handle zero case
            Instruction::LocalGet(0), // value
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Return "0o0"
                Instruction::Call(self.get_octal_zero_string_function_index()),
            Instruction::Else,
                // Convert non-zero value
                Instruction::LocalGet(0), // value
                Instruction::Call(self.get_convert_to_octal_string_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for detecting literal type
    fn generate_detect_literal_type(&self) -> Vec<Instruction> {
        vec![
            // Parameters: literal_string_ptr (0)
            // Locals: prefix_char1 (1), prefix_char2 (2)
            
            // Get string length first
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::I32Const(2),
            Instruction::I32LtU, // length < 2
            Instruction::If(BlockType::Result(ValType::I32)),
                // Too short for extended literal
                Instruction::I32Const(0), // Decimal
            Instruction::Else,
                // Check first character
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }),
                Instruction::LocalSet(1), // prefix_char1
                
                // Check second character
                Instruction::LocalGet(0),
                Instruction::Call(self.get_string_data_ptr_function_index()),
                Instruction::I32Load8U(MemArg { offset: 1, align: 0, memory_index: 0 }),
                Instruction::LocalSet(2), // prefix_char2
                
                // Check if starts with '0'
                Instruction::LocalGet(1),
                Instruction::I32Const(48), // '0'
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Starts with '0' - check second character
                    Instruction::LocalGet(2),
                    Instruction::I32Const(120), // 'x'
                    Instruction::I32Eq,
                    Instruction::LocalGet(2),
                    Instruction::I32Const(88), // 'X'
                    Instruction::I32Eq,
                    Instruction::I32Or,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        // Hexadecimal
                        Instruction::I32Const(1),
                    Instruction::Else,
                        Instruction::LocalGet(2),
                        Instruction::I32Const(98), // 'b'
                        Instruction::I32Eq,
                        Instruction::LocalGet(2),
                        Instruction::I32Const(66), // 'B'
                        Instruction::I32Eq,
                        Instruction::I32Or,
                        Instruction::If(BlockType::Result(ValType::I32)),
                            // Binary
                            Instruction::I32Const(2),
                        Instruction::Else,
                            Instruction::LocalGet(2),
                            Instruction::I32Const(111), // 'o'
                            Instruction::I32Eq,
                            Instruction::LocalGet(2),
                            Instruction::I32Const(79), // 'O'
                            Instruction::I32Eq,
                            Instruction::I32Or,
                            Instruction::If(BlockType::Result(ValType::I32)),
                                // Octal
                                Instruction::I32Const(3),
                            Instruction::Else,
                                // Decimal (starts with 0 but no special prefix)
                                Instruction::I32Const(0),
                            Instruction::End,
                        Instruction::End,
                    Instruction::End,
                Instruction::Else,
                    // Doesn't start with '0' - decimal
                    Instruction::I32Const(0),
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for parsing any literal type
    fn generate_parse_any_literal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: literal_string_ptr (0)
            // Locals: literal_type (1), result (2)
            
            // Detect literal type
            Instruction::LocalGet(0),
            Instruction::Call(self.get_detect_literal_type_function_index()),
            Instruction::LocalSet(1), // literal_type
            
            // Switch on literal type
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Hexadecimal
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::LocalGet(0),
                Instruction::Call(self.get_parse_hex_integer_function_index()),
            Instruction::Else,
                Instruction::LocalGet(1),
                Instruction::I32Const(2), // Binary
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    Instruction::LocalGet(0),
                    Instruction::Call(self.get_parse_binary_integer_function_index()),
                Instruction::Else,
                    Instruction::LocalGet(1),
                    Instruction::I32Const(3), // Octal
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        Instruction::LocalGet(0),
                        Instruction::Call(self.get_parse_octal_integer_function_index()),
                    Instruction::Else,
                        // Decimal - parse normally
                        Instruction::LocalGet(0),
                        Instruction::Call(self.get_parse_decimal_integer_function_index()),
                    Instruction::End,
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for parsing any literal type to 64-bit
    fn generate_parse_any_literal_64(&self) -> Vec<Instruction> {
        vec![
            // Parameters: literal_string_ptr (0)
            // Locals: literal_type (1), result (2)
            
            // Detect literal type
            Instruction::LocalGet(0),
            Instruction::Call(self.get_detect_literal_type_function_index()),
            Instruction::LocalSet(1), // literal_type
            
            // Switch on literal type
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Hexadecimal
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I64)),
                Instruction::LocalGet(0),
                Instruction::Call(self.get_parse_hex_integer_64_function_index()),
            Instruction::Else,
                Instruction::LocalGet(1),
                Instruction::I32Const(2), // Binary
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I64)),
                    Instruction::LocalGet(0),
                    Instruction::Call(self.get_parse_binary_integer_64_function_index()),
                Instruction::Else,
                    Instruction::LocalGet(1),
                    Instruction::I32Const(3), // Octal
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I64)),
                        Instruction::LocalGet(0),
                        Instruction::Call(self.get_parse_octal_integer_64_function_index()),
                    Instruction::Else,
                        // Decimal - parse normally
                        Instruction::LocalGet(0),
                        Instruction::Call(self.get_parse_decimal_integer_64_function_index()),
                    Instruction::End,
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for validating any literal format
    fn generate_validate_any_literal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: literal_string_ptr (0)
            // Locals: literal_type (1), is_valid (2)
            
            // Detect literal type
            Instruction::LocalGet(0),
            Instruction::Call(self.get_detect_literal_type_function_index()),
            Instruction::LocalSet(1), // literal_type
            
            // Switch on literal type for validation
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Hexadecimal
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::LocalGet(0),
                Instruction::Call(self.get_validate_hex_function_index()),
            Instruction::Else,
                Instruction::LocalGet(1),
                Instruction::I32Const(2), // Binary
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    Instruction::LocalGet(0),
                    Instruction::Call(self.get_validate_binary_function_index()),
                Instruction::Else,
                    Instruction::LocalGet(1),
                    Instruction::I32Const(3), // Octal
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        Instruction::LocalGet(0),
                        Instruction::Call(self.get_validate_octal_function_index()),
                    Instruction::Else,
                        // Decimal - always valid if it's numeric
                        Instruction::LocalGet(0),
                        Instruction::Call(self.get_validate_decimal_function_index()),
                    Instruction::End,
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for getting numeric base
    fn generate_get_literal_base(&self) -> Vec<Instruction> {
        vec![
            // Parameters: literal_type (0)
            // Local: base (1)
            
            // Switch on literal type to return base
            Instruction::LocalGet(0),
            Instruction::I32Const(1), // Hexadecimal
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::I32Const(16), // Base 16
            Instruction::Else,
                Instruction::LocalGet(0),
                Instruction::I32Const(2), // Binary
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    Instruction::I32Const(2), // Base 2
                Instruction::Else,
                    Instruction::LocalGet(0),
                    Instruction::I32Const(3), // Octal
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        Instruction::I32Const(8), // Base 8
                    Instruction::Else,
                        Instruction::I32Const(10), // Base 10 (decimal)
                    Instruction::End,
                Instruction::End,
            Instruction::End,
        ]
    }

    // Helper function indices - Numeric Literals uses 1200-1250 range
    fn get_string_length_function_index(&self) -> u32 { 1200 }
    fn get_string_data_ptr_function_index(&self) -> u32 { 1201 }
    fn get_parse_hex_digits_function_index(&self) -> u32 { 1202 }
    fn get_parse_hex_digits_64_function_index(&self) -> u32 { 1203 }
    fn get_validate_hex_digits_function_index(&self) -> u32 { 1204 }
    fn get_hex_zero_string_function_index(&self) -> u32 { 1205 }
    fn get_convert_to_hex_string_function_index(&self) -> u32 { 1206 }
    fn get_parse_binary_digits_function_index(&self) -> u32 { 1207 }
    fn get_parse_binary_digits_64_function_index(&self) -> u32 { 1208 }
    fn get_validate_binary_digits_function_index(&self) -> u32 { 1209 }
    fn get_binary_zero_string_function_index(&self) -> u32 { 1210 }
    fn get_convert_to_binary_string_function_index(&self) -> u32 { 1211 }
    fn get_parse_octal_digits_function_index(&self) -> u32 { 1212 }
    fn get_parse_octal_digits_64_function_index(&self) -> u32 { 1213 }
    fn get_validate_octal_digits_function_index(&self) -> u32 { 1214 }
    fn get_octal_zero_string_function_index(&self) -> u32 { 1215 }
    fn get_convert_to_octal_string_function_index(&self) -> u32 { 1216 }
    fn get_detect_literal_type_function_index(&self) -> u32 { 1217 }
    fn get_parse_hex_integer_function_index(&self) -> u32 { 1218 }
    fn get_parse_binary_integer_function_index(&self) -> u32 { 1219 }
    fn get_parse_octal_integer_function_index(&self) -> u32 { 1220 }
    fn get_parse_decimal_integer_function_index(&self) -> u32 { 1221 }
    fn get_parse_hex_integer_64_function_index(&self) -> u32 { 1222 }
    fn get_parse_binary_integer_64_function_index(&self) -> u32 { 1223 }
    fn get_parse_octal_integer_64_function_index(&self) -> u32 { 1224 }
    fn get_parse_decimal_integer_64_function_index(&self) -> u32 { 1225 }
    fn get_validate_hex_function_index(&self) -> u32 { 1226 }
    fn get_validate_binary_function_index(&self) -> u32 { 1227 }
    fn get_validate_octal_function_index(&self) -> u32 { 1228 }
    fn get_validate_decimal_function_index(&self) -> u32 { 1229 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_literals_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager.clone());
        
        // Test that manager is created successfully
        assert!(literals_manager.memory_manager.borrow().data.len() > 0);
    }

    #[test]
    fn test_hexadecimal_functions_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager);
        
        let parse_hex = literals_manager.generate_parse_hex_integer();
        assert!(!parse_hex.is_empty());
        // Should call helper function to parse hex digits
        assert!(parse_hex.iter().any(|inst| matches!(inst, Instruction::Call(_))));
        
        let parse_hex_64 = literals_manager.generate_parse_hex_integer_64();
        assert!(!parse_hex_64.is_empty());
        // Should initialize with I64Const(0)
        assert!(matches!(parse_hex_64[0], Instruction::I64Const(0)));
        
        let validate_hex = literals_manager.generate_validate_hex();
        assert!(!validate_hex.is_empty());
        // Should contain nested validation logic
        let if_count = validate_hex.iter().filter(|inst| matches!(inst, Instruction::If(_))).count();
        assert!(if_count >= 2);
        
        let to_hex = literals_manager.generate_integer_to_hex();
        assert!(!to_hex.is_empty());
        // Should handle zero case first
        assert!(matches!(to_hex[1], Instruction::I32Eqz));
    }

    #[test]
    fn test_binary_functions_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager);
        
        let parse_binary = literals_manager.generate_parse_binary_integer();
        assert!(!parse_binary.is_empty());
        // Should initialize result to 0
        assert!(matches!(parse_binary[0], Instruction::I32Const(0)));
        
        let validate_binary = literals_manager.generate_validate_binary();
        assert!(!validate_binary.is_empty());
        // Should check for '0b' prefix
        assert!(validate_binary.iter().any(|inst| matches!(inst, Instruction::I32Const(48)))); // '0'
        assert!(validate_binary.iter().any(|inst| matches!(inst, Instruction::I32Const(98)))); // 'b'
        
        let to_binary = literals_manager.generate_integer_to_binary();
        assert!(!to_binary.is_empty());
        // Should handle zero case
        assert!(to_binary.iter().any(|inst| matches!(inst, Instruction::I32Eqz)));
    }

    #[test]
    fn test_octal_functions_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager);
        
        let parse_octal = literals_manager.generate_parse_octal_integer();
        assert!(!parse_octal.is_empty());
        // Should skip "0o" prefix
        assert!(parse_octal.iter().any(|inst| matches!(inst, Instruction::I32Const(2))));
        
        let validate_octal = literals_manager.generate_validate_octal();
        assert!(!validate_octal.is_empty());
        // Should check for '0o' prefix
        assert!(validate_octal.iter().any(|inst| matches!(inst, Instruction::I32Const(111)))); // 'o'
        
        let to_octal = literals_manager.generate_integer_to_octal();
        assert!(!to_octal.is_empty());
        // Should handle zero case
        assert!(to_octal.iter().any(|inst| matches!(inst, Instruction::I32Eqz)));
    }

    #[test]
    fn test_literal_detection_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager);
        
        let detect_type = literals_manager.generate_detect_literal_type();
        assert!(!detect_type.is_empty());
        // Should contain complex branching logic for type detection
        let if_count = detect_type.iter().filter(|inst| matches!(inst, Instruction::If(_))).count();
        assert!(if_count >= 4); // Multiple levels of checks
        
        // Should check for all prefix characters
        assert!(detect_type.iter().any(|inst| matches!(inst, Instruction::I32Const(48)))); // '0'
        assert!(detect_type.iter().any(|inst| matches!(inst, Instruction::I32Const(120)))); // 'x'
        assert!(detect_type.iter().any(|inst| matches!(inst, Instruction::I32Const(98)))); // 'b'
        assert!(detect_type.iter().any(|inst| matches!(inst, Instruction::I32Const(111)))); // 'o'
        
        let parse_any = literals_manager.generate_parse_any_literal();
        assert!(!parse_any.is_empty());
        // Should call detect type function first
        assert!(matches!(parse_any[1], Instruction::Call(_)));
        
        let get_base = literals_manager.generate_get_literal_base();
        assert!(!get_base.is_empty());
        // Should return different bases (2, 8, 10, 16)
        assert!(get_base.iter().any(|inst| matches!(inst, Instruction::I32Const(2))));
        assert!(get_base.iter().any(|inst| matches!(inst, Instruction::I32Const(8))));
        assert!(get_base.iter().any(|inst| matches!(inst, Instruction::I32Const(10))));
        assert!(get_base.iter().any(|inst| matches!(inst, Instruction::I32Const(16))));
    }

    #[test]
    fn test_validation_functions_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager);
        
        let validate_any = literals_manager.generate_validate_any_literal();
        assert!(!validate_any.is_empty());
        // Should detect type first then validate
        assert!(matches!(validate_any[1], Instruction::Call(_))); // detect type call
        
        // Should contain switching logic for different validations
        let if_count = validate_any.iter().filter(|inst| matches!(inst, Instruction::If(_))).count();
        assert!(if_count >= 3); // One for each literal type
    }

    #[test]
    fn test_64_bit_parsing_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let literals_manager = NumericLiteralsManager::new(memory_manager);
        
        let parse_any_64 = literals_manager.generate_parse_any_literal_64();
        assert!(!parse_any_64.is_empty());
        // Should return I64 values - check for any If block with I64 result
        let has_i64_result = parse_any_64.iter().any(|inst| matches!(inst, Instruction::If(BlockType::Result(ValType::I64))));
        assert!(has_i64_result, "Should contain If blocks that return I64 values");
        
        // Should contain calls to 64-bit parsing functions
        let call_count = parse_any_64.iter().filter(|inst| matches!(inst, Instruction::Call(_))).count();
        assert!(call_count >= 4); // detect + 3 parsers minimum
    }
}