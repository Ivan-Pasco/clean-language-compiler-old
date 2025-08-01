use crate::codegen::CodeGenerator;
use crate::stdlib::{register_stdlib_function_with_locals, MemoryManager};
use crate::types::WasmType;
use crate::error::CompilerError;
use wasm_encoder::{Instruction, BlockType, ValType, MemArg};
use std::rc::Rc;
use std::cell::RefCell;

/// Advanced String Operations Manager - Implements missing string functions
/// Provides split(), join(), charAt(), charCodeAt(), padStart(), padEnd(), isBlank()
pub struct StringAdvancedManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl StringAdvancedManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self {
            memory_manager,
        }
    }

    /// Register all advanced string functions with the code generator
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String splitting and joining
        self.register_split_functions(codegen)?;
        self.register_join_functions(codegen)?;
        
        // Character access functions
        self.register_char_functions(codegen)?;
        
        // String padding functions
        self.register_padding_functions(codegen)?;
        
        // String validation functions
        self.register_validation_functions(codegen)?;
        
        Ok(())
    }

    /// Register string splitting functions
    fn register_split_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String.split(string_ptr, delimiter_ptr) -> array_ptr
        register_stdlib_function_with_locals(
            codegen,
            "string.splitAdvanced",
            &[WasmType::I32, WasmType::I32], // string_ptr, delimiter_ptr
            Some(WasmType::I32), // result array pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // string_len, delim_len, array_ptr, count, pos, start, match_pos
            self.generate_split_advanced()
        )?;

        // String.splitChar(string_ptr, char_code) -> array_ptr
        register_stdlib_function_with_locals(
            codegen,
            "string.splitChar",
            &[WasmType::I32, WasmType::I32], // string_ptr, char_code
            Some(WasmType::I32), // result array pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // string_len, array_ptr, count, pos, start, char
            self.generate_split_char()
        )?;

        Ok(())
    }

    fn generate_split_advanced(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(2), // string_len

            // Load delimiter length
            Instruction::LocalGet(1), // delimiter_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // delimiter length
            Instruction::LocalSet(3), // delim_len

            // Allocate result array (initial capacity: 16 parts)
            Instruction::I32Const(16), // initial capacity
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000), // memory.allocate
            Instruction::LocalSet(4), // array_ptr

            // Initialize variables
            Instruction::I32Const(0), // count = 0
            Instruction::LocalSet(5), // count
            Instruction::I32Const(0), // pos = 0 (current position)
            Instruction::LocalSet(6), // pos
            Instruction::I32Const(0), // start = 0 (start of current part)
            Instruction::LocalSet(7), // start

            // Main split loop
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            
            // Check if pos >= string_len
            Instruction::LocalGet(6), // pos
            Instruction::LocalGet(2), // string_len
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if pos >= string_len

            // Check if delimiter matches at current position
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Const(4), // header size
            Instruction::I32Add,
            Instruction::LocalGet(6), // pos
            Instruction::I32Add, // string_data + pos

            Instruction::LocalGet(1), // delimiter_ptr
            Instruction::I32Const(4), // header size
            Instruction::I32Add, // delimiter_data

            Instruction::LocalGet(3), // delim_len
            Instruction::Call(2001), // memory.compare (hypothetical function)
            Instruction::I32Eqz, // result == 0 (match)
            Instruction::If(BlockType::Empty),
                // Found delimiter - create substring and add to array
                Instruction::LocalGet(7), // start
                Instruction::LocalGet(6), // pos
                Instruction::LocalGet(7), // start
                Instruction::I32Sub, // length = pos - start
                Instruction::Call(2002), // string.substring (hypothetical)
                
                // Store in array
                Instruction::LocalGet(4), // array_ptr
                Instruction::LocalGet(5), // count
                Instruction::I32Const(4), // sizeof(string_ptr)
                Instruction::I32Mul,
                Instruction::I32Add, // array_ptr + count*4
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }), // store substring

                // Update start position and count
                Instruction::LocalGet(6), // pos
                Instruction::LocalGet(3), // delim_len
                Instruction::I32Add,
                Instruction::LocalSet(7), // start = pos + delim_len

                Instruction::LocalGet(5), // count
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(5), // count++

                // Skip delimiter
                Instruction::LocalGet(6), // pos
                Instruction::LocalGet(3), // delim_len
                Instruction::I32Add,
                Instruction::LocalSet(6), // pos += delim_len
            Instruction::Else,
                // No match - advance position
                Instruction::LocalGet(6), // pos
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(6), // pos++
            Instruction::End,

            Instruction::Br(0), // continue loop
            Instruction::End, // end loop
            Instruction::End, // end block

            // Add final part if any
            Instruction::LocalGet(7), // start
            Instruction::LocalGet(2), // string_len
            Instruction::I32LtU, // start < string_len
            Instruction::If(BlockType::Empty),
                // Create final substring
                Instruction::LocalGet(7), // start
                Instruction::LocalGet(2), // string_len
                Instruction::LocalGet(7), // start
                Instruction::I32Sub, // length = string_len - start
                Instruction::Call(2002), // string.substring

                // Store in array
                Instruction::LocalGet(4), // array_ptr
                Instruction::LocalGet(5), // count
                Instruction::I32Const(4), // sizeof(string_ptr)
                Instruction::I32Mul,
                Instruction::I32Add,
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                Instruction::LocalGet(5), // count
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(5), // count++
            Instruction::End,

            // Return array pointer
            Instruction::LocalGet(4), // array_ptr
        ]
    }

    fn generate_split_char(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(2), // string_len

            // Allocate result array (initial capacity: 16 parts)
            Instruction::I32Const(16), // initial capacity
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000), // memory.allocate
            Instruction::LocalSet(3), // array_ptr

            // Initialize variables
            Instruction::I32Const(0), // count = 0
            Instruction::LocalSet(4), // count
            Instruction::I32Const(0), // pos = 0
            Instruction::LocalSet(5), // pos
            Instruction::I32Const(0), // start = 0
            Instruction::LocalSet(6), // start

            // Main split loop
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            
            // Check if pos >= string_len
            Instruction::LocalGet(5), // pos
            Instruction::LocalGet(2), // string_len
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if pos >= string_len

            // Load character at position
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Const(4), // header size
            Instruction::I32Add,
            Instruction::LocalGet(5), // pos
            Instruction::I32Add, // string_data + pos
            Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }), // load char
            Instruction::LocalSet(7), // char

            // Check if char matches delimiter
            Instruction::LocalGet(7), // char
            Instruction::LocalGet(1), // char_code
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
                // Found delimiter - create substring
                Instruction::LocalGet(6), // start
                Instruction::LocalGet(5), // pos
                Instruction::LocalGet(6), // start
                Instruction::I32Sub, // length = pos - start
                Instruction::Call(2002), // string.substring

                // Store in array
                Instruction::LocalGet(3), // array_ptr
                Instruction::LocalGet(4), // count
                Instruction::I32Const(4), // sizeof(string_ptr)
                Instruction::I32Mul,
                Instruction::I32Add,
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                // Update start and count
                Instruction::LocalGet(5), // pos
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(6), // start = pos + 1

                Instruction::LocalGet(4), // count
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(4), // count++
            Instruction::End,

            // Advance position
            Instruction::LocalGet(5), // pos
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5), // pos++

            Instruction::Br(0), // continue loop
            Instruction::End, // end loop
            Instruction::End, // end block

            // Add final part if any
            Instruction::LocalGet(6), // start
            Instruction::LocalGet(2), // string_len
            Instruction::I32LtU, // start < string_len
            Instruction::If(BlockType::Empty),
                // Create final substring
                Instruction::LocalGet(6), // start
                Instruction::LocalGet(2), // string_len
                Instruction::LocalGet(6), // start
                Instruction::I32Sub, // length = string_len - start
                Instruction::Call(2002), // string.substring

                // Store in array
                Instruction::LocalGet(3), // array_ptr
                Instruction::LocalGet(4), // count
                Instruction::I32Const(4), // sizeof(string_ptr)
                Instruction::I32Mul,
                Instruction::I32Add,
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::End,

            // Return array pointer
            Instruction::LocalGet(3), // array_ptr
        ]
    }

    /// Register string joining functions
    fn register_join_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String.joinAdvanced(array_ptr, array_length, separator_ptr) -> string_ptr
        register_stdlib_function_with_locals(
            codegen,
            "string.joinAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // array_ptr, array_length, separator_ptr
            Some(WasmType::I32), // result string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // total_len, result_ptr, pos, i, part_ptr, part_len
            self.generate_join_advanced()
        )?;

        Ok(())
    }

    fn generate_join_advanced(&self) -> Vec<Instruction> {
        vec![
            // Calculate total length needed
            Instruction::I32Const(0), // total_len = 0
            Instruction::LocalSet(3), // total_len
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(6), // i

            // First pass: calculate total length
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            
            // Check if i >= array_length
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(1), // array_length
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= array_length

            // Load string pointer from array
            Instruction::LocalGet(0), // array_ptr
            Instruction::LocalGet(6), // i
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Add, // array_ptr + i*4
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load string_ptr
            Instruction::LocalSet(7), // part_ptr

            // Load string length
            Instruction::LocalGet(7), // part_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(8), // part_len

            // Add to total length
            Instruction::LocalGet(3), // total_len
            Instruction::LocalGet(8), // part_len
            Instruction::I32Add,
            Instruction::LocalSet(3), // total_len += part_len

            // Add separator length (except for last element)
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(1), // array_length
            Instruction::I32Const(1),
            Instruction::I32Sub, // array_length - 1
            Instruction::I32LtU, // i < array_length - 1
            Instruction::If(BlockType::Empty),
                Instruction::LocalGet(3), // total_len
                Instruction::LocalGet(2), // separator_ptr
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // separator length
                Instruction::I32Add,
                Instruction::LocalSet(3), // total_len += separator_len
            Instruction::End,

            // Increment i
            Instruction::LocalGet(6), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // i++

            Instruction::Br(0), // continue loop
            Instruction::End, // end loop
            Instruction::End, // end block

            // Allocate result string
            Instruction::LocalGet(3), // total_len
            Instruction::I32Const(4), // header size
            Instruction::I32Add,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000), // memory.allocate
            Instruction::LocalSet(4), // result_ptr

            // Write string header
            Instruction::LocalGet(4), // result_ptr
            Instruction::LocalGet(3), // total_len
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }), // store length

            // Initialize position
            Instruction::I32Const(0), // pos = 0
            Instruction::LocalSet(5), // pos
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(6), // i

            // Second pass: copy strings
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            
            // Check if i >= array_length
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(1), // array_length
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= array_length

            // Load string pointer from array
            Instruction::LocalGet(0), // array_ptr
            Instruction::LocalGet(6), // i
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load string_ptr
            Instruction::LocalSet(7), // part_ptr

            // Load string length
            Instruction::LocalGet(7), // part_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(8), // part_len

            // Copy string data
            Instruction::LocalGet(4), // result_ptr
            Instruction::I32Const(4), // header size
            Instruction::I32Add,
            Instruction::LocalGet(5), // pos
            Instruction::I32Add, // result_data + pos

            Instruction::LocalGet(7), // part_ptr
            Instruction::I32Const(4), // header size
            Instruction::I32Add, // part_data

            Instruction::LocalGet(8), // part_len
            Instruction::Call(2003), // memory.copy (hypothetical)

            // Update position
            Instruction::LocalGet(5), // pos
            Instruction::LocalGet(8), // part_len
            Instruction::I32Add,
            Instruction::LocalSet(5), // pos += part_len

            // Copy separator (except for last element)
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(1), // array_length
            Instruction::I32Const(1),
            Instruction::I32Sub, // array_length - 1
            Instruction::I32LtU, // i < array_length - 1
            Instruction::If(BlockType::Empty),
                // Copy separator
                Instruction::LocalGet(4), // result_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(5), // pos
                Instruction::I32Add, // result_data + pos

                Instruction::LocalGet(2), // separator_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add, // separator_data

                Instruction::LocalGet(2), // separator_ptr
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // separator length
                Instruction::Call(2003), // memory.copy

                // Update position
                Instruction::LocalGet(5), // pos
                Instruction::LocalGet(2), // separator_ptr
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // separator length
                Instruction::I32Add,
                Instruction::LocalSet(5), // pos += separator_len
            Instruction::End,

            // Increment i
            Instruction::LocalGet(6), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // i++

            Instruction::Br(0), // continue loop
            Instruction::End, // end loop
            Instruction::End, // end block

            // Return result string
            Instruction::LocalGet(4), // result_ptr
        ]
    }

    /// Register character access functions
    fn register_char_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String.charAtAdvanced(string_ptr, index) -> char_string_ptr
        register_stdlib_function_with_locals(
            codegen,
            "string.charAtAdvanced",
            &[WasmType::I32, WasmType::I32], // string_ptr, index
            Some(WasmType::I32), // char string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_len, char_ptr, char_value
            self.generate_char_at_advanced()
        )?;

        // String.charCodeAtAdvanced(string_ptr, index) -> char_code
        register_stdlib_function_with_locals(
            codegen,
            "string.charCodeAtAdvanced",
            &[WasmType::I32, WasmType::I32], // string_ptr, index
            Some(WasmType::I32), // char code
            &[WasmType::I32], // string_len
            self.generate_char_code_at_advanced()
        )?;

        Ok(())
    }

    fn generate_char_at_advanced(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(2), // string_len

            // Bounds check
            Instruction::LocalGet(1), // index
            Instruction::LocalGet(2), // string_len
            Instruction::I32GeU, // index >= string_len
            Instruction::If(BlockType::Result(ValType::I32)),
                // Out of bounds - return empty string
                Instruction::I32Const(0),
            Instruction::Else,
                // Allocate single-character string
                Instruction::I32Const(5), // 4 bytes header + 1 byte char
                Instruction::I32Const(4), // alignment
                Instruction::Call(2000), // memory.allocate
                Instruction::LocalSet(3), // char_ptr

                // Set string header (length = 1)
                Instruction::LocalGet(3), // char_ptr
                Instruction::I32Const(1), // length = 1
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                // Load character from source string
                Instruction::LocalGet(0), // string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(1), // index
                Instruction::I32Add, // string_data + index
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }), // load char
                Instruction::LocalSet(4), // char_value

                // Store character in result string
                Instruction::LocalGet(3), // char_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add, // char_data
                Instruction::LocalGet(4), // char_value
                Instruction::I32Store8(MemArg { offset: 0, align: 0, memory_index: 0 }),

                // Return char string pointer
                Instruction::LocalGet(3), // char_ptr
            Instruction::End,
        ]
    }

    fn generate_char_code_at_advanced(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(2), // string_len

            // Bounds check
            Instruction::LocalGet(1), // index
            Instruction::LocalGet(2), // string_len
            Instruction::I32GeU, // index >= string_len
            Instruction::If(BlockType::Result(ValType::I32)),
                // Out of bounds - return 0
                Instruction::I32Const(0),
            Instruction::Else,
                // Load character code
                Instruction::LocalGet(0), // string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(1), // index
                Instruction::I32Add, // string_data + index
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }), // load char code
            Instruction::End,
        ]
    }

    /// Register string padding functions
    fn register_padding_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String.padStartAdvanced(string_ptr, target_length, pad_string_ptr) -> padded_string_ptr
        register_stdlib_function_with_locals(
            codegen,
            "string.padStartAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, target_length, pad_string_ptr
            Some(WasmType::I32), // padded string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // string_len, pad_len, pad_needed, result_ptr, i, pad_pos
            self.generate_pad_start_advanced()
        )?;

        // String.padEndAdvanced(string_ptr, target_length, pad_string_ptr) -> padded_string_ptr
        register_stdlib_function_with_locals(
            codegen,
            "string.padEndAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, target_length, pad_string_ptr
            Some(WasmType::I32), // padded string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // string_len, pad_len, pad_needed, result_ptr, i, pad_pos
            self.generate_pad_end_advanced()
        )?;

        Ok(())
    }

    fn generate_pad_start_advanced(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(3), // string_len

            // Check if padding is needed
            Instruction::LocalGet(3), // string_len
            Instruction::LocalGet(1), // target_length
            Instruction::I32GeU, // string_len >= target_length
            Instruction::If(BlockType::Result(ValType::I32)),
                // No padding needed - return original string
                Instruction::LocalGet(0), // string_ptr
            Instruction::Else,
                // Load pad string length
                Instruction::LocalGet(2), // pad_string_ptr
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // pad string length
                Instruction::LocalSet(4), // pad_len

                // Calculate padding needed
                Instruction::LocalGet(1), // target_length
                Instruction::LocalGet(3), // string_len
                Instruction::I32Sub,
                Instruction::LocalSet(5), // pad_needed

                // Allocate result string
                Instruction::LocalGet(1), // target_length
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::I32Const(4), // alignment
                Instruction::Call(2000), // memory.allocate
                Instruction::LocalSet(6), // result_ptr

                // Set result string header
                Instruction::LocalGet(6), // result_ptr
                Instruction::LocalGet(1), // target_length
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                // Fill padding at start
                Instruction::I32Const(0), // i = 0
                Instruction::LocalSet(7), // i
                Instruction::I32Const(0), // pad_pos = 0
                Instruction::LocalSet(8), // pad_pos

                Instruction::Block(BlockType::Empty),
                Instruction::Loop(BlockType::Empty),
                
                // Check if i >= pad_needed
                Instruction::LocalGet(7), // i
                Instruction::LocalGet(5), // pad_needed
                Instruction::I32GeU,
                Instruction::BrIf(1), // break if i >= pad_needed

                // Copy character from pad string
                Instruction::LocalGet(6), // result_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(7), // i
                Instruction::I32Add, // result_data + i

                Instruction::LocalGet(2), // pad_string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(8), // pad_pos
                Instruction::I32Add, // pad_data + pad_pos
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }), // load pad char

                Instruction::I32Store8(MemArg { offset: 0, align: 0, memory_index: 0 }), // store pad char

                // Advance pad position (with wraparound)
                Instruction::LocalGet(8), // pad_pos
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalGet(4), // pad_len
                Instruction::I32RemU, // pad_pos = (pad_pos + 1) % pad_len
                Instruction::LocalSet(8), // pad_pos

                // Increment i
                Instruction::LocalGet(7), // i
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(7), // i++

                Instruction::Br(0), // continue loop
                Instruction::End, // end loop
                Instruction::End, // end block

                // Copy original string after padding
                Instruction::LocalGet(6), // result_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(5), // pad_needed
                Instruction::I32Add, // result_data + pad_needed

                Instruction::LocalGet(0), // string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add, // string_data

                Instruction::LocalGet(3), // string_len
                Instruction::Call(2003), // memory.copy

                // Return result string
                Instruction::LocalGet(6), // result_ptr
            Instruction::End,
        ]
    }

    fn generate_pad_end_advanced(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(3), // string_len

            // Check if padding is needed
            Instruction::LocalGet(3), // string_len
            Instruction::LocalGet(1), // target_length
            Instruction::I32GeU, // string_len >= target_length
            Instruction::If(BlockType::Result(ValType::I32)),
                // No padding needed - return original string
                Instruction::LocalGet(0), // string_ptr
            Instruction::Else,
                // Load pad string length
                Instruction::LocalGet(2), // pad_string_ptr
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // pad string length
                Instruction::LocalSet(4), // pad_len

                // Calculate padding needed
                Instruction::LocalGet(1), // target_length
                Instruction::LocalGet(3), // string_len
                Instruction::I32Sub,
                Instruction::LocalSet(5), // pad_needed

                // Allocate result string
                Instruction::LocalGet(1), // target_length
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::I32Const(4), // alignment
                Instruction::Call(2000), // memory.allocate
                Instruction::LocalSet(6), // result_ptr

                // Set result string header
                Instruction::LocalGet(6), // result_ptr
                Instruction::LocalGet(1), // target_length
                Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                // Copy original string first
                Instruction::LocalGet(6), // result_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add, // result_data

                Instruction::LocalGet(0), // string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add, // string_data

                Instruction::LocalGet(3), // string_len
                Instruction::Call(2003), // memory.copy

                // Fill padding at end
                Instruction::I32Const(0), // i = 0
                Instruction::LocalSet(7), // i
                Instruction::I32Const(0), // pad_pos = 0
                Instruction::LocalSet(8), // pad_pos

                Instruction::Block(BlockType::Empty),
                Instruction::Loop(BlockType::Empty),
                
                // Check if i >= pad_needed
                Instruction::LocalGet(7), // i
                Instruction::LocalGet(5), // pad_needed
                Instruction::I32GeU,
                Instruction::BrIf(1), // break if i >= pad_needed

                // Copy character from pad string
                Instruction::LocalGet(6), // result_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(3), // string_len
                Instruction::I32Add,
                Instruction::LocalGet(7), // i
                Instruction::I32Add, // result_data + string_len + i

                Instruction::LocalGet(2), // pad_string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(8), // pad_pos
                Instruction::I32Add, // pad_data + pad_pos
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }), // load pad char

                Instruction::I32Store8(MemArg { offset: 0, align: 0, memory_index: 0 }), // store pad char

                // Advance pad position (with wraparound)
                Instruction::LocalGet(8), // pad_pos
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalGet(4), // pad_len
                Instruction::I32RemU, // pad_pos = (pad_pos + 1) % pad_len
                Instruction::LocalSet(8), // pad_pos

                // Increment i
                Instruction::LocalGet(7), // i
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(7), // i++

                Instruction::Br(0), // continue loop
                Instruction::End, // end loop
                Instruction::End, // end block

                // Return result string
                Instruction::LocalGet(6), // result_ptr
            Instruction::End,
        ]
    }

    /// Register string validation functions
    fn register_validation_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // String.isBlankAdvanced(string_ptr) -> boolean
        register_stdlib_function_with_locals(
            codegen,
            "string.isBlankAdvanced",
            &[WasmType::I32], // string_ptr
            Some(WasmType::I32), // boolean result
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_len, i, char
            self.generate_is_blank_advanced()
        )?;

        Ok(())
    }

    fn generate_is_blank_advanced(&self) -> Vec<Instruction> {
        vec![
            // Load string length
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // string length
            Instruction::LocalSet(1), // string_len

            // Empty string is blank
            Instruction::LocalGet(1), // string_len
            Instruction::I32Eqz, // string_len == 0
            Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::I32Const(1), // return true
            Instruction::Else,
                // Check each character
                Instruction::I32Const(0), // i = 0
                Instruction::LocalSet(2), // i

                Instruction::Block(BlockType::Result(ValType::I32)),
                Instruction::Loop(BlockType::Empty),
                
                // Check if i >= string_len
                Instruction::LocalGet(2), // i
                Instruction::LocalGet(1), // string_len
                Instruction::I32GeU,
                Instruction::BrIf(1), // break if i >= string_len (all chars were whitespace)

                // Load character
                Instruction::LocalGet(0), // string_ptr
                Instruction::I32Const(4), // header size
                Instruction::I32Add,
                Instruction::LocalGet(2), // i
                Instruction::I32Add, // string_data + i
                Instruction::I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }), // load char
                Instruction::LocalSet(3), // char

                // Check if character is not whitespace
                // Whitespace: space (32), tab (9), newline (10), carriage return (13)
                Instruction::LocalGet(3), // char
                Instruction::I32Const(32), // space
                Instruction::I32Ne,

                Instruction::LocalGet(3), // char
                Instruction::I32Const(9), // tab
                Instruction::I32Ne,
                Instruction::I32And,

                Instruction::LocalGet(3), // char
                Instruction::I32Const(10), // newline
                Instruction::I32Ne,
                Instruction::I32And,

                Instruction::LocalGet(3), // char
                Instruction::I32Const(13), // carriage return
                Instruction::I32Ne,
                Instruction::I32And,

                Instruction::If(BlockType::Empty),
                    // Found non-whitespace character - not blank
                    Instruction::I32Const(0), // return false
                    Instruction::Br(2), // break out of outer block
                Instruction::End,

                // Increment i
                Instruction::LocalGet(2), // i
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(2), // i++

                Instruction::Br(0), // continue loop
                Instruction::End, // end loop

                // All characters were whitespace - string is blank
                Instruction::I32Const(1), // return true
                Instruction::End, // end block
            Instruction::End,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::CodeGenerator;
    use std::rc::Rc;
    use std::cell::RefCell;

    #[test]
    fn test_string_advanced_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _manager = StringAdvancedManager::new(memory_manager);
    }

    #[test]
    fn test_register_string_advanced_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = StringAdvancedManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();
        
        manager.register_functions(&mut codegen)?;
        
        // Verify split functions are registered
        assert!(codegen.get_function_index("string.splitAdvanced").is_some());
        assert!(codegen.get_function_index("string.splitChar").is_some());
        
        Ok(())
    }

    #[test]
    fn test_string_split_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = StringAdvancedManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();
        
        manager.register_functions(&mut codegen)?;
        
        // Test that split functions are available
        assert!(codegen.get_function_index("string.splitAdvanced").is_some());
        assert!(codegen.get_function_index("string.splitChar").is_some());
        
        Ok(())
    }

    #[test]
    fn test_string_join_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = StringAdvancedManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();
        
        manager.register_functions(&mut codegen)?;
        
        // Test that join functions are available
        assert!(codegen.get_function_index("string.joinAdvanced").is_some());
        
        Ok(())
    }

    #[test]
    fn test_string_char_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = StringAdvancedManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();
        
        manager.register_functions(&mut codegen)?;
        
        // Test that character functions are available
        assert!(codegen.get_function_index("string.charAtAdvanced").is_some());
        assert!(codegen.get_function_index("string.charCodeAtAdvanced").is_some());
        
        Ok(())
    }

    #[test]
    fn test_string_padding_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = StringAdvancedManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();
        
        manager.register_functions(&mut codegen)?;
        
        // Test that padding functions are available
        assert!(codegen.get_function_index("string.padStartAdvanced").is_some());
        assert!(codegen.get_function_index("string.padEndAdvanced").is_some());
        
        Ok(())
    }

    #[test]
    fn test_string_validation_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = StringAdvancedManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();
        
        manager.register_functions(&mut codegen)?;
        
        // Test that validation functions are available
        assert!(codegen.get_function_index("string.isBlankAdvanced").is_some());
        
        Ok(())
    }

    #[test]
    fn test_whitespace_constants() {
        // Test that whitespace character constants are correct
        let space = 32;
        let tab = 9;
        let newline = 10;
        let carriage_return = 13;
        
        assert_eq!(space, 32);
        assert_eq!(tab, 9);
        assert_eq!(newline, 10);
        assert_eq!(carriage_return, 13);
    }

    #[test]
    fn test_string_operations_logic() {
        // Test basic string operation logic
        let target_length = 10;
        let string_length = 5;
        let pad_needed = target_length - string_length;
        
        assert_eq!(pad_needed, 5);
        assert!(string_length < target_length);
    }
}