//! JSON Module for Clean Language
//!
//! Pure WASM implementation of JSON parsing and stringifying.
//! No host imports required - fully portable across all WASM runtimes.
//! BOOK: json-module

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
use crate::types::WasmType;
use wasm_encoder::{Instruction, MemArg};

/// JSON Value Type Tags (stored in high bits of pointer)
/// These tags identify the type of JSON value at runtime.
/// WASM codegen embeds these as i32 literals; they are public for external inspection tools.
pub const JSON_TAG_NULL: i32 = 0;
pub const JSON_TAG_BOOLEAN: i32 = 1;
pub const JSON_TAG_NUMBER: i32 = 2;
pub const JSON_TAG_STRING: i32 = 3;
pub const JSON_TAG_ARRAY: i32 = 4;
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

        // Register spec-canonical names as aliases (spec stdlib-reference.md §json):
        //   json.encode  ≡  json.dataToText
        //   json.decode  ≡  json.textToData
        //   json.get     ≡  __json_get_field (key-based access)
        // The old names remain valid for backward compatibility.
        if let Some(idx) = codegen.get_function_index("json.dataToText") {
            codegen.add_function_alias("json.encode", idx);
        }
        if let Some(idx) = codegen.get_function_index("json.textToData") {
            codegen.add_function_alias("json.decode", idx);
        }
        // json.get(json_ptr: i32, key_ptr: i32) -> i32
        // Adapts the 2-arg language calling convention to the 3-arg internal
        // __json_get_field(any_ptr, key_content_ptr, key_len) convention.
        // The key_ptr is a Clean Language string (4-byte length prefix + content);
        // this wrapper expands it to (key_ptr+4, mem[key_ptr]) before the call.
        // Note: when a plugin bridge (e.g. frame.auth) provides json.get, its
        // wrapper registration runs after this and overwrites this entry.
        if let Some(raw_idx) = codegen.get_function_index("__json_get_field") {
            register_stdlib_function(
                codegen,
                "json.get",
                &[WasmType::I32, WasmType::I32], // (json_ptr, key_ptr)
                Some(WasmType::I32),
                vec![
                    Instruction::LocalGet(0), // any_ptr = json_ptr
                    Instruction::LocalGet(1), // key_ptr
                    Instruction::I32Const(4),
                    Instruction::I32Add, // key content start = key_ptr + 4
                    Instruction::LocalGet(1),
                    Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }), // key length = mem[key_ptr]
                    Instruction::Call(raw_idx),
                ],
            )?;
        }

        tracing::debug!("JSON module: All functions registered successfully");
        Ok(())
    }

    fn register_parse_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Phase 4 Implementation: Register helper functions and simplify main parser
        // Step 1: Get malloc index (required by all parsing functions)
        let malloc_index = codegen
            .get_function_index("__malloc")
            .expect("__malloc must be registered before JSON parsing functions");

        // Step 2: Calculate what the function indices will be
        // We need to know these in advance for mutual recursion
        // Get next available function index
        let base_idx = codegen.get_next_function_index();
        let value_idx_predicted = base_idx; // Will be registered first
        let object_idx_predicted = base_idx + 1; // Will be registered second
        let array_idx_predicted = base_idx + 2; // Will be registered third

        // Step 3: Register all three helper functions with correct indices
        // __json_parse_value - uses predicted object and array indices
        let value_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_parse_value",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, position_ptr, length
            Some(WasmType::I32),                            // returns value_ptr
            &[
                WasmType::I32, // Local 3: position (cached from position_ptr)
                WasmType::I32, // Local 4: current_character
                WasmType::I32, // Local 5: result_ptr / value_ptr
                WasmType::I32, // Local 6: start_position (for number/string parsing)
                WasmType::I32, // Local 7: value_length / parse_pos
                WasmType::I32, // Local 8: temp / is_negative
                WasmType::I32, // Local 9: temp
                WasmType::I32, // Local 10: temp
                WasmType::I32, // Local 11: temp
                WasmType::F64, // Local 12: decimal_divisor (F64)
                WasmType::F64, // Local 13: temp_f64 (for F64Store operand swapping)
                WasmType::F64, // Local 14: temp_f64_2 (additional F64 temp for array elem parsing)
            ],
            self.generate_parse_value_instructions(
                object_idx_predicted,
                array_idx_predicted,
                malloc_index,
            ),
        )?;

        // Verify our prediction was correct
        assert_eq!(
            value_idx, value_idx_predicted,
            "Function index prediction failed for __json_parse_value"
        );

        // __json_parse_object - uses actual value_idx
        let object_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_parse_object",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, position_ptr, length
            Some(WasmType::I32),                            // returns object_ptr
            &[
                WasmType::I32, // Local 3: position (cached from position_ptr)
                WasmType::I32, // Local 4: current_character
                WasmType::I32, // Local 5: pair_count
                WasmType::I32, // Local 6: object_ptr (allocated memory)
                WasmType::I32, // Local 7: loop counter i
                WasmType::I32, // Local 8: start_position / key_start / num_start / str_start
                WasmType::I32, // Local 9: key_len / str_len / parse_pos
                WasmType::I32, // Local 10: key_ptr / is_negative / temp / depth
                WasmType::I32, // Local 11: value_ptr / str_ptr
                WasmType::I32, // Local 12: temp
                WasmType::F64, // Local 13: decimal_divisor (F64)
                WasmType::F64, // Local 14: temp_f64 (for F64Store operand swapping)
            ],
            self.generate_parse_object_instructions(value_idx, malloc_index),
        )?;

        assert_eq!(
            object_idx, object_idx_predicted,
            "Function index prediction failed for __json_parse_object"
        );

        // __json_parse_array - uses actual value_idx
        let array_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_parse_array",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, position_ptr, length
            Some(WasmType::I32),                            // returns array_ptr
            &[
                WasmType::I32, // Local 3: position (cached from position_ptr)
                WasmType::I32, // Local 4: current_character
                WasmType::I32, // Local 5: element_count
                WasmType::I32, // Local 6: array_ptr (allocated memory)
                WasmType::I32, // Local 7: loop counter i
                WasmType::I32, // Local 8: start_position / num_start / str_start
                WasmType::I32, // Local 9: element_ptr / value / str_len / parse_pos
                WasmType::I32, // Local 10: depth tracker / is_negative
                WasmType::I32, // Local 11: value_ptr / str_ptr / temp
                WasmType::I32, // Local 12: temp
                WasmType::F64, // Local 13: decimal_divisor (F64)
                WasmType::F64, // Local 14: temp_f64 (for F64Store operand swapping)
            ],
            self.generate_parse_array_instructions(value_idx, malloc_index),
        )?;

        assert_eq!(
            array_idx, array_idx_predicted,
            "Function index prediction failed for __json_parse_array"
        );

        // Step 4: Register main public API functions using the simplified implementation

        // json.textToData(text: string) -> any
        // Parse JSON text into a data structure
        register_stdlib_function_with_locals(
            codegen,
            "json.textToData",
            &[WasmType::I32],    // string pointer
            Some(WasmType::I32), // returns data pointer
            &[
                WasmType::I32, // Local 1: position_ptr (allocated temp)
                WasmType::I32, // Local 2: length
                WasmType::I32, // Local 3: string_end (heap guard temp)
            ],
            self.generate_text_to_data_instructions(value_idx, malloc_index),
        )?;

        // json.tryTextToData(text: string) -> any (returns null on error)
        // Parse JSON text, returns null (0) on parse error instead of throwing
        register_stdlib_function_with_locals(
            codegen,
            "json.tryTextToData",
            &[WasmType::I32],    // string pointer
            Some(WasmType::I32), // returns data pointer (null on error)
            &[
                WasmType::I32, // Local 1: position_ptr (allocated temp)
                WasmType::I32, // Local 2: length
                WasmType::I32, // Local 3: string_end (heap guard temp)
            ],
            self.generate_try_text_to_data_instructions(value_idx, malloc_index),
        )?;

        Ok(())
    }

    fn register_stringify_operations(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Get required function indices for stringify operations.
        // string.concat is reachability-gated (Import Minimality Rule): it is
        // only registered when the program uses string concatenation or JSON.
        // If it is absent the program does not call json.dataToText or
        // json.prettyDataToText, so skip the stringify helpers entirely.
        let Some(string_concat_index) = codegen.get_function_index("string.concat") else {
            tracing::debug!(
                "JSON stringify: string.concat not registered (Import Minimality) — \
                 skipping stringify operations (json.textToData still available)"
            );
            return Ok(());
        };

        // Get required function indices for stringify operations
        let malloc_index = codegen
            .get_function_index("__malloc")
            .expect("__malloc must be registered before JSON stringify functions");

        let int_to_string_index = codegen
            .get_function_index("integer.toString")
            .or_else(|| codegen.get_function_index("int_to_string"))
            .expect("integer.toString must be registered before JSON stringify functions");

        let float_to_string_index = codegen
            .get_function_index("float_to_string")
            .expect("float_to_string must be registered before JSON stringify functions");

        // Calculate what the function indices will be for mutual recursion
        let base_idx = codegen.get_next_function_index();
        let quote_string_idx_predicted = base_idx;
        let stringify_value_idx_predicted = base_idx + 1;
        let stringify_array_idx_predicted = base_idx + 2;
        let stringify_object_idx_predicted = base_idx + 3;

        // Register helper functions for recursive stringify

        // 1. __json_quote_string(str_ptr: i32) -> i32
        let quote_string_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_quote_string",
            &[WasmType::I32],    // str_ptr
            Some(WasmType::I32), // returns quoted string
            &[
                WasmType::I32, // Local 1: orig_len
                WasmType::I32, // Local 2: result_ptr
            ],
            self.generate_quote_string_instructions(malloc_index),
        )?;

        assert_eq!(
            quote_string_idx, quote_string_idx_predicted,
            "Function index prediction failed for __json_quote_string"
        );

        // 2. __json_stringify_value(boxed_ptr: i32) -> i32
        let stringify_value_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_stringify_value",
            &[WasmType::I32],    // boxed_ptr
            Some(WasmType::I32), // returns string
            &[
                WasmType::I32, // Local 1: type_tag
                WasmType::I32, // Local 2: value/temp
                WasmType::I32, // Local 3: result_ptr
                WasmType::I32, // Local 4: temp
                WasmType::I32, // Local 5: temp2
                WasmType::F64, // Local 6: f64_value
            ],
            self.generate_stringify_value_instructions(
                malloc_index,
                int_to_string_index,
                float_to_string_index,
                quote_string_idx,
                stringify_array_idx_predicted,
                stringify_object_idx_predicted,
            ),
        )?;

        assert_eq!(
            stringify_value_idx, stringify_value_idx_predicted,
            "Function index prediction failed for __json_stringify_value"
        );

        // 3. __json_stringify_array(array_ptr: i32) -> i32
        let stringify_array_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_stringify_array",
            &[WasmType::I32],    // array_ptr
            Some(WasmType::I32), // returns string
            &[
                WasmType::I32, // Local 1: count
                WasmType::I32, // Local 2: i
                WasmType::I32, // Local 3: result
                WasmType::I32, // Local 4: elem_ptr
                WasmType::I32, // Local 5: elem_str
                WasmType::I32, // Local 6: temp
                WasmType::I32, // Local 7: boxed_ptr
            ],
            self.generate_stringify_array_instructions(
                malloc_index,
                string_concat_index,
                stringify_value_idx,
            ),
        )?;

        assert_eq!(
            stringify_array_idx, stringify_array_idx_predicted,
            "Function index prediction failed for __json_stringify_array"
        );

        // 4. __json_stringify_object(object_ptr: i32) -> i32
        let stringify_object_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_stringify_object",
            &[WasmType::I32],    // object_ptr
            Some(WasmType::I32), // returns string
            &[
                WasmType::I32, // Local 1: count
                WasmType::I32, // Local 2: i
                WasmType::I32, // Local 3: result
                WasmType::I32, // Local 4: key_ptr
                WasmType::I32, // Local 5: val_ptr
                WasmType::I32, // Local 6: key_str
                WasmType::I32, // Local 7: val_str
                WasmType::I32, // Local 8: temp
                WasmType::I32, // Local 9: boxed_ptr
            ],
            self.generate_stringify_object_instructions(
                malloc_index,
                string_concat_index,
                quote_string_idx,
                stringify_value_idx,
            ),
        )?;

        assert_eq!(
            stringify_object_idx, stringify_object_idx_predicted,
            "Function index prediction failed for __json_stringify_object"
        );

        // json.dataToText(data: any) -> string
        // Convert data structure to JSON text
        register_stdlib_function_with_locals(
            codegen,
            "json.dataToText",
            &[WasmType::I32],    // data pointer
            Some(WasmType::I32), // returns string pointer
            &[],
            self.generate_data_to_text_instructions(stringify_value_idx),
        )?;

        // json.prettyDataToText(data: any) -> string
        // Convert data structure to formatted JSON text with indentation
        // For now, same as dataToText (pretty printing to be added later)
        register_stdlib_function_with_locals(
            codegen,
            "json.prettyDataToText",
            &[WasmType::I32],    // data pointer
            Some(WasmType::I32), // returns formatted string pointer
            &[],
            self.generate_data_to_text_instructions(stringify_value_idx),
        )?;

        Ok(())
    }

    /// Register JSON access operations for bracket notation support
    /// These functions enable data["field"] and data[0] syntax
    fn register_access_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Get malloc index for boxing compact values
        let malloc_index = codegen
            .get_function_index("__malloc")
            .expect("__malloc must be registered before JSON access functions");

        // First register string comparison helper needed by field accessor
        register_stdlib_function_with_locals(
            codegen,
            "__memcmp_bytes",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // ptr1, len1, ptr2, len2
            Some(WasmType::I32), // returns i32 (0 if equal, non-zero otherwise)
            &[WasmType::I32],    // locals: i (loop counter)
            self.generate_memcmp_bytes_instructions(),
        )?;

        // Get memcmp index for field comparison
        let memcmp_index = codegen
            .get_function_index("__memcmp_bytes")
            .expect("__memcmp_bytes must be registered before __json_get_field");

        // __json_get_field(any_ptr: i32, key_ptr: i32, key_len: i32) -> i32
        // Access a field on a JSON object by string key
        // Returns pointer to field value, or null (0) if not found
        // NOTE: Handles compact boolean encoding (1=false, 2=true) by boxing them
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
                WasmType::I32, // Local 7: current_value_ptr (raw value from object)
                WasmType::I32, // Local 8: match result
                WasmType::I32, // Local 9: key_data_ptr (current key data, skipping length)
                WasmType::I32, // Local 10: boxed_ptr (for boxing compact values)
            ],
            self.generate_get_field_instructions(malloc_index, memcmp_index),
        )?;

        // __json_get_index(any_ptr: i32, index: i32) -> i32
        // Access an element on a JSON array by integer index
        // Returns pointer to element, or null (0) if out of bounds
        // NOTE: Handles compact boolean encoding (1=false, 2=true) by boxing them
        register_stdlib_function_with_locals(
            codegen,
            "__json_get_index",
            &[WasmType::I32, WasmType::I32], // any_ptr, index
            Some(WasmType::I32),             // returns any pointer
            &[
                WasmType::I32, // Local 2: count
                WasmType::I32, // Local 3: raw_value (for boxing check)
                WasmType::I32, // Local 4: boxed_ptr (for boxing compact values)
            ],
            self.generate_get_index_instructions(malloc_index),
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
            Instruction::If(wasm_encoder::BlockType::Empty),
            // All bytes matched - return 0
            Instruction::I32Const(0),
            Instruction::Br(2), // Exit block with value
            Instruction::End,
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
    /// CRITICAL: Handles compact boolean encoding (1=false, 2=true) by boxing them
    fn generate_get_field_instructions(
        &self,
        malloc_index: u32,
        memcmp_index: u32,
    ) -> Vec<Instruction<'static>> {
        // Parameters:
        // Local 0: object_ptr (i32) - pointer to JSON object
        // Local 1: key_ptr (i32) - pointer to key string CONTENT (raw bytes, already past length prefix)
        //          NOTE: MIR codegen uses load_string_argument_for_print which skips the 4-byte length prefix
        // Local 2: key_len (i32) - length of key string
        //
        // Working Locals:
        // Local 3: count (i32) - number of key-value pairs in object
        // Local 4: i (i32) - loop counter
        // Local 5: current_key_ptr (i32) - pointer to current key being checked
        // Local 6: current_key_len (i32) - length of current key
        // Local 7: current_value_ptr (i32) - raw value from object (may be compact encoded)
        // Local 8: match_result (i32) - result from memcmp (0 = match)
        // Local 9: key_data_ptr (i32) - pointer to current key data (skipping length prefix)
        // Local 10: boxed_ptr (i32) - pointer to boxed value (for compact booleans)
        //
        // Object Memory Layout: [i32 count][i32 key0_ptr][i32 val0_ptr][i32 key1_ptr][i32 val1_ptr]...
        // String Memory Layout: [i32 length][bytes...]
        //
        // Compact Encoding (stored directly in object value slots):
        // 0 = null (returned as-is)
        // 1 = false (must be boxed to [tag=2][value=0][padding=0])
        // 2 = true (must be boxed to [tag=2][value=1][padding=0])
        // >2 = pointer to boxed value (returned as-is)

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
            Instruction::If(wasm_encoder::BlockType::Empty),
            // No match found - return null (0)
            Instruction::I32Const(0),
            Instruction::Br(2), // Exit block with value
            Instruction::End,
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
            // Compare keys using full byte-by-byte comparison
            // Call __memcmp_bytes(key_data_ptr, current_key_len, key_ptr, key_len)
            // Returns 0 if equal, non-zero if different
            Instruction::LocalGet(9), // key_data_ptr (stored key bytes, after length prefix)
            Instruction::LocalGet(6), // current_key_len
            Instruction::LocalGet(1), // key_ptr (search key bytes, already past length prefix)
            Instruction::LocalGet(2), // key_len
            Instruction::Call(memcmp_index),
            Instruction::LocalSet(8), // match_result (0 = match, non-zero = no match)
            // Check if keys matched (match_result == 0)
            Instruction::LocalGet(8), // match_result
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Keys matched! Load the corresponding value
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
            Instruction::LocalSet(7), // Store raw value in current_value_ptr
            // NOTE: Check for compact boolean encoding and box if needed
            // Compact encoding: 0=null, 1=false, 2=true
            // Check if value == 1 (compact false)
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Value is compact false (1) - allocate and box as boolean
            Instruction::I32Const(12), // 12 bytes for boxed boolean
            Instruction::Call(malloc_index),
            Instruction::LocalSet(10), // Store in boxed_ptr
            // Store tag=2 (Boolean) at offset 0
            Instruction::LocalGet(10),
            Instruction::I32Const(2), // AnyTypeTag::Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value=0 (false) at offset 4
            Instruction::LocalGet(10),
            Instruction::I32Const(0), // false = 0
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store padding=0 at offset 8
            Instruction::LocalGet(10),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed boolean pointer
            Instruction::LocalGet(10),
            Instruction::Else,
            // Check if value == 2 (compact true)
            Instruction::LocalGet(7),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Value is compact true (2) - allocate and box as boolean
            Instruction::I32Const(12), // 12 bytes for boxed boolean
            Instruction::Call(malloc_index),
            Instruction::LocalSet(10), // Store in boxed_ptr
            // Store tag=2 (Boolean) at offset 0
            Instruction::LocalGet(10),
            Instruction::I32Const(2), // AnyTypeTag::Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value=1 (true) at offset 4
            Instruction::LocalGet(10),
            Instruction::I32Const(1), // true = 1
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store padding=0 at offset 8
            Instruction::LocalGet(10),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed boolean pointer
            Instruction::LocalGet(10),
            Instruction::Else,
            // Value is not compact encoded (0=null or >2=pointer)
            // Return as-is
            Instruction::LocalGet(7),
            Instruction::End, // End inner if (value == 2)
            Instruction::End, // End outer if (value == 1)
            // Now we have the (possibly boxed) value on stack
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
    /// CRITICAL: Handles compact boolean encoding (1=false, 2=true) by boxing them
    fn generate_get_index_instructions(&self, malloc_index: u32) -> Vec<Instruction<'static>> {
        // Parameters:
        // Local 0: array_ptr (i32) - pointer to JSON array
        // Local 1: index (i32) - array index to access
        //
        // Working Locals:
        // Local 2: count (i32) - array length
        // Local 3: raw_value (i32) - raw value from array (may be compact encoded)
        // Local 4: boxed_ptr (i32) - pointer to boxed value (for compact booleans)
        //
        // Array Memory Layout: [i32 count][i32 elem0_ptr][i32 elem1_ptr][i32 elem2_ptr]...
        //
        // Compact Encoding (stored directly in array element slots):
        // 0 = null (returned as-is)
        // 1 = false (must be boxed to [tag=2][value=0][padding=0])
        // 2 = true (must be boxed to [tag=2][value=1][padding=0])
        // >2 = pointer to boxed value (returned as-is)
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
            // Valid index - load element
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
            Instruction::LocalSet(3), // Store raw value
            // NOTE: Check for compact boolean encoding and box if needed
            // Compact encoding: 0=null, 1=false, 2=true
            // Check if value == 1 (compact false)
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Value is compact false (1) - allocate and box as boolean
            Instruction::I32Const(12), // 12 bytes for boxed boolean
            Instruction::Call(malloc_index),
            Instruction::LocalSet(4), // Store in boxed_ptr
            // Store tag=2 (Boolean) at offset 0
            Instruction::LocalGet(4),
            Instruction::I32Const(2), // AnyTypeTag::Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value=0 (false) at offset 4
            Instruction::LocalGet(4),
            Instruction::I32Const(0), // false = 0
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store padding=0 at offset 8
            Instruction::LocalGet(4),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed boolean pointer
            Instruction::LocalGet(4),
            Instruction::Else,
            // Check if value == 2 (compact true)
            Instruction::LocalGet(3),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Value is compact true (2) - allocate and box as boolean
            Instruction::I32Const(12), // 12 bytes for boxed boolean
            Instruction::Call(malloc_index),
            Instruction::LocalSet(4), // Store in boxed_ptr
            // Store tag=2 (Boolean) at offset 0
            Instruction::LocalGet(4),
            Instruction::I32Const(2), // AnyTypeTag::Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value=1 (true) at offset 4
            Instruction::LocalGet(4),
            Instruction::I32Const(1), // true = 1
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store padding=0 at offset 8
            Instruction::LocalGet(4),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed boolean pointer
            Instruction::LocalGet(4),
            Instruction::Else,
            // Value is not compact encoded (0=null or >2=pointer)
            // Return as-is
            Instruction::LocalGet(3),
            Instruction::End, // End inner if (value == 2)
            Instruction::End, // End outer if (value == 1)
            // Now we have the (possibly boxed) value on stack - return it
            Instruction::Else,
            // Invalid index (negative or >= count) - return null
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM instructions for json.textToData
    /// Parses a JSON string and returns a pointer to the parsed data structure
    /// PHASE 4 IMPLEMENTATION: Simplified main parser using helper functions
    fn generate_text_to_data_instructions(
        &self,
        parse_value_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        // HEAP_PTR_GLOBAL = 0 (from native_stdlib/mod.rs)
        const HEAP_PTR_GLOBAL: u32 = 0;

        vec![
            // Local variable declarations:
            // Local 0: string_ptr (parameter)
            // Local 1: position_ptr (allocated temp for tracking parse position)
            // Local 2: length (string length)
            // Local 3: string_end (temp for heap guard)

            // Step 1: Get string length (needed for heap guard before any malloc)
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // length
            // Step 2: HEAP GUARD - Ensure __heap_ptr is past the input string.
            // When a host function (e.g. _db_query) writes a string into WASM memory
            // without going through WASM's __malloc, __heap_ptr may still point BELOW
            // the string. Subsequent WASM-side malloc calls would then return addresses
            // that overlap with the string, corrupting it during parsing.
            //
            // Fix: compute string_end = (string_ptr + 4 + length + 7) & ~7
            // If __heap_ptr < string_end, advance it.
            Instruction::LocalGet(0), // string_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(2), // length
            Instruction::I32Add,
            Instruction::I32Const(7),
            Instruction::I32Add,
            Instruction::I32Const(-8), // 0xFFFFFFF8 = ~7
            Instruction::I32And,       // aligned string_end
            Instruction::LocalSet(3),  // store in temp local
            // Compare: if __heap_ptr < string_end, advance it
            Instruction::GlobalGet(HEAP_PTR_GLOBAL),
            Instruction::LocalGet(3), // string_end
            Instruction::I32LtU,      // __heap_ptr < string_end?
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),                // string_end
            Instruction::GlobalSet(HEAP_PTR_GLOBAL), // advance heap past string
            Instruction::End,
            // Step 3: Allocate 4 bytes for position storage (now safe from overlap)
            Instruction::I32Const(4),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(1), // position_ptr
            // Step 4: Initialize position to 0
            Instruction::LocalGet(1), // position_ptr
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Step 5: Call value parser
            // This handles all JSON types: null, boolean, number, string, array, object
            Instruction::LocalGet(0), // string_ptr
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(2), // length
            Instruction::Call(parse_value_index),
            // Returns value_ptr (position updated at position_ptr by the parser)
        ]
    }

    /// Generate WASM instructions for json.tryTextToData
    /// Same as textToData but returns null on error instead of failing
    /// PHASE 4 IMPLEMENTATION: Simplified using helper functions
    fn generate_try_text_to_data_instructions(
        &self,
        parse_value_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        // For now, same as textToData - error handling to be added in future phase
        // The helper functions already handle most edge cases gracefully
        self.generate_text_to_data_instructions(parse_value_index, malloc_index)
    }

    /// Generate WASM instructions for json.dataToText
    /// Converts a boxed JSON value back to JSON text string
    ///
    /// Memory layout of boxed values (12 bytes each):
    /// - Null (tag 0):    [tag=0, 0, 0]
    /// - Integer (tag 1): [tag=1, i32_value, 0]
    /// - Boolean (tag 2): [tag=2, bool (0/1), 0]
    /// - Number (tag 3):  [tag=3, f64_lo, f64_hi] (8 bytes for f64)
    /// - String (tag 4):  [tag=4, string_ptr, 0]
    /// - List (tag 5):    [tag=5, array_ptr, 0]
    /// - Object (tag 6):  [tag=6, object_ptr, 0]
    ///
    /// Parameters:
    /// - Local 0: data pointer (parameter)
    /// - Local 1: type_tag
    /// - Local 2: value / temp
    /// - Local 3: result_ptr
    /// - Local 4: temp
    /// - Local 5: string_ptr
    /// - Local 6: f64_value
    fn generate_data_to_text_instructions(
        &self,
        stringify_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // Simply call __json_stringify_value with the boxed_ptr parameter
            Instruction::LocalGet(0),
            Instruction::Call(stringify_value_idx),
        ]
    }

    fn generate_quote_string_instructions(&self, malloc_index: u32) -> Vec<Instruction<'static>> {
        vec![
            // Read original length from str_ptr
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // orig_len
            // Allocate orig_len + 2 + 4 bytes (for quotes and length prefix)
            Instruction::LocalGet(1),
            Instruction::I32Const(6), // 2 quotes + 4 length prefix
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(2), // result_ptr
            // Store new length (orig_len + 2)
            Instruction::LocalGet(2),
            Instruction::LocalGet(1),
            Instruction::I32Const(2),
            Instruction::I32Add,
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store opening quote at offset 4
            Instruction::LocalGet(2),
            Instruction::I32Const(34), // '"'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // Copy content from str_ptr+4 to result_ptr+5
            Instruction::LocalGet(2),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            // Store closing quote at offset 4 + 1 + orig_len
            Instruction::LocalGet(2),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Const(34), // '"'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            // Return result_ptr
            Instruction::LocalGet(2),
        ]
    }

    fn generate_stringify_value_instructions(
        &self,
        malloc_index: u32,
        int_to_string_index: u32,
        float_to_string_index: u32,
        quote_string_idx: u32,
        stringify_array_idx: u32,
        stringify_object_idx: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // Check if boxed_ptr is null (0)
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Return "null" string
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(0x6C6C756E), // "null"
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::Else,
            // Read type tag
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // type_tag
            // Tag 0: Null
            Instruction::LocalGet(1),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(0x6C6C756E),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::Else,
            // Tag 1: Integer
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(int_to_string_index),
            Instruction::Else,
            // Tag 2: Boolean
            Instruction::LocalGet(1),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // true
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(0x65757274), // "true"
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::Else,
            // false
            Instruction::I32Const(9),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3),
            Instruction::I32Const(5),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(0x736C6166), // "fals"
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(101), // 'e'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 8,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::End,
            Instruction::Else,
            // Tag 3: Number
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 4,
                align: 3,
                memory_index: 0,
            }),
            Instruction::Call(float_to_string_index),
            Instruction::Else,
            // Tag 4: String
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(quote_string_idx),
            Instruction::Else,
            // Tag 5: List/Array
            Instruction::LocalGet(1),
            Instruction::I32Const(5),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(stringify_array_idx),
            Instruction::Else,
            // Tag 6: Object
            Instruction::LocalGet(1),
            Instruction::I32Const(6),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(stringify_object_idx),
            Instruction::Else,
            // Unknown type - return "null"
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(0x6C6C756E),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::End, // end tag 6
            Instruction::End, // end tag 5
            Instruction::End, // end tag 4
            Instruction::End, // end tag 3
            Instruction::End, // end tag 2
            Instruction::End, // end tag 1
            Instruction::End, // end tag 0
            Instruction::End, // end null check
        ]
    }

    fn generate_stringify_array_instructions(
        &self,
        malloc_index: u32,
        string_concat_index: u32,
        stringify_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // Read count from array_ptr[0]
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // count
            // Check if count == 0
            Instruction::LocalGet(1),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Return "[]"
            Instruction::I32Const(6),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(6),
            Instruction::I32Const(2),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::I32Const(91), // '['
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::I32Const(93), // ']'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::Else,
            // Create "["
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3), // result
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(91), // '['
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(2), // i = 0
            // Loop over elements
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if i >= count
            Instruction::LocalGet(2),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1), // break out of loop
            // If i > 0, append ","
            Instruction::LocalGet(2),
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Create ","
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(6),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::I32Const(44), // ','
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // result = string.concat(result, ",")
            Instruction::LocalGet(3),
            Instruction::LocalGet(6),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(3),
            Instruction::End,
            // Load elem_ptr from array_ptr + 4 + i*4
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32Const(2),
            Instruction::I32Shl, // i * 4
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // elem_ptr
            // Handle compact booleans: if elem_ptr == 1 (false) or == 2 (true), box them
            Instruction::LocalGet(4),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Box false
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(7), // boxed_ptr
            Instruction::I32Const(2), // tag = Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(0), // false
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::LocalSet(4), // update elem_ptr
            Instruction::Else,
            Instruction::LocalGet(4),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Box true
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(7), // boxed_ptr
            Instruction::I32Const(2), // tag = Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(1), // true
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::LocalSet(4), // update elem_ptr
            Instruction::End,
            Instruction::End,
            // elem_str = stringify_value(elem_ptr)
            Instruction::LocalGet(4),
            Instruction::Call(stringify_value_idx),
            Instruction::LocalSet(5), // elem_str
            // result = string.concat(result, elem_str)
            Instruction::LocalGet(3),
            Instruction::LocalGet(5),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(3),
            // i++
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(2),
            Instruction::Br(0), // continue loop
            Instruction::End,   // end loop
            Instruction::End,   // end block
            // Append "]"
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(6),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::I32Const(93), // ']'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // result = string.concat(result, "]")
            Instruction::LocalGet(3),
            Instruction::LocalGet(6),
            Instruction::Call(string_concat_index),
            Instruction::End, // end count == 0 check
        ]
    }

    fn generate_stringify_object_instructions(
        &self,
        malloc_index: u32,
        string_concat_index: u32,
        quote_string_idx: u32,
        stringify_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // Read count from object_ptr[0]
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // count
            // Check if count == 0
            Instruction::LocalGet(1),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Return "{}"
            Instruction::I32Const(6),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(8),
            Instruction::I32Const(2),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(8),
            Instruction::I32Const(123), // '{'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(8),
            Instruction::I32Const(125), // '}'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(8),
            Instruction::Else,
            // Create "{"
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(3), // result
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(123), // '{'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(2), // i = 0
            // Loop over key-value pairs
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if i >= count
            Instruction::LocalGet(2),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1), // break out of loop
            // If i > 0, append ","
            Instruction::LocalGet(2),
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Create ","
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(8),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(8),
            Instruction::I32Const(44), // ','
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // result = string.concat(result, ",")
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(3),
            Instruction::End,
            // Load key_ptr from object_ptr + 4 + i*8
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32Const(3),
            Instruction::I32Shl, // i * 8
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // key_ptr
            // key_str = quote_string(key_ptr)
            Instruction::LocalGet(4),
            Instruction::Call(quote_string_idx),
            Instruction::LocalSet(6), // key_str
            // result = string.concat(result, key_str)
            Instruction::LocalGet(3),
            Instruction::LocalGet(6),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(3),
            // Append ":"
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(8),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(8),
            Instruction::I32Const(58), // ':'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // result = string.concat(result, ":")
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(3),
            // Load val_ptr from object_ptr + 8 + i*8
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32Const(3),
            Instruction::I32Shl, // i * 8
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(5), // val_ptr
            // Handle compact booleans: if val_ptr == 1 (false) or == 2 (true), box them
            Instruction::LocalGet(5),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Box false
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(9), // boxed_ptr
            Instruction::I32Const(2), // tag = Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(0), // false
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::LocalSet(5), // update val_ptr
            Instruction::Else,
            Instruction::LocalGet(5),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Box true
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(9), // boxed_ptr
            Instruction::I32Const(2), // tag = Boolean
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(1), // true
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::LocalSet(5), // update val_ptr
            Instruction::End,
            Instruction::End,
            // val_str = stringify_value(val_ptr)
            Instruction::LocalGet(5),
            Instruction::Call(stringify_value_idx),
            Instruction::LocalSet(7), // val_str
            // result = string.concat(result, val_str)
            Instruction::LocalGet(3),
            Instruction::LocalGet(7),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(3),
            // i++
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(2),
            Instruction::Br(0), // continue loop
            Instruction::End,   // end loop
            Instruction::End,   // end block
            // Append "}"
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(8),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(8),
            Instruction::I32Const(125), // '}'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // result = string.concat(result, "}")
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_index),
            Instruction::End, // end count == 0 check
        ]
    }

    // ====================================================================================
    // PHASE 1-3: RECURSIVE JSON PARSER HELPERS
    // ====================================================================================
    // These helper functions enable parsing of nested JSON structures (objects and arrays)
    // by extracting the inline parsing logic into separate WASM functions that can
    // call each other recursively.
    //
    // Function signatures:
    // - __json_parse_value(string_ptr: i32, position_ptr: i32, length: i32) -> i32
    // - __json_parse_object(string_ptr: i32, position_ptr: i32, length: i32) -> i32
    // - __json_parse_array(string_ptr: i32, position_ptr: i32, length: i32) -> i32
    //
    // Position pointer pattern:
    // All functions use position_ptr as a memory location (not a value) to track
    // the current parsing position. Functions read the position at entry, update it
    // during parsing, and write it back before returning. This enables position
    // tracking across recursive calls.
    // ====================================================================================

    /// Generate WASM instructions for __json_parse_value
    /// Dispatches to appropriate parser based on value type
    ///
    /// Parameters:
    /// - string_ptr (i32): Pointer to JSON string
    /// - position_ptr (i32): Memory location containing current position
    /// - length (i32): String length
    ///
    /// Returns:
    /// - i32: Pointer to parsed value (or 0 for null)
    fn generate_parse_value_instructions(
        &self,
        parse_object_index: u32,
        parse_array_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // Local variable declarations:
            // Local 0: string_ptr (parameter)
            // Local 1: position_ptr (parameter - memory location)
            // Local 2: length (parameter)
            // Local 3: position (cached from position_ptr)
            // Local 4: current_character
            // Local 5: result_ptr / value_ptr
            // Local 6: start_position (for number/string parsing)
            // Local 7: value_length / parse_pos
            // Local 8: temp / is_negative
            // Local 9: temp
            // Local 10: temp
            // Local 11: temp
            // Local 12: decimal_divisor (F64)

            // Entry: Read position from memory
            Instruction::LocalGet(1), // position_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // position (cached)
            // Skip whitespace (space=32, tab=9, newline=10, return=13)
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check bounds
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if position >= length
            // Load character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // current_character
            // Check if whitespace
            Instruction::LocalGet(4),
            Instruction::I32Const(32), // space
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9), // tab
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10), // newline
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13), // return
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Increment position
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(1), // Continue loop
            Instruction::End,
            Instruction::Br(1), // Exit loop (non-whitespace found)
            Instruction::End,
            Instruction::End,
            // Read first non-whitespace character (already in Local 4)
            // We already have it from the whitespace skip loop

            // Dispatch based on character
            // Check for '{' (123) - object
            Instruction::LocalGet(4),
            Instruction::I32Const(123),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Write position back before call
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Call parse_object
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_object_index),
            // NOTE: Box the object result with AnyTypeTag::Object (6)
            // Save raw object pointer to local 5
            Instruction::LocalSet(5),
            // Allocate 12 bytes for boxed any
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(9), // boxed_ptr in local 9
            // Store type tag 6 (Object) at offset 0
            Instruction::LocalGet(9),
            Instruction::I32Const(6), // AnyTypeTag::Object = 6
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store object pointer at offset 4
            Instruction::LocalGet(9),
            Instruction::LocalGet(5), // raw object pointer
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 8
            Instruction::LocalGet(9),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed object pointer
            Instruction::LocalGet(9),
            Instruction::Else,
            // Check for '[' (91) - array
            Instruction::LocalGet(4),
            Instruction::I32Const(91),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Write position back before call
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Call parse_array
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_array_index),
            // NOTE: Box the array result with AnyTypeTag::List (5)
            // Save raw array pointer to local 5
            Instruction::LocalSet(5),
            // Allocate 12 bytes for boxed any
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(9), // boxed_ptr in local 9
            // Store type tag 5 (List) at offset 0
            Instruction::LocalGet(9),
            Instruction::I32Const(5), // AnyTypeTag::List = 5
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store array pointer at offset 4
            Instruction::LocalGet(9),
            Instruction::LocalGet(5), // raw array pointer
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 8
            Instruction::LocalGet(9),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed array pointer
            Instruction::LocalGet(9),
            Instruction::Else,
            // Check for '"' (34) - string
            Instruction::LocalGet(4),
            Instruction::I32Const(34),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Skip opening '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Save string start position
            Instruction::LocalGet(3),
            Instruction::LocalSet(6), // str_start = position
            // Find closing '"'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit loop if position >= length
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check for closing '"'
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate string length
            Instruction::LocalGet(3),
            Instruction::LocalGet(6),
            Instruction::I32Sub,
            Instruction::LocalSet(7), // str_len
            // Allocate memory: 4 bytes (length) + str_len
            Instruction::I32Const(4),
            Instruction::LocalGet(7),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5), // str_ptr
            // Store string length
            Instruction::LocalGet(5),
            Instruction::LocalGet(7),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Copy string bytes
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(6),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            // Skip closing '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Write position back
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Now box the string: allocate 12 bytes for boxed any
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(8), // boxed_ptr in local 8
            // Store type tag 4 (String) at offset 0
            Instruction::LocalGet(8),
            Instruction::I32Const(4), // String tag
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store string pointer at offset 4
            Instruction::LocalGet(8),
            Instruction::LocalGet(5), // str_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 8
            Instruction::LocalGet(8),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed string pointer
            Instruction::LocalGet(8),
            Instruction::Else,
            // Check for digit (48-57) or '-' (45) - number
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Parse number value
            Instruction::LocalGet(3),
            Instruction::LocalSet(6), // num_start = position
            // Find end of number
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if char is digit or '.' or 'e' or 'E' or '+' or '-'
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(4),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(101), // 'e'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(69), // 'E'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(43), // '+'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Allocate 12 bytes: 4 (type tag = 3) + 8 (f64 value)
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5), // value_ptr
            // Store type tag = 3 (number)
            Instruction::LocalGet(5),
            Instruction::I32Const(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize accumulator to 0.0
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Const(0.0),
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Initialize parsing state
            Instruction::LocalGet(6), // num_start
            Instruction::LocalSet(7), // parse_pos = num_start
            Instruction::I32Const(0),
            Instruction::LocalSet(8), // is_negative = 0
            // Check for negative sign
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7),
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
            Instruction::LocalSet(8), // is_negative = 1
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // parse_pos++ (skip '-')
            Instruction::End,
            // Parse integer part - accumulate digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if parse_pos >= position (end of number)
            Instruction::LocalGet(7),
            Instruction::LocalGet(3),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit if not a digit (decimal point or end)
            // Load current accumulator value
            Instruction::LocalGet(5),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            Instruction::F64Add,
            // Save to temp local 13 (F64)
            Instruction::LocalSet(13),
            // Store back (address first, then value for F64Store)
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(13),
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Increment parse_pos
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            Instruction::Br(0), // Continue loop
            Instruction::End,
            Instruction::End,
            // Parse decimal point if present
            // Check if parse_pos < position and current char is '.'
            Instruction::LocalGet(7), // parse_pos
            Instruction::LocalGet(3), // position (end of number)
            Instruction::I32LtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if it's a decimal point '.' (46)
            Instruction::LocalGet(4),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Skip the decimal point
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            // Initialize decimal divisor to 10.0
            Instruction::F64Const(10.0),
            Instruction::LocalSet(12),
            // Parse fractional digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if parse_pos >= position (end of number)
            Instruction::LocalGet(7),
            Instruction::LocalGet(3),
            Instruction::I32GeU,
            Instruction::BrIf(1), // Exit if done
            // Get current character
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit if not a digit
            // Load current accumulator value
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Convert digit to value (char - '0')
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            // Divide by decimal_divisor
            Instruction::LocalGet(12),
            Instruction::F64Div,
            // Add to accumulator
            Instruction::F64Add,
            // Save result to temp local 13 (F64)
            Instruction::LocalSet(13),
            // Store back (address first, then value for F64Store)
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(13),
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            // Multiply divisor by 10 for next digit
            Instruction::LocalGet(12),
            Instruction::F64Const(10.0),
            Instruction::F64Mul,
            Instruction::LocalSet(12),
            // Increment parse_pos
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            Instruction::Br(0), // Continue loop
            Instruction::End,   // End fractional loop
            Instruction::End,   // End fractional block
            Instruction::End,   // End decimal point check
            Instruction::End,   // End parse_pos < position check
            // Apply negative sign if needed
            Instruction::LocalGet(8), // is_negative
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Negate the value
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Neg,
            // Save negated value to temp local 13
            Instruction::LocalSet(13),
            // Store back (address first, then value)
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(13),
            Instruction::F64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::End,
            // Write position back
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return number pointer
            Instruction::LocalGet(5),
            Instruction::Else,
            // Check for 't' (116) - true
            Instruction::LocalGet(4),
            Instruction::I32Const(116),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Skip past "true" (4 characters)
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Write position back
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Allocate 12 bytes for boxed boolean (true)
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5),
            // Store type tag 2 (Boolean) at offset 0
            Instruction::LocalGet(5),
            Instruction::I32Const(2), // Boolean tag
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value 1 (true) at offset 4
            Instruction::LocalGet(5),
            Instruction::I32Const(1), // true = 1
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 8 (padding)
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed boolean pointer
            Instruction::LocalGet(5),
            Instruction::Else,
            // Check for 'f' (102) - false
            Instruction::LocalGet(4),
            Instruction::I32Const(102),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Skip past "false" (5 characters)
            Instruction::LocalGet(3),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Write position back
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Allocate 12 bytes for boxed boolean (false)
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5),
            // Store type tag 2 (Boolean) at offset 0
            Instruction::LocalGet(5),
            Instruction::I32Const(2), // Boolean tag
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value 0 (false) at offset 4
            Instruction::LocalGet(5),
            Instruction::I32Const(0), // false = 0
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 8 (padding)
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed boolean pointer
            Instruction::LocalGet(5),
            Instruction::Else,
            // Check for 'n' (110) - null
            Instruction::LocalGet(4),
            Instruction::I32Const(110),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Skip past "null" (4 characters)
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Write position back
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Allocate 12 bytes for boxed null
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5),
            // Store type tag 0 (Null) at offset 0
            Instruction::LocalGet(5),
            Instruction::I32Const(0), // Null tag
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 4
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store 0 at offset 8
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return boxed null pointer
            Instruction::LocalGet(5),
            Instruction::Else,
            // Unknown value - return boxed null
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5),
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(5),
            Instruction::End, // End 'n' check
            Instruction::End, // End 'f' check
            Instruction::End, // End 't' check
            Instruction::End, // End number check
            Instruction::End, // End string check
            Instruction::End, // End array check
            Instruction::End, // End object check
        ]
    }

    /// Generate WASM instructions for __json_parse_object
    /// Parses JSON object: {"key1":value1,"key2":value2,...}
    ///
    /// Parameters:
    /// - string_ptr (i32): Pointer to JSON string
    /// - position_ptr (i32): Memory location containing current position
    /// - length (i32): String length
    ///
    /// Returns:
    /// - i32: Pointer to object or 0 on error
    fn generate_parse_object_instructions(
        &self,
        parse_value_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // Local variable declarations:
            // Local 0: string_ptr (parameter)
            // Local 1: position_ptr (parameter - memory location)
            // Local 2: length (parameter)
            // Local 3: position (cached from position_ptr)
            // Local 4: current_character
            // Local 5: pair_count
            // Local 6: object_ptr (allocated memory)
            // Local 7: loop counter i
            // Local 8: start_position / key_start / num_start / str_start
            // Local 9: key_len / str_len / parse_pos
            // Local 10: key_ptr / is_negative / temp / depth
            // Local 11: value_ptr / str_ptr
            // Local 12: temp
            // Local 13: temp
            // Local 14: decimal_divisor (F64)

            // Read position from memory into cache
            Instruction::LocalGet(1), // position_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // position (cached)
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
            Instruction::LocalGet(3), // position
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3), // position++
            // Step 2: Count pairs by scanning for commas and keys
            // Save current position
            Instruction::LocalGet(3),
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
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block if end reached
            // Get current char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // char
            // Check if whitespace
            Instruction::LocalGet(4),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit whitespace loop if not whitespace
            // Increment position
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0), // Continue whitespace loop
            Instruction::End,   // End whitespace loop
            Instruction::End,   // End whitespace block
            // Check for '}' (empty object or end of object)
            Instruction::LocalGet(4),
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
            Instruction::I32Const(0),
            Instruction::LocalSet(12), // in_string = 0 (for tracking quoted strings)
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check bounds
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            // Get char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Increment position
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Check for quote character to toggle in_string
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Toggle in_string: in_string = 1 - in_string
            Instruction::I32Const(1),
            Instruction::LocalGet(12),
            Instruction::I32Sub,
            Instruction::LocalSet(12),
            Instruction::End,
            // Only check for structure characters when NOT inside a string
            Instruction::LocalGet(12),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Check for nested '{' or '['
            Instruction::LocalGet(4),
            Instruction::I32Const(123), // '{'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(91), // '['
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10), // depth++
            Instruction::End,
            // Check for '}' or ']' at depth 0 FIRST (before decrementing)
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Is this a closing brace at depth 0? If so, exit
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // depth is 0, this is an outer brace - check for ',' or '}' exit
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(4), // Exit skip block (adjusted: 2 Ifs + in_string If + Loop = Block)
            Instruction::Else,
            // depth > 0, this is an inner closing brace - decrement depth
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10), // depth--
            Instruction::End,
            Instruction::End,
            // At depth 0, check for ','
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::BrIf(3), // Exit skip loop for comma (adjusted for in_string If)
            Instruction::End,
            Instruction::End,   // End in_string check
            Instruction::Br(0), // Continue skip loop
            Instruction::End,   // End skip loop
            Instruction::End,   // End skip block
            // Check if we hit '}' (end of object)
            Instruction::LocalGet(4),
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
            Instruction::Call(malloc_index), // Call __malloc
            Instruction::LocalSet(6),        // object_ptr = malloc(...)
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
            Instruction::LocalSet(3), // position = start_position
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
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // NOTE: Skip comma separator between key-value pairs
            // After parsing a value, we might be at ',' before the next key
            // Check if current char is ',' and skip it
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Skip the comma
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Skip whitespace after comma
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::I32Const(32), // space
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9), // tab
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10), // newline
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13), // return
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit if not whitespace
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::End, // End comma check
            // Parse key (must be string starting with '"')
            // Skip opening '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Find end of string (closing '"')
            Instruction::LocalGet(3),
            Instruction::LocalSet(8), // key_start = position
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check for closing '"'
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate key length
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::I32Sub,
            Instruction::LocalSet(9), // key_len = position - key_start
            // Allocate memory for key string: 4 bytes (length) + key_len bytes
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(malloc_index), // Call __malloc
            Instruction::LocalSet(10),       // key_ptr = malloc(...)
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
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Skip whitespace before ':'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Skip ':' character
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Skip whitespace before value
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Parse value - check first character to determine type
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if value is a number (digit or '-')
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse number value
            Instruction::LocalGet(3),
            Instruction::LocalSet(8), // num_start = position
            // Find end of number
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if char is digit or '.' or 'e' or 'E' or '+' or '-'
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(4),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(101), // 'e'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(69), // 'E'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(43), // '+'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // IMPROVED NUMBER PARSER - Multi-digit integers and floats
            // Parse the complete numeric value from num_start to current position

            // Allocate 12 bytes: 4 (type tag = 3) + 8 (f64 value)
            Instruction::I32Const(12),
            Instruction::Call(malloc_index), // __malloc
            Instruction::LocalSet(11),       // value_ptr
            // Store type tag = 3 (number)
            Instruction::LocalGet(11),
            Instruction::I32Const(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize accumulator to 0.0 (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Const(0.0),
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
            Instruction::LocalGet(3),
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
            Instruction::LocalSet(4),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            Instruction::F64Add,
            // Save to temp local 14 (F64)
            Instruction::LocalSet(14),
            // Store back (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(14),
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
            Instruction::LocalGet(3), // position (end of number)
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
            Instruction::LocalSet(4),
            // Check if it's a decimal point '.' (46)
            Instruction::LocalGet(4),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Skip the decimal point
            Instruction::LocalGet(9),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            // Initialize decimal divisor to 10.0 (use local 13, not 14 - 14 is for temp storage)
            Instruction::F64Const(10.0),
            Instruction::LocalSet(13),
            // Parse fractional digits
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check if parse_pos >= position (end of number)
            Instruction::LocalGet(9),
            Instruction::LocalGet(3),
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
            Instruction::LocalSet(4),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            // Divide by decimal_divisor (local 13)
            Instruction::LocalGet(13),
            Instruction::F64Div,
            // Add to accumulator
            Instruction::F64Add,
            // Save result to temp local 14 (F64)
            Instruction::LocalSet(14),
            // Store back (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(14),
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
            // Save negated value to temp local 14
            Instruction::LocalSet(14),
            // Store back (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(14),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse string value (similar to key parsing)
            // Skip opening '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Save string start position
            Instruction::LocalGet(3),
            Instruction::LocalSet(8), // str_start = position
            // Find closing '"'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check for closing '"'
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate string length
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::I32Sub,
            Instruction::LocalSet(9), // str_len
            // Allocate memory: 4 bytes (length) + str_len
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(malloc_index), // __malloc
            Instruction::LocalSet(11),       // str_ptr
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
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // NOTE: Box the string as an any type value
            // Allocate 12 bytes for boxed structure: [tag=4][str_ptr][padding]
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(12), // boxed_ptr in local 12 (temp)
            // Store type tag 4 (String) at offset 0
            Instruction::LocalGet(12),
            Instruction::I32Const(4), // AnyTypeTag::String
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store string pointer at offset 4
            Instruction::LocalGet(12),
            Instruction::LocalGet(11), // str_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store padding=0 at offset 8
            Instruction::LocalGet(12),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Store boxed pointer in object (not raw str_ptr)
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(12), // boxed_ptr (not str_ptr)
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Check if value is 'true'
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Else,
            // Check if value is 'false'
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Else,
            // Check if value is 'null'
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Else,
            // NESTED STRUCTURE SUPPORT
            // Check if value is '{' (nested object) or '[' (nested array)
            // Use recursive call to parse_value

            // Write current position to memory before recursive call
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(3), // cached position
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Recursive call to value parser
            Instruction::LocalGet(0), // string_ptr
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(2), // length
            Instruction::Call(parse_value_index),
            Instruction::LocalSet(11), // value_ptr
            // Read updated position
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // update position cache
            // Store nested value pointer in object
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(11), // value_ptr
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
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Track nesting depth
            Instruction::LocalGet(4),
            Instruction::I32Const(123),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(91),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10),
            Instruction::End,
            // NOTE: Check for '}' or ']' - handle depth correctly
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Is this at depth 0? If so, exit for outer brace
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // depth is 0, check for ',' or '}' to exit
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(3), // Exit skip block (Br(3) from inside 2 Ifs + Loop = Block)
            Instruction::Else,
            // depth > 0, decrement for inner closing brace
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10), // depth--
            Instruction::End,
            Instruction::End,
            // At depth 0, check for ','
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::BrIf(2), // Exit skip loop for comma
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
            // Write final position back to memory
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(3), // cached position
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return object pointer
            Instruction::LocalGet(6),
        ]
    }

    /// Generate WASM instructions for __json_parse_array
    /// Parses JSON array: [value1,value2,value3,...]
    ///
    /// Parameters:
    /// - string_ptr (i32): Pointer to JSON string
    /// - position_ptr (i32): Memory location containing current position
    /// - length (i32): String length
    ///
    /// Returns:
    /// - i32: Pointer to array or 0 on error
    fn generate_parse_array_instructions(
        &self,
        parse_value_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        // Local 0: string_ptr (parameter)
        // Local 1: position_ptr (parameter - memory location)
        // Local 2: length (parameter)
        // Local 3: position (cached from position_ptr)
        // Local 4: current_character
        // Local 5: element_count
        // Local 6: array_ptr (allocated memory)
        // Local 7: loop counter i
        // Local 8: start_position / num_start / str_start
        // Local 9: element_ptr / value / str_len / parse_pos
        // Local 10: depth tracker / is_negative
        // Local 11: value_ptr / str_ptr / temp
        // Local 12: temp
        // Local 13: decimal_divisor (F64)
        // Local 14: temp

        vec![
            // Read position from memory into cache
            Instruction::LocalGet(1), // position_ptr
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // position (cached)
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
            Instruction::LocalGet(3), // position
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3), // position++
            // Step 2: Count elements by scanning
            // Save current position
            Instruction::LocalGet(3),
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
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            // Get current char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // char
            // Check if whitespace
            Instruction::LocalGet(4),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1), // Exit whitespace loop
            // Increment position
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0), // Continue whitespace loop
            Instruction::End,   // End whitespace loop
            Instruction::End,   // End whitespace block
            // Check for ']' (empty array or end of array)
            Instruction::LocalGet(4),
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
            Instruction::I32Const(0),
            Instruction::LocalSet(12), // in_string = 0 (for tracking quoted strings)
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Check bounds
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer block
            // Get char
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Increment position
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Check for quote character to toggle in_string
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Toggle in_string: in_string = 1 - in_string
            Instruction::I32Const(1),
            Instruction::LocalGet(12),
            Instruction::I32Sub,
            Instruction::LocalSet(12),
            Instruction::End,
            // Only check for structure characters when NOT inside a string
            Instruction::LocalGet(12),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Track nesting depth for nested arrays/objects
            Instruction::LocalGet(4),
            Instruction::I32Const(123), // '{'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::BrIf(3), // Exit skip loop (adjusted for in_string If)
            Instruction::End,
            Instruction::End,   // End in_string check
            Instruction::Br(0), // Continue skip loop
            Instruction::End,   // End skip loop
            Instruction::End,   // End skip block
            // Check if we hit ']' (end of array)
            Instruction::LocalGet(4),
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
            Instruction::Call(malloc_index), // Call __malloc
            Instruction::LocalSet(6),        // array_ptr = malloc(...)
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
            Instruction::LocalSet(3), // position = start_position
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
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(9),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(10),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Parse element value - check first character to determine type
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if element is a number (digit or '-')
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57), // '9'
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse number (reuse number parsing logic from object parser)
            Instruction::LocalGet(3),
            Instruction::LocalSet(8), // num_start = position
            // Find end of number
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check if char is part of number
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::LocalGet(4),
            Instruction::I32Const(46), // '.'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Allocate and parse number (same as object parser)
            Instruction::I32Const(12),
            Instruction::Call(malloc_index), // __malloc
            Instruction::LocalSet(11),       // value_ptr
            // Store type tag = 3
            Instruction::LocalGet(11),
            Instruction::I32Const(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize accumulator (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::F64Const(0.0),
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
            Instruction::LocalGet(3),
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
            Instruction::LocalSet(4),
            // Check if digit
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            Instruction::F64Add,
            // Save to temp local 14 (F64)
            Instruction::LocalSet(14),
            // Store back (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(14),
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
            Instruction::LocalGet(3), // position (end of number)
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
            Instruction::LocalSet(4),
            // Check if it's a decimal point '.' (46)
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
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
            Instruction::LocalSet(4),
            // Check if it's a digit (48-57)
            Instruction::LocalGet(4),
            Instruction::I32Const(48), // '0'
            Instruction::I32GeU,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32Sub,
            Instruction::F64ConvertI32S,
            // Divide by decimal_divisor
            Instruction::LocalGet(13),
            Instruction::F64Div,
            // Add to accumulator
            Instruction::F64Add,
            // Save result to temp local 14 (F64)
            Instruction::LocalSet(14),
            // Store back (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(14),
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
            // Save negated value to temp local 14
            Instruction::LocalSet(14),
            // Store back (address first, then value)
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(14),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // Parse string element
            // Skip opening '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Save string start position
            Instruction::LocalGet(3),
            Instruction::LocalSet(8), // str_start = position
            // Find closing '"'
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Check for closing '"'
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Calculate string length
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::I32Sub,
            Instruction::LocalSet(9), // str_len
            // Allocate memory: 4 bytes (length) + str_len
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(malloc_index), // __malloc
            Instruction::LocalSet(11),       // str_ptr
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
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // NOTE: Box the string as an any type value
            // Allocate 12 bytes for boxed structure: [tag=4][str_ptr][padding]
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(12), // boxed_ptr in local 12 (temp)
            // Store type tag 4 (String) at offset 0
            Instruction::LocalGet(12),
            Instruction::I32Const(4), // AnyTypeTag::String
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store string pointer at offset 4
            Instruction::LocalGet(12),
            Instruction::LocalGet(11), // str_ptr
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store padding=0 at offset 8
            Instruction::LocalGet(12),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Store boxed pointer in array (not raw str_ptr)
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(12), // boxed_ptr (not str_ptr)
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Check if element is 'true'
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Else,
            // Check if element is 'false'
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
            Instruction::I32Const(5),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Else,
            // Check if element is 'null'
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Else,
            // Nested object/array - use recursive call to parse_value
            // Write current position to memory before recursive call
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(3), // cached position
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Recursive call to value parser
            Instruction::LocalGet(0), // string_ptr
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(2), // length
            Instruction::Call(parse_value_index),
            Instruction::LocalSet(9), // element_ptr
            // Read updated position
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // update position cache
            // Store nested element pointer in array
            Instruction::LocalGet(6), // array_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(9), // element_ptr (parsed nested value)
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
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3),
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Track depth
            Instruction::LocalGet(4),
            Instruction::I32Const(123),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(91),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(10),
            Instruction::End,
            Instruction::LocalGet(4),
            Instruction::I32Const(125),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
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
            Instruction::LocalGet(4),
            Instruction::I32Const(44),
            Instruction::I32Eq,
            Instruction::LocalGet(4),
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
            // Write final position back to memory
            Instruction::LocalGet(1), // position_ptr
            Instruction::LocalGet(3), // cached position
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return array pointer
            Instruction::LocalGet(6),
        ]
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
