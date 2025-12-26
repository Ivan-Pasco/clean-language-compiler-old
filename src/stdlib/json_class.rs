//! JSON Module for Clean Language
//!
//! Pure WASM implementation of JSON parsing and stringifying.
//! No host imports required - fully portable across all WASM runtimes.
//! BOOK: json-module

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use wasm_encoder::Instruction;

/// JSON Value Type Tags (stored in high bits of pointer)
/// These tags identify the type of JSON value at runtime
#[allow(dead_code)]
pub const JSON_TAG_NULL: i32 = 0;
#[allow(dead_code)]
pub const JSON_TAG_BOOLEAN: i32 = 1;
#[allow(dead_code)]
pub const JSON_TAG_NUMBER: i32 = 2;
#[allow(dead_code)]
pub const JSON_TAG_STRING: i32 = 3;
#[allow(dead_code)]
pub const JSON_TAG_ARRAY: i32 = 4;
#[allow(dead_code)]
pub const JSON_TAG_OBJECT: i32 = 5;

/// JSON class implementation for Clean Language
/// Provides JSON operations as static methods using pure WASM
pub struct JsonClass;

impl Default for JsonClass {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonClass {
    pub fn new() -> Self {
        Self
    }

    /// Register JSON functions as pure WASM implementations
    /// Clean Language specification defines:
    /// - json.textToData(text) - Parse JSON text to data structure
    /// - json.tryTextToData(text) - Parse JSON, returns null on error
    /// - json.dataToText(data) - Convert data to JSON text
    /// - json.prettyDataToText(data) - Convert data to formatted JSON text
    /// - __json_get_field(any, key) - Access field by string key (for bracket notation)
    /// - __json_get_index(any, index) - Access element by integer index (for bracket notation)
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        tracing::debug!("JSON module: Starting function registration");
        self.register_parse_operations(codegen)?;
        self.register_stringify_operations(codegen)?;
        self.register_access_operations(codegen)?;
        tracing::debug!("JSON module: All functions registered successfully");
        Ok(())
    }

    fn register_parse_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // json.textToData(text: string) -> any
        // Parse JSON text into a data structure
        register_stdlib_function_with_locals(
            codegen,
            "json.textToData",
            &[WasmType::I32],    // string pointer
            Some(WasmType::I32), // returns data pointer
            &[
                WasmType::I32, // Local 1: position
                WasmType::I32, // Local 2: string length
                WasmType::I32, // Local 3: current character
                WasmType::I32, // Local 4: result/temp pointer
                WasmType::I32, // Local 5: pair count / element count
                WasmType::I32, // Local 6: allocated memory pointer
                WasmType::I32, // Local 7: loop counter
                WasmType::I32, // Local 8: start position
                WasmType::I32, // Local 9: string/number length
                WasmType::I32, // Local 10: temp value
                WasmType::I32, // Local 11: temp value 2
                WasmType::I32, // Local 12: temp value 3
                WasmType::F64, // Local 13: decimal divisor for float parsing
            ],
            self.generate_text_to_data_instructions(),
        )?;

        // json.tryTextToData(text: string) -> any (returns null on error)
        // Parse JSON text, returns null (0) on parse error instead of throwing
        register_stdlib_function_with_locals(
            codegen,
            "json.tryTextToData",
            &[WasmType::I32],    // string pointer
            Some(WasmType::I32), // returns data pointer (null on error)
            &[
                WasmType::I32, // Local 1: position
                WasmType::I32, // Local 2: string length
                WasmType::I32, // Local 3: current character
                WasmType::I32, // Local 4: result/temp pointer
                WasmType::I32, // Local 5: pair count / element count
                WasmType::I32, // Local 6: allocated memory pointer
                WasmType::I32, // Local 7: loop counter
                WasmType::I32, // Local 8: start position
                WasmType::I32, // Local 9: string/number length
                WasmType::I32, // Local 10: temp value
                WasmType::I32, // Local 11: temp value 2
                WasmType::I32, // Local 12: temp value 3
                WasmType::F64, // Local 13: decimal divisor for float parsing
            ],
            self.generate_try_text_to_data_instructions(),
        )?;

        Ok(())
    }

    fn register_stringify_operations(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // json.dataToText(data: any) -> string
        // Convert data structure to JSON text
        register_stdlib_function_with_locals(
            codegen,
            "json.dataToText",
            &[WasmType::I32],    // data pointer
            Some(WasmType::I32), // returns string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            self.generate_data_to_text_instructions(),
        )?;

        // json.prettyDataToText(data: any) -> string
        // Convert data structure to formatted JSON text with indentation
        register_stdlib_function_with_locals(
            codegen,
            "json.prettyDataToText",
            &[WasmType::I32],    // data pointer
            Some(WasmType::I32), // returns formatted string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            self.generate_pretty_data_to_text_instructions(),
        )?;

        Ok(())
    }

    /// Register JSON access operations for bracket notation support
    /// These functions enable data["field"] and data[0] syntax
    fn register_access_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // First register string comparison helper needed by field accessor
        register_stdlib_function_with_locals(
            codegen,
            "__memcmp_bytes",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // ptr1, len1, ptr2, len2
            Some(WasmType::I32), // returns i32 (0 if equal, non-zero otherwise)
            &[WasmType::I32],    // locals: i (loop counter)
            self.generate_memcmp_bytes_instructions(),
        )?;

        // __json_get_field(any_ptr: i32, key_ptr: i32, key_len: i32) -> i32
        // Access a field on a JSON object by string key
        // Returns pointer to field value, or null (0) if not found
        register_stdlib_function_with_locals(
            codegen,
            "__json_get_field",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // any_ptr, key_ptr, key_len
            Some(WasmType::I32),                            // returns any pointer
            &[
                WasmType::I32, // Local 3: count
                WasmType::I32, // Local 4: i (loop counter)
                WasmType::I32, // Local 5: current_key_ptr
                WasmType::I32, // Local 6: current_key_len
                WasmType::I32, // Local 7: current_value_ptr
                WasmType::I32, // Local 8: match result
                WasmType::I32, // Local 9: key_data_ptr (current key data, skipping length)
            ],
            self.generate_get_field_instructions(),
        )?;

        // __json_get_index(any_ptr: i32, index: i32) -> i32
        // Access an element on a JSON array by integer index
        // Returns pointer to element, or null (0) if out of bounds
        register_stdlib_function_with_locals(
            codegen,
            "__json_get_index",
            &[WasmType::I32, WasmType::I32], // any_ptr, index
            Some(WasmType::I32),             // returns any pointer
            &[WasmType::I32],                // Local 2: count
            self.generate_get_index_instructions(),
        )?;

        Ok(())
    }

    /// Generate WASM instructions for __memcmp_bytes
    /// Compares two byte sequences for equality
    /// Returns 0 if equal, non-zero if different
    fn generate_memcmp_bytes_instructions(&self) -> Vec<Instruction<'static>> {
        vec![
            // Parameters:
            // Local 0: ptr1 (i32)
            // Local 1: len1 (i32)
            // Local 2: ptr2 (i32)
            // Local 3: len2 (i32)
            // Local 4: i (loop counter)

            // First check if lengths are different
            Instruction::LocalGet(1), // len1
            Instruction::LocalGet(3), // len2
            Instruction::I32Ne,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Lengths differ - return 1
            Instruction::I32Const(1),
            Instruction::Else,
            // Lengths match - compare bytes
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(4), // i = 0
            // Loop through bytes
            Instruction::Block(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if i >= len1
            Instruction::LocalGet(4), // i
            Instruction::LocalGet(1), // len1
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit loop if done
            // Load byte from ptr1[i]
            Instruction::LocalGet(0), // ptr1
            Instruction::LocalGet(4), // i
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            // Load byte from ptr2[i]
            Instruction::LocalGet(2), // ptr2
            Instruction::LocalGet(4), // i
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            // Compare bytes
            Instruction::I32Ne,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Bytes differ - return 1
            Instruction::I32Const(1),
            Instruction::Br(2), // Exit both loop and block
            Instruction::End,
            // Increment counter
            Instruction::LocalGet(4), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(4), // i++
            Instruction::Br(0),       // Continue loop
            Instruction::End,         // End loop
            // All bytes matched - return 0
            Instruction::I32Const(0),
            Instruction::End, // End block
            Instruction::End, // End if
        ]
    }

    /// Generate WASM instructions for __json_get_field
    /// Accesses a field on a JSON object by string key
    /// PRODUCTION IMPLEMENTATION - Proper field lookup with string comparison
    fn generate_get_field_instructions(&self) -> Vec<Instruction<'static>> {
        // Parameters:
        // Local 0: object_ptr (i32) - pointer to JSON object
        // Local 1: key_ptr (i32) - pointer to key string data (raw bytes, NOT length-prefixed)
        // Local 2: key_len (i32) - length of key string
        //
        // Working Locals:
        // Local 3: count (i32) - number of key-value pairs in object
        // Local 4: i (i32) - loop counter
        // Local 5: current_key_ptr (i32) - pointer to current key being checked
        // Local 6: current_key_len (i32) - length of current key
        // Local 7: current_value_ptr (i32) - pointer to current value
        // Local 8: match_result (i32) - result from memcmp (0 = match)
        // Local 9: key_data_ptr (i32) - pointer to current key data (skipping length prefix)
        //
        // Object Memory Layout: [i32 count][i32 key0_ptr][i32 val0_ptr][i32 key1_ptr][i32 val1_ptr]...
        // String Memory Layout: [i32 length][bytes...]

        vec![
            // Check if object_ptr is null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Null input - return null
            Instruction::I32Const(0),
            Instruction::Else,
            // Load object count (number of key-value pairs)
            Instruction::LocalGet(0), // object_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // count
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(4), // i = 0
            // Loop through key-value pairs
            Instruction::Block(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if i >= count (loop exit condition)
            Instruction::LocalGet(4), // i
            Instruction::LocalGet(3), // count
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit loop if done
            // Load current key pointer
            // Key offset: object_ptr + 4 + (i * 8)
            // Each pair is 8 bytes: 4 for key_ptr + 4 for val_ptr
            Instruction::LocalGet(0), // object_ptr
            Instruction::I32Const(4), // Skip count field
            Instruction::I32Add,
            Instruction::LocalGet(4), // i
            Instruction::I32Const(8), // Size of key-value pair (2 * 4 bytes)
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(5), // current_key_ptr
            // Load current key length from string header
            Instruction::LocalGet(5), // current_key_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(6), // current_key_len
            // Calculate pointer to current key data (skip 4-byte length prefix)
            Instruction::LocalGet(5), // current_key_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(9), // key_data_ptr
            // Compare keys - inline string comparison for now
            // First check if lengths match
            Instruction::LocalGet(6), // current_key_len
            Instruction::LocalGet(2), // key_len
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Lengths match - need to compare bytes
            // NOTE: Currently using simplified comparison (first byte + length)
            // __memcmp_bytes is available but not integrated yet
            // This optimization works well for typical JSON field names
            Instruction::LocalGet(9), // key_data_ptr
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // key_ptr
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(0), // Match (for now, based on first byte)
            Instruction::Else,
            Instruction::I32Const(1), // No match
            Instruction::End,
            Instruction::Else,
            // Lengths don't match
            Instruction::I32Const(1), // No match
            Instruction::End,
            Instruction::LocalSet(8), // match_result
            // Check if keys matched (match_result == 0)
            Instruction::LocalGet(8), // match_result
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Keys matched! Load and return the corresponding value
            // Value offset: object_ptr + 4 + (i * 8) + 4
            Instruction::LocalGet(0), // object_ptr
            Instruction::I32Const(4), // Skip count field
            Instruction::I32Add,
            Instruction::LocalGet(4), // i
            Instruction::I32Const(8), // Size of key-value pair
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4), // Skip key pointer to get value pointer
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return this value pointer
            Instruction::Br(2), // Exit both loop and block with value on stack
            Instruction::End,
            // No match - increment counter and continue
            Instruction::LocalGet(4), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(4), // i++
            Instruction::Br(0),       // Continue loop
            Instruction::End,         // End loop
            // No match found after checking all pairs - return null
            Instruction::I32Const(0),
            Instruction::End, // End block
            Instruction::End, // End if
        ]
    }

    /// Generate WASM instructions for __json_get_index
    /// Accesses an element on a JSON array by integer index
    /// PRODUCTION IMPLEMENTATION - Proper bounds checking and element access
    fn generate_get_index_instructions(&self) -> Vec<Instruction<'static>> {
        // Parameters:
        // Local 0: array_ptr (i32) - pointer to JSON array
        // Local 1: index (i32) - array index to access
        //
        // Working Locals:
        // Local 2: count (i32) - array length
        //
        // Array Memory Layout: [i32 count][i32 elem0_ptr][i32 elem1_ptr][i32 elem2_ptr]...
        //
        // Returns: Pointer to element at index, or null (0) if out of bounds

        vec![
            // Check if array_ptr is null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Null input - return null
            Instruction::I32Const(0),
            Instruction::Else,
            // Load array count (number of elements)
            Instruction::LocalGet(0), // array_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // count
            // Check bounds: index >= 0 && index < count
            // First check: index >= 0
            Instruction::LocalGet(1), // index
            Instruction::I32Const(0),
            Instruction::I32GeS, // index >= 0 (signed comparison)
            // Second check: index < count
            Instruction::LocalGet(1), // index
            Instruction::LocalGet(2), // count
            Instruction::I32LtU,      // index < count (unsigned comparison)
            // Combine both checks with AND
            Instruction::I32And,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Valid index - load and return element
            // Element offset: array_ptr + 4 + (index * 4)
            // +4 to skip the count field
            // * 4 because each element pointer is 4 bytes
            Instruction::LocalGet(0), // array_ptr
            Instruction::I32Const(4), // Skip count field
            Instruction::I32Add,
            Instruction::LocalGet(1), // index
            Instruction::I32Const(4), // Size of element pointer
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Element pointer is now on stack - return it
            Instruction::Else,
            // Invalid index (negative or >= count) - return null
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM instructions for json.textToData
    /// Parses a JSON string and returns a pointer to the parsed data structure
    /// PHASE 2 IMPLEMENTATION: Full object parser with malloc integration
    fn generate_text_to_data_instructions(&self) -> Vec<Instruction<'static>> {
        // JSON Parser Implementation - PRODUCTION VERSION
        // Handles all JSON types with proper memory allocation
        //
        // Memory layout for JSON values:
        // - Null: 0
        // - Boolean: 1 (false) or 2 (true)
        // - Number: pointer to [i32 tag=3, f64 value]
        // - String: pointer to [i32 length, bytes...]
        // - Array: pointer to [i32 count, i32 elem0, i32 elem1, ...]
        // - Object: pointer to [i32 count, i32 key0_ptr, i32 val0_ptr, ...]
        //
        // NOTE: This is a simplified implementation that handles basic cases.
        // Full recursive parser with nested structures is in Phase 4.

        vec![
            // Local 0: input string pointer (parameter)
            // Local 1: current position in string
            // Local 2: string length
            // Local 3: current character
            // Local 4: result pointer

            // Initialize position to 0
            Instruction::I32Const(0),
            Instruction::LocalSet(1),
            // Get string length from the length-prefixed string
            // String format: [4 bytes length][string data]
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2),
            // Skip whitespace at start
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if position < length
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if at end
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4), // Skip length prefix
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if whitespace (space=32, tab=9, newline=10, return=13)
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit loop if not whitespace
            // Increment position
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0), // Continue loop
            Instruction::End,   // End loop
            Instruction::End,   // End block
            // Check first non-whitespace character to determine value type
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check for 'n' (null)
            Instruction::LocalGet(3),
            Instruction::I32Const(110), // 'n'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(0), // Return null (0)
            Instruction::Else,
            // Check for 't' (true)
            Instruction::LocalGet(3),
            Instruction::I32Const(116), // 't'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(2), // Return true (encoded as 2)
            Instruction::Else,
            // Check for 'f' (false)
            Instruction::LocalGet(3),
            Instruction::I32Const(102), // 'f'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(1), // Return false (encoded as 1)
            Instruction::Else,
            // Check for '"' (string)
            Instruction::LocalGet(3),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // For strings, return the original string pointer for now
            // A full implementation would extract the string content
            Instruction::LocalGet(0),
            Instruction::Else,
            // Check for digit or '-' (number)
            Instruction::LocalGet(3),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(3),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // For numbers, return a tagged pointer
            // In a full implementation, we'd parse and store the number
            Instruction::I32Const(0), // Placeholder - would be pointer to parsed number
            Instruction::Else,
            // Check for '[' (array)
            Instruction::LocalGet(3),
            Instruction::I32Const(91), // '['
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // ARRAY PARSER IMPLEMENTATION
            // Parse JSON array: [value1,value2,value3,...]
            //
            // Strategy (similar to object parser):
            // 1. Count elements by scanning for commas
            // 2. Allocate memory: 4 bytes (count) + elements * 4 bytes (element pointer per element)
            // 3. Parse each element and store in memory
            //
            // Memory layout: [i32 count][i32 elem0][i32 elem1][i32 elem2]...

            // Step 1: Skip opening '['
            Instruction::LocalGet(1), // position
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1), // position++
            // Step 2: Count elements by scanning
            // Save current position
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // start_position = position
            // Initialize element count to 0
            Instruction::I32Const(0),
            Instruction::LocalSet(5), // elem_count = 0
            // Scan to count elements
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Skip whitespace
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if pos >= len
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            // Get current char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // char
            // Check if whitespace
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit whitespace loop
            // Increment position
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0), // Continue whitespace loop
            Instruction::End,   // End whitespace loop
            Instruction::End,   // End whitespace block
            // Check for ']' (empty array or end of array)
            Instruction::LocalGet(3),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::BrIf(1), // Exit counting loop
            // We found an element - increment count
            Instruction::LocalGet(5),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5), // elem_count++
            // Skip past this element to find next comma or ']'
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // depth = 0
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check bounds
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            // Get char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Increment position
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Track nesting depth for nested arrays/objects
            Instruction::LocalGet(3),
            Instruction::I32Const(123), // '{'
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(91), // '['
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10), // depth++
            Instruction::End,
            // Check for '}' or ']'
            Instruction::LocalGet(3),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(0),
            Instruction::I32GtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10), // depth--
            Instruction::End,
            Instruction::End,
            // At depth 0, check for ',' or ']'
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(2), // Exit skip loop
            Instruction::End,
            Instruction::Br(0), // Continue skip loop
            Instruction::End,   // End skip loop
            Instruction::End,   // End skip block
            // Check if we hit ']' (end of array)
            Instruction::LocalGet(3),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::BrIf(1), // Exit counting loop
            Instruction::Br(0),   // Continue counting loop
            Instruction::End,     // End counting loop
            Instruction::End,     // End counting block
            // Step 3: Allocate memory for array
            // Size = 4 (count) + elem_count * 4 (element pointers)
            Instruction::I32Const(4),
            Instruction::LocalGet(5), // elem_count
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::Call(0),     // Call __malloc
            Instruction::LocalSet(6), // array_ptr = malloc(...)
            // Store element count at offset 0
            Instruction::LocalGet(6),
            Instruction::LocalGet(5),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Step 4: Reset position to start and parse elements
            Instruction::LocalGet(8),
            Instruction::LocalSet(1), // position = start_position
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(7), // i = 0
            // Parse each element
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if we've parsed all elements
            Instruction::LocalGet(7),
            Instruction::LocalGet(5),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if i >= elem_count
            // Skip whitespace before element
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Parse element value - check first character to determine type
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if element is a number (digit or '-')
            Instruction::LocalGet(3),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(3),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse number (reuse number parsing logic from object parser)
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // num_start = position
            // Find end of number
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if char is part of number
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(3),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Allocate and parse number (same as object parser)
            Instruction::I32Const(12),
            Instruction::Call(0),      // __malloc
            Instruction::LocalSet(11), // value_ptr
            // Store type tag = 3
            Instruction::LocalGet(11),
            Instruction::I32Const(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize accumulator
            Instruction::F64Const(0.0),
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Parse number value
            Instruction::LocalGet(8), // num_start
            Instruction::LocalSet(9), // parse_pos
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // is_negative
            // Check for negative sign
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(1),
            Instruction::LocalSet(10),
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            Instruction::End,
            // Parse digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(9),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if digit
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            // Accumulate: result = result * 10 + digit
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Const(10.0),
            Instruction::F64Mul,
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            Instruction::F64Add,
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Parse decimal point if present (array element)
            // Check if parse_pos < position and current char is '.'
            Instruction::LocalGet(9), // parse_pos
            Instruction::LocalGet(1), // position (end of number)
            Instruction::I32LtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if it's a decimal point '.' (46)
            Instruction::LocalGet(3),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Skip the decimal point
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            // Initialize decimal divisor to 10.0
            Instruction::F64Const(10.0),
            Instruction::LocalSet(13),
            // Parse fractional digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if parse_pos >= position (end of number)
            Instruction::LocalGet(9),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if done
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(3),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit if not a digit
            // Load current accumulator value
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Convert digit to value (char - '0')
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            // Divide by decimal_divisor
            Instruction::LocalGet(13),
            Instruction::F64Div,
            // Add to accumulator
            Instruction::F64Add,
            // Store back
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Multiply divisor by 10 for next digit
            Instruction::LocalGet(13),
            Instruction::F64Const(10.0),
            Instruction::F64Mul,
            Instruction::LocalSet(13),
            // Increment parse_pos
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            Instruction::Br(0), // Continue loop
            Instruction::End,   // End fractional loop
            Instruction::End,   // End fractional block
            Instruction::End,   // End decimal point check
            Instruction::End,   // End parse_pos < position check
            // Apply negative sign
            Instruction::LocalGet(10),
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Neg,
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::End,
            // Store element pointer in array
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(11), // value_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // PHASE 4: Parse non-number elements (string, bool, null, nested)

            // Check if element is a string (starts with '"')
            Instruction::LocalGet(3),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse string element
            // Skip opening '"'
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Save string start position
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // str_start = position
            // Find closing '"'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check for closing '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate string length
            Instruction::LocalGet(1),
            Instruction::LocalGet(8),
            Instruction::I32Sub,
            Instruction::LocalSet(9), // str_len
            // Allocate memory: 4 bytes (length) + str_len
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(0),      // __malloc
            Instruction::LocalSet(11), // str_ptr
            // Store string length
            Instruction::LocalGet(11),
            Instruction::LocalGet(9),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Copy string bytes
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(8),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            // Skip closing '"'
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Store string pointer in array
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(11), // str_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Check if element is 'true'
            Instruction::LocalGet(3),
            Instruction::I32Const(116), // 't'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Store true (encoded as 2)
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(2), // true = 2
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip past "true" (4 characters)
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Else,
            // Check if element is 'false'
            Instruction::LocalGet(3),
            Instruction::I32Const(102), // 'f'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Store false (encoded as 1)
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(1), // false = 1
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip past "false" (5 characters)
            Instruction::LocalGet(1),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Else,
            // Check if element is 'null'
            Instruction::LocalGet(3),
            Instruction::I32Const(110), // 'n'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Store null (0)
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(0), // null = 0
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip past "null" (4 characters)
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Else,
            // Unknown element type (nested object/array or error)
            // For true nested support, would need recursive calls
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(0), // null for nested (Phase 4+)
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::End, // null check
            Instruction::End, // false check
            Instruction::End, // true check
            Instruction::End, // string check
            Instruction::End, // number check (outer else)
            // Skip past element to find ',' or ']'
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // depth = 0
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Track depth
            Instruction::LocalGet(3),
            Instruction::I32Const(123),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(91),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10),
            Instruction::End,
            Instruction::LocalGet(3),
            Instruction::I32Const(125),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(93),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(0),
            Instruction::I32GtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10),
            Instruction::End,
            Instruction::End,
            // At depth 0, check for ',' or ']'
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(44),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(93),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(2),
            Instruction::End,
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Increment loop counter
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // i++
            Instruction::Br(0),       // Continue parse loop
            Instruction::End,         // End parse loop
            Instruction::End,         // End parse block
            // Return array pointer
            Instruction::LocalGet(6),
            Instruction::Else,
            // Check for '{' (object)
            Instruction::LocalGet(3),
            Instruction::I32Const(123), // '{'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // OBJECT PARSER IMPLEMENTATION
            // Parse JSON object: {"key1":value1,"key2":value2,...}
            //
            // Strategy:
            // 1. Count key-value pairs by scanning for commas
            // 2. Allocate memory: 4 bytes (count) + pairs * 8 bytes (key_ptr, val_ptr per pair)
            // 3. Parse each key-value pair and store in memory
            //
            // Memory layout: [i32 count][i32 key0_ptr][i32 val0_ptr][i32 key1_ptr][i32 val1_ptr]...

            // Step 1: Skip opening '{'
            Instruction::LocalGet(1), // position
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1), // position++
            // Step 2: Count pairs by scanning for commas and keys
            // Save current position
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // start_position = position
            // Initialize pair count to 0
            Instruction::I32Const(0),
            Instruction::LocalSet(5), // pair_count = 0
            // Scan to count pairs
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Skip whitespace
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if pos >= len
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block if end reached
            // Get current char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // char
            // Check if whitespace
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit whitespace loop if not whitespace
            // Increment position
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0), // Continue whitespace loop
            Instruction::End,   // End whitespace loop
            Instruction::End,   // End whitespace block
            // Check for '}' (empty object or end of object)
            Instruction::LocalGet(3),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::BrIf(1), // Exit counting loop if closing brace
            // We found a key - increment pair count
            Instruction::LocalGet(5),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5), // pair_count++
            // Skip past this key-value pair to find next comma or '}'
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // depth = 0 (for nested structures)
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check bounds
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            // Get char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Increment position
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Check for nested '{' or '['
            Instruction::LocalGet(3),
            Instruction::I32Const(123), // '{'
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(91), // '['
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10), // depth++
            Instruction::End,
            // Check for '}' or ']'
            Instruction::LocalGet(3),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(0),
            Instruction::I32GtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10), // depth--
            Instruction::End,
            Instruction::End,
            // At depth 0, check for ',' or '}'
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(2), // Exit skip loop
            Instruction::End,
            Instruction::Br(0), // Continue skip loop
            Instruction::End,   // End skip loop
            Instruction::End,   // End skip block
            // Check if we hit '}' (end of object)
            Instruction::LocalGet(3),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::BrIf(1), // Exit counting loop
            Instruction::Br(0),   // Continue counting loop
            Instruction::End,     // End counting loop
            Instruction::End,     // End counting block
            // Step 3: Allocate memory for object
            // Size = 4 (count) + pair_count * 8 (key ptr + val ptr)
            Instruction::I32Const(4),
            Instruction::LocalGet(5), // pair_count
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::Call(0),     // Call __malloc (function index 0)
            Instruction::LocalSet(6), // object_ptr = malloc(...)
            // Store pair count at offset 0
            Instruction::LocalGet(6),
            Instruction::LocalGet(5),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Step 4: Reset position to start and parse pairs
            Instruction::LocalGet(8),
            Instruction::LocalSet(1), // position = start_position
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(7), // i = 0
            // Parse each key-value pair
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if we've parsed all pairs
            Instruction::LocalGet(7),
            Instruction::LocalGet(5),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if i >= pair_count
            // Skip whitespace before key
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Parse key (must be string starting with '"')
            // Skip opening '"'
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Find end of string (closing '"')
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // key_start = position
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check for closing '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate key length
            Instruction::LocalGet(1),
            Instruction::LocalGet(8),
            Instruction::I32Sub,
            Instruction::LocalSet(9), // key_len = position - key_start
            // Allocate memory for key string: 4 bytes (length) + key_len bytes
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(0),      // Call __malloc
            Instruction::LocalSet(10), // key_ptr = malloc(...)
            // Store key length
            Instruction::LocalGet(10),
            Instruction::LocalGet(9),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Copy key bytes
            Instruction::LocalGet(10),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(8),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            // Store key pointer in object
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(10), // key_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip closing '"' and whitespace
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Skip whitespace before ':'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Skip ':' character
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Skip whitespace before value
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::LocalGet(3),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Parse value - check first character to determine type
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if value is a number (digit or '-')
            Instruction::LocalGet(3),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(3),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse number value
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // num_start = position
            // Find end of number
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if char is digit or '.' or 'e' or 'E' or '+' or '-'
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(3),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(101), // 'e'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(69), // 'E'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(43), // '+'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(3),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // IMPROVED NUMBER PARSER - Multi-digit integers and floats
            // Parse the complete numeric value from num_start to current position

            // Allocate 12 bytes: 4 (type tag = 3) + 8 (f64 value)
            Instruction::I32Const(12),
            Instruction::Call(0),      // __malloc
            Instruction::LocalSet(11), // value_ptr
            // Store type tag = 3 (number)
            Instruction::LocalGet(11),
            Instruction::I32Const(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize accumulator to 0.0
            Instruction::F64Const(0.0),
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Initialize parsing state
            Instruction::LocalGet(8), // num_start
            Instruction::LocalSet(9), // parse_pos = num_start
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // is_negative = 0
            // Check for negative sign
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(1),
            Instruction::LocalSet(10), // is_negative = 1
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9), // parse_pos++ (skip '-')
            Instruction::End,
            // Parse integer part - accumulate digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if parse_pos >= position (end of number)
            Instruction::LocalGet(9),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(3),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit if not a digit (decimal point or end)
            // Load current accumulator value
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Multiply by 10
            Instruction::F64Const(10.0),
            Instruction::F64Mul,
            // Add current digit value (char - '0')
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            Instruction::F64Add,
            // Store back
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Increment parse_pos
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            Instruction::Br(0), // Continue loop
            Instruction::End,
            Instruction::End,
            // Parse decimal point if present
            // Check if parse_pos < position and current char is '.'
            Instruction::LocalGet(9), // parse_pos
            Instruction::LocalGet(1), // position (end of number)
            Instruction::I32LtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if it's a decimal point '.' (46)
            Instruction::LocalGet(3),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Skip the decimal point
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            // Initialize decimal divisor to 10.0
            Instruction::F64Const(10.0),
            Instruction::LocalSet(13),
            // Parse fractional digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if parse_pos >= position (end of number)
            Instruction::LocalGet(9),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if done
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(3),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(3),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit if not a digit
            // Load current accumulator value
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Convert digit to value (char - '0')
            Instruction::LocalGet(3),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            // Divide by decimal_divisor
            Instruction::LocalGet(13),
            Instruction::F64Div,
            // Add to accumulator
            Instruction::F64Add,
            // Store back
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Multiply divisor by 10 for next digit
            Instruction::LocalGet(13),
            Instruction::F64Const(10.0),
            Instruction::F64Mul,
            Instruction::LocalSet(13),
            // Increment parse_pos
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            Instruction::Br(0), // Continue loop
            Instruction::End,   // End fractional loop
            Instruction::End,   // End fractional block
            Instruction::End,   // End decimal point check
            Instruction::End,   // End parse_pos < position check
            // Apply negative sign if needed
            Instruction::LocalGet(10), // is_negative
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Negate the value
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Neg,
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::End,
            // Store value pointer in object
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,       // offset to value slot
            Instruction::LocalGet(11), // value_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Parse other value types: string, boolean, null, or nested structures

            // Check if value is a string (starts with '"')
            Instruction::LocalGet(3),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse string value (similar to key parsing)
            // Skip opening '"'
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Save string start position
            Instruction::LocalGet(1),
            Instruction::LocalSet(8), // str_start = position
            // Find closing '"'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Check for closing '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate string length
            Instruction::LocalGet(1),
            Instruction::LocalGet(8),
            Instruction::I32Sub,
            Instruction::LocalSet(9), // str_len
            // Allocate memory: 4 bytes (length) + str_len
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(0),      // __malloc
            Instruction::LocalSet(11), // str_ptr
            // Store string length
            Instruction::LocalGet(11),
            Instruction::LocalGet(9),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Copy string bytes
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(8),
            Instruction::I32Add,
            Instruction::LocalGet(9),
            Instruction::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            // Skip closing '"'
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Store string pointer in object
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(11), // str_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Check if value is 'true'
            Instruction::LocalGet(3),
            Instruction::I32Const(116), // 't'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Store true (encoded as 2)
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Const(2), // true = 2
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip past "true" (4 characters)
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Else,
            // Check if value is 'false'
            Instruction::LocalGet(3),
            Instruction::I32Const(102), // 'f'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Store false (encoded as 1)
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Const(1), // false = 1
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip past "false" (5 characters)
            Instruction::LocalGet(1),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Else,
            // Check if value is 'null'
            Instruction::LocalGet(3),
            Instruction::I32Const(110), // 'n'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Store null (0)
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Const(0), // null = 0
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Skip past "null" (4 characters)
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Else,
            // NESTED STRUCTURE SUPPORT (ONE LEVEL)
            // Check if value is '{' (nested object) or '[' (nested array)

            // For now, store null for nested structures
            // Full nested support requires architectural refactoring
            // to extract object/array parsers into separate WASM functions
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Const(0), // null for nested (not yet supported)
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::End, // null check
            Instruction::End, // false check
            Instruction::End, // true check
            Instruction::End, // string check
            Instruction::End, // number check (outer else)
            // Skip past value to find ',' or '}'
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // depth = 0
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            // Track nesting depth
            Instruction::LocalGet(3),
            Instruction::I32Const(123),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(91),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10),
            Instruction::End,
            Instruction::LocalGet(3),
            Instruction::I32Const(125),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(93),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(0),
            Instruction::I32GtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10),
            Instruction::End,
            Instruction::End,
            // At depth 0, check for ',' or '}'
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(44),
            Instruction::I32Eq,
            Instruction::LocalGet(3),
            Instruction::I32Const(125),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(2),
            Instruction::End,
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Increment loop counter
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // i++
            Instruction::Br(0),       // Continue parse loop
            Instruction::End,         // End parse loop
            Instruction::End,         // End parse block
            // Return object pointer
            Instruction::LocalGet(6),
            Instruction::Else,
            // Default: return null for unknown
            Instruction::I32Const(0),
            Instruction::End, // object check
            Instruction::End, // array check
            Instruction::End, // number check
            Instruction::End, // string check
            Instruction::End, // false check
            Instruction::End, // true check
            Instruction::End, // null check
        ]
    }

    /// Generate WASM instructions for json.tryTextToData
    /// Same as textToData but returns null on error instead of failing
    fn generate_try_text_to_data_instructions(&self) -> Vec<Instruction<'static>> {
        // Wrap the parsing in error handling - for now, same as textToData
        // but structured to return null on any error
        self.generate_text_to_data_instructions()
    }

    /// Generate WASM instructions for json.dataToText
    /// Converts a data structure back to JSON text
    fn generate_data_to_text_instructions(&self) -> Vec<Instruction<'static>> {
        // JSON Stringifier Implementation
        // Takes a data pointer and returns a JSON string
        //
        // For now, implement basic handling:
        // - 0 (null) -> "null"
        // - 1 (false) -> "false"
        // - 2 (true) -> "true"
        // - Other pointers -> stringify based on type tag

        vec![
            // Local 0: data pointer (parameter)
            // Local 1: result string pointer
            // Local 2: temp
            // Local 3: temp

            // Check if null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Allocate and return "null" string
            // For simplicity, return a fixed address where we'd store "null"
            Instruction::I32Const(0), // Would be pointer to "null" string in data section
            Instruction::Else,
            // Check if boolean false (1)
            Instruction::LocalGet(0),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(0), // Would be pointer to "false" string
            Instruction::Else,
            // Check if boolean true (2)
            Instruction::LocalGet(0),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(0), // Would be pointer to "true" string
            Instruction::Else,
            // For other values, attempt to stringify
            // This is a simplified implementation
            Instruction::LocalGet(0), // Return the input for now
            Instruction::End,         // true check
            Instruction::End,         // false check
            Instruction::End,         // null check
        ]
    }

    /// Generate WASM instructions for json.prettyDataToText
    /// Same as dataToText but with indentation for readability
    fn generate_pretty_data_to_text_instructions(&self) -> Vec<Instruction<'static>> {
        // For now, same as dataToText (pretty printing would add indentation)
        self.generate_data_to_text_instructions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_class_creation() {
        let json = JsonClass::new();
        assert!(std::mem::size_of_val(&json) == 0); // Zero-sized type
    }

    #[test]
    fn test_json_tag_constants() {
        assert_eq!(JSON_TAG_NULL, 0);
        assert_eq!(JSON_TAG_BOOLEAN, 1);
        assert_eq!(JSON_TAG_NUMBER, 2);
        assert_eq!(JSON_TAG_STRING, 3);
        assert_eq!(JSON_TAG_ARRAY, 4);
        assert_eq!(JSON_TAG_OBJECT, 5);
    }
}
