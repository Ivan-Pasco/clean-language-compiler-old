//! JSON Module for Clean Language
//!
//! Pure WASM implementation of JSON parsing and stringifying.
//! No host imports required - fully portable across all WASM runtimes.
//! BOOK: json-module

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use wasm_encoder::{Instruction, MemArg};

/// JSON Value Type Tags — 12-byte boxed representation: [i32 tag][payload][padding]
///
/// Layout per tag:
///   0 = null    → [0][0][0]
///   1 = integer → [1][i32_value][0]      (produced by external builders, never by the parser)
///   2 = boolean → [2][i32 0/1][0]        (0 = false, 1 = true)
///   3 = number  → [3][f64_lo][f64_hi]    (full 8-byte f64 at offset 4)
///   4 = string  → [4][str_ptr][0]        (ptr to [i32 len][bytes…])
///   5 = array   → [5][raw_arr_ptr][0]    (ptr to [i32 count][elem0_ptr]…)
///   6 = object  → [6][raw_obj_ptr][0]    (ptr to [i32 count][key0_ptr][val0_ptr]…)
///
/// Compact boolean encoding in object/array slots (stored raw, not as a full box):
///   0 = null (slot value), 1 = false, 2 = true
/// Access helpers (__json_get_field, __json_get_index) box compact values before returning.
pub const JSON_TAG_NULL: i32 = 0;
pub const JSON_TAG_INTEGER: i32 = 1;
pub const JSON_TAG_BOOLEAN: i32 = 2;
pub const JSON_TAG_NUMBER: i32 = 3;
pub const JSON_TAG_STRING: i32 = 4;
pub const JSON_TAG_ARRAY: i32 = 5;
pub const JSON_TAG_OBJECT: i32 = 6;

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

    /// Box a compact boolean stored in `raw_local` into a 12-byte Any value, in-place.
    ///
    /// If `raw_local` == 1 (compact false) or == 2 (compact true), allocates a 12-byte
    /// Any box and writes the resulting pointer back to `raw_local`. Any other value is
    /// left unchanged. Uses `BlockType::Empty` — no net stack effect.
    fn compact_bool_box_inplace(
        raw_local: u32,
        boxed_local: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(raw_local),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(boxed_local),
            Instruction::I32Const(JSON_TAG_BOOLEAN),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(0), // false
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::LocalSet(raw_local),
            Instruction::Else,
            Instruction::LocalGet(raw_local),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(boxed_local),
            Instruction::I32Const(JSON_TAG_BOOLEAN),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(1), // true
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::LocalSet(raw_local),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Box a compact boolean in `raw_local` and return the result on the stack.
    ///
    /// If `raw_local` == 1 (compact false) or == 2 (compact true), allocates a 12-byte
    /// Any box and returns the pointer. Any other value (null=0 or real pointer) is
    /// returned as-is. Uses `BlockType::Result(I32)` — leaves exactly one i32 on the
    /// stack, which is the (possibly-boxed) value.
    fn compact_bool_unbox_returning(
        raw_local: u32,
        boxed_local: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(raw_local),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(boxed_local),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(JSON_TAG_BOOLEAN),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(0), // false
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::Else,
            Instruction::LocalGet(raw_local),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(12),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(boxed_local),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(JSON_TAG_BOOLEAN),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(1), // true
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(boxed_local),
            Instruction::Else,
            Instruction::LocalGet(raw_local), // null or real pointer — pass through
            Instruction::End,
            Instruction::End,
        ]
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
        // json.get(json_ptr: i32, path_ptr: i32) -> i32
        // Adapts the 2-arg language calling convention to the 3-arg internal
        // __json_get_path(obj_boxed, path_content_ptr, path_len) convention,
        // which walks a dot-separated path through the JSON value per spec
        // §8 (foundation/spec/stdlib-reference.md). The path_ptr is a Clean
        // Language string (4-byte length prefix + content); this wrapper
        // expands it to (path_ptr + 4, mem[path_ptr]) before the call.
        //
        // RUNTIME002: When json_ptr is a boxed String (tag=4), the caller passed
        // raw JSON text rather than a pre-decoded object. Auto-parse it via
        // json.textToData so json.get(raw_json_string, path) works transparently
        // — no explicit json.decode() needed. A boxed Object (tag=6) or any other
        // tag is passed through unchanged.
        //
        // Note: when a plugin bridge (e.g. frame.auth) provides json.get, its
        // wrapper registration runs after this and overwrites this entry.
        if let (Some(path_idx), Some(text_to_data_idx)) = (
            codegen.get_function_index("__json_get_path"),
            codegen.get_function_index("json.textToData"),
        ) {
            register_stdlib_function_with_locals(
                codegen,
                "json.get",
                &[WasmType::I32, WasmType::I32], // (json_ptr, path_ptr)
                Some(WasmType::I32),
                // Local 2: type_tag
                // Local 3: obj_boxed_ptr (the parsed-tree pointer fed to __json_get_path)
                // Local 4: str_ptr (cache key — the inner JSON source string ptr)
                // Local 5: parsed_ptr (cache miss path stages the result here)
                &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                vec![
                    // null guard
                    Instruction::LocalGet(0),
                    Instruction::I32Eqz,
                    Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
                    Instruction::I32Const(0),
                    Instruction::Else,
                    // read type tag at offset 0 of the boxed Any
                    Instruction::LocalGet(0),
                    Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::LocalSet(2), // type_tag
                    // if type_tag == 4 (String): extract str_ptr and check cache before parsing.
                    //
                    // The SSR shape `while c != "": c = json.get(j, i.toString())` calls into
                    // json.get N times per loop with the same `j` source. Without this cache
                    // every call re-runs the recursive-descent JSON parser, allocating a fresh
                    // tree on the bump heap. With a 1000-element source the per-iter tree is
                    // ~24 KB, blowing the 32 MB default WASM memory cap in <1500 iters
                    // (CMP-SSR-MALLOC-OOM-CONDITIONAL-HELPER, fp e4c682d19d00).
                    //
                    // Cache invariant (validated below): the cached parsed pointer is still
                    // intact in heap memory iff (a) the cached source ptr matches the one we
                    // just loaded AND (b) __heap_ptr (global 0) is still >= the heap floor we
                    // recorded when we landed the tree (global 3). Condition (b) catches
                    // mem_scope_pop reclamation between calls — when the bump pointer rolls
                    // back below our tree, the bytes are unsafe to read and we must re-parse.
                    Instruction::LocalGet(2),
                    Instruction::I32Const(JSON_TAG_STRING),
                    Instruction::I32Eq,
                    Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
                    // str_ptr = memory[json_ptr + 4]
                    Instruction::LocalGet(0),
                    Instruction::I32Load(MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::LocalSet(4),
                    // Cache hit predicate:
                    //   str_ptr == cache_src  &&  cache_src != 0  &&  __heap_ptr >= cache_floor
                    Instruction::LocalGet(4),
                    Instruction::GlobalGet(1), // cache_src
                    Instruction::I32Eq,
                    Instruction::GlobalGet(1),
                    Instruction::I32Const(0),
                    Instruction::I32Ne, // cache_src != 0
                    Instruction::I32And,
                    Instruction::GlobalGet(0), // __heap_ptr
                    Instruction::GlobalGet(3), // cache_floor
                    Instruction::I32GeU,
                    Instruction::I32And,
                    Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
                    // hit — reuse cached parsed tree
                    Instruction::GlobalGet(2),
                    Instruction::Else,
                    // miss — parse, then memoize (source ptr, parsed ptr, floor)
                    Instruction::LocalGet(4),
                    Instruction::Call(text_to_data_idx),
                    Instruction::LocalSet(5), // parsed_ptr
                    Instruction::LocalGet(4),
                    Instruction::GlobalSet(1), // cache_src := str_ptr
                    Instruction::LocalGet(5),
                    Instruction::GlobalSet(2), // cache_parsed := parsed_ptr
                    Instruction::GlobalGet(0),
                    Instruction::GlobalSet(3), // cache_floor := __heap_ptr (post-parse)
                    Instruction::LocalGet(5),
                    Instruction::End, // closes cache hit/miss
                    Instruction::Else,
                    Instruction::LocalGet(0), // already a boxed Any object or other type
                    Instruction::End,         // closes tag==String check
                    Instruction::LocalSet(3), // obj_boxed_ptr
                    // __json_get_path expects the boxed Any directly (not the raw inner
                    // pointer) because it must re-read the tag for every segment to
                    // decide between field vs index dispatch.
                    Instruction::LocalGet(3),
                    // path content ptr = path_ptr + 4
                    Instruction::LocalGet(1),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    // path length = mem[path_ptr]
                    Instruction::LocalGet(1),
                    Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::Call(path_idx),
                    Instruction::End, // closes null check
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
        let skip_string_idx_predicted = base_idx; // Will be registered first
        let parse_string_idx_predicted = base_idx + 1; // Will be registered second
        let value_idx_predicted = base_idx + 2; // Will be registered third
        let object_idx_predicted = base_idx + 3; // Will be registered fourth
        let array_idx_predicted = base_idx + 4; // Will be registered fifth

        // Step 3: Register all helper functions with correct indices

        // __json_skip_string - advance position past a JSON string (escape-aware, no output)
        let skip_string_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_skip_string",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, position_ptr, length
            None,                                           // void — only updates position_ptr
            &[
                WasmType::I32, // Local 3: position
                WasmType::I32, // Local 4: current_char
            ],
            self.generate_skip_string_instructions(),
        )?;

        assert_eq!(
            skip_string_idx, skip_string_idx_predicted,
            "Function index prediction failed for __json_skip_string"
        );

        // __json_parse_string - parse a JSON string starting at '"', return Clean Language string ptr
        let parse_string_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_parse_string",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // string_ptr, position_ptr, length
            Some(WasmType::I32),                            // returns pointer to new Clean string
            &[
                WasmType::I32, // Local 3: position
                WasmType::I32, // Local 4: current_char
                WasmType::I32, // Local 5: out_ptr (output buffer)
                WasmType::I32, // Local 6: out_write_pos
                WasmType::I32, // Local 7: next_char (escape processing)
            ],
            self.generate_parse_string_instructions(malloc_index),
        )?;

        assert_eq!(
            parse_string_idx, parse_string_idx_predicted,
            "Function index prediction failed for __json_parse_string"
        );

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
                parse_string_idx,
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
            self.generate_parse_object_instructions(
                value_idx,
                skip_string_idx,
                parse_string_idx,
                malloc_index,
            ),
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
            self.generate_parse_array_instructions(
                value_idx,
                skip_string_idx,
                parse_string_idx,
                malloc_index,
            ),
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
                WasmType::I32, // Local 3: string_end / scan_pos
                WasmType::I32, // Local 4: first_char (pre-validation)
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
        let pretty_value_idx_predicted = base_idx + 4;
        let pretty_array_idx_predicted = base_idx + 5;
        let pretty_object_idx_predicted = base_idx + 6;

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
                WasmType::I32, // Local 3: i (loop counter)
                WasmType::I32, // Local 4: out_len (bytes written, excluding quotes)
                WasmType::I32, // Local 5: current_byte
                WasmType::I32, // Local 6: nibble (scratch for \u00XX hex conversion)
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

        // 5. __json_pretty_stringify_value(boxed_ptr: i32, indent: i32) -> i32
        let pretty_value_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_pretty_stringify_value",
            &[WasmType::I32, WasmType::I32], // boxed_ptr, indent_level
            Some(WasmType::I32),             // returns string
            &[
                WasmType::I32, // Local 2: type_tag
                WasmType::I32, // Local 3: value/temp
                WasmType::I32, // Local 4: result_ptr
                WasmType::I32, // Local 5: temp
                WasmType::F64, // Local 6: f64_value
            ],
            self.generate_pretty_stringify_value_instructions(
                malloc_index,
                int_to_string_index,
                float_to_string_index,
                quote_string_idx,
                pretty_array_idx_predicted,
                pretty_object_idx_predicted,
                stringify_array_idx,  // fallback for compact scalar arrays
                stringify_object_idx, // fallback for compact scalar objects
            ),
        )?;

        assert_eq!(
            pretty_value_idx, pretty_value_idx_predicted,
            "Function index prediction failed for __json_pretty_stringify_value"
        );

        // 6. __json_pretty_stringify_array(array_ptr: i32, indent: i32) -> i32
        let pretty_array_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_pretty_stringify_array",
            &[WasmType::I32, WasmType::I32], // array_ptr, indent_level
            Some(WasmType::I32),             // returns string
            &[
                WasmType::I32, // Local 2: count
                WasmType::I32, // Local 3: i
                WasmType::I32, // Local 4: result
                WasmType::I32, // Local 5: elem_ptr
                WasmType::I32, // Local 6: elem_str
                WasmType::I32, // Local 7: temp
                WasmType::I32, // Local 8: boxed_ptr
                WasmType::I32, // Local 9: indent_str
                WasmType::I32, // Local 10: closing_indent_str
            ],
            self.generate_pretty_stringify_array_instructions(
                malloc_index,
                string_concat_index,
                pretty_value_idx,
            ),
        )?;

        assert_eq!(
            pretty_array_idx, pretty_array_idx_predicted,
            "Function index prediction failed for __json_pretty_stringify_array"
        );

        // 7. __json_pretty_stringify_object(object_ptr: i32, indent: i32) -> i32
        let pretty_object_idx = register_stdlib_function_with_locals(
            codegen,
            "__json_pretty_stringify_object",
            &[WasmType::I32, WasmType::I32], // object_ptr, indent_level
            Some(WasmType::I32),             // returns string
            &[
                WasmType::I32, // Local 2: count
                WasmType::I32, // Local 3: i
                WasmType::I32, // Local 4: result
                WasmType::I32, // Local 5: key_ptr
                WasmType::I32, // Local 6: val_ptr
                WasmType::I32, // Local 7: key_str
                WasmType::I32, // Local 8: val_str
                WasmType::I32, // Local 9: temp
                WasmType::I32, // Local 10: boxed_ptr
                WasmType::I32, // Local 11: indent_str
                WasmType::I32, // Local 12: closing_indent_str
            ],
            self.generate_pretty_stringify_object_instructions(
                malloc_index,
                string_concat_index,
                quote_string_idx,
                pretty_value_idx,
            ),
        )?;

        assert_eq!(
            pretty_object_idx, pretty_object_idx_predicted,
            "Function index prediction failed for __json_pretty_stringify_object"
        );

        // __json_encode_cln_list(list_ptr: i32, elem_tag: i32) -> i32
        // Walks a native Clean list and encodes each element directly as JSON.
        // Used by the json.encode(List<T>) dispatch in mir_builder/expressions.rs.
        register_stdlib_function_with_locals(
            codegen,
            "__json_encode_cln_list",
            &[WasmType::I32, WasmType::I32], // list_ptr, elem_tag
            Some(WasmType::I32),
            &[
                WasmType::I32, // 2: count
                WasmType::I32, // 3: i
                WasmType::I32, // 4: result_ptr
                WasmType::I32, // 5: elem_str
                WasmType::I32, // 6: elem_addr
                WasmType::I32, // 7: elem_i32
                WasmType::I32, // 8: temp_buf
                WasmType::I32, // 9: elem_stride
                WasmType::F64, // 10: elem_f64
            ],
            self.generate_encode_cln_list_instructions(
                malloc_index,
                string_concat_index,
                int_to_string_index,
                float_to_string_index,
                quote_string_idx,
            ),
        )?;

        // __json_encode_cln_pairs(pairs_ptr: i32, val_tag: i32) -> i32
        // Walks a native Clean pairs map and encodes each entry directly as JSON.
        // Used by the json.encode(Pairs<K,V>) dispatch in mir_builder/expressions.rs.
        register_stdlib_function_with_locals(
            codegen,
            "__json_encode_cln_pairs",
            &[WasmType::I32, WasmType::I32], // pairs_ptr, val_tag
            Some(WasmType::I32),
            &[
                WasmType::I32, // 2: count
                WasmType::I32, // 3: i
                WasmType::I32, // 4: result_ptr
                WasmType::I32, // 5: entry_addr
                WasmType::I32, // 6: key_ptr
                WasmType::I32, // 7: val_i32
                WasmType::I32, // 8: key_str
                WasmType::I32, // 9: val_str
                WasmType::I32, // 10: temp_buf
            ],
            self.generate_encode_cln_pairs_instructions(
                malloc_index,
                string_concat_index,
                int_to_string_index,
                quote_string_idx,
            ),
        )?;

        // __json_from_cln_list(list_ptr: i32, elem_tag: i32) -> i32
        // Materializes a native Clean list as the JSON-array tree layout
        // (`[count][boxed_elem_ptr]…`) so the existing __json_stringify_array
        // helper can walk it. Used by emit_box_any (mir_builder/types.rs) when
        // a typed list is boxed to Any — for example as a field value inside
        // an `any data = { items: [...] }` object literal.
        register_stdlib_function_with_locals(
            codegen,
            "__json_from_cln_list",
            &[WasmType::I32, WasmType::I32], // list_ptr, elem_tag
            Some(WasmType::I32),             // returns json_array_ptr
            &[
                WasmType::I32, // 2: count
                WasmType::I32, // 3: i
                WasmType::I32, // 4: elem_addr
                WasmType::I32, // 5: box_ptr
                WasmType::I32, // 6: json_array_ptr
                WasmType::I32, // 7: dest_slot_addr
                WasmType::I32, // 8: elem_stride
            ],
            self.generate_from_cln_list_instructions(malloc_index),
        )?;

        // __json_from_cln_pairs(pairs_ptr: i32, val_tag: i32) -> i32
        // Materializes a native Clean pairs map as the JSON-object tree layout
        // (`[count][key_ptr][boxed_val_ptr]…`) so __json_stringify_object can
        // walk it. Used by emit_box_any when a typed pairs is boxed to Any.
        register_stdlib_function_with_locals(
            codegen,
            "__json_from_cln_pairs",
            &[WasmType::I32, WasmType::I32], // pairs_ptr, val_tag
            Some(WasmType::I32),             // returns json_object_ptr
            &[
                WasmType::I32, // 2: count
                WasmType::I32, // 3: i
                WasmType::I32, // 4: entry_addr
                WasmType::I32, // 5: key_ptr
                WasmType::I32, // 6: val_raw
                WasmType::I32, // 7: json_object_ptr
                WasmType::I32, // 8: box_ptr
                WasmType::I32, // 9: dest_entry_addr
            ],
            self.generate_from_cln_pairs_instructions(malloc_index),
        )?;

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
        // Convert data structure to indented JSON text
        register_stdlib_function_with_locals(
            codegen,
            "json.prettyDataToText",
            &[WasmType::I32],    // data pointer
            Some(WasmType::I32), // returns formatted string pointer
            &[],
            self.generate_pretty_data_to_text_instructions(pretty_value_idx),
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

        // __json_get_path(obj_boxed_ptr: i32, path_content_ptr: i32, path_len: i32) -> i32
        //
        // Descend through a JSON value following a dot-separated path, e.g.
        // `data.rows.0.name`. Each segment is dispatched to `__json_get_field`
        // when the current target is an object, or to `__json_get_index` when
        // the current target is an array AND the segment is all-digits.
        // Returns the boxed any pointer of the resolved value, or 0 if any
        // segment fails to resolve (per the spec, `json.get` never raises —
        // it returns null on miss). The `__json_get_field` and `__json_get_index`
        // primitives stay single-segment; path logic lives only here.
        //
        // Spec: foundation/spec/stdlib-reference.md §8 — "json.get function uses
        // dot-separated paths: json.get(result, \"data.rows.0.name\")".
        let field_index = codegen
            .get_function_index("__json_get_field")
            .expect("__json_get_field must be registered before __json_get_path");
        let index_index = codegen
            .get_function_index("__json_get_index")
            .expect("__json_get_index must be registered before __json_get_path");

        register_stdlib_function_with_locals(
            codegen,
            "__json_get_path",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // obj_boxed, path_content, path_len
            Some(WasmType::I32),                            // returns boxed any pointer
            &[
                WasmType::I32, // Local 3: cursor
                WasmType::I32, // Local 4: seg_end
                WasmType::I32, // Local 5: cur_boxed
                WasmType::I32, // Local 6: cur_tag
                WasmType::I32, // Local 7: cur_raw_ptr
                WasmType::I32, // Local 8: idx_acc
                WasmType::I32, // Local 9: is_digit_seg
                WasmType::I32, // Local 10: byte
                WasmType::I32, // Local 11: scan_i
                WasmType::I32, // Local 12: seg_len
            ],
            self.generate_get_path_instructions(field_index, index_index),
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

        let mut instrs = vec![
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
        ];
        instrs.extend(Self::compact_bool_unbox_returning(7, 10, malloc_index));
        instrs.extend([
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
        ]);
        instrs
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

        let mut instrs = vec![
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
        ];
        instrs.extend(Self::compact_bool_unbox_returning(3, 4, malloc_index));
        instrs.extend([
            // Now we have the (possibly boxed) value on stack - return it
            Instruction::Else,
            // Invalid index (negative or >= count) - return null
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
        ]);
        instrs
    }

    /// Generate WASM instructions for `__json_get_path`.
    ///
    /// Walks a dot-separated path through a boxed JSON value, dispatching each
    /// segment to `__json_get_field` (object) or `__json_get_index` (array) per
    /// the spec at `foundation/spec/stdlib-reference.md` §8. Returns 0 (null)
    /// at the first segment that fails to resolve.
    fn generate_get_path_instructions(
        &self,
        field_index: u32,
        index_index: u32,
    ) -> Vec<Instruction<'static>> {
        // Parameters:
        // Local 0: obj_boxed_ptr  — boxed any of starting target
        // Local 1: path_content_ptr — raw path bytes (already past 4-byte len prefix)
        // Local 2: path_len
        //
        // Working locals:
        // Local 3: cursor
        // Local 4: seg_end
        // Local 5: cur_boxed (mutates as we descend)
        // Local 6: cur_tag
        // Local 7: cur_raw_ptr
        // Local 8: idx_acc — parsed integer for numeric segments
        // Local 9: is_digit_seg — 1 when segment is all ASCII digits
        // Local 10: byte
        // Local 11: scan_i
        // Local 12: seg_len
        const DOT_BYTE: i32 = 46; // '.'
        const ZERO_BYTE: i32 = 48; // '0'
        const NINE_BYTE: i32 = 57; // '9'

        vec![
            // Empty-path shortcut: return obj_boxed_ptr unchanged.
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::Else,
            // cur_boxed = obj_boxed_ptr; cursor = 0
            Instruction::LocalGet(0),
            Instruction::LocalSet(5),
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            // Block(I32): final result of the walk
            Instruction::Block(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Loop: one iteration per path segment
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // Bail if current target is null
            Instruction::LocalGet(5),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Br(2), // exit outer Block with 0
            Instruction::End,
            // Find seg_end: scan from cursor until '.' or path_len.
            Instruction::LocalGet(3),
            Instruction::LocalSet(4), // seg_end = cursor
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(4),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // seg_end >= path_len → exit scan block
            Instruction::LocalGet(1),
            Instruction::LocalGet(4),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(10),
            Instruction::LocalGet(10),
            Instruction::I32Const(DOT_BYTE),
            Instruction::I32Eq,
            Instruction::BrIf(1), // hit '.' → exit scan block
            Instruction::LocalGet(4),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(4),
            Instruction::Br(0), // continue scan loop
            Instruction::End,   // end scan loop
            Instruction::End,   // end scan block
            // seg_len = seg_end - cursor
            Instruction::LocalGet(4),
            Instruction::LocalGet(3),
            Instruction::I32Sub,
            Instruction::LocalSet(12),
            // Empty segment (consecutive dots, leading/trailing dot) → return null.
            Instruction::LocalGet(12),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Br(2),
            Instruction::End,
            // is_digit_seg = 1; idx_acc = 0; scan_i = cursor
            Instruction::I32Const(1),
            Instruction::LocalSet(9),
            Instruction::I32Const(0),
            Instruction::LocalSet(8),
            Instruction::LocalGet(3),
            Instruction::LocalSet(11),
            // Walk the segment bytes; track if all are digits and accumulate value.
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(11),
            Instruction::LocalGet(4),
            Instruction::I32GeU,
            Instruction::BrIf(1), // scan_i >= seg_end → exit
            Instruction::LocalGet(1),
            Instruction::LocalGet(11),
            Instruction::I32Add,
            Instruction::I32Load8U(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(10),
            // (byte < '0') | (byte > '9') → not a digit
            Instruction::LocalGet(10),
            Instruction::I32Const(ZERO_BYTE),
            Instruction::I32LtU,
            Instruction::LocalGet(10),
            Instruction::I32Const(NINE_BYTE),
            Instruction::I32GtU,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::LocalSet(9),
            Instruction::Br(2), // exit digit block
            Instruction::End,
            // idx_acc = idx_acc * 10 + (byte - '0')
            Instruction::LocalGet(8),
            Instruction::I32Const(10),
            Instruction::I32Mul,
            Instruction::LocalGet(10),
            Instruction::I32Const(ZERO_BYTE),
            Instruction::I32Sub,
            Instruction::I32Add,
            Instruction::LocalSet(8),
            // scan_i++
            Instruction::LocalGet(11),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(11),
            Instruction::Br(0), // continue digit loop
            Instruction::End,   // end digit loop
            Instruction::End,   // end digit block
            // Unbox cur_boxed: tag at offset 0, raw inner ptr at offset 4.
            Instruction::LocalGet(5),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(6), // cur_tag
            Instruction::LocalGet(5),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(7), // cur_raw_ptr
            // Dispatch:
            //   array  + numeric segment → __json_get_index
            //   object + any segment     → __json_get_field
            //   anything else            → return null
            Instruction::LocalGet(9),
            Instruction::LocalGet(6),
            Instruction::I32Const(JSON_TAG_ARRAY),
            Instruction::I32Eq,
            Instruction::I32And,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(7),
            Instruction::LocalGet(8),
            Instruction::Call(index_index),
            Instruction::LocalSet(5),
            Instruction::Else,
            Instruction::LocalGet(6),
            Instruction::I32Const(JSON_TAG_OBJECT),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(7),
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Add, // path_content_ptr + cursor
            Instruction::LocalGet(12),
            Instruction::Call(field_index),
            Instruction::LocalSet(5),
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::Br(3), // current target is not traversable → exit Block w/ 0
            Instruction::End,
            Instruction::End,
            // If the call returned null, propagate it out.
            Instruction::LocalGet(5),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Br(2),
            Instruction::End,
            // If we consumed the entire path, the walk is finished.
            Instruction::LocalGet(4),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(5),
            Instruction::Br(2),
            Instruction::End,
            // cursor = seg_end + 1 (skip the dot)
            Instruction::LocalGet(4),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0), // continue outer loop
            Instruction::End,   // end Loop
            // Fallthrough — unreachable in practice; Block's static result type.
            Instruction::I32Const(0),
            Instruction::End, // end Block
            Instruction::End, // end outer if
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
    /// Validates the input before parsing and returns null (pointer 0) on invalid JSON.
    ///
    /// Locals:
    ///   0: string_ptr (param)
    ///   1: position_ptr (malloc'd later)
    ///   2: length
    ///   3: scan_pos (pre-check) / string_end (heap guard)
    ///   4: first_char
    fn generate_try_text_to_data_instructions(
        &self,
        parse_value_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        const HEAP_PTR_GLOBAL: u32 = 0;
        vec![
            // Load length
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // length
            // Empty string → return 0
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(0),
            Instruction::Else,
            // Find first non-whitespace char into local 4
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // scan_pos = 0
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // exit: all whitespace → first_char stays 0
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
            Instruction::LocalSet(4), // first_char
            // is whitespace?
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
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(1), // continue loop
            Instruction::End,
            Instruction::Br(1), // non-whitespace found, exit
            Instruction::End,   // end loop
            Instruction::End,   // end block
            // Validate first char is a JSON token start
            // Valid: '{' '[' '"' 't' 'f' 'n' '-' '0'-'9'
            Instruction::LocalGet(4),
            Instruction::I32Const(123), // '{'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(91), // '['
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(116), // 't'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(102), // 'f'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(110), // 'n'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::LocalGet(4),
            Instruction::I32Const(45), // '-'
            Instruction::I32Eq,
            Instruction::I32Or,
            // digit: first_char >= '0' && first_char <= '9'
            Instruction::LocalGet(4),
            Instruction::I32Const(48),
            Instruction::I32GeU,
            Instruction::LocalGet(4),
            Instruction::I32Const(57),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::I32Or,
            // is_valid on stack; invert for the if-invalid branch
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(0), // invalid JSON token start → return null
            Instruction::Else,
            // Valid input — proceed with full textToData logic:
            // heap guard, allocate position_ptr, call parser
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(2),
            Instruction::I32Add,
            Instruction::I32Const(7),
            Instruction::I32Add,
            Instruction::I32Const(-8),
            Instruction::I32And,
            Instruction::LocalSet(3), // string_end (aligned)
            Instruction::GlobalGet(HEAP_PTR_GLOBAL),
            Instruction::LocalGet(3),
            Instruction::I32LtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::GlobalSet(HEAP_PTR_GLOBAL),
            Instruction::End,
            Instruction::I32Const(4),
            Instruction::Call(malloc_index),
            Instruction::LocalSet(1), // position_ptr
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_value_index),
            Instruction::End, // end is_valid else
            Instruction::End, // end length == 0 else
        ]
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

    /// Generate WASM instructions for json.prettyDataToText
    /// Calls __json_pretty_stringify_value with indent level 0.
    fn generate_pretty_data_to_text_instructions(
        &self,
        pretty_stringify_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0), // data pointer
            Instruction::I32Const(0), // indent = 0
            Instruction::Call(pretty_stringify_value_idx),
        ]
    }

    /// Generate WASM instructions for __json_pretty_stringify_value
    /// Same dispatch as __json_stringify_value but passes indent to array/object handlers.
    #[allow(clippy::too_many_arguments)]
    fn generate_pretty_stringify_value_instructions(
        &self,
        malloc_index: u32,
        int_to_string_index: u32,
        float_to_string_index: u32,
        quote_string_idx: u32,
        pretty_array_idx: u32,
        pretty_object_idx: u32,
        _stringify_array_idx: u32,
        _stringify_object_idx: u32,
    ) -> Vec<Instruction<'static>> {
        // Parameters: Local 0 = boxed_ptr, Local 1 = indent_level
        // Working:    Local 2 = type_tag, Local 3 = value, Local 4 = result_ptr, Local 5 = temp, Local 6 = f64_value
        vec![
            // null pointer → return "null"
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(4),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(0x6C6C756E), // "null"
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::Else,
            // Read type tag
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // type_tag
            // Tag 0: Null
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(4),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(0x6C6C756E),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::Else,
            // Tag 1: Integer
            Instruction::LocalGet(2),
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
            Instruction::LocalGet(2),
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
            Instruction::LocalTee(4),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(0x65757274),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::Else,
            // false
            Instruction::I32Const(9),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(4),
            Instruction::I32Const(5),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(0x736C6166),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(101), // 'e'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 8,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::End,
            Instruction::Else,
            // Tag 3: Number
            Instruction::LocalGet(2),
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
            Instruction::LocalGet(2),
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
            // Tag 5: List/Array — use pretty version
            Instruction::LocalGet(2),
            Instruction::I32Const(5),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // indent_level
            Instruction::Call(pretty_array_idx),
            Instruction::Else,
            // Tag 6: Object — use pretty version
            Instruction::LocalGet(2),
            Instruction::I32Const(6),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // indent_level
            Instruction::Call(pretty_object_idx),
            Instruction::Else,
            // Unknown type → "null"
            Instruction::I32Const(8),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(4),
            Instruction::I32Const(4),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(0x6C6C756E),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::End, // tag 6
            Instruction::End, // tag 5
            Instruction::End, // tag 4
            Instruction::End, // tag 3
            Instruction::End, // tag 2
            Instruction::End, // tag 1
            Instruction::End, // tag 0
            Instruction::End, // null check
        ]
    }

    /// Generate WASM instructions for __json_pretty_stringify_array
    /// Produces `[\n  value,\n  value\n]` with `(indent+1)*2` spaces per element line
    /// and `indent*2` spaces for the closing bracket.
    fn generate_pretty_stringify_array_instructions(
        &self,
        malloc_index: u32,
        string_concat_index: u32,
        pretty_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        // Parameters: Local 0 = array_ptr, Local 1 = indent_level
        // Working:    Local 2=count, Local 3=i, Local 4=result, Local 5=elem_ptr,
        //             Local 6=elem_str, Local 7=temp, Local 8=boxed_ptr,
        //             Local 9=indent_str (for elements), Local 10=closing_indent_str
        let mut instrs = vec![
            // Read count
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // count
            // Empty array → return "[]"
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(6),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(7),
            Instruction::I32Const(2),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(91), // '['
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(93), // ']'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::Else,
            // Build element-indent string: "\n" + (indent+1)*2 spaces
            // byte_count = 1 (newline) + (indent+1)*2
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Const(2),
            Instruction::I32Mul,
            Instruction::I32Const(1),
            Instruction::I32Add,      // total = 1 + (indent+1)*2
            Instruction::LocalSet(7), // save byte count
            // allocate indent_str
            Instruction::I32Const(4),
            Instruction::LocalGet(7),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(9), // indent_str (for element lines)
            // store length
            Instruction::LocalGet(9),
            Instruction::LocalGet(7),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // write '\n' as first byte
            Instruction::LocalGet(9),
            Instruction::I32Const(10), // '\n'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // fill remaining bytes with spaces using a loop
            // reuse local 3 (i) as write offset starting at 1
            Instruction::I32Const(1),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(7),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(9),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Const(32), // ' '
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Build closing-indent string: "\n" + indent*2 spaces
            Instruction::LocalGet(1),
            Instruction::I32Const(2),
            Instruction::I32Mul,
            Instruction::I32Const(1),
            Instruction::I32Add, // total = 1 + indent*2
            Instruction::LocalSet(7),
            Instruction::I32Const(4),
            Instruction::LocalGet(7),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(10), // closing_indent_str
            Instruction::LocalGet(10),
            Instruction::LocalGet(7),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(10),
            Instruction::I32Const(10), // '\n'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(7),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(10),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Const(32), // ' '
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // result = "["
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(4), // result
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(91), // '['
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // i = 0
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            // Loop over elements
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // If i > 0, append ","
            Instruction::LocalGet(3),
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(7),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(44), // ','
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::LocalGet(7),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            Instruction::End,
            // Append element-indent string
            Instruction::LocalGet(4),
            Instruction::LocalGet(9),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            // Load elem_ptr
            Instruction::LocalGet(0),
            Instruction::LocalGet(3),
            Instruction::I32Const(2),
            Instruction::I32Shl, // i * 4
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(5),
        ];
        instrs.extend(Self::compact_bool_box_inplace(5, 8, malloc_index));
        instrs.extend([
            // elem_str = pretty_stringify_value(elem_ptr, indent+1)
            Instruction::LocalGet(5),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add, // indent + 1
            Instruction::Call(pretty_value_idx),
            Instruction::LocalSet(6),
            // result = concat(result, elem_str)
            Instruction::LocalGet(4),
            Instruction::LocalGet(6),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            // i++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Append closing_indent + "]"
            Instruction::LocalGet(4),
            Instruction::LocalGet(10),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(7),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(7),
            Instruction::I32Const(93), // ']'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::LocalGet(7),
            Instruction::Call(string_concat_index),
            Instruction::End, // end count == 0 check
        ]);
        instrs
    }

    /// Generate WASM instructions for __json_pretty_stringify_object
    /// Produces `{\n  "key": value,\n  ...\n}` with proper indentation.
    fn generate_pretty_stringify_object_instructions(
        &self,
        malloc_index: u32,
        string_concat_index: u32,
        quote_string_idx: u32,
        pretty_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        // Parameters: Local 0 = object_ptr, Local 1 = indent_level
        // Working:    Local 2=count, Local 3=i, Local 4=result, Local 5=key_ptr,
        //             Local 6=val_ptr, Local 7=key_str, Local 8=val_str,
        //             Local 9=temp, Local 10=boxed_ptr, Local 11=indent_str, Local 12=closing_indent_str
        let mut instrs = vec![
            // Read count
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // count
            // Empty object → return "{}"
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(6),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(9),
            Instruction::I32Const(2),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(123), // '{'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(125), // '}'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::Else,
            // Build element-indent string: "\n" + (indent+1)*2 spaces
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Const(2),
            Instruction::I32Mul,
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9), // byte_count
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(11), // indent_str
            Instruction::LocalGet(11),
            Instruction::LocalGet(9),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(11),
            Instruction::I32Const(10), // '\n'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(9),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(11),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Const(32), // ' '
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Build closing-indent: "\n" + indent*2 spaces
            Instruction::LocalGet(1),
            Instruction::I32Const(2),
            Instruction::I32Mul,
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(9),
            Instruction::I32Const(4),
            Instruction::LocalGet(9),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(12), // closing_indent_str
            Instruction::LocalGet(12),
            Instruction::LocalGet(9),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(12),
            Instruction::I32Const(10), // '\n'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(9),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(12),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::I32Const(32), // ' '
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // result = "{"
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(4), // result
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(123), // '{'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            // i = 0
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            // Loop over key-value pairs
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // If i > 0, append ","
            Instruction::LocalGet(3),
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(9),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(44), // ','
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::LocalGet(9),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            Instruction::End,
            // Append element-indent
            Instruction::LocalGet(4),
            Instruction::LocalGet(11),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            // Load key_ptr from object_ptr + 4 + i*8
            Instruction::LocalGet(0),
            Instruction::LocalGet(3),
            Instruction::I32Const(3),
            Instruction::I32Shl, // i * 8
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(5), // key_ptr
            // key_str = quote_string(key_ptr)
            Instruction::LocalGet(5),
            Instruction::Call(quote_string_idx),
            Instruction::LocalSet(7), // key_str
            // result = concat(result, key_str)
            Instruction::LocalGet(4),
            Instruction::LocalGet(7),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            // Append ": "
            Instruction::I32Const(6), // 4-byte header + 2 bytes
            Instruction::Call(malloc_index),
            Instruction::LocalTee(9),
            Instruction::I32Const(2),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(58), // ':'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(32), // ' '
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::LocalGet(9),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            // Load val_ptr from object_ptr + 8 + i*8
            Instruction::LocalGet(0),
            Instruction::LocalGet(3),
            Instruction::I32Const(3),
            Instruction::I32Shl, // i * 8
            Instruction::I32Add,
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(6), // val_ptr
        ];
        instrs.extend(Self::compact_bool_box_inplace(6, 10, malloc_index));
        instrs.extend([
            // val_str = pretty_stringify_value(val_ptr, indent+1)
            Instruction::LocalGet(6),
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::Call(pretty_value_idx),
            Instruction::LocalSet(8), // val_str
            // result = concat(result, val_str)
            Instruction::LocalGet(4),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            // i++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // Append closing_indent + "}"
            Instruction::LocalGet(4),
            Instruction::LocalGet(12),
            Instruction::Call(string_concat_index),
            Instruction::LocalSet(4),
            Instruction::I32Const(5),
            Instruction::Call(malloc_index),
            Instruction::LocalTee(9),
            Instruction::I32Const(1),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(9),
            Instruction::I32Const(125), // '}'
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::LocalGet(9),
            Instruction::Call(string_concat_index),
            Instruction::End, // end count == 0 check
        ]);
        instrs
    }

    fn generate_quote_string_instructions(&self, malloc_index: u32) -> Vec<Instruction<'static>> {
        // RFC 8259 §7-compliant quoting.
        //
        // Locals:
        //   0 = str_ptr (param)
        //   1 = orig_len
        //   2 = result_ptr
        //   3 = i
        //   4 = out_len
        //   5 = current_byte
        //   6 = nibble (scratch for \u00XX hex conversion)
        //
        // Escapes implemented:
        //   " → \"        \ → \\        BS (0x08) → \b
        //   TAB (0x09) → \t   LF (0x0A) → \n   FF (0x0C) → \f   CR (0x0D) → \r
        //   any other byte in 0x00..0x1F → \u00XX (six-byte lowercase form)
        //   all other bytes → emitted as-is (UTF-8 bytes pass through;
        //   RFC 8259 §7 does not require ASCII escaping of >0x7F).
        //
        // Memory layout of the result string:
        //   [4-byte length][ "  ][escaped content...][ " ]
        //
        // Allocation: worst case per input byte is the 6-byte \u00XX form,
        // plus 2 quote bytes plus the 4-byte length prefix. orig_len*6 + 6
        // is a safe upper bound. The previous orig_len*2 + 6 sizing assumed
        // every escape was at most two bytes — true for the implemented set
        // back then, but adding \u00XX (six bytes) would have overrun by 4
        // bytes per such input byte under the old math.
        #[rustfmt::skip]
        fn wb(v: &mut Vec<Instruction<'static>>, b: i32) {
            // store byte B at (result_ptr + out_len + 5), then out_len++
            v.extend([
                Instruction::LocalGet(2), Instruction::LocalGet(4), Instruction::I32Add,
                Instruction::I32Const(b),
                Instruction::I32Store8(wasm_encoder::MemArg { offset: 5, align: 0, memory_index: 0 }),
                Instruction::LocalGet(4), Instruction::I32Const(1), Instruction::I32Add,
                Instruction::LocalSet(4),
            ]);
        }

        // Emit one ASCII-hex character for a nibble of `current_byte` (local 5).
        // `shift` is 4 for the high nibble, 0 for the low nibble. Uses local 6
        // as scratch (declared via register_stdlib_function_with_locals).
        // Formula: nibble + 48 + (nibble >= 10 ? 39 : 0) — gives '0'..'9' for
        // 0..9 and 'a'..'f' for 10..15 with no branches and no temp stack juggling.
        #[rustfmt::skip]
        fn wb_hex(v: &mut Vec<Instruction<'static>>, shift: i32) {
            // local 6 = (current_byte >> shift) & 0xF
            v.extend([
                Instruction::LocalGet(5),
                Instruction::I32Const(shift),
                Instruction::I32ShrU,
                Instruction::I32Const(0xF),
                Instruction::I32And,
                Instruction::LocalSet(6),
            ]);
            // dest = result_ptr + out_len ; written at offset +5 (after length+quote)
            v.extend([
                Instruction::LocalGet(2),
                Instruction::LocalGet(4),
                Instruction::I32Add,
                // value = nibble + 48 + (nibble >= 10 ? 39 : 0)
                Instruction::LocalGet(6),
                Instruction::I32Const(48),
                Instruction::I32Add,
                Instruction::LocalGet(6),
                Instruction::I32Const(10),
                Instruction::I32GeU,
                Instruction::I32Const(39),
                Instruction::I32Mul,
                Instruction::I32Add,
                Instruction::I32Store8(wasm_encoder::MemArg { offset: 5, align: 0, memory_index: 0 }),
                // out_len++
                Instruction::LocalGet(4),
                Instruction::I32Const(1),
                Instruction::I32Add,
                Instruction::LocalSet(4),
            ]);
        }

        let mut v: Vec<Instruction<'static>> = Vec::new();

        // orig_len = mem[str_ptr]
        v.extend([
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1),
        ]);
        // result_ptr = malloc(orig_len * 6 + 6)
        v.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(6),
            Instruction::I32Mul,
            Instruction::I32Const(6),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(2),
        ]);
        // Write opening '"' at result_ptr+4
        v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Const(34),
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
        ]);
        // i = 0, out_len = 0
        v.extend([
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            Instruction::I32Const(0),
            Instruction::LocalSet(4),
        ]);
        // block $break { loop $continue {
        v.extend([
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
        ]);
        // if i >= orig_len: break
        v.extend([
            Instruction::LocalGet(3),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1),
        ]);
        // current_byte = mem[str_ptr + 4 + i]
        v.extend([
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
            Instruction::LocalSet(5),
        ]);

        // Dispatch ladder. Each named-escape gets its own If/Else; the final
        // `Else` is a control-byte check that ends in either \u00XX or pass-through.
        // The order is: \  "  BS  TAB  LF  FF  CR  (control-byte fallback).
        // Named cases come first so they win even when their value is < 0x20
        // (BS, TAB, LF, FF, CR all are — without the named cases first the
        // generic control-byte branch would still escape them, but as \u00XX
        // instead of the more readable named form RFC 8259 prefers).
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(92),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 92);
        v.push(Instruction::Else);
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(34),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 34);
        v.push(Instruction::Else);
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(8), // BS
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 98); // 'b'
        v.push(Instruction::Else);
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(9), // TAB
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 116); // 't'
        v.push(Instruction::Else);
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(10), // LF
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 110); // 'n'
        v.push(Instruction::Else);
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(12), // FF
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 102); // 'f'
        v.push(Instruction::Else);
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(13), // CR
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92);
        wb(&mut v, 114); // 'r'
        v.push(Instruction::Else);
        // Control bytes 0x00..0x1F that didn't match a named escape → \u00XX
        v.extend([
            Instruction::LocalGet(5),
            Instruction::I32Const(0x20),
            Instruction::I32LtU,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        wb(&mut v, 92); // '\'
        wb(&mut v, 117); // 'u'
        wb(&mut v, 48); // '0'
        wb(&mut v, 48); // '0'
        wb_hex(&mut v, 4);
        wb_hex(&mut v, 0);
        v.push(Instruction::Else);
        // Pass-through for printable / UTF-8 bytes
        v.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(4),
            Instruction::I32Add,
            Instruction::LocalGet(5),
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(4),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(4),
        ]);
        // Close 8 if/else blocks (one per named escape + the control-byte branch)
        v.extend([
            Instruction::End,
            Instruction::End,
            Instruction::End,
            Instruction::End,
            Instruction::End,
            Instruction::End,
            Instruction::End,
            Instruction::End,
        ]);
        // i++; continue loop
        v.extend([
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
        ]);
        // end loop; end block
        v.extend([Instruction::End, Instruction::End]);

        // Write closing '"' at result_ptr+5+out_len
        v.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(4),
            Instruction::I32Add,
            Instruction::I32Const(34),
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
        ]);
        // Store final length (out_len + 2) at result_ptr
        v.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(4),
            Instruction::I32Const(2),
            Instruction::I32Add,
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]);
        // Return result_ptr
        v.push(Instruction::LocalGet(2));
        v
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
        let mut instrs = vec![
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
        ];
        instrs.extend(Self::compact_bool_box_inplace(4, 7, malloc_index));
        instrs.extend([
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
        ]);
        instrs
    }

    fn generate_stringify_object_instructions(
        &self,
        malloc_index: u32,
        string_concat_index: u32,
        quote_string_idx: u32,
        stringify_value_idx: u32,
    ) -> Vec<Instruction<'static>> {
        let mut instrs = vec![
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
        ];
        instrs.extend(Self::compact_bool_box_inplace(5, 9, malloc_index));
        instrs.extend([
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
        ]);
        instrs
    }

    /// Allocate a length-prefixed Clean string of `bytes` length and store the pointer
    /// in `out_local`. After execution the local holds the pointer; the string is
    /// `[i32 len][raw bytes…]`. Used by helpers that emit short literal strings such
    /// as `"["`, `"]"`, `","`, `":"`, `"true"`, `"false"`, `"null"`.
    fn emit_alloc_const_str(
        v: &mut Vec<Instruction<'static>>,
        bytes: &[u8],
        malloc_idx: u32,
        out_local: u32,
    ) {
        let len = bytes.len() as i32;
        v.extend([
            Instruction::I32Const(4 + len),
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(out_local),
            Instruction::LocalGet(out_local),
            Instruction::I32Const(len),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]);
        for (i, &b) in bytes.iter().enumerate() {
            v.extend([
                Instruction::LocalGet(out_local),
                Instruction::I32Const(b as i32),
                Instruction::I32Store8(MemArg {
                    offset: (4 + i) as u64,
                    align: 0,
                    memory_index: 0,
                }),
            ]);
        }
    }

    /// Emit a dispatch on `tag_local` that encodes the raw i32 value in `val_local`
    /// as a JSON-compatible Clean string and leaves the resulting string pointer in
    /// `out_local`. Handles tags `1` (Integer), `2` (Boolean), `4` (String); anything
    /// else falls through to `"null"`. The Number tag (`3`) is f64 and is handled
    /// separately at call sites (see `generate_encode_cln_list_instructions`).
    fn emit_encode_primitive_i32(
        v: &mut Vec<Instruction<'static>>,
        val_local: u32,
        tag_local: u32,
        out_local: u32,
        int_to_string_idx: u32,
        quote_string_idx: u32,
        malloc_idx: u32,
    ) {
        // if tag == 1 (Integer)
        v.extend([
            Instruction::LocalGet(tag_local),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(val_local),
            Instruction::Call(int_to_string_idx),
            Instruction::LocalSet(out_local),
            Instruction::Else,
            // elif tag == 2 (Boolean)
            Instruction::LocalGet(tag_local),
            Instruction::I32Const(2),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // if val != 0: "true" else "false"
            Instruction::LocalGet(val_local),
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        Self::emit_alloc_const_str(v, b"true", malloc_idx, out_local);
        v.push(Instruction::Else);
        Self::emit_alloc_const_str(v, b"false", malloc_idx, out_local);
        v.extend([
            Instruction::End,
            Instruction::Else,
            // elif tag == 4 (String) — val is a Clean string pointer; quote it for JSON
            Instruction::LocalGet(tag_local),
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(val_local),
            Instruction::Call(quote_string_idx),
            Instruction::LocalSet(out_local),
            Instruction::Else,
            // default — emit "null"
        ]);
        Self::emit_alloc_const_str(v, b"null", malloc_idx, out_local);
        v.extend([Instruction::End, Instruction::End, Instruction::End]);
    }

    /// Generate WASM body for `__json_encode_cln_list(list_ptr: i32, elem_tag: i32) -> i32`.
    ///
    /// Walks a native Clean list (header `[len:i32@0][cap:i32@4][type_id:i32@8][pad@12]`,
    /// elements from offset 16) and encodes each element directly into a JSON array
    /// string. Element stride is 8 bytes when `elem_tag == 3` (Number/f64) and 4 bytes
    /// otherwise. Used by the `json.encode(List<T>)` dispatch in `mir_builder/expressions.rs`
    /// when `T` is a primitive — for richer element types (nested collections, class
    /// instances) the dispatch is expected to take a different path.
    fn generate_encode_cln_list_instructions(
        &self,
        malloc_idx: u32,
        string_concat_idx: u32,
        int_to_string_idx: u32,
        float_to_string_idx: u32,
        quote_string_idx: u32,
    ) -> Vec<Instruction<'static>> {
        // Locals (declared via register_stdlib_function_with_locals):
        //   0: list_ptr (param)        — base of the Clean list
        //   1: elem_tag (param)        — element AnyTypeTag (1,2,3,4)
        //   2: count                   — number of elements
        //   3: i                       — loop counter
        //   4: result_ptr              — accumulating result string
        //   5: elem_str                — encoded current element
        //   6: elem_addr               — list_ptr + 16 + i*stride
        //   7: elem_i32                — raw i32 element (for non-Number tags)
        //   8: temp_buf                — scratch for "[", "]", "," etc.
        //   9: elem_stride             — 4 or 8
        //  10: elem_f64                — f64 element (for tag 3)
        //
        // Note: no null guard. Address 0 is a valid list pointer in this runtime
        // because the bump allocator's heap state lives at `mem[0]`, so the first
        // list materializes at address 0. The empty-list check below handles the
        // genuinely empty case.
        const LIST_DATA_OFFSET: u32 = 16;
        let mut v: Vec<Instruction<'static>> = Vec::new();

        // count = mem[list_ptr + 0]
        v.extend([
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2),
        ]);

        // empty? return "[]"
        v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
        ]);
        Self::emit_alloc_const_str(&mut v, b"[]", malloc_idx, 4);
        v.extend([Instruction::LocalGet(4), Instruction::Else]);

        // elem_stride = (elem_tag == 3) ? 8 : 4
        v.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(8),
            Instruction::Else,
            Instruction::I32Const(4),
            Instruction::End,
            Instruction::LocalSet(9),
        ]);

        // result = "["
        Self::emit_alloc_const_str(&mut v, b"[", malloc_idx, 4);

        // i = 0
        v.extend([
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // if i >= count: break
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // if i > 0: result = concat(result, ",")
            Instruction::LocalGet(3),
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        Self::emit_alloc_const_str(&mut v, b",", malloc_idx, 8);
        v.extend([
            Instruction::LocalGet(4),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
            Instruction::End,
            // elem_addr = list_ptr + 16 + i * elem_stride
            Instruction::LocalGet(0),
            Instruction::I32Const(LIST_DATA_OFFSET as i32),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::LocalGet(9),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(6),
            // Dispatch by tag: tag 3 (Number/f64) loads f64 and converts; others load i32
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(6),
            Instruction::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::LocalSet(10),
            Instruction::LocalGet(10),
            Instruction::Call(float_to_string_idx),
            Instruction::LocalSet(5),
            Instruction::Else,
            Instruction::LocalGet(6),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(7),
        ]);
        Self::emit_encode_primitive_i32(
            &mut v,
            7,
            1,
            5,
            int_to_string_idx,
            quote_string_idx,
            malloc_idx,
        );
        v.extend([
            Instruction::End,
            // result = concat(result, elem_str)
            Instruction::LocalGet(4),
            Instruction::LocalGet(5),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
            // i++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
        ]);

        // result = concat(result, "]")
        Self::emit_alloc_const_str(&mut v, b"]", malloc_idx, 8);
        v.extend([
            Instruction::LocalGet(4),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
            // emit final result
            Instruction::LocalGet(4),
            Instruction::End, // end count==0 if
        ]);

        v
    }

    /// Generate WASM body for `__json_encode_cln_pairs(pairs_ptr: i32, val_tag: i32) -> i32`.
    ///
    /// Walks a native Clean pairs map (header `[count:i32@0][capacity:i32@4]`, entries
    /// from offset 8 as `[key_ptr:i32][val:i32]` of 8 bytes each) and encodes each
    /// entry as `"key":value` directly into a JSON object string. Keys are Clean
    /// strings and are quoted via `__json_quote_string`. Values are dispatched by
    /// `val_tag` (`1`=Integer, `2`=Boolean, `4`=String); other tags emit `null`.
    /// `pairs<K,V>` cannot hold `V=number` because pairs entries are 4 bytes — the
    /// type checker rejects that combination, so a tag-3 case is not handled here.
    fn generate_encode_cln_pairs_instructions(
        &self,
        malloc_idx: u32,
        string_concat_idx: u32,
        int_to_string_idx: u32,
        quote_string_idx: u32,
    ) -> Vec<Instruction<'static>> {
        // Locals:
        //   0: pairs_ptr (param)
        //   1: val_tag (param)
        //   2: count
        //   3: i
        //   4: result_ptr
        //   5: entry_addr
        //   6: key_ptr
        //   7: val_i32
        //   8: key_str
        //   9: val_str
        //  10: temp_buf
        //
        // Note: no null guard. See `generate_encode_cln_list_instructions` —
        // address 0 is a valid pointer in this runtime's bump allocator.
        const PAIRS_HEADER_SIZE: u32 = 8;
        const PAIRS_ENTRY_SIZE: u32 = 8;
        let mut v: Vec<Instruction<'static>> = Vec::new();

        // count = mem[pairs_ptr + 0]
        v.extend([
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2),
        ]);

        // empty? return "{}"
        v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
        ]);
        Self::emit_alloc_const_str(&mut v, b"{}", malloc_idx, 4);
        v.extend([Instruction::LocalGet(4), Instruction::Else]);

        // result = "{"
        Self::emit_alloc_const_str(&mut v, b"{", malloc_idx, 4);

        // i = 0
        v.extend([
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // if i >= count: break
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // if i > 0: result = concat(result, ",")
            Instruction::LocalGet(3),
            Instruction::If(wasm_encoder::BlockType::Empty),
        ]);
        Self::emit_alloc_const_str(&mut v, b",", malloc_idx, 10);
        v.extend([
            Instruction::LocalGet(4),
            Instruction::LocalGet(10),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
            Instruction::End,
            // entry_addr = pairs_ptr + 8 + i * 8
            Instruction::LocalGet(0),
            Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(5),
            // key_ptr = mem[entry_addr + 0]
            Instruction::LocalGet(5),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(6),
            // val_i32 = mem[entry_addr + 4]
            Instruction::LocalGet(5),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(7),
            // key_str = quote_string(key_ptr)
            Instruction::LocalGet(6),
            Instruction::Call(quote_string_idx),
            Instruction::LocalSet(8),
            // result = concat(result, key_str)
            Instruction::LocalGet(4),
            Instruction::LocalGet(8),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
        ]);
        // result = concat(result, ":")
        Self::emit_alloc_const_str(&mut v, b":", malloc_idx, 10);
        v.extend([
            Instruction::LocalGet(4),
            Instruction::LocalGet(10),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
        ]);
        // val_str = encode_primitive(val_i32, val_tag)
        Self::emit_encode_primitive_i32(
            &mut v,
            7,
            1,
            9,
            int_to_string_idx,
            quote_string_idx,
            malloc_idx,
        );
        v.extend([
            // result = concat(result, val_str)
            Instruction::LocalGet(4),
            Instruction::LocalGet(9),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
            // i++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
        ]);

        // result = concat(result, "}")
        Self::emit_alloc_const_str(&mut v, b"}", malloc_idx, 10);
        v.extend([
            Instruction::LocalGet(4),
            Instruction::LocalGet(10),
            Instruction::Call(string_concat_idx),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::End, // end count==0 if
        ]);

        v
    }

    /// Generate WASM body for `__json_from_cln_list(list_ptr: i32, elem_tag: i32) -> i32`.
    ///
    /// Converts a native Clean list (header `[len@0][cap@4][type_id@8][pad@12]`,
    /// raw elements from offset 16) into the JSON-array tree layout
    /// `[count:i32][boxed_elem_ptr_0:i32]…` that `__json_stringify_array`
    /// expects. Each element is wrapped in a fresh 12-byte boxed Any
    /// `[tag][val_lo][val_hi]` so the recursive stringify walks correctly even
    /// when the Any value is later embedded in a JSON object literal.
    /// Element stride is 8 bytes for `elem_tag == 3` (Number/f64) and 4 bytes
    /// for every other tag.
    ///
    /// Used by `emit_box_any` in `mir_builder/types.rs` whenever a typed list
    /// is boxed to Any — the box's payload is the converter's result rather
    /// than the raw Clean list pointer.
    fn generate_from_cln_list_instructions(&self, malloc_idx: u32) -> Vec<Instruction<'static>> {
        // Locals:
        //   0: list_ptr (param)
        //   1: elem_tag (param)
        //   2: count
        //   3: i
        //   4: elem_addr (in clean list)
        //   5: box_ptr  (12-byte boxed Any per element)
        //   6: json_array_ptr (result)
        //   7: dest_slot_addr (in json array)
        //   8: elem_stride (4 or 8)
        const LIST_DATA_OFFSET: u32 = 16;
        let mut v: Vec<Instruction<'static>> = Vec::new();

        // count = mem[list_ptr + 0]
        v.extend([
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2),
        ]);

        // json_array_ptr = malloc(4 + count * 4)
        v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(6),
        ]);

        // Store count at offset 0 of the JSON array
        v.extend([
            Instruction::LocalGet(6),
            Instruction::LocalGet(2),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]);

        // elem_stride = (elem_tag == 3) ? 8 : 4
        v.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(8),
            Instruction::Else,
            Instruction::I32Const(4),
            Instruction::End,
            Instruction::LocalSet(8),
        ]);

        // i = 0
        v.extend([
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // if i >= count: break
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // elem_addr = list_ptr + 16 + i * elem_stride
            Instruction::LocalGet(0),
            Instruction::I32Const(LIST_DATA_OFFSET as i32),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::LocalGet(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(4),
            // box_ptr = malloc(12)
            Instruction::I32Const(12),
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(5),
            // box_ptr[0] = elem_tag
            Instruction::LocalGet(5),
            Instruction::LocalGet(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store the value: Number (tag 3) writes a full f64 at offset 4
            // (spanning bytes 4..12, which is where stringify_value's F64Load
            // reads). Other tags write a single i32 at offset 4 and leave
            // offset 8 zero. The store path is selected by a runtime branch
            // on elem_tag because we do not know the type statically here.
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // f64 case
            Instruction::LocalGet(5),
            Instruction::LocalGet(4),
            Instruction::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Store(MemArg {
                offset: 4,
                align: 3,
                memory_index: 0,
            }),
            Instruction::Else,
            // i32 case
            Instruction::LocalGet(5),
            Instruction::LocalGet(4),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Zero the padding at offset 8 so stringify_value never reads
            // garbage when it walks past the i32 payload (e.g. for an unboxed
            // f64 reinterpret of a tag=1 box, which would otherwise be
            // implementation-defined behavior).
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::End,
            // dest_slot_addr = json_array_ptr + 4 + i * 4
            Instruction::LocalGet(6),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(7),
            // mem[dest_slot_addr] = box_ptr
            Instruction::LocalGet(7),
            Instruction::LocalGet(5),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // i++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
            Instruction::LocalGet(6),
        ]);

        v
    }

    /// Generate WASM body for `__json_from_cln_pairs(pairs_ptr: i32, val_tag: i32) -> i32`.
    ///
    /// Converts a native Clean pairs map (header `[count@0][cap@4]`, entries
    /// from offset 8 as `[key_ptr][val]` of 8 bytes) into the JSON-object
    /// tree layout `[count:i32][key_ptr][boxed_val_ptr]…` that
    /// `__json_stringify_object` expects. Each value is wrapped in a fresh
    /// 12-byte boxed Any tagged with `val_tag`; the key string pointer is
    /// reused as-is (strings are immutable and `__json_quote_string` does
    /// not mutate them).
    fn generate_from_cln_pairs_instructions(&self, malloc_idx: u32) -> Vec<Instruction<'static>> {
        // Locals:
        //   0: pairs_ptr (param)
        //   1: val_tag (param)
        //   2: count
        //   3: i
        //   4: entry_addr (in clean pairs)
        //   5: key_ptr
        //   6: val_raw
        //   7: json_object_ptr (result)
        //   8: box_ptr (12-byte boxed Any per entry)
        //   9: dest_entry_addr (in json object)
        const PAIRS_HEADER_SIZE: u32 = 8;
        const PAIRS_ENTRY_SIZE: u32 = 8;
        let mut v: Vec<Instruction<'static>> = Vec::new();

        // count = mem[pairs_ptr + 0]
        v.extend([
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2),
        ]);

        // json_object_ptr = malloc(4 + count * 8)
        v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(7),
        ]);

        // Store count
        v.extend([
            Instruction::LocalGet(7),
            Instruction::LocalGet(2),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]);

        // i = 0
        v.extend([
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            // if i >= count: break
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // entry_addr = pairs_ptr + 8 + i * 8
            Instruction::LocalGet(0),
            Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(4),
            // key_ptr = mem[entry_addr + 0]
            Instruction::LocalGet(4),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(5),
            // val_raw = mem[entry_addr + 4]
            Instruction::LocalGet(4),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(6),
            // box_ptr = malloc(12)
            Instruction::I32Const(12),
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(8),
            // box_ptr[0] = val_tag
            Instruction::LocalGet(8),
            Instruction::LocalGet(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // box_ptr[4] = val_raw  (pairs values are always i32 per spec —
            // pairs<K, number> is rejected by the typechecker because pairs
            // entries are only 4 bytes wide)
            Instruction::LocalGet(8),
            Instruction::LocalGet(6),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // box_ptr[8] = 0
            Instruction::LocalGet(8),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // dest_entry_addr = json_object_ptr + 4 + i * 8
            Instruction::LocalGet(7),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(9),
            // mem[dest_entry_addr + 0] = key_ptr
            Instruction::LocalGet(9),
            Instruction::LocalGet(5),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // mem[dest_entry_addr + 4] = box_ptr
            Instruction::LocalGet(9),
            Instruction::LocalGet(8),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // i++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
            Instruction::LocalGet(7),
        ]);

        v
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

    /// Generate WASM instructions for __json_skip_string
    ///
    /// Advances the position stored at `position_ptr` past a complete JSON string,
    /// including correctly handling backslash escape sequences so that a `\"` inside
    /// a string does not terminate the scan prematurely.
    ///
    /// Precondition: position_ptr holds the index of the opening `"` character.
    /// Postcondition: position_ptr holds the index of the byte *after* the closing `"`.
    ///
    /// Parameters:
    /// - Local 0: string_ptr  (i32)  — pointer to Clean Language string [len][bytes…]
    /// - Local 1: position_ptr (i32) — pointer to i32 position cell
    /// - Local 2: length (i32)       — total byte length of the JSON text
    ///
    /// Working locals:
    /// - Local 3: position (cached)
    /// - Local 4: current_char
    fn generate_skip_string_instructions(&self) -> Vec<Instruction<'static>> {
        vec![
            // Load position from position_ptr into local cache
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Skip the opening '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Scan loop: advance until we find the unescaped closing '"'
            Instruction::Block(wasm_encoder::BlockType::Empty), // outer_block  exit=1
            Instruction::Loop(wasm_encoder::BlockType::Empty),  // scan_loop    restart=0
            // bounds check
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // exit outer_block (end of input, no closing quote)
            // load char at string_ptr + 4 + position
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
            // position++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // if char == '"': closing quote found → exit
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1), // exit outer_block
            // if char == '\\': skip the next byte (escaped character)
            Instruction::LocalGet(4),
            Instruction::I32Const(92), // '\\'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3), // skip escaped char
            Instruction::End,
            Instruction::Br(0), // continue scan_loop
            Instruction::End,   // end scan_loop
            Instruction::End,   // end outer_block
            // Write updated position back to position_ptr
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WASM instructions for __json_parse_string
    ///
    /// Parses a JSON string starting at the `"` whose position is in `position_ptr`.
    /// Allocates a new Clean Language string buffer, writes decoded bytes (including
    /// fully-resolved escape sequences), stores the final length, updates `position_ptr`
    /// to point past the closing `"`, and returns a pointer to the new string.
    ///
    /// Escape handling:
    ///   `\"` → 34   `\\` → 92   `\/` → 47   `\n` → 10   `\r` → 13
    ///   `\t` →  9   `\b` →  8   `\f` → 12   `\uXXXX` → '?' (63) placeholder
    ///   any other `\X` → X (pass-through)
    ///
    /// Parameters:
    /// - Local 0: string_ptr   (i32) — pointer to Clean Language string [len][bytes…]
    /// - Local 1: position_ptr (i32) — pointer to i32 position cell (AT opening `"`)
    /// - Local 2: length       (i32) — total JSON byte length
    ///
    /// Working locals:
    /// - Local 3: position (cached)
    /// - Local 4: current_char
    /// - Local 5: out_ptr (allocated output buffer)
    /// - Local 6: out_write_pos (number of bytes written so far)
    /// - Local 7: next_char (for escape processing)
    fn generate_parse_string_instructions(&self, malloc_index: u32) -> Vec<Instruction<'static>> {
        vec![
            // Load position from position_ptr
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Skip the opening '"'
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Allocate output buffer: 4-byte length header + at most `length` content bytes
            // The worst case is that every byte of the original JSON maps to one output byte.
            Instruction::I32Const(4),
            Instruction::LocalGet(2),
            Instruction::I32Add,
            Instruction::Call(malloc_index),
            Instruction::LocalSet(5), // out_ptr
            // out_write_pos = 0
            Instruction::I32Const(0),
            Instruction::LocalSet(6),
            // Parse loop
            Instruction::Block(wasm_encoder::BlockType::Empty), // outer_block  exit=1
            Instruction::Loop(wasm_encoder::BlockType::Empty),  // parse_loop   restart=0
            // bounds check
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1), // exit if end of input
            // load char at string_ptr + 4 + position
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
            // position++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // if char == '"': end of string
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::BrIf(1), // exit outer_block
            // if char == '\\': handle escape sequence
            Instruction::LocalGet(4),
            Instruction::I32Const(92), // '\\'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // bounds check before reading next char
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::Br(3), // exit outer_block (truncated escape at end of input)
            Instruction::End,
            // read next_char
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
            Instruction::LocalSet(7),
            // position++
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // Map next_char → output byte, stored back into local 4
            // '"' (34)
            Instruction::LocalGet(7),
            Instruction::I32Const(34),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(34),
            Instruction::LocalSet(4),
            Instruction::Else,
            // '\\' (92)
            Instruction::LocalGet(7),
            Instruction::I32Const(92),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(92),
            Instruction::LocalSet(4),
            Instruction::Else,
            // '/' (47)
            Instruction::LocalGet(7),
            Instruction::I32Const(47),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(47),
            Instruction::LocalSet(4),
            Instruction::Else,
            // 'n' (110) → newline (10)
            Instruction::LocalGet(7),
            Instruction::I32Const(110),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(10),
            Instruction::LocalSet(4),
            Instruction::Else,
            // 'r' (114) → carriage return (13)
            Instruction::LocalGet(7),
            Instruction::I32Const(114),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(13),
            Instruction::LocalSet(4),
            Instruction::Else,
            // 't' (116) → tab (9)
            Instruction::LocalGet(7),
            Instruction::I32Const(116),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(9),
            Instruction::LocalSet(4),
            Instruction::Else,
            // 'b' (98) → backspace (8)
            Instruction::LocalGet(7),
            Instruction::I32Const(98),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(8),
            Instruction::LocalSet(4),
            Instruction::Else,
            // 'f' (102) → form feed (12)
            Instruction::LocalGet(7),
            Instruction::I32Const(102),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(12),
            Instruction::LocalSet(4),
            Instruction::Else,
            // 'u' (117) → \uXXXX: skip 4 hex digits, emit '?' (63)
            Instruction::LocalGet(7),
            Instruction::I32Const(117),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalSet(3), // skip 4 hex digits
            Instruction::I32Const(63),
            Instruction::LocalSet(4), // '?'
            Instruction::Else,
            // unknown escape: pass next_char through unchanged
            Instruction::LocalGet(7),
            Instruction::LocalSet(4),
            Instruction::End, // 'u'
            Instruction::End, // 'f'
            Instruction::End, // 'b'
            Instruction::End, // 't'
            Instruction::End, // 'r'
            Instruction::End, // 'n'
            Instruction::End, // '/'
            Instruction::End, // '\\'
            Instruction::End, // '"'
            // Write the decoded byte (local 4) to out_ptr[4 + out_write_pos]
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(6),
            Instruction::I32Add,
            Instruction::LocalGet(4),
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // out_write_pos++
            Instruction::Br(1),       // continue parse_loop
            Instruction::End,         // end escape if
            // Not an escape: write the raw byte directly
            Instruction::LocalGet(5),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(6),
            Instruction::I32Add,
            Instruction::LocalGet(4),
            Instruction::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(6),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // out_write_pos++
            Instruction::Br(0),       // continue parse_loop
            Instruction::End,         // end parse_loop
            Instruction::End,         // end outer_block
            // Store the actual byte count into the length header at out_ptr[0]
            Instruction::LocalGet(5),
            Instruction::LocalGet(6),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Write updated position back to position_ptr
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return out_ptr (pointer to new Clean Language string)
            Instruction::LocalGet(5),
        ]
    }

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
        parse_string_index: u32,
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
            // Check for '"' (34) - string: delegate to __json_parse_string
            Instruction::LocalGet(4),
            Instruction::I32Const(34),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            // Write cached position back to position_ptr before the call
            // (position currently points AT the '"'; parse_string expects that)
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Call __json_parse_string(string_ptr, position_ptr, length) -> str_ptr
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_string_index),
            Instruction::LocalSet(5), // str_ptr
            // Read updated position back from position_ptr
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // refresh cached position
            // Box the string: allocate 12 bytes for boxed any
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
    ///
    /// Local variable layout:
    /// - Local 0: string_ptr (parameter)
    /// - Local 1: position_ptr (parameter - memory location)
    /// - Local 2: length (parameter)
    /// - Local 3: position (cached from position_ptr)
    /// - Local 4: current_character
    /// - Local 5: pair_count
    /// - Local 6: object_ptr (allocated memory)
    /// - Local 7: loop counter i
    /// - Local 8: start_position / key_start / num_start / str_start
    /// - Local 9: key_len / str_len / parse_pos
    /// - Local 10: key_ptr / is_negative / temp / depth
    /// - Local 11: value_ptr / str_ptr
    /// - Local 12: temp
    /// - Local 13: temp
    /// - Local 14: decimal_divisor (F64)
    fn generate_parse_object_instructions(
        &self,
        parse_value_index: u32,
        skip_string_index: u32,
        parse_string_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        let mut instrs = Vec::new();
        instrs.extend(self.generate_parse_object_init_instructions());
        instrs.extend(self.generate_parse_object_count_pairs_instructions(skip_string_index));
        instrs.extend(self.generate_parse_object_alloc_instructions(malloc_index));
        instrs.extend(self.generate_parse_object_parse_pairs_instructions(
            parse_value_index,
            parse_string_index,
            malloc_index,
        ));
        instrs.extend(self.generate_parse_object_finalize_instructions());
        instrs
    }

    /// Phase 1: Read position from memory into local cache and skip the opening '{'.
    ///
    /// Reads: Local 1 (position_ptr), Local 2 (length)
    /// Writes: Local 3 (position), Local 8 (start_position), Local 5 (pair_count)
    fn generate_parse_object_init_instructions(&self) -> Vec<Instruction<'static>> {
        vec![
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
        ]
    }

    /// Phase 2: Scan forward from current position to count the number of key-value pairs.
    ///
    /// Uses depth tracking and calls `__json_skip_string` when a `"` is encountered so
    /// that escape sequences inside strings cannot be misinterpreted as structural chars.
    ///
    /// Reads:  Local 0 (string_ptr), Local 1 (position_ptr), Local 2 (length),
    ///         Local 3 (position), Local 5 (pair_count)
    /// Writes: Local 3 (position advanced past entire object),
    ///         Local 4 (char), Local 5 (pair_count), Local 10 (depth)
    fn generate_parse_object_count_pairs_instructions(
        &self,
        skip_string_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // outer_block / outer_loop: iterate over top-level key-value pairs
            Instruction::Block(wasm_encoder::BlockType::Empty), // outer_block  label=1
            Instruction::Loop(wasm_encoder::BlockType::Empty),  // outer_loop   label=0
            // --- skip leading whitespace ---
            Instruction::Block(wasm_encoder::BlockType::Empty), // ws_block     label=1 (inner)
            Instruction::Loop(wasm_encoder::BlockType::Empty),  // ws_loop      label=0 (inner)
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // exit outer_block if end of input
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
            // is whitespace?
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
            Instruction::BrIf(1), // non-whitespace: exit ws_block
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0), // continue ws_loop
            Instruction::End,   // end ws_loop
            Instruction::End,   // end ws_block
            // --- check for '}' that ends the object ---
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::BrIf(1), // exit outer_loop
            // --- found a key: pair_count++ ---
            Instruction::LocalGet(5),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5),
            // --- skip this entire key:value pair to find next ',' or '}' ---
            // depth tracks nesting inside nested objects/arrays
            Instruction::I32Const(0),
            Instruction::LocalSet(10), // depth = 0
            // skip_block / skip_loop: advance position to end of this pair
            Instruction::Block(wasm_encoder::BlockType::Empty), // skip_block  label=1
            Instruction::Loop(wasm_encoder::BlockType::Empty),  // skip_loop   label=0
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // end-of-input → exit outer_block
            // peek char, then advance position
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
            Instruction::LocalSet(3), // position++
            // --- if '"': skip the whole string via __json_skip_string ---
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // position is now one past the opening '"'; write (position-1) to position_ptr
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // call __json_skip_string(string_ptr, position_ptr, length)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(skip_string_index),
            // read updated position back
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::Br(0), // continue skip_loop
            Instruction::End,
            // --- '{' or '[': depth++ ---
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
            Instruction::LocalSet(10),
            Instruction::Br(1), // continue skip_loop
            Instruction::End,
            // --- '}' or ']' ---
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // depth == 0 → this is the outer closing brace: exit skip_block.
            // Br index: from inside two nested Ifs (outer brace If + inner depth==0 If)
            // the label stack is [innerIf, outerIf, skip_loop(Loop), skip_block(Block)].
            // Br(3) targets skip_block. Br(2) would restart skip_loop (Loop labels mean
            // "go to start"), which lets the scanner march past the object's closing
            // brace and miscounts characters in the next array element as additional
            // key/value pairs (STDLIB-JSON-INDEX-OBJECT-ARRAY-PAST-ZERO).
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::Br(3), // exit skip_block
            Instruction::End,
            // depth > 0: decrement and continue
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10),
            Instruction::Br(1), // continue skip_loop
            Instruction::End,
            // --- ',' at depth 0: separator between pairs; exit skip_block ---
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::I32And,
            Instruction::BrIf(1), // exit skip_block
            Instruction::Br(0),   // continue skip_loop
            Instruction::End,     // end skip_loop
            Instruction::End,     // end skip_block
            // If we exited because of '}': end the outer loop too
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::BrIf(1), // exit outer_loop
            Instruction::Br(0),   // continue outer_loop
            Instruction::End,     // end outer_loop
            Instruction::End,     // end outer_block
        ]
    }

    /// Phase 3: Allocate object memory and reset position for the parse pass.
    ///
    /// Allocates `4 + pair_count * 8` bytes (count header + key/value pointer pairs),
    /// stores the pair count at offset 0, resets position to `start_position`, and
    /// initialises the loop counter `i = 0`.
    ///
    /// Reads: Local 5 (pair_count), Local 8 (start_position)
    /// Writes: Local 6 (object_ptr), Local 3 (position reset), Local 7 (i = 0)
    fn generate_parse_object_alloc_instructions(
        &self,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
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
        ]
    }

    /// Phase 4: Outer loop that parses each key-value pair and stores pointers in the object.
    ///
    /// For each pair: skips leading whitespace and any preceding comma, parses the key string
    /// (allocates and copies it), skips `:`, then dispatches on the first byte of the value
    /// to parse numbers (integer/float), strings, booleans, null, or nested structures via a
    /// recursive call to `parse_value`. After storing the value pointer, advances past any
    /// trailing separator before incrementing the loop counter.
    ///
    /// This block must remain monolithic because all `BrIf` depth offsets are relative to
    /// the nesting established by the enclosing `Block + Loop` opened here.
    fn generate_parse_object_parse_pairs_instructions(
        &self,
        parse_value_index: u32,
        parse_string_index: u32,
        malloc_index: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
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
            // Position is currently AT the opening '"'
            // Write position to position_ptr and call __json_parse_string
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Call __json_parse_string(string_ptr, position_ptr, length) → key_ptr
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_string_index),
            Instruction::LocalSet(10), // key_ptr
            // Read updated position back
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
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
            // Position is AT the opening '"'. Write to position_ptr and call __json_parse_string.
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Call __json_parse_string(string_ptr, position_ptr, length) → str_ptr
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_string_index),
            Instruction::LocalSet(11), // str_ptr
            // Read updated position back
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Box the string: allocate 12 bytes for boxed structure: [tag=4][str_ptr][padding]
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
            // Store boxed pointer in object
            Instruction::LocalGet(6), // object_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(12), // boxed_ptr
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
        ]
    }

    /// Phase 5: Write the final position back to the position_ptr memory location and return
    /// the object pointer.
    ///
    /// Reads: Local 1 (position_ptr), Local 3 (cached position), Local 6 (object_ptr)
    fn generate_parse_object_finalize_instructions(&self) -> Vec<Instruction<'static>> {
        vec![
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
        skip_string_index: u32,
        parse_string_index: u32,
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
            // Uses __json_skip_string when a '"' is seen (escape-aware)
            Instruction::I32Const(0),
            Instruction::LocalSet(10),                          // depth = 0
            Instruction::Block(wasm_encoder::BlockType::Empty), // skip_block  label=1
            Instruction::Loop(wasm_encoder::BlockType::Empty),  // skip_loop   label=0
            // Check bounds
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(3), // Exit to outer counting block
            // Peek char, advance position
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
            // '"': skip entire string via __json_skip_string
            Instruction::LocalGet(4),
            Instruction::I32Const(34), // '"'
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // write (position-1) to position_ptr so skip_string starts at opening '"'
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(skip_string_index),
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            Instruction::Br(0), // continue skip_loop
            Instruction::End,
            // '{' or '[': depth++
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
            Instruction::Br(1),        // continue skip_loop
            Instruction::End,
            // '}' or ']'
            Instruction::LocalGet(4),
            Instruction::I32Const(125), // '}'
            Instruction::I32Eq,
            Instruction::LocalGet(4),
            Instruction::I32Const(93), // ']'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // depth == 0: outer close → exit skip_block.
            // Br index: from inside two nested Ifs (outer brace If + inner depth==0 If)
            // the label stack is [innerIf, outerIf, skip_loop(Loop), skip_block(Block)].
            // Br(3) targets skip_block. Br(2) would restart skip_loop and cause the
            // counter to march past the array's `]` (STDLIB-JSON-INDEX-OBJECT-ARRAY-PAST-ZERO).
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::Br(3), // exit skip_block
            Instruction::End,
            // depth > 0: decrement
            Instruction::LocalGet(10),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(10),
            Instruction::Br(1), // continue skip_loop
            Instruction::End,
            // ',' at depth 0: element separator → exit skip_block
            Instruction::LocalGet(4),
            Instruction::I32Const(44), // ','
            Instruction::I32Eq,
            Instruction::LocalGet(10),
            Instruction::I32Eqz,
            Instruction::I32And,
            Instruction::BrIf(1), // exit skip_block
            Instruction::Br(0),   // continue skip_loop
            Instruction::End,     // end skip_loop
            Instruction::End,     // end skip_block
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
            // Position is AT the opening '"'. Write to position_ptr, call __json_parse_string.
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Call __json_parse_string(string_ptr, position_ptr, length) → str_ptr
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::Call(parse_string_index),
            Instruction::LocalSet(11), // str_ptr
            // Read updated position back
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Box the string as an any type value
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
        // Verify constants match the literal I32Const values emitted in the WASM instruction
        // generators. If these fail the constants diverge from the runtime representation.
        assert_eq!(JSON_TAG_NULL, 0, "null tag must be 0");
        assert_eq!(JSON_TAG_INTEGER, 1, "integer tag must be 1");
        assert_eq!(
            JSON_TAG_BOOLEAN, 2,
            "boolean tag must be 2 (parser writes I32Const(2))"
        );
        assert_eq!(
            JSON_TAG_NUMBER, 3,
            "number tag must be 3 (parser writes I32Const(3))"
        );
        assert_eq!(
            JSON_TAG_STRING, 4,
            "string tag must be 4 (parser writes I32Const(4))"
        );
        assert_eq!(
            JSON_TAG_ARRAY, 5,
            "array tag must be 5 (parser writes I32Const(5))"
        );
        assert_eq!(
            JSON_TAG_OBJECT, 6,
            "object tag must be 6 (parser writes I32Const(6))"
        );

        // Sanity: all tags distinct and in order
        let tags = [
            JSON_TAG_NULL,
            JSON_TAG_INTEGER,
            JSON_TAG_BOOLEAN,
            JSON_TAG_NUMBER,
            JSON_TAG_STRING,
            JSON_TAG_ARRAY,
            JSON_TAG_OBJECT,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j], "tag collision at indices {i} and {j}");
            }
        }
    }
}
