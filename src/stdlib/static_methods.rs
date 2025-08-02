use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use wasm_encoder::{Instruction, BlockType, ValType, MemArg};
use crate::stdlib::register_stdlib_function_with_locals;
use std::rc::Rc;
use std::cell::RefCell;
use crate::stdlib::memory::MemoryManager;

/// Static method calls implementation for Clean Language
/// Enables ClassName.method() calls without instantiation
pub struct StaticMethodManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl StaticMethodManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all static method functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_math_static_methods(codegen)?;
        self.register_string_static_methods(codegen)?;
        self.register_list_static_methods(codegen)?;
        self.register_file_static_methods(codegen)?;
        self.register_http_static_methods(codegen)?;
        self.register_console_static_methods(codegen)?;
        Ok(())
    }

    /// Register Math class static methods
    fn register_math_static_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Math.add(a, b) - Add two numbers
        register_stdlib_function_with_locals(
            codegen,
            "Math.add",
            &[WasmType::F64, WasmType::F64], // a, b
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_add()
        )?;

        // Math.subtract(a, b) - Subtract two numbers
        register_stdlib_function_with_locals(
            codegen,
            "Math.subtract",
            &[WasmType::F64, WasmType::F64], // a, b
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_subtract()
        )?;

        // Math.multiply(a, b) - Multiply two numbers
        register_stdlib_function_with_locals(
            codegen,
            "Math.multiply",
            &[WasmType::F64, WasmType::F64], // a, b
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_multiply()
        )?;

        // Math.divide(a, b) - Divide two numbers
        register_stdlib_function_with_locals(
            codegen,
            "Math.divide",
            &[WasmType::F64, WasmType::F64], // a, b
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_divide()
        )?;

        // Math.max(a, b) - Return maximum of two numbers
        register_stdlib_function_with_locals(
            codegen,
            "Math.max",
            &[WasmType::F64, WasmType::F64], // a, b
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_max()
        )?;

        // Math.min(a, b) - Return minimum of two numbers
        register_stdlib_function_with_locals(
            codegen,
            "Math.min",
            &[WasmType::F64, WasmType::F64], // a, b
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_min()
        )?;

        // Math.abs(a) - Return absolute value
        register_stdlib_function_with_locals(
            codegen,
            "Math.abs",
            &[WasmType::F64], // a
            Some(WasmType::F64), // result
            &[], // no locals needed
            self.generate_math_abs()
        )?;

        // Math.random() - Generate random number between 0 and 1
        register_stdlib_function_with_locals(
            codegen,
            "Math.random",
            &[], // no parameters
            Some(WasmType::F64), // random number
            &[WasmType::I32], // seed local
            self.generate_math_random()
        )?;

        // Math.randomInt(max) - Generate random integer from 0 to max-1
        register_stdlib_function_with_locals(
            codegen,
            "Math.randomInt",
            &[WasmType::I32], // max
            Some(WasmType::I32), // random integer
            &[WasmType::F64], // temp for calculation
            self.generate_math_random_int()
        )?;

        // Math.randomRange(min, max) - Generate random integer between min and max
        register_stdlib_function_with_locals(
            codegen,
            "Math.randomRange",
            &[WasmType::I32, WasmType::I32], // min, max
            Some(WasmType::I32), // random integer in range
            &[WasmType::I32, WasmType::F64], // range, temp
            self.generate_math_random_range()
        )?;

        // Math.parseInteger(string) - Parse string to integer
        register_stdlib_function_with_locals(
            codegen,
            "Math.parseInteger",
            &[WasmType::I32], // string_ptr
            Some(WasmType::I32), // parsed integer
            &[WasmType::I32, WasmType::I32], // result, error_flag
            self.generate_math_parse_integer()
        )?;

        // Math.parseNumber(string) - Parse string to number
        register_stdlib_function_with_locals(
            codegen,
            "Math.parseNumber",
            &[WasmType::I32], // string_ptr
            Some(WasmType::F64), // parsed number
            &[WasmType::F64, WasmType::I32], // result, error_flag
            self.generate_math_parse_number()
        )?;

        Ok(())
    }

    /// Register String class static methods
    fn register_string_static_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String.empty() - Create empty string
        register_stdlib_function_with_locals(
            codegen,
            "String.empty",
            &[], // no parameters
            Some(WasmType::I32), // empty string pointer
            &[WasmType::I32], // allocation result
            self.generate_string_empty()
        )?;

        // String.length(text) - Get string length
        register_stdlib_function_with_locals(
            codegen,
            "String.length",
            &[WasmType::I32], // string pointer
            Some(WasmType::I32), // length
            &[], // no locals needed
            self.generate_string_length()
        )?;

        // String.toUpperCase(text) - Convert to uppercase
        register_stdlib_function_with_locals(
            codegen,
            "String.toUpperCase",
            &[WasmType::I32], // string pointer
            Some(WasmType::I32), // uppercase string pointer
            &[WasmType::I32, WasmType::I32], // result_ptr, i
            self.generate_string_to_upper_case()
        )?;

        // String.toLowerCase(text) - Convert to lowercase
        register_stdlib_function_with_locals(
            codegen,
            "String.toLowerCase",
            &[WasmType::I32], // string pointer
            Some(WasmType::I32), // lowercase string pointer
            &[WasmType::I32, WasmType::I32], // result_ptr, i
            self.generate_string_to_lower_case()
        )?;

        // String.contains(text, substring) - Check if text contains substring
        register_stdlib_function_with_locals(
            codegen,
            "String.contains",
            &[WasmType::I32, WasmType::I32], // text_ptr, substring_ptr
            Some(WasmType::I32), // boolean result
            &[WasmType::I32], // found_index
            self.generate_string_contains()
        )?;

        // String.trim(text) - Remove leading and trailing whitespace
        register_stdlib_function_with_locals(
            codegen,
            "String.trim",
            &[WasmType::I32], // string pointer
            Some(WasmType::I32), // trimmed string pointer
            &[WasmType::I32, WasmType::I32], // start_pos, end_pos
            self.generate_string_trim()
        )?;

        // String.fromInteger(value) - Convert integer to string
        register_stdlib_function_with_locals(
            codegen,
            "String.fromInteger",
            &[WasmType::I32], // integer value
            Some(WasmType::I32), // string pointer
            &[WasmType::I32, WasmType::I32], // str_ptr, digit_count
            self.generate_string_from_integer()
        )?;

        // String.fromNumber(value) - Convert number to string
        register_stdlib_function_with_locals(
            codegen,
            "String.fromNumber",
            &[WasmType::F64], // number value
            Some(WasmType::I32), // string pointer
            &[WasmType::I32, WasmType::I32], // str_ptr, precision
            self.generate_string_from_number()
        )?;

        // String.fromBoolean(value) - Convert boolean to string
        register_stdlib_function_with_locals(
            codegen,
            "String.fromBoolean",
            &[WasmType::I32], // boolean value
            Some(WasmType::I32), // string pointer
            &[WasmType::I32], // str_ptr
            self.generate_string_from_boolean()
        )?;

        // String.repeat(text, count) - Repeat string n times
        register_stdlib_function_with_locals(
            codegen,
            "String.repeat",
            &[WasmType::I32, WasmType::I32], // text_ptr, count
            Some(WasmType::I32), // repeated string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, total_len, i
            self.generate_string_repeat()
        )?;

        Ok(())
    }

    /// Register List class static methods
    fn register_list_static_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // List.empty() - Create empty list
        register_stdlib_function_with_locals(
            codegen,
            "List.empty",
            &[], // no parameters
            Some(WasmType::I32), // empty list pointer
            &[WasmType::I32], // allocation result
            self.generate_list_empty()
        )?;

        // List.length(list) - Get list length
        register_stdlib_function_with_locals(
            codegen,
            "List.length",
            &[WasmType::I32], // list pointer
            Some(WasmType::I32), // length
            &[], // no locals needed
            self.generate_list_length()
        )?;

        // List.get(list, index) - Get element at index
        register_stdlib_function_with_locals(
            codegen,
            "List.get",
            &[WasmType::I32, WasmType::I32], // list_ptr, index
            Some(WasmType::I32), // element value
            &[WasmType::I32], // element_ptr
            self.generate_list_get()
        )?;

        // List.contains(list, value) - Check if list contains value
        register_stdlib_function_with_locals(
            codegen,
            "List.contains",
            &[WasmType::I32, WasmType::I32], // list_ptr, value
            Some(WasmType::I32), // boolean result
            &[WasmType::I32, WasmType::I32], // i, found
            self.generate_list_contains()
        )?;

        // List.range(start, end) - Create list with range of integers
        register_stdlib_function_with_locals(
            codegen,
            "List.range",
            &[WasmType::I32, WasmType::I32], // start, end
            Some(WasmType::I32), // list pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // list_ptr, size, i
            self.generate_list_range()
        )?;

        // List.repeat(value, count) - Create list with repeated value
        register_stdlib_function_with_locals(
            codegen,
            "List.repeat",
            &[WasmType::I32, WasmType::I32], // value, count
            Some(WasmType::I32), // list pointer
            &[WasmType::I32, WasmType::I32], // list_ptr, i
            self.generate_list_repeat()
        )?;

        Ok(())
    }

    /// Register File class static methods
    fn register_file_static_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // File.exists(path) - Check if file exists
        register_stdlib_function_with_locals(
            codegen,
            "File.exists",
            &[WasmType::I32], // path_ptr
            Some(WasmType::I32), // boolean result
            &[WasmType::I32], // file_handle
            self.generate_file_exists()
        )?;

        // File.readText(path) - Read entire file as text
        register_stdlib_function_with_locals(
            codegen,
            "File.readText",
            &[WasmType::I32], // path_ptr
            Some(WasmType::I32), // content string pointer
            &[WasmType::I32, WasmType::I32], // file_handle, content_ptr
            self.generate_file_read_text()
        )?;

        // File.writeText(path, content) - Write text to file
        register_stdlib_function_with_locals(
            codegen,
            "File.writeText",
            &[WasmType::I32, WasmType::I32], // path_ptr, content_ptr
            Some(WasmType::I32), // success boolean
            &[WasmType::I32], // file_handle
            self.generate_file_write_text()
        )?;

        Ok(())
    }

    /// Register Http class static methods
    fn register_http_static_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Http.get(url) - Simple GET request
        register_stdlib_function_with_locals(
            codegen,
            "Http.get",
            &[WasmType::I32], // url_ptr
            Some(WasmType::I32), // response string pointer
            &[WasmType::I32, WasmType::I32], // request_handle, response_ptr
            self.generate_http_get()
        )?;

        // Http.post(url, data) - Simple POST request
        register_stdlib_function_with_locals(
            codegen,
            "Http.post",
            &[WasmType::I32, WasmType::I32], // url_ptr, data_ptr
            Some(WasmType::I32), // response string pointer
            &[WasmType::I32, WasmType::I32], // request_handle, response_ptr
            self.generate_http_post()
        )?;

        Ok(())
    }

    /// Register Console class static methods
    fn register_console_static_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Console.clear() - Clear console screen
        register_stdlib_function_with_locals(
            codegen,
            "Console.clear",
            &[], // no parameters
            None, // void
            &[WasmType::I32], // operation
            self.generate_console_clear()
        )?;

        // Console.readLine() - Read line from input
        register_stdlib_function_with_locals(
            codegen,
            "Console.readLine",
            &[], // no parameters
            Some(WasmType::I32), // input string pointer
            &[WasmType::I32, WasmType::I32], // buffer_ptr, input_ptr
            self.generate_console_read_line()
        )?;

        Ok(())
    }

    /// Generate WASM for Math.add(a, b)
    fn generate_math_add(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0), b (1)
            Instruction::LocalGet(0), // a
            Instruction::LocalGet(1), // b
            Instruction::F64Add,
        ]
    }

    /// Generate WASM for Math.subtract(a, b)
    fn generate_math_subtract(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0), b (1)
            Instruction::LocalGet(0), // a
            Instruction::LocalGet(1), // b
            Instruction::F64Sub,
        ]
    }

    /// Generate WASM for Math.multiply(a, b)
    fn generate_math_multiply(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0), b (1)
            Instruction::LocalGet(0), // a
            Instruction::LocalGet(1), // b
            Instruction::F64Mul,
        ]
    }

    /// Generate WASM for Math.divide(a, b)
    fn generate_math_divide(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0), b (1)
            Instruction::LocalGet(0), // a
            Instruction::LocalGet(1), // b
            Instruction::F64Div,
        ]
    }

    /// Generate WASM for Math.max(a, b)
    fn generate_math_max(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0), b (1)
            Instruction::LocalGet(0), // a
            Instruction::LocalGet(1), // b
            Instruction::F64Max,
        ]
    }

    /// Generate WASM for Math.min(a, b)
    fn generate_math_min(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0), b (1)
            Instruction::LocalGet(0), // a
            Instruction::LocalGet(1), // b
            Instruction::F64Min,
        ]
    }

    /// Generate WASM for Math.abs(a)
    fn generate_math_abs(&self) -> Vec<Instruction> {
        vec![
            // Parameters: a (0)
            Instruction::LocalGet(0), // a
            Instruction::F64Abs,
        ]
    }

    /// Generate WASM for Math.random()
    fn generate_math_random(&self) -> Vec<Instruction> {
        vec![
            // Local: seed (0)
            
            // Simple linear congruential generator
            // seed = (seed * 1103515245 + 12345) & 0x7fffffff
            Instruction::I32Const(self.get_random_seed_address()),
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::I32Const(1103515245),
            Instruction::I32Mul,
            Instruction::I32Const(12345),
            Instruction::I32Add,
            Instruction::I32Const(0x7fffffff),
            Instruction::I32And,
            Instruction::LocalTee(0), // Store new seed
            
            // Store back to memory
            Instruction::I32Const(self.get_random_seed_address()),
            Instruction::LocalGet(0),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Convert to float 0.0 to 1.0
            Instruction::LocalGet(0),
            Instruction::F64ConvertI32U,
            Instruction::F64Const(2147483647.0), // 0x7fffffff as float
            Instruction::F64Div,
        ]
    }

    /// Generate WASM for Math.randomInt(max)
    fn generate_math_random_int(&self) -> Vec<Instruction> {
        vec![
            // Parameters: max (0)
            // Local: temp (1)
            
            // Get random float and multiply by max
            Instruction::Call(self.get_math_random_function_index()),
            Instruction::LocalGet(0), // max
            Instruction::F64ConvertI32S,
            Instruction::F64Mul,
            Instruction::I32TruncF64S, // Convert back to integer
        ]
    }

    /// Generate WASM for Math.randomRange(min, max)
    fn generate_math_random_range(&self) -> Vec<Instruction> {
        vec![
            // Parameters: min (0), max (1)
            // Locals: range (2), temp (3)
            
            // Calculate range = max - min
            Instruction::LocalGet(1), // max
            Instruction::LocalGet(0), // min
            Instruction::I32Sub,
            Instruction::LocalSet(2), // range
            
            // Get random int in range
            Instruction::LocalGet(2),
            Instruction::Call(self.get_math_random_int_function_index()),
            
            // Add min offset
            Instruction::LocalGet(0), // min
            Instruction::I32Add,
        ]
    }

    /// Generate WASM for Math.parseInteger(string)
    fn generate_math_parse_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: string_ptr (0)
            // Locals: result (1), error_flag (2)
            
            // Simplified integer parsing (would call actual parser)
            Instruction::LocalGet(0),
            Instruction::Call(self.get_parse_integer_function_index()),
            Instruction::LocalSet(1),
            
            // Check for parse error (simplified)
            Instruction::LocalGet(1),
            Instruction::I32Const(-1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Parse failed - throw error and return 0
                Instruction::I32Const(1001), // Parse error code
                Instruction::Call(self.get_set_error_function_index()),
                Instruction::I32Const(0),
            Instruction::Else,
                // Parse succeeded
                Instruction::LocalGet(1),
            Instruction::End,
        ]
    }

    /// Generate WASM for Math.parseNumber(string)
    fn generate_math_parse_number(&self) -> Vec<Instruction> {
        vec![
            // Parameters: string_ptr (0)
            // Locals: result (1), error_flag (2)
            
            // Simplified number parsing (would call actual parser)
            Instruction::LocalGet(0),
            Instruction::Call(self.get_parse_number_function_index()),
            Instruction::LocalTee(1),
            
            // Check if result is NaN (simplified error check)
            Instruction::LocalGet(1),
            Instruction::LocalGet(1),
            Instruction::F64Ne, // NaN != NaN
            Instruction::If(BlockType::Result(ValType::F64)),
                // Parse failed - throw error and return 0.0
                Instruction::I32Const(1002), // Parse error code
                Instruction::Call(self.get_set_error_function_index()),
                Instruction::F64Const(0.0),
            Instruction::Else,
                // Parse succeeded
                Instruction::LocalGet(1),
            Instruction::End,
        ]
    }

    /// Generate WASM for String.empty()
    fn generate_string_empty(&self) -> Vec<Instruction> {
        vec![
            // Local: allocation_result (0)
            
            // Allocate empty string (header only)
            Instruction::I32Const(12), // String header size
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalTee(0),
            
            // Initialize string header: length = 0
            Instruction::I32Const(0), // length
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Return string pointer
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for String.length(text)
    fn generate_string_length(&self) -> Vec<Instruction> {
        vec![
            // Parameters: text_ptr (0)
            // Get length from string header
            Instruction::LocalGet(0), // text_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load length
        ]
    }

    /// Generate WASM for String.toUpperCase(text)
    fn generate_string_to_upper_case(&self) -> Vec<Instruction> {
        vec![
            // Parameters: text_ptr (0)
            // Locals: result_ptr (1), i (2)
            
            // Call existing string_to_upper_case function
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_to_upper_function_index()),
        ]
    }

    /// Generate WASM for String.toLowerCase(text)
    fn generate_string_to_lower_case(&self) -> Vec<Instruction> {
        vec![
            // Parameters: text_ptr (0)
            // Locals: result_ptr (1), i (2)
            
            // Call existing string_to_lower_case function
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_to_lower_function_index()),
        ]
    }

    /// Generate WASM for String.contains(text, substring)
    fn generate_string_contains(&self) -> Vec<Instruction> {
        vec![
            // Parameters: text_ptr (0), substring_ptr (1)
            // Local: found_index (2)
            
            // Call existing string_contains function
            Instruction::LocalGet(0), // text
            Instruction::LocalGet(1), // substring
            Instruction::Call(self.get_string_contains_function_index()),
            
            // Check if found (index >= 0)
            Instruction::I32Const(-1),
            Instruction::I32Ne, // not equal to -1 means found
        ]
    }

    /// Generate WASM for String.trim(text)
    fn generate_string_trim(&self) -> Vec<Instruction> {
        vec![
            // Parameters: text_ptr (0)
            // Locals: start_pos (1), end_pos (2)
            
            // Call existing string_trim function
            Instruction::LocalGet(0),
            Instruction::Call(self.get_string_trim_function_index()),
        ]
    }

    /// Generate WASM for String.fromInteger(value)
    fn generate_string_from_integer(&self) -> Vec<Instruction> {
        vec![
            // Parameters: integer_value (0)
            // Locals: str_ptr (1), digit_count (2)
            
            // Call integer to string conversion function
            Instruction::LocalGet(0),
            Instruction::Call(self.get_integer_to_string_function_index()),
        ]
    }

    /// Generate WASM for String.fromNumber(value)
    fn generate_string_from_number(&self) -> Vec<Instruction> {
        vec![
            // Parameters: number_value (0)
            // Locals: str_ptr (1), precision (2)
            
            // Call number to string conversion function
            Instruction::LocalGet(0),
            Instruction::Call(self.get_number_to_string_function_index()),
        ]
    }

    /// Generate WASM for String.fromBoolean(value)
    fn generate_string_from_boolean(&self) -> Vec<Instruction> {
        vec![
            // Parameters: boolean_value (0)
            // Local: str_ptr (1)
            
            Instruction::LocalGet(0),
            Instruction::If(BlockType::Result(ValType::I32)),
                // True - return "true" string
                Instruction::Call(self.get_true_string_function_index()),
            Instruction::Else,
                // False - return "false" string
                Instruction::Call(self.get_false_string_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for String.repeat(text, count)
    fn generate_string_repeat(&self) -> Vec<Instruction> {
        vec![
            // Parameters: text_ptr (0), count (1)
            // Locals: result_ptr (2), total_len (3), i (4)
            
            // Calculate total length needed
            Instruction::LocalGet(0), // text_ptr
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::LocalGet(1), // count
            Instruction::I32Mul,
            Instruction::LocalSet(3), // total_len
            
            // Allocate result string
            Instruction::LocalGet(3),
            Instruction::I32Const(12), // header size
            Instruction::I32Add,
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(2), // result_ptr
            
            // Set result string length
            Instruction::LocalGet(2),
            Instruction::LocalGet(3),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Copy text count times (simplified loop)
            Instruction::LocalGet(0), // source
            Instruction::LocalGet(2), // dest
            Instruction::LocalGet(1), // count
            Instruction::Call(self.get_string_repeat_copy_function_index()),
            
            // Return result pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for List.empty()
    fn generate_list_empty(&self) -> Vec<Instruction> {
        vec![
            // Local: allocation_result (0)
            
            // Allocate empty list (header only)
            Instruction::I32Const(16), // List header size
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalTee(0),
            
            // Initialize list header: size = 0, capacity = 0
            Instruction::I32Const(0), // size
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            Instruction::LocalGet(0),
            Instruction::I32Const(0), // capacity
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Return list pointer
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for List.length(list)
    fn generate_list_length(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0)
            // Get size from list header
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load size
        ]
    }

    /// Generate WASM for List.get(list, index)
    fn generate_list_get(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), index (1)
            // Local: element_ptr (2)
            
            // Call existing list get function
            Instruction::LocalGet(0), // list
            Instruction::LocalGet(1), // index
            Instruction::Call(self.get_list_get_function_index()),
        ]
    }

    /// Generate WASM for List.contains(list, value)
    fn generate_list_contains(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), value (1)
            // Locals: i (2), found (3)
            
            // Call existing list contains function
            Instruction::LocalGet(0), // list
            Instruction::LocalGet(1), // value
            Instruction::Call(self.get_list_contains_function_index()),
        ]
    }

    /// Generate WASM for List.range(start, end)
    fn generate_list_range(&self) -> Vec<Instruction> {
        vec![
            // Parameters: start (0), end (1)
            // Locals: list_ptr (2), size (3), i (4)
            
            // Calculate size = end - start
            Instruction::LocalGet(1), // end
            Instruction::LocalGet(0), // start
            Instruction::I32Sub,
            Instruction::LocalSet(3), // size
            
            // Create list with calculated size
            Instruction::LocalGet(3),
            Instruction::Call(self.get_create_list_function_index()),
            Instruction::LocalSet(2), // list_ptr
            
            // Fill list with range values (simplified)
            Instruction::LocalGet(2), // list
            Instruction::LocalGet(0), // start
            Instruction::LocalGet(1), // end
            Instruction::Call(self.get_fill_range_function_index()),
            
            // Return list pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for List.repeat(value, count)
    fn generate_list_repeat(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), count (1)
            // Locals: list_ptr (2), i (3)
            
            // Create list with specified count
            Instruction::LocalGet(1), // count
            Instruction::Call(self.get_create_list_function_index()),
            Instruction::LocalSet(2), // list_ptr
            
            // Fill list with repeated value (simplified)
            Instruction::LocalGet(2), // list
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(1), // count
            Instruction::Call(self.get_fill_repeat_function_index()),
            
            // Return list pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for File.exists(path)
    fn generate_file_exists(&self) -> Vec<Instruction> {
        vec![
            // Parameters: path_ptr (0)
            // Local: file_handle (1)
            
            // Attempt to open file for reading
            Instruction::LocalGet(0), // path
            Instruction::I32Const(0), // read mode
            Instruction::Call(self.get_file_open_function_index()),
            Instruction::LocalTee(1), // file_handle
            
            // Check if file handle is valid
            Instruction::I32Const(-1),
            Instruction::I32Ne, // handle != -1 means file exists
            
            // Close file if it was opened
            Instruction::LocalGet(1),
            Instruction::I32Const(-1),
            Instruction::I32Ne,
            Instruction::If(BlockType::Empty),
                Instruction::LocalGet(1),
                Instruction::Call(self.get_file_close_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for File.readText(path)
    fn generate_file_read_text(&self) -> Vec<Instruction> {
        vec![
            // Parameters: path_ptr (0)
            // Locals: file_handle (1), content_ptr (2)
            
            // Open file for reading
            Instruction::LocalGet(0), // path
            Instruction::I32Const(0), // read mode
            Instruction::Call(self.get_file_open_function_index()),
            Instruction::LocalTee(1), // file_handle
            
            // Check if file opened successfully
            Instruction::I32Const(-1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // File open failed - return null
                Instruction::I32Const(0),
            Instruction::Else,
                // Read file content
                Instruction::LocalGet(1),
                Instruction::Call(self.get_file_read_all_function_index()),
                Instruction::LocalTee(2), // content_ptr
                
                // Close file
                Instruction::LocalGet(1),
                Instruction::Call(self.get_file_close_function_index()),
                
                // Return content
                Instruction::LocalGet(2),
            Instruction::End,
        ]
    }

    /// Generate WASM for File.writeText(path, content)
    fn generate_file_write_text(&self) -> Vec<Instruction> {
        vec![
            // Parameters: path_ptr (0), content_ptr (1)
            // Local: file_handle (2)
            
            // Open file for writing
            Instruction::LocalGet(0), // path
            Instruction::I32Const(1), // write mode
            Instruction::Call(self.get_file_open_function_index()),
            Instruction::LocalTee(2), // file_handle
            
            // Check if file opened successfully
            Instruction::I32Const(-1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // File open failed - return false
                Instruction::I32Const(0),
            Instruction::Else,
                // Write content to file
                Instruction::LocalGet(2), // file_handle
                Instruction::LocalGet(1), // content
                Instruction::Call(self.get_file_write_function_index()),
                
                // Close file
                Instruction::LocalGet(2),
                Instruction::Call(self.get_file_close_function_index()),
                
                // Return true for success
                Instruction::I32Const(1),
            Instruction::End,
        ]
    }

    /// Generate WASM for Http.get(url)
    fn generate_http_get(&self) -> Vec<Instruction> {
        vec![
            // Parameters: url_ptr (0)
            // Locals: request_handle (1), response_ptr (2)
            
            // Create HTTP GET request
            Instruction::LocalGet(0), // url
            Instruction::I32Const(0), // GET method
            Instruction::Call(self.get_http_request_function_index()),
            Instruction::LocalTee(1), // request_handle
            
            // Execute request and get response
            Instruction::Call(self.get_http_execute_function_index()),
            Instruction::LocalSet(2), // response_ptr
            
            // Clean up request handle (if needed)
            Instruction::LocalGet(1),
            Instruction::Call(self.get_http_cleanup_function_index()),
            
            // Return response
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for Http.post(url, data)
    fn generate_http_post(&self) -> Vec<Instruction> {
        vec![
            // Parameters: url_ptr (0), data_ptr (1)
            // Locals: request_handle (2), response_ptr (3)
            
            // Create HTTP POST request
            Instruction::LocalGet(0), // url
            Instruction::I32Const(1), // POST method
            Instruction::Call(self.get_http_request_function_index()),
            Instruction::LocalTee(2), // request_handle
            
            // Set POST data
            Instruction::LocalGet(1), // data
            Instruction::Call(self.get_http_set_data_function_index()),
            
            // Execute request and get response
            Instruction::LocalGet(2),
            Instruction::Call(self.get_http_execute_function_index()),
            Instruction::LocalSet(3), // response_ptr
            
            // Clean up request handle
            Instruction::LocalGet(2),
            Instruction::Call(self.get_http_cleanup_function_index()),
            
            // Return response
            Instruction::LocalGet(3),
        ]
    }

    /// Generate WASM for Console.clear()
    fn generate_console_clear(&self) -> Vec<Instruction> {
        vec![
            // Local: operation (0)
            
            // Call system console clear function
            Instruction::Call(self.get_console_clear_function_index()),
        ]
    }

    /// Generate WASM for Console.readLine()
    fn generate_console_read_line(&self) -> Vec<Instruction> {
        vec![
            // Locals: buffer_ptr (0), input_ptr (1)
            
            // Allocate buffer for input
            Instruction::I32Const(256), // Buffer size
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalTee(0), // buffer_ptr
            
            // Read line from console input
            Instruction::I32Const(256), // max length
            Instruction::Call(self.get_console_read_function_index()),
            Instruction::LocalSet(1), // input_ptr
            
            // Return input string
            Instruction::LocalGet(1),
        ]
    }

    // Helper function indices and memory addresses
    fn get_random_seed_address(&self) -> i32 { 0x2000 }
    fn get_math_random_function_index(&self) -> u32 { 700 }
    fn get_math_random_int_function_index(&self) -> u32 { 701 }
    fn get_parse_integer_function_index(&self) -> u32 { 702 }
    fn get_parse_number_function_index(&self) -> u32 { 703 }
    fn get_set_error_function_index(&self) -> u32 { 704 }
    fn get_allocate_function_index(&self) -> u32 { 705 }
    fn get_integer_to_string_function_index(&self) -> u32 { 706 }
    fn get_number_to_string_function_index(&self) -> u32 { 707 }
    fn get_true_string_function_index(&self) -> u32 { 708 }
    fn get_false_string_function_index(&self) -> u32 { 709 }
    fn get_string_length_function_index(&self) -> u32 { 710 }
    fn get_string_repeat_copy_function_index(&self) -> u32 { 711 }
    fn get_string_to_upper_function_index(&self) -> u32 { 712 }
    fn get_string_to_lower_function_index(&self) -> u32 { 713 }
    fn get_string_contains_function_index(&self) -> u32 { 714 }
    fn get_string_trim_function_index(&self) -> u32 { 715 }
    fn get_list_get_function_index(&self) -> u32 { 716 }
    fn get_list_contains_function_index(&self) -> u32 { 717 }
    fn get_create_list_function_index(&self) -> u32 { 718 }
    fn get_fill_range_function_index(&self) -> u32 { 719 }
    fn get_fill_repeat_function_index(&self) -> u32 { 720 }
    fn get_file_open_function_index(&self) -> u32 { 721 }
    fn get_file_close_function_index(&self) -> u32 { 722 }
    fn get_file_read_all_function_index(&self) -> u32 { 723 }
    fn get_file_write_function_index(&self) -> u32 { 724 }
    fn get_http_request_function_index(&self) -> u32 { 725 }
    fn get_http_execute_function_index(&self) -> u32 { 726 }
    fn get_http_set_data_function_index(&self) -> u32 { 727 }
    fn get_http_cleanup_function_index(&self) -> u32 { 728 }
    fn get_console_clear_function_index(&self) -> u32 { 729 }
    fn get_console_read_function_index(&self) -> u32 { 730 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_method_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let static_manager = StaticMethodManager::new(memory_manager.clone());
        
        // Test that manager is created successfully
        assert!(static_manager.memory_manager.borrow().data.len() > 0);
    }

    #[test]
    fn test_math_random_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let static_manager = StaticMethodManager::new(memory_manager);
        
        let instructions = static_manager.generate_math_random();
        assert!(!instructions.is_empty());
        
        // Should contain multiplication and division for random generation
        assert!(matches!(instructions[3], Instruction::I32Mul));
        // F64Div should be at the end of the instruction sequence
        assert!(matches!(instructions[instructions.len() - 1], Instruction::F64Div));
    }

    #[test]
    fn test_string_static_methods() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let static_manager = StaticMethodManager::new(memory_manager);
        
        let empty_instructions = static_manager.generate_string_empty();
        assert!(!empty_instructions.is_empty());
        assert!(matches!(empty_instructions[1], Instruction::Call(_)));
        
        let from_bool_instructions = static_manager.generate_string_from_boolean();
        assert!(!from_bool_instructions.is_empty());
        assert!(matches!(from_bool_instructions[1], Instruction::If(_)));
    }

    #[test]
    fn test_list_static_methods() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let static_manager = StaticMethodManager::new(memory_manager);
        
        let empty_instructions = static_manager.generate_list_empty();
        assert!(!empty_instructions.is_empty());
        
        let range_instructions = static_manager.generate_list_range();
        assert!(!range_instructions.is_empty());
        // Size calculation: LocalGet(1), LocalGet(0), I32Sub should be at indices 0, 1, 2
        assert!(matches!(range_instructions[2], Instruction::I32Sub)); // size calculation
    }

    #[test]
    fn test_file_static_methods() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let static_manager = StaticMethodManager::new(memory_manager);
        
        let exists_instructions = static_manager.generate_file_exists();
        assert!(!exists_instructions.is_empty());
        
        let read_instructions = static_manager.generate_file_read_text();
        assert!(!read_instructions.is_empty());
        // After: LocalGet(0), I32Const(0), Call, LocalTee(1), I32Const(-1), I32Eq, If should be at index 6
        assert!(matches!(read_instructions[6], Instruction::If(_))); // Error handling
    }

    #[test]
    fn test_http_static_methods() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let static_manager = StaticMethodManager::new(memory_manager);
        
        let get_instructions = static_manager.generate_http_get();
        assert!(!get_instructions.is_empty());
        
        let post_instructions = static_manager.generate_http_post();
        assert!(!post_instructions.is_empty());
        // POST should have additional data setting step
        assert!(post_instructions.len() > get_instructions.len());
    }
}