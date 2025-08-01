use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use wasm_encoder::{Instruction, BlockType, ValType, MemArg};
use crate::stdlib::register_stdlib_function_with_locals;
use std::rc::Rc;
use std::cell::RefCell;
use crate::stdlib::memory::MemoryManager;

/// Type precision implementation for Clean Language
/// Enables integer:8, integer:32, number:64 etc. precision control
pub struct TypePrecisionManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl TypePrecisionManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }    
    }

    /// Register all type precision functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_integer_precision_functions(codegen)?;
        self.register_number_precision_functions(codegen)?;
        self.register_precision_conversion_functions(codegen)?;
        self.register_precision_validation_functions(codegen)?;
        Ok(())
    }

    /// Register integer precision functions for different bit sizes
    fn register_integer_precision_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Create 8-bit integer with range validation
        register_stdlib_function_with_locals(
            codegen,
            "integer.create8",
            &[WasmType::I32], // input value
            Some(WasmType::I32), // 8-bit integer pointer
            &[WasmType::I32, WasmType::I32], // validated_value, int_ptr
            self.generate_create_integer_8()
        )?;

        // Create 16-bit integer with range validation
        register_stdlib_function_with_locals(
            codegen,
            "integer.create16",
            &[WasmType::I32], // input value
            Some(WasmType::I32), // 16-bit integer pointer
            &[WasmType::I32, WasmType::I32], // validated_value, int_ptr
            self.generate_create_integer_16()
        )?;

        // Create 32-bit integer (default)
        register_stdlib_function_with_locals(
            codegen,
            "integer.create32",
            &[WasmType::I32], // input value
            Some(WasmType::I32), // 32-bit integer pointer
            &[WasmType::I32], // int_ptr
            self.generate_create_integer_32()
        )?;

        // Create 64-bit integer with extended range
        register_stdlib_function_with_locals(
            codegen,
            "integer.create64",
            &[WasmType::I64], // input value (64-bit)
            Some(WasmType::I32), // 64-bit integer pointer
            &[WasmType::I32], // int_ptr
            self.generate_create_integer_64()
        )?;

        // Get value from precision integer
        register_stdlib_function_with_locals(
            codegen,
            "integer.getValue",
            &[WasmType::I32], // precision integer pointer
            Some(WasmType::I32), // extracted value
            &[WasmType::I32, WasmType::I32], // precision_type, value
            self.generate_get_integer_value()
        )?;

        // Get precision bits from integer
        register_stdlib_function_with_locals(
            codegen,
            "integer.getPrecision",
            &[WasmType::I32], // precision integer pointer
            Some(WasmType::I32), // precision bits (8, 16, 32, 64)
            &[WasmType::I32], // precision_type
            self.generate_get_integer_precision()
        )?;

        Ok(())
    }

    /// Register number precision functions for different precisions
    fn register_number_precision_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Create 32-bit number (single precision float)
        register_stdlib_function_with_locals(
            codegen,
            "number.create32",
            &[WasmType::F64], // input value
            Some(WasmType::I32), // 32-bit number pointer
            &[WasmType::F32, WasmType::I32], // converted_value, num_ptr
            self.generate_create_number_32()
        )?;

        // Create 64-bit number (double precision float)
        register_stdlib_function_with_locals(
            codegen,
            "number.create64",
            &[WasmType::F64], // input value
            Some(WasmType::I32), // 64-bit number pointer
            &[WasmType::I32], // num_ptr
            self.generate_create_number_64()
        )?;

        // Get value from precision number
        register_stdlib_function_with_locals(
            codegen,
            "number.getValue",
            &[WasmType::I32], // precision number pointer
            Some(WasmType::F64), // extracted value
            &[WasmType::I32, WasmType::F64], // precision_type, value
            self.generate_get_number_value()
        )?;

        // Get precision bits from number
        register_stdlib_function_with_locals(
            codegen,
            "number.getPrecision",
            &[WasmType::I32], // precision number pointer
            Some(WasmType::I32), // precision bits (32, 64)
            &[WasmType::I32], // precision_type
            self.generate_get_number_precision()
        )?;

        // Convert between number precisions
        register_stdlib_function_with_locals(
            codegen,
            "number.convertPrecision",
            &[WasmType::I32, WasmType::I32], // source_ptr, target_precision
            Some(WasmType::I32), // converted number pointer
            &[WasmType::I32, WasmType::F64, WasmType::I32], // source_precision, value, result_ptr
            self.generate_convert_number_precision()
        )?;

        Ok(())
    }

    /// Register precision conversion functions between different types
    fn register_precision_conversion_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Convert precision integer to precision number
        register_stdlib_function_with_locals(
            codegen,
            "precision.integerToNumber",
            &[WasmType::I32, WasmType::I32], // int_ptr, target_number_precision
            Some(WasmType::I32), // precision number pointer
            &[WasmType::I32, WasmType::F64, WasmType::I32], // int_value, converted_value, result_ptr
            self.generate_integer_to_number_precision()
        )?;

        // Convert precision number to precision integer
        register_stdlib_function_with_locals(
            codegen,
            "precision.numberToInteger",
            &[WasmType::I32, WasmType::I32], // num_ptr, target_integer_precision
            Some(WasmType::I32), // precision integer pointer
            &[WasmType::F64, WasmType::I32, WasmType::I32], // num_value, converted_value, result_ptr
            self.generate_number_to_integer_precision()
        )?;

        // Cast between different integer precisions
        register_stdlib_function_with_locals(
            codegen,
            "precision.castInteger",
            &[WasmType::I32, WasmType::I32], // source_int_ptr, target_precision
            Some(WasmType::I32), // casted integer pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // source_value, casted_value, result_ptr
            self.generate_cast_integer_precision()
        )?;

        Ok(())
    }

    /// Register precision validation and utility functions
    fn register_precision_validation_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Validate integer fits in precision range
        register_stdlib_function_with_locals(
            codegen,
            "precision.validateInteger",
            &[WasmType::I32, WasmType::I32], // value, precision_bits
            Some(WasmType::I32), // 1 if valid, 0 if out of range
            &[WasmType::I32, WasmType::I32, WasmType::I32], // min_value, max_value, is_valid
            self.generate_validate_integer_precision()
        )?;

        // Get precision info (min/max values for integer precisions)
        register_stdlib_function_with_locals(
            codegen,
            "precision.getIntegerRange",
            &[WasmType::I32], // precision_bits
            Some(WasmType::I32), // range info pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // range_ptr, min_value, max_value
            self.generate_get_integer_range()
        )?;

        // Check if precision is supported
        register_stdlib_function_with_locals(
            codegen,
            "precision.isSupported",
            &[WasmType::I32, WasmType::I32], // type_id (1=int, 2=num), precision_bits
            Some(WasmType::I32), // 1 if supported, 0 if not
            &[WasmType::I32], // is_supported
            self.generate_is_precision_supported()
        )?;

        // Get memory size for precision type
        register_stdlib_function_with_locals(
            codegen,
            "precision.getMemorySize",
            &[WasmType::I32, WasmType::I32], // type_id, precision_bits
            Some(WasmType::I32), // memory size in bytes
            &[WasmType::I32], // memory_size
            self.generate_get_precision_memory_size()
        )?;

        Ok(())
    }

    /// Generate WASM for creating 8-bit integer
    fn generate_create_integer_8(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_value (0)
            // Locals: validated_value (1), int_ptr (2)
            
            // Clamp value to 8-bit signed range (-128 to 127) 
            Instruction::LocalGet(0), // input_value
            Instruction::I32Const(-128), // min value
            Instruction::I32Const(127), // max value
            Instruction::Call(self.get_clamp_integer_function_index()),
            Instruction::LocalSet(1), // validated_value
            
            // Allocate precision integer structure (16 bytes: value, precision, type_id, reserved)
            Instruction::I32Const(16),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(2), // int_ptr
            
            // Store validated value
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // validated_value
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Store precision (8 bits)
            Instruction::LocalGet(2),
            Instruction::I32Const(8),
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Store type ID (1 = integer)
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Return pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for creating 16-bit integer
    fn generate_create_integer_16(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_value (0)
            // Locals: validated_value (1), int_ptr (2)
            
            // Clamp value to 16-bit signed range (-32768 to 32767)
            Instruction::LocalGet(0), // input_value
            Instruction::I32Const(-32768), // min value
            Instruction::I32Const(32767), // max value
            Instruction::Call(self.get_clamp_integer_function_index()),
            Instruction::LocalSet(1), // validated_value
            
            // Allocate precision integer structure
            Instruction::I32Const(16),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(2), // int_ptr
            
            // Store validated value
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // validated_value
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Store precision (16 bits)
            Instruction::LocalGet(2),
            Instruction::I32Const(16),
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Store type ID (1 = integer)
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Return pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for creating 32-bit integer (standard)
    fn generate_create_integer_32(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_value (0)
            // Local: int_ptr (1)
            
            // Allocate precision integer structure
            Instruction::I32Const(16),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // int_ptr
            
            // Store value (no clamping needed for 32-bit)
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // input_value
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Store precision (32 bits)
            Instruction::LocalGet(1),
            Instruction::I32Const(32),
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Store type ID (1 = integer)
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Return pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for creating 64-bit integer
    fn generate_create_integer_64(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_value (0) - 64-bit
            // Local: int_ptr (1)
            
            // Allocate precision integer structure (24 bytes for 64-bit value)
            Instruction::I32Const(24),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // int_ptr
            
            // Store 64-bit value
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // input_value (64-bit)
            Instruction::I64Store(MemArg { offset: 0, align: 3, memory_index: 0 }),
            
            // Store precision (64 bits)
            Instruction::LocalGet(1),
            Instruction::I32Const(64),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Store type ID (1 = integer)
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            // Return pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for extracting value from precision integer
    fn generate_get_integer_value(&self) -> Vec<Instruction> {
        vec![
            // Parameters: precision_int_ptr (0)
            // Locals: precision_type (1), value (2)
            
            // Load precision type
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 4, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // precision_type
            
            // Switch on precision type to load appropriate value
            Instruction::LocalGet(1),
            Instruction::I32Const(64),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // 64-bit integer - load as I64 then convert to I32
                Instruction::LocalGet(0),
                Instruction::I64Load(MemArg { offset: 0, align: 3, memory_index: 0 }),
                Instruction::I32WrapI64, // Convert to 32-bit for return
            Instruction::Else,
                // 8, 16, 32-bit integers - load as I32
                Instruction::LocalGet(0),
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::End,
        ]
    }

    /// Generate WASM for getting precision bits from integer
    fn generate_get_integer_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: precision_int_ptr (0)
            // Local: precision_type (1)
            
            // Load and return precision bits
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 4, align: 2, memory_index: 0 }),
        ]
    }

    /// Generate WASM for creating 32-bit number (single precision)
    fn generate_create_number_32(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_value (0) - F64
            // Locals: converted_value (1) - F32, num_ptr (2)
            
            // Convert F64 to F32
            Instruction::LocalGet(0),
            Instruction::F32DemoteF64,
            Instruction::LocalSet(1), // converted_value
            
            // Allocate precision number structure
            Instruction::I32Const(16),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(2), // num_ptr
            
            // Store F32 value
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // converted_value
            Instruction::F32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Store precision (32 bits)
            Instruction::LocalGet(2),
            Instruction::I32Const(32),
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Store type ID (2 = number)
            Instruction::LocalGet(2),
            Instruction::I32Const(2),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Return pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for creating 64-bit number (double precision)
    fn generate_create_number_64(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_value (0) - F64
            // Local: num_ptr (1)
            
            // Allocate precision number structure (24 bytes for F64)
            Instruction::I32Const(24),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // num_ptr
            
            // Store F64 value
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // input_value
            Instruction::F64Store(MemArg { offset: 0, align: 3, memory_index: 0 }),
            
            // Store precision (64 bits)
            Instruction::LocalGet(1),
            Instruction::I32Const(64),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Store type ID (2 = number)
            Instruction::LocalGet(1),
            Instruction::I32Const(2),
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            // Return pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for extracting value from precision number
    fn generate_get_number_value(&self) -> Vec<Instruction> {
        vec![
            // Parameters: precision_num_ptr (0)
            // Locals: precision_type (1), value (2) - F64
            
            // Load precision type
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 4, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // precision_type (offset differs for F64 storage)
            
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // precision_type (corrected offset)
            
            // Switch on precision type
            Instruction::LocalGet(1),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
                // 32-bit number - load F32 and promote to F64
                Instruction::LocalGet(0),
                Instruction::F32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
                Instruction::F64PromoteF32,
            Instruction::Else,
                // 64-bit number - load F64 directly
                Instruction::LocalGet(0),
                Instruction::F64Load(MemArg { offset: 0, align: 3, memory_index: 0 }),
            Instruction::End,
        ]
    }

    /// Generate WASM for getting precision bits from number
    fn generate_get_number_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: precision_num_ptr (0)
            // Local: precision_type (1)
            
            // Load and return precision bits (check both possible offsets)
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }), // For F64 layout
        ]
    }

    /// Generate WASM for converting between number precisions
    fn generate_convert_number_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: source_ptr (0), target_precision (1)
            // Locals: source_precision (2), value (3), result_ptr (4)
            
            // Get source precision
            Instruction::LocalGet(0),
            Instruction::Call(self.get_get_number_precision_function_index()),
            Instruction::LocalSet(2), // source_precision
            
            // Get source value
            Instruction::LocalGet(0),
            Instruction::Call(self.get_get_number_value_function_index()),
            Instruction::LocalSet(3), // value
            
            // Create target precision number
            Instruction::LocalGet(1), // target_precision
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Create 32-bit number
                Instruction::LocalGet(3), // value
                Instruction::Call(self.get_create_number_32_function_index()),
            Instruction::Else,
                // Create 64-bit number
                Instruction::LocalGet(3), // value
                Instruction::Call(self.get_create_number_64_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for converting precision integer to precision number
    fn generate_integer_to_number_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: int_ptr (0), target_number_precision (1)
            // Locals: int_value (2), converted_value (3), result_ptr (4)
            
            // Get integer value
            Instruction::LocalGet(0),
            Instruction::Call(self.get_get_integer_value_function_index()),
            Instruction::LocalSet(2), // int_value
            
            // Convert to floating point
            Instruction::LocalGet(2),
            Instruction::F64ConvertI32S,
            Instruction::LocalSet(3), // converted_value
            
            // Create precision number with target precision
            Instruction::LocalGet(1), // target_number_precision
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Create 32-bit number
                Instruction::LocalGet(3), // converted_value
                Instruction::Call(self.get_create_number_32_function_index()),
            Instruction::Else,
                // Create 64-bit number
                Instruction::LocalGet(3), // converted_value
                Instruction::Call(self.get_create_number_64_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for converting precision number to precision integer
    fn generate_number_to_integer_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: num_ptr (0), target_integer_precision (1)
            // Locals: num_value (2), converted_value (3), result_ptr (4)
            
            // Get number value
            Instruction::LocalGet(0),
            Instruction::Call(self.get_get_number_value_function_index()),
            Instruction::LocalSet(2), // num_value
            
            // Convert to integer (truncate)
            Instruction::LocalGet(2),
            Instruction::I32TruncF64S,
            Instruction::LocalSet(3), // converted_value
            
            // Create precision integer with target precision
            Instruction::LocalGet(1), // target_integer_precision
            
            // Switch on target precision
            Instruction::I32Const(8),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Create 8-bit integer
                Instruction::LocalGet(3), // converted_value
                Instruction::Call(self.get_create_integer_8_function_index()),
            Instruction::Else,
                Instruction::LocalGet(1),
                Instruction::I32Const(16),
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Create 16-bit integer
                    Instruction::LocalGet(3), // converted_value
                    Instruction::Call(self.get_create_integer_16_function_index()),   
                Instruction::Else,
                    Instruction::LocalGet(1),
                    Instruction::I32Const(64),
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        // Create 64-bit integer
                        Instruction::LocalGet(3), // converted_value
                        Instruction::I64ExtendI32S, // Extend to 64-bit
                        Instruction::Call(self.get_create_integer_64_function_index()),
                    Instruction::Else,
                        // Create 32-bit integer (default)
                        Instruction::LocalGet(3), // converted_value
                        Instruction::Call(self.get_create_integer_32_function_index()),
                    Instruction::End,
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for casting between integer precisions
    fn generate_cast_integer_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: source_int_ptr (0), target_precision (1)
            // Locals: source_value (2), casted_value (3), result_ptr (4)
            
            // Get source value
            Instruction::LocalGet(0),
            Instruction::Call(self.get_get_integer_value_function_index()),
            Instruction::LocalSet(2), // source_value
            
            // Cast to target precision (same as number to integer conversion)
            Instruction::LocalGet(1), // target_precision
            
            // Switch on target precision
            Instruction::I32Const(8),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Cast to 8-bit
                Instruction::LocalGet(2), // source_value
                Instruction::Call(self.get_create_integer_8_function_index()),
            Instruction::Else,
                Instruction::LocalGet(1),
                Instruction::I32Const(16),
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Cast to 16-bit
                    Instruction::LocalGet(2), // source_value
                    Instruction::Call(self.get_create_integer_16_function_index()),
                Instruction::Else,
                    Instruction::LocalGet(1),
                    Instruction::I32Const(64),
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        // Cast to 64-bit
                        Instruction::LocalGet(2), // source_value
                        Instruction::I64ExtendI32S, // Extend to 64-bit
                        Instruction::Call(self.get_create_integer_64_function_index()),
                    Instruction::Else,
                        // Cast to 32-bit (default)
                        Instruction::LocalGet(2), // source_value
                        Instruction::Call(self.get_create_integer_32_function_index()),
                    Instruction::End,
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for validating integer fits in precision range
    fn generate_validate_integer_precision(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), precision_bits (1)
            // Locals: min_value (2), max_value (3), is_valid (4)
            
            // Get min/max values for precision
            Instruction::LocalGet(1), // precision_bits
            
            // Switch on precision bits to set ranges
            Instruction::I32Const(8),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
                // 8-bit range: -128 to 127
                Instruction::I32Const(-128),
                Instruction::LocalSet(2), // min_value
                Instruction::I32Const(127),
                Instruction::LocalSet(3), // max_value
            Instruction::Else,
                Instruction::LocalGet(1),
                Instruction::I32Const(16),
                Instruction::I32Eq,
                Instruction::If(BlockType::Empty),
                    // 16-bit range: -32768 to 32767
                    Instruction::I32Const(-32768),
                    Instruction::LocalSet(2), // min_value
                    Instruction::I32Const(32767),
                    Instruction::LocalSet(3), // max_value
                Instruction::Else,
                    // 32-bit and 64-bit: no range check needed (use full I32 range)
                    Instruction::I32Const(i32::MIN),
                    Instruction::LocalSet(2), // min_value
                    Instruction::I32Const(2147483647),
                    Instruction::LocalSet(3), // max_value
                Instruction::End,
            Instruction::End,
            
            // Check if value is in range
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(2), // min_value
            Instruction::I32GeS, // value >= min_value
            
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(3), // max_value
            Instruction::I32LeS, // value <= max_value
            
            Instruction::I32And, // Both conditions must be true
        ]
    }

    /// Generate WASM for getting integer range info
    fn generate_get_integer_range(&self) -> Vec<Instruction> {
        vec![
            // Parameters: precision_bits (0)
            // Locals: range_ptr (1), min_value (2), max_value (3)
            
            // Allocate range structure (8 bytes: min, max)
            Instruction::I32Const(8),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // range_ptr
            
            // Set min/max based on precision
            Instruction::LocalGet(0), // precision_bits
            Instruction::I32Const(8),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
                // 8-bit range
                Instruction::I32Const(-128),
                Instruction::LocalSet(2), // min_value
                Instruction::I32Const(127),
                Instruction::LocalSet(3), // max_value
            Instruction::Else,
                Instruction::LocalGet(0),
                Instruction::I32Const(16),
                Instruction::I32Eq,
                Instruction::If(BlockType::Empty),
                    // 16-bit range
                    Instruction::I32Const(-32768),
                    Instruction::LocalSet(2), // min_value
                    Instruction::I32Const(32767),
                    Instruction::LocalSet(3), // max_value
                Instruction::Else,
                    // 32-bit range
                    Instruction::I32Const(i32::MIN),
                    Instruction::LocalSet(2), // min_value
                    Instruction::I32Const(2147483647),
                    Instruction::LocalSet(3), // max_value
                Instruction::End,
            Instruction::End,
            
            // Store min value
            Instruction::LocalGet(1),
            Instruction::LocalGet(2), // min_value
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Store max value
            Instruction::LocalGet(1),
            Instruction::LocalGet(3), // max_value
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Return range pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for checking if precision is supported
    fn generate_is_precision_supported(&self) -> Vec<Instruction> {
        vec![
            // Parameters: type_id (0), precision_bits (1)
            // Local: is_supported (2)
            
            Instruction::LocalGet(0), // type_id
            Instruction::I32Const(1), // integer type
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Integer type - support 8, 16, 32, 64 bits
                Instruction::LocalGet(1), // precision_bits
                Instruction::I32Const(8),
                Instruction::I32Eq,
                
                Instruction::LocalGet(1),
                Instruction::I32Const(16),
                Instruction::I32Eq,
                Instruction::I32Or,
                
                Instruction::LocalGet(1),
                Instruction::I32Const(32),
                Instruction::I32Eq,
                Instruction::I32Or,
                
                Instruction::LocalGet(1),
                Instruction::I32Const(64),
                Instruction::I32Eq,
                Instruction::I32Or,
            Instruction::Else,
                Instruction::LocalGet(0), // type_id
                Instruction::I32Const(2), // number type
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Number type - support 32, 64 bits
                    Instruction::LocalGet(1), // precision_bits
                    Instruction::I32Const(32),
                    Instruction::I32Eq,
                    
                    Instruction::LocalGet(1),
                    Instruction::I32Const(64),
                    Instruction::I32Eq,
                    Instruction::I32Or,
                Instruction::Else,
                    // Unknown type - not supported
                    Instruction::I32Const(0),
                Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for getting memory size for precision type
    fn generate_get_precision_memory_size(&self) -> Vec<Instruction> {
        vec![
            // Parameters: type_id (0), precision_bits (1)
            // Local: memory_size (2)
            
            Instruction::LocalGet(0), // type_id
            Instruction::I32Const(1), // integer type
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // Integer memory sizes
                Instruction::LocalGet(1), // precision_bits
                Instruction::I32Const(64),
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // 64-bit integer: 24 bytes (8 for value + 16 for metadata)
                    Instruction::I32Const(24),
                Instruction::Else,
                    // 8, 16, 32-bit integers: 16 bytes (4 for value + 12 for metadata)
                    Instruction::I32Const(16),
                Instruction::End,
            Instruction::Else,
                Instruction::LocalGet(0), // type_id  
                Instruction::I32Const(2), // number type
                Instruction::I32Eq,
                Instruction::If(BlockType::Result(ValType::I32)),
                    // Number memory sizes
                    Instruction::LocalGet(1), // precision_bits
                    Instruction::I32Const(64),
                    Instruction::I32Eq,
                    Instruction::If(BlockType::Result(ValType::I32)),
                        // 64-bit number: 24 bytes (8 for F64 + 16 for metadata)
                        Instruction::I32Const(24),
                    Instruction::Else,
                        // 32-bit number: 16 bytes (4 for F32 + 12 for metadata)
                        Instruction::I32Const(16),
                    Instruction::End,
                Instruction::Else,
                    // Unknown type
                    Instruction::I32Const(0),
                Instruction::End,
            Instruction::End,
        ]
    }

    // Helper function indices - Type Precision uses 1000-1030 range
    fn get_clamp_integer_function_index(&self) -> u32 { 1000 }
    fn get_allocate_function_index(&self) -> u32 { 1001 }
    fn get_get_integer_value_function_index(&self) -> u32 { 1002 }
    fn get_get_number_value_function_index(&self) -> u32 { 1003 }
    fn get_get_integer_precision_function_index(&self) -> u32 { 1004 }
    fn get_get_number_precision_function_index(&self) -> u32 { 1005 }
    fn get_create_integer_8_function_index(&self) -> u32 { 1006 }
    fn get_create_integer_16_function_index(&self) -> u32 { 1007 }
    fn get_create_integer_32_function_index(&self) -> u32 { 1008 }
    fn get_create_integer_64_function_index(&self) -> u32 { 1009 }
    fn get_create_number_32_function_index(&self) -> u32 { 1010 }
    fn get_create_number_64_function_index(&self) -> u32 { 1011 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_precision_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let precision_manager = TypePrecisionManager::new(memory_manager.clone());
        
        // Test that manager is created successfully
        assert!(precision_manager.memory_manager.borrow().data.len() > 0);
    }

    #[test]
    fn test_integer_precision_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let precision_manager = TypePrecisionManager::new(memory_manager);
        
        let create_8 = precision_manager.generate_create_integer_8();
        assert!(!create_8.is_empty());
        // Should clamp value first
        assert!(matches!(create_8[3], Instruction::Call(_)));
        
        let create_16 = precision_manager.generate_create_integer_16();
        assert!(!create_16.is_empty());
        
        let create_32 = precision_manager.generate_create_integer_32();
        assert!(!create_32.is_empty());
        // 32-bit doesn't need clamping
        assert!(create_32.len() < create_8.len());
        
        let create_64 = precision_manager.generate_create_integer_64();
        assert!(!create_64.is_empty());
        // Should use I64Store for 64-bit value
        assert!(create_64.iter().any(|inst| matches!(inst, Instruction::I64Store(_))));
    }

    #[test]
    fn test_number_precision_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let precision_manager = TypePrecisionManager::new(memory_manager);
        
        let create_32 = precision_manager.generate_create_number_32();
        assert!(!create_32.is_empty());
        // Should demote F64 to F32
        assert!(create_32.iter().any(|inst| matches!(inst, Instruction::F32DemoteF64)));
        
        let create_64 = precision_manager.generate_create_number_64();
        assert!(!create_64.is_empty());
        // Should store F64 directly
        assert!(create_64.iter().any(|inst| matches!(inst, Instruction::F64Store(_))));
    }

    #[test]
    fn test_precision_conversion_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let precision_manager = TypePrecisionManager::new(memory_manager);
        
        let int_to_num = precision_manager.generate_integer_to_number_precision();
        assert!(!int_to_num.is_empty());
        // Should convert integer to float
        assert!(int_to_num.iter().any(|inst| matches!(inst, Instruction::F64ConvertI32S)));
        
        let num_to_int = precision_manager.generate_number_to_integer_precision();
        assert!(!num_to_int.is_empty());
        // Should truncate float to integer
        assert!(num_to_int.iter().any(|inst| matches!(inst, Instruction::I32TruncF64S)));
        
        let cast_int = precision_manager.generate_cast_integer_precision();
        assert!(!cast_int.is_empty());
        // Should contain nested If blocks for precision switching
        let if_count = cast_int.iter().filter(|inst| matches!(inst, Instruction::If(_))).count();
        assert!(if_count >= 3); // Multiple precision cases
    }

    #[test]
    fn test_precision_validation_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let precision_manager = TypePrecisionManager::new(memory_manager);
        
        let validate = precision_manager.generate_validate_integer_precision();
        assert!(!validate.is_empty());
        // Should perform range checks with I32GeS and I32LeS
        assert!(validate.iter().any(|inst| matches!(inst, Instruction::I32GeS)));
        assert!(validate.iter().any(|inst| matches!(inst, Instruction::I32LeS)));
        assert!(validate.iter().any(|inst| matches!(inst, Instruction::I32And)));
        
        let range_info = precision_manager.generate_get_integer_range();
        assert!(!range_info.is_empty());
        // Should store min and max values
        let store_count = range_info.iter().filter(|inst| matches!(inst, Instruction::I32Store(_))).count();
        assert_eq!(store_count, 2); // min and max values
        
        let supported = precision_manager.generate_is_precision_supported();
        assert!(!supported.is_empty());
        // Should check both integer and number types
        let eq_count = supported.iter().filter(|inst| matches!(inst, Instruction::I32Eq)).count();
        assert!(eq_count >= 6); // Type checks and precision checks
    }

    #[test]
    fn test_memory_size_calculation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let precision_manager = TypePrecisionManager::new(memory_manager);
        
        let memory_size = precision_manager.generate_get_precision_memory_size();
        assert!(!memory_size.is_empty());
        // Should return different sizes based on type and precision
        // 16 bytes for small types, 24 bytes for 64-bit types
        assert!(memory_size.iter().any(|inst| matches!(inst, Instruction::I32Const(16))));
        assert!(memory_size.iter().any(|inst| matches!(inst, Instruction::I32Const(24))));
    }
}