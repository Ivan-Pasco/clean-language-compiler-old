//! JSON Module for Clean Language
//!
//! JSON parsing, serialization and path extraction are Layer 2: they are the
//! host bridges `_json_decode_v2`, `_json_encode_v2`, `_json_encode_pretty_v2`
//! and `_json_get` (foundation/spec/stdlib-reference.md §8). The compiler
//! does not embed a JSON parser. What stays here is pure in-memory work on
//! boxed-Any values (foundation/spec/platform/BOXED_ANY_ABI.md):
//!
//! - `__json_get_field` / `__json_get_index` — member and element access for
//!   `data.key`, `data["key"]` and `data[0]` on a boxed object or array;
//! - `__json_from_cln_list` / `__json_from_cln_list_i64` /
//!   `__json_from_cln_pairs` — build the boxed-Any tree of a typed Clean
//!   list or pairs map when it is boxed to `any`;
//! - `__json_from_cln_list_boxed` — the same for a list whose elements are
//!   already boxed (`list<any>`, mapped lists of class instances);
//! - `__json_try_decode_v2` / `__json_decode_v2_or_raise` — the sentinel
//!   checks after a `_json_decode_v2` call (D5).
//!
//! BOOK: json-module

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use wasm_encoder::{Instruction, MemArg};

/// Boxed-Any type tags — 12-byte representation `[i32 tag][payload: 8 bytes]`
/// (BOXED_ANY_ABI.md §1-§2):
///
///   0 = null    → [0][0][0]
///   1 = integer → [1][i64 value]          (little-endian across offsets 4 and 8)
///   2 = boolean → [2][i32 0/1][0]
///   3 = number  → [3][f64 value]          (offsets 4..12)
///   4 = string  → [4][str_ptr][0]         (ptr to [i32 len][bytes…])
///   5 = array   → [5][raw_arr_ptr][0]     (ptr to [i32 count][elem0_ptr]…)
///   6 = object  → [6][raw_obj_ptr][0]     (ptr to [i32 count][key0_ptr][val0_ptr]…)
pub const JSON_TAG_NULL: i32 = 0;
pub const JSON_TAG_INTEGER: i32 = 1;
pub const JSON_TAG_BOOLEAN: i32 = 2;
pub const JSON_TAG_NUMBER: i32 = 3;
pub const JSON_TAG_STRING: i32 = 4;
pub const JSON_TAG_ARRAY: i32 = 5;
pub const JSON_TAG_OBJECT: i32 = 6;

/// JSON support functions registered into every module.
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

    /// Register the boxed-Any helpers listed in the module documentation.
    /// `json.*` calls themselves are lowered to the host bridges by the MIR
    /// builder (`mir_builder/expressions.rs`).
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        let malloc_index = codegen
            .get_function_index("__malloc")
            .expect("__malloc must be registered before the JSON helpers");

        self.register_access_operations(codegen)?;

        // __json_from_cln_list(list_ptr: i32, elem_tag: i32) -> i32
        // Materializes a native Clean list as the JSON-tree array layout
        // (`[count][boxed_elem_ptr]…`, BOXED_ANY_ABI §3.2). Used by
        // emit_box_any (mir_builder/types.rs) when a typed list is boxed to
        // Any — for example as a field value inside an `any data = { items:
        // [...] }` object literal, or as the argument of json.encode.
        // `__json_from_cln_list_i64` is the same for a `list<integer:64>`
        // (called with the Number tag): each i64 element becomes a number.
        for (name, i64_elements) in [
            ("__json_from_cln_list", false),
            ("__json_from_cln_list_i64", true),
        ] {
            register_stdlib_function_with_locals(
                codegen,
                name,
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
                self.generate_from_cln_list_instructions(malloc_index, i64_elements),
            )?;
        }

        // __json_from_cln_pairs(pairs_ptr: i32, val_tag: i32) -> i32
        // Materializes a native Clean pairs map as the JSON-tree object
        // layout (`[count][key_ptr][boxed_val_ptr]…`, BOXED_ANY_ABI §3.3).
        // Used by emit_box_any when a typed pairs is boxed to Any.
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

        // __json_try_decode_v2(raw: i32) -> i32
        // Post-check wrapper around a `_json_decode_v2` bridge return value.
        // Per BOXED_ANY_ABI §D5 the bridge returns 0 on parse failure. This
        // helper substitutes a freshly allocated tag-0 boxed-Any (null tag)
        // for the 0 sentinel and returns non-zero results unchanged. Emitted
        // by mir_builder/expressions.rs for `json.tryTextToData`.
        register_stdlib_function_with_locals(
            codegen,
            "__json_try_decode_v2",
            &[WasmType::I32],    // raw bridge result (0 on parse failure)
            Some(WasmType::I32), // returns non-zero boxed-Any pointer
            &[
                WasmType::I32, // 1: null_box_ptr
            ],
            Self::generate_json_try_decode_v2_instructions(malloc_index),
        )?;

        // __json_from_cln_list_boxed(list_ptr: i32) -> i32
        // The JSON-tree array of a native Clean list whose elements are
        // already boxed-Any pointers (`list<any>`, or a list of class
        // instances mapped through their `__json_tree_*` builder). Each
        // element pointer is copied; a 0 element becomes a null box.
        let null_or_box_index = codegen
            .get_function_index("__json_try_decode_v2")
            .expect("__json_try_decode_v2 is registered above");
        register_stdlib_function_with_locals(
            codegen,
            "__json_from_cln_list_boxed",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // returns json_array_ptr
            &[
                WasmType::I32, // 1: count
                WasmType::I32, // 2: i
                WasmType::I32, // 3: json_array_ptr
            ],
            Self::generate_from_cln_list_boxed_instructions(malloc_index, null_or_box_index),
        )?;

        // __json_decode_v2_or_raise(raw: i32) -> i32
        // Post-check wrapper around a `_json_decode_v2` bridge return value.
        // Per D5 raises (via WASM unreachable trap) when the bridge returned
        // the 0 sentinel; otherwise returns the pointer unchanged. Emitted
        // by mir_builder/expressions.rs for `json.textToData` / `json.decode`.
        register_stdlib_function_with_locals(
            codegen,
            "__json_decode_v2_or_raise",
            &[WasmType::I32],    // raw bridge result (0 on parse failure)
            Some(WasmType::I32), // returns non-zero boxed-Any pointer
            &[],
            Self::generate_json_decode_v2_or_raise_instructions(),
        )?;

        Ok(())
    }

    /// Register member and element access for bracket and dot notation on
    /// boxed objects and arrays (`data["field"]`, `data.field`, `data[0]`).
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

        let memcmp_index = codegen
            .get_function_index("__memcmp_bytes")
            .expect("__memcmp_bytes must be registered before __json_get_field");

        // __json_get_field(object_ptr: i32, key_ptr: i32, key_len: i32) -> i32
        // Access a member of a JSON object by key. Returns the member's boxed
        // value, or null (0) if the key is absent.
        register_stdlib_function_with_locals(
            codegen,
            "__json_get_field",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // object_ptr, key_ptr, key_len
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
            self.generate_get_field_instructions(memcmp_index),
        )?;

        // __json_get_index(array_ptr: i32, index: i32) -> i32
        // Access an element of a JSON array by index. Returns the element's
        // boxed value, or null (0) if the index is out of bounds.
        register_stdlib_function_with_locals(
            codegen,
            "__json_get_index",
            &[WasmType::I32, WasmType::I32], // array_ptr, index
            Some(WasmType::I32),             // returns any pointer
            &[
                WasmType::I32, // Local 2: count
            ],
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
    /// Accesses a field on a JSON object by string key and returns the
    /// member's boxed value, or null (0) when the key is absent.
    fn generate_get_field_instructions(&self, memcmp_index: u32) -> Vec<Instruction<'static>> {
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
        // Local 7: current_value_ptr (i32) - boxed member value
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
            Instruction::LocalSet(7), // current_value_ptr
            Instruction::LocalGet(7),
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
    /// Accesses an element on a JSON array by integer index and returns the
    /// element's boxed value, or null (0) when the index is out of bounds.
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
            // The boxed element is on the stack - return it
            Instruction::Else,
            // Invalid index (negative or >= count) - return null
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
        ]
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
    ///
    /// `i64_elements`: the 8-byte elements are `integer:64` values, converted
    /// to f64 for the Number box.
    fn generate_from_cln_list_instructions(
        &self,
        malloc_idx: u32,
        i64_elements: bool,
    ) -> Vec<Instruction<'static>> {
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
            // (spanning bytes 4..12). Other tags write a single i32 at offset 4 and leave
            // offset 8 zero. The store path is selected by a runtime branch
            // on elem_tag because we do not know the type statically here.
            Instruction::LocalGet(1),
            Instruction::I32Const(3),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            // f64 case
            Instruction::LocalGet(5),
            Instruction::LocalGet(4),
        ]);
        let element_load = MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        };
        if i64_elements {
            v.extend([
                Instruction::I64Load(element_load),
                Instruction::F64ConvertI64S,
            ]);
        } else {
            v.push(Instruction::F64Load(element_load));
        }
        v.extend([
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
            // Second payload word (BOXED_ANY_ABI §2): the sign extension of
            // an integer, so the 8 bytes hold the i64 value; 0 otherwise.
            Instruction::LocalGet(5),
            Instruction::LocalGet(1),
            Instruction::I32Const(JSON_TAG_INTEGER),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(5),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(31),
            Instruction::I32ShrS,
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::End,
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
    /// tree layout `[count:i32][key_ptr][boxed_val_ptr]…` of BOXED_ANY_ABI §3.3. Each value is wrapped in a fresh
    /// 12-byte boxed Any tagged with `val_tag`; the key string pointer is
    /// reused as-is (strings are immutable).
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
            // box_ptr[8] = second payload word (BOXED_ANY_ABI §2): the sign
            // extension of an integer, so the 8 bytes hold the i64 value;
            // 0 otherwise.
            Instruction::LocalGet(8),
            Instruction::LocalGet(1),
            Instruction::I32Const(JSON_TAG_INTEGER),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::LocalGet(6),
            Instruction::I32Const(31),
            Instruction::I32ShrS,
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::End,
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

    /// Generate WASM body for `__json_try_decode_v2(raw: i32) -> i32`.
    ///
    /// Layout:
    ///   local 0: raw (param)
    ///   local 1: null_box_ptr (scratch)
    ///
    /// Semantics (Delivery 2 §D5): if raw != 0, return raw; else allocate a
    /// fresh 12-byte block, store tag=0 (Null) at offset 0, zero out offsets
    /// 4 and 8, and return the block pointer.
    fn generate_json_try_decode_v2_instructions(malloc_idx: u32) -> Vec<Instruction<'static>> {
        vec![
            // if raw != 0 → return raw
            Instruction::LocalGet(0),
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(0),
            Instruction::Return,
            Instruction::End,
            // null_box_ptr = malloc(12)
            Instruction::I32Const(12),
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(1),
            // null_box_ptr[0] = 0 (AnyTypeTag::Null)
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // null_box_ptr[4] = 0
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // null_box_ptr[8] = 0
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // return null_box_ptr
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM body for `__json_from_cln_list_boxed(list_ptr: i32) -> i32`.
    ///
    /// The Clean list keeps its 4-byte elements from offset 16; each one is a
    /// boxed-Any pointer and goes into the JSON array `[count][elem_ptr]…`
    /// unchanged, except that 0 becomes a null box (`__json_try_decode_v2`).
    fn generate_from_cln_list_boxed_instructions(
        malloc_idx: u32,
        null_or_box_idx: u32,
    ) -> Vec<Instruction<'static>> {
        let word = |offset: u64| MemArg {
            offset,
            align: 2,
            memory_index: 0,
        };
        vec![
            // count = mem[list_ptr]
            Instruction::LocalGet(0),
            Instruction::I32Load(word(0)),
            Instruction::LocalSet(1),
            // json_array_ptr = malloc(4 + count * 4); json_array_ptr[0] = count
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::Call(malloc_idx),
            Instruction::LocalSet(3),
            Instruction::LocalGet(3),
            Instruction::LocalGet(1),
            Instruction::I32Store(word(0)),
            // for i in 0..count
            Instruction::I32Const(0),
            Instruction::LocalSet(2),
            Instruction::Block(wasm_encoder::BlockType::Empty),
            Instruction::Loop(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(2),
            Instruction::LocalGet(1),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // json_array_ptr[4 + i*4] = box_or_null(mem[list_ptr + 16 + i*4])
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::LocalGet(2),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(word(16)),
            Instruction::Call(null_or_box_idx),
            Instruction::I32Store(word(4)),
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(2),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(3),
        ]
    }

    /// Generate WASM body for `__json_decode_v2_or_raise(raw: i32) -> i32`.
    ///
    /// Layout:
    ///   local 0: raw (param)
    ///
    /// Semantics (Delivery 2 §D5): if raw != 0, return raw; else trap via
    /// WASM `unreachable`. The trap is Clean's raise mechanism at the WASM
    /// level — the runtime host translates it into a `error()` with message
    /// "JSON parse failure" for the caller.
    fn generate_json_decode_v2_or_raise_instructions() -> Vec<Instruction<'static>> {
        vec![
            // if raw == 0 → trap
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::Unreachable,
            Instruction::End,
            // return raw
            Instruction::LocalGet(0),
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
        // The tags are the boxed-Any wire contract (BOXED_ANY_ABI.md §1).
        assert_eq!(JSON_TAG_NULL, 0, "null tag must be 0");
        assert_eq!(JSON_TAG_INTEGER, 1, "integer tag must be 1");
        assert_eq!(JSON_TAG_BOOLEAN, 2, "boolean tag must be 2");
        assert_eq!(JSON_TAG_NUMBER, 3, "number tag must be 3");
        assert_eq!(JSON_TAG_STRING, 4, "string tag must be 4");
        assert_eq!(JSON_TAG_ARRAY, 5, "array tag must be 5");
        assert_eq!(JSON_TAG_OBJECT, 6, "object tag must be 6");

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
