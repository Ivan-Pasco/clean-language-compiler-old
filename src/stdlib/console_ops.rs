use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use wasm_encoder::Instruction;

/// Console input operations for Clean Language
/// Provides type-safe console input functionality
pub struct ConsoleOperations {
    #[allow(dead_code)]
    heap_start: usize,
}

impl ConsoleOperations {
    pub fn new(heap_start: usize) -> Self {
        Self { heap_start }
    }

    /// Register all console input functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_basic_input(codegen)?;
        self.register_typed_input(codegen)?;
        self.register_validation_input(codegen)?;
        Ok(())
    }

    fn register_basic_input(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // input(string prompt) -> string
        // Basic string input with prompt
        register_stdlib_function(
            codegen,
            "input",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::I32),             // returns string pointer
            self.generate_input_function(),
        )?;

        // input_integer(string prompt) -> integer
        register_stdlib_function(
            codegen,
            "input_integer",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::I32),             // returns integer
            self.generate_input_integer_function(),
        )?;

        // input_yesno(string prompt) -> boolean
        register_stdlib_function(
            codegen,
            "input_yesno",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::I32),             // returns boolean
            self.generate_input_yesno_function(),
        )?;

        Ok(())
    }

    fn register_typed_input(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Console.inputInteger(string prompt) -> integer
        register_stdlib_function(
            codegen,
            "Console.inputInteger",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::I32),             // returns integer
            self.generate_input_integer_function(),
        )?;

        // Console.inputNumber(string prompt) -> number
        register_stdlib_function(
            codegen,
            "Console.inputNumber",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::F64),             // returns number
            self.generate_input_number_function(),
        )?;

        // Console.inputBoolean(string prompt) -> boolean
        register_stdlib_function(
            codegen,
            "Console.inputBoolean",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::I32),             // returns boolean (as i32)
            self.generate_input_boolean_function(),
        )?;

        Ok(())
    }

    fn register_validation_input(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Console.inputYesNo(string prompt) -> boolean
        register_stdlib_function(
            codegen,
            "Console.inputYesNo",
            &[WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len
            Some(WasmType::I32),             // returns boolean
            self.generate_input_yesno_function(),
        )?;

        // Console.inputRange(string prompt, integer min, integer max) -> integer
        register_stdlib_function(
            codegen,
            "Console.inputRange",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // prompt_ptr, prompt_len, min, max
            Some(WasmType::I32),                                           // returns integer
            self.generate_input_range_function(),
        )?;

        Ok(())
    }

    /// Generate WebAssembly instructions for basic input function
    fn generate_input_function(&self) -> Vec<Instruction> {
        vec![
            // Get prompt pointer and length (args 0, 1)
            Instruction::LocalGet(0), // prompt_ptr
            Instruction::LocalGet(1), // prompt_len
            // Call host function input(prompt_ptr, prompt_len) -> string_ptr
            // This calls the runtime input function (import index 2)
            Instruction::Call(2), // input is at import index 2
        ]
    }

    /// Generate WebAssembly instructions for integer input function
    fn generate_input_integer_function(&self) -> Vec<Instruction> {
        vec![
            // Get prompt pointer and length (args 0, 1)
            Instruction::LocalGet(0), // prompt_ptr
            Instruction::LocalGet(1), // prompt_len
            // Call host function input_integer(prompt_ptr, prompt_len) -> integer
            // This calls the runtime input_integer function with validation (import index 3)
            Instruction::Call(3), // input_integer is at import index 3
        ]
    }

    /// Generate WebAssembly instructions for number input function
    fn generate_input_number_function(&self) -> Vec<Instruction> {
        vec![
            // Get prompt pointer and length (args 0, 1)
            Instruction::LocalGet(0), // prompt_ptr
            Instruction::LocalGet(1), // prompt_len
            // Call host function input_number(prompt_ptr, prompt_len) -> number
            // This calls the runtime input_number function with validation (import index 4)
            Instruction::Call(4), // input_number is at import index 4
        ]
    }

    /// Generate WebAssembly instructions for boolean input function
    fn generate_input_boolean_function(&self) -> Vec<Instruction> {
        vec![
            // Get prompt pointer and length (args 0, 1)
            Instruction::LocalGet(0), // prompt_ptr
            Instruction::LocalGet(1), // prompt_len
            // Call host function input_yesno(prompt_ptr, prompt_len) -> boolean
            // This calls the runtime input_yesno function with validation (import index 5)
            Instruction::Call(5), // input_yesno is at import index 5
        ]
    }

    /// Generate WebAssembly instructions for yes/no input function
    fn generate_input_yesno_function(&self) -> Vec<Instruction> {
        vec![
            // Get prompt pointer and length (args 0, 1)
            Instruction::LocalGet(0), // prompt_ptr
            Instruction::LocalGet(1), // prompt_len
            // Call host function input_yesno(prompt_ptr, prompt_len) -> boolean
            // This calls the runtime input_yesno function with y/n validation (import index 5)
            Instruction::Call(5), // input_yesno is at import index 5
        ]
    }

    /// Generate WebAssembly instructions for range input function
    fn generate_input_range_function(&self) -> Vec<Instruction> {
        vec![
            // Get all arguments (prompt_ptr, prompt_len, min, max)
            Instruction::LocalGet(0), // prompt_ptr
            Instruction::LocalGet(1), // prompt_len
            Instruction::LocalGet(2), // min
            Instruction::LocalGet(3), // max
            // Call host function input_range(prompt_ptr, prompt_len, min, max) -> integer
            // This calls a new runtime function for range validation (import index 6)
            Instruction::Call(6), // input_range is at import index 6
        ]
    }
}

/// Console class for static method calls
/// Provides namespace organization for console input functions
pub struct ConsoleClass;

impl ConsoleClass {
    pub fn new() -> Self {
        Self
    }

    /// Register Console class static methods
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        let console_ops = ConsoleOperations::new(0x10000); // Use default heap start
        console_ops.register_functions(codegen)?;
        Ok(())
    }
}
