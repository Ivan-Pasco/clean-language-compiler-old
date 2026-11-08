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
//! - `__json_to_cln_list` / `__any_convert` — the reverse: a boxed JSON
//!   array or object as a `list<any>`, or as a typed list or pairs map
//!   converted element by element, where an `any` becomes one;
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

/// The helper converting a boxed JSON array to a `list<any>`.
pub const JSON_TO_CLN_LIST: &str = "__json_to_cln_list";
/// The error `__json_to_cln_list` raises for a value that is not an array.
const NOT_A_LIST_MESSAGE: &str = "value is not a list";
/// The helper computing `.length()` of an `any`.
pub const ANY_LENGTH: &str = "__any_length";
/// The error `__any_length` raises for a value that has no length.
const NO_LENGTH_MESSAGE: &str = "value has no length";
/// The error `__json_decode_v2_or_raise` raises (stdlib-reference.md §8, D5).
const JSON_PARSE_FAILURE_MESSAGE: &str = "JSON parse failure";

/// The helper converting a boxed `any` to a typed list or pairs map:
/// `__any_convert(any_ptr, descriptor, position) -> value`. The descriptor
/// is a string constant naming the target type, one code per level
/// (`list<list<integer>>` is `[[i`, `pairs<string, number>` is `{n`: a
/// pairs entry holds the address of an 8-byte cell for a `number` or
/// `integer:64` value); `position` is the index of the code to convert to.
pub const ANY_CONVERT: &str = "__any_convert";

/// The helper boxing a typed Clean value into `any`:
/// `__cln_to_any(value, descriptor, position) -> any`. The descriptor has the
/// format of [`ANY_CONVERT`]'s and names the value's type. `value` is the
/// value itself for the 4-byte types (an integer, a boolean, a string, an
/// instance, an `any`, a list or pairs pointer) and the address of the value
/// for `number` and `integer:64`.
pub const CLN_TO_ANY: &str = "__cln_to_any";

/// Descriptor codes of [`ANY_CONVERT`].
pub mod any_descriptor {
    /// `any`: the box itself.
    pub const ANY: u8 = b'a';
    /// `integer` (32-bit): an integer value.
    pub const INTEGER: u8 = b'i';
    /// `integer:64`: an integer value (list elements only).
    pub const INTEGER_64: u8 = b'l';
    /// `number`: a number or integer value (list elements only).
    pub const NUMBER: u8 = b'n';
    /// `boolean`: a boolean value.
    pub const BOOLEAN: u8 = b'b';
    /// `string`: a string value; null is `""`, as `string s = value` gives.
    pub const STRING: u8 = b's';
    /// A class instance boxed into `any`.
    pub const INSTANCE: u8 = b'c';
    /// `list<T>`, followed by the code of `T`: a JSON array.
    pub const LIST: u8 = b'[';
    /// `pairs<string, V>`, followed by the code of `V`: a JSON object.
    pub const PAIRS: u8 = b'{';
}

/// The error `__any_convert` raises when the value does not have the
/// target type.
fn conversion_message(code: u8) -> &'static str {
    use any_descriptor::*;
    match code {
        INTEGER | INTEGER_64 => "value is not an integer",
        NUMBER => "value is not a number",
        BOOLEAN => "value is not a boolean",
        STRING => "value is not a string",
        INSTANCE => "value is not a class instance",
        LIST => NOT_A_LIST_MESSAGE,
        _ => "value is not an object",
    }
}

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

        // __json_to_cln_list(any_ptr: i32) -> i32
        // The `list<any>` of a boxed JSON array (stdlib-reference.md §8: a
        // JSON array is a `list<any>`): a native Clean list whose elements
        // are the array's boxed element pointers. Used where an `any` is
        // assigned to a `list<any>` and where an `any` is iterated. A null
        // value gives a null list; any other non-array value raises an error.
        let list_allocate_index = codegen
            .get_function_index("list.allocate")
            .expect("list.allocate must be registered before the JSON helpers");
        let err_raise_index = codegen
            .get_function_index("__err_raise")
            .expect("__err_raise must be registered before the JSON helpers");
        let not_a_list_message = codegen.get_or_create_string_offset(NOT_A_LIST_MESSAGE)?;
        register_stdlib_function_with_locals(
            codegen,
            JSON_TO_CLN_LIST,
            &[WasmType::I32],    // any_ptr
            Some(WasmType::I32), // returns list_ptr
            &[
                WasmType::I32, // 1: raw_array_ptr
                WasmType::I32, // 2: count
                WasmType::I32, // 3: list_ptr
                WasmType::I32, // 4: i
            ],
            Self::generate_json_to_cln_list_instructions(
                list_allocate_index,
                err_raise_index,
                not_a_list_message,
            ),
        )?;

        // __any_length(any_ptr: i32) -> i32
        // `.length()` of an `any`: the element count of a JSON array, the
        // entry count of a JSON object, the byte length of a string (as
        // `string.length`); 0 for null. Any other value raises an error.
        let no_length_message = codegen.get_or_create_string_offset(NO_LENGTH_MESSAGE)?;
        register_stdlib_function_with_locals(
            codegen,
            ANY_LENGTH,
            &[WasmType::I32],    // any_ptr
            Some(WasmType::I32), // returns the length
            &[WasmType::I32],    // 1: tag
            Self::generate_any_length_instructions(err_raise_index, no_length_message),
        )?;

        // __any_convert(any_ptr: i32, descriptor: i32, position: i32) -> i32
        // A boxed JSON array or object as the typed Clean list or pairs map
        // the descriptor names (see `ANY_CONVERT`), converted element by
        // element. Null gives null; a value of another type raises an error,
        // so `onError` can catch it.
        let pairs_new_index = codegen
            .get_function_index("__pairs_new")
            .expect("__pairs_new must be registered before the JSON helpers");
        let pairs_set_index = codegen
            .get_function_index("__pairs_set")
            .expect("__pairs_set must be registered before the JSON helpers");
        let mut messages = Vec::new();
        for code in [
            any_descriptor::INTEGER,
            any_descriptor::INTEGER_64,
            any_descriptor::NUMBER,
            any_descriptor::BOOLEAN,
            any_descriptor::STRING,
            any_descriptor::INSTANCE,
            any_descriptor::LIST,
            any_descriptor::PAIRS,
        ] {
            messages.push((
                code,
                codegen.get_or_create_string_offset(conversion_message(code))?,
            ));
        }
        let runtime = AnyConvertRuntime {
            self_index: codegen.next_function_index(),
            malloc: malloc_index,
            list_allocate: list_allocate_index,
            pairs_new: pairs_new_index,
            pairs_set: pairs_set_index,
            err_raise: err_raise_index,
            empty_string: codegen.get_or_create_string_offset("")?,
            messages,
        };
        let any_convert_index = register_stdlib_function_with_locals(
            codegen,
            ANY_CONVERT,
            &[WasmType::I32, WasmType::I32, WasmType::I32], // any_ptr, descriptor, position
            Some(WasmType::I32),
            &[WasmType::I32; 11], // 3..=13, see `AnyConvertRuntime`
            runtime.instructions(),
        )?;
        debug_assert_eq!(any_convert_index, runtime.self_index);

        // __cln_to_any(value: i32, descriptor: i32, position: i32) -> i32
        // The boxed `any` of a typed Clean value (see `CLN_TO_ANY`): a list
        // or pairs map becomes a JSON array or object, converted element by
        // element, so nested collections keep their elements.
        let to_any = ClnToAnyRuntime {
            self_index: codegen.next_function_index(),
            malloc: malloc_index,
        };
        let to_any_index = register_stdlib_function_with_locals(
            codegen,
            CLN_TO_ANY,
            &[WasmType::I32, WasmType::I32, WasmType::I32], // value, descriptor, position
            Some(WasmType::I32),
            &[WasmType::I32; 10], // 3..=12, see `ClnToAnyRuntime`
            to_any.instructions(),
        )?;
        debug_assert_eq!(to_any_index, to_any.self_index);

        // __json_decode_v2_or_raise(raw: i32) -> i32
        // Post-check wrapper around a `_json_decode_v2` bridge return value.
        // Per D5 raises the error "JSON parse failure" when the bridge
        // returned the 0 sentinel; otherwise returns the pointer unchanged.
        // Emitted by mir_builder/types.rs for `json.textToData` /
        // `json.decode`.
        let parse_failure_message =
            codegen.get_or_create_string_offset(JSON_PARSE_FAILURE_MESSAGE)?;
        register_stdlib_function_with_locals(
            codegen,
            "__json_decode_v2_or_raise",
            &[WasmType::I32],    // raw bridge result (0 on parse failure)
            Some(WasmType::I32), // returns non-zero boxed-Any pointer
            &[],
            Self::generate_json_decode_v2_or_raise_instructions(
                err_raise_index,
                parse_failure_message,
            ),
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

        // __json_get_field(any_ptr: i32, key_ptr: i32, key_len: i32) -> i32
        // Access a member of a boxed JSON object by key. Returns the member's
        // boxed value, or null (0) if the key is absent or the value is not
        // a JSON object (a string, a list, a class instance, null).
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
            self.generate_get_field_instructions(memcmp_index),
        )?;

        // __json_get_index(any_ptr: i32, index: i32) -> i32
        // Access an element of a boxed JSON array by index. Returns the
        // element's boxed value, or null (0) if the index is out of bounds or
        // the value is not a JSON array.
        register_stdlib_function_with_locals(
            codegen,
            "__json_get_index",
            &[WasmType::I32, WasmType::I32], // any_ptr, index
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
    /// Replace the boxed `any` in local 0 with its payload when the box
    /// has tag `tag`, and with 0 (no value) otherwise: `.get` and member or
    /// element access on an `any` that holds a string, a number or null
    /// find nothing instead of reading the payload as an object or array.
    fn payload_of_tag(tag: i32) -> Vec<Instruction<'static>> {
        use wasm_encoder::{BlockType, ValType};
        let word = |offset| {
            Instruction::I32Load(MemArg {
                offset,
                align: 2,
                memory_index: 0,
            })
        };
        vec![
            Instruction::LocalGet(0),
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            word(0),
            Instruction::I32Const(tag),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            word(4),
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::LocalSet(0),
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

        let mut v = Self::payload_of_tag(JSON_TAG_OBJECT);
        v.extend([
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
            // Signed: a boxed class instance (tag 6) starts with its
            // negative header word instead of a count, and has no members.
            Instruction::I32GeS,
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
        ]);
        v
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

        let mut v = Self::payload_of_tag(JSON_TAG_ARRAY);
        v.extend([
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

    /// Generate WASM body for `__json_to_cln_list(any_ptr: i32) -> i32`.
    ///
    /// Locals: 0 any_ptr (param), 1 raw_array_ptr, 2 count, 3 list_ptr, 4 i.
    fn generate_json_to_cln_list_instructions(
        list_allocate_idx: u32,
        err_raise_idx: u32,
        not_a_list_message: u32,
    ) -> Vec<Instruction<'static>> {
        use wasm_encoder::BlockType;
        let word = |offset: u64| MemArg {
            offset,
            align: 2,
            memory_index: 0,
        };
        vec![
            // A null pointer or a null box is a null list.
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(0),
            Instruction::I32Load(word(0)),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            // Any other non-array value raises.
            Instruction::LocalGet(0),
            Instruction::I32Load(word(0)),
            Instruction::I32Const(JSON_TAG_ARRAY),
            Instruction::I32Ne,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(not_a_list_message as i32),
            Instruction::Call(err_raise_idx),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            // raw = any_ptr[4]; count = raw[0]
            Instruction::LocalGet(0),
            Instruction::I32Load(word(4)),
            Instruction::LocalTee(1),
            Instruction::I32Load(word(0)),
            Instruction::LocalSet(2),
            // list = list.allocate(count); list.size = count
            Instruction::LocalGet(2),
            Instruction::Call(list_allocate_idx),
            Instruction::LocalTee(3),
            Instruction::LocalGet(2),
            Instruction::I32Store(word(0)),
            // for i in 0..count: list[16 + i*4] = raw[4 + i*4]
            Instruction::I32Const(0),
            Instruction::LocalSet(4),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(4),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(3),
            Instruction::LocalGet(4),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::LocalGet(4),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(word(4)),
            Instruction::I32Store(word(16)),
            Instruction::LocalGet(4),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(4),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(3),
        ]
    }

    /// Generate WASM body for `__any_length(any_ptr: i32) -> i32`.
    ///
    /// Locals: 0 any_ptr (param), 1 tag. A string, array and object payload
    /// each point at a structure whose first word is its length or count.
    fn generate_any_length_instructions(
        err_raise_idx: u32,
        no_length_message: u32,
    ) -> Vec<Instruction<'static>> {
        use wasm_encoder::BlockType;
        let word = |offset: u64| MemArg {
            offset,
            align: 2,
            memory_index: 0,
        };
        vec![
            // A null pointer or a null box has length 0.
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(0),
            Instruction::I32Load(word(0)),
            Instruction::LocalTee(1),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            // string, array, object: the first word of the payload.
            Instruction::LocalGet(1),
            Instruction::I32Const(JSON_TAG_STRING),
            Instruction::I32GeU,
            Instruction::LocalGet(1),
            Instruction::I32Const(JSON_TAG_OBJECT),
            Instruction::I32LeU,
            Instruction::I32And,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(0),
            Instruction::I32Load(word(4)),
            Instruction::I32Load(word(0)),
            Instruction::Return,
            Instruction::End,
            // integer, boolean, number: no length.
            Instruction::I32Const(no_length_message as i32),
            Instruction::Call(err_raise_idx),
            Instruction::I32Const(0),
        ]
    }

    /// Generate WASM body for `__json_decode_v2_or_raise(raw: i32) -> i32`.
    ///
    /// Layout:
    ///   local 0: raw (param)
    ///
    /// Semantics (Delivery 2 §D5): if raw != 0, return raw; else raise the
    /// error "JSON parse failure" (`__err_raise`), which an `onError`
    /// handler catches; outside every handler it stops the program.
    fn generate_json_decode_v2_or_raise_instructions(
        err_raise_idx: u32,
        parse_failure_message: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            // if raw == 0 → raise
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(parse_failure_message as i32),
            Instruction::Call(err_raise_idx),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            // return raw
            Instruction::LocalGet(0),
        ]
    }
}

/// The body of [`CLN_TO_ANY`].
///
/// Locals: 0 value, 1 descriptor, 2 position (parameters); 3 code, 4 box,
/// 5 count, 6 i, 7 tree (the JSON array or object), 8 element code,
/// 9 element address, 10 element stride, 11 element argument, 12 entry.
struct ClnToAnyRuntime {
    self_index: u32,
    malloc: u32,
}

impl ClnToAnyRuntime {
    const VALUE: u32 = 0;
    const DESCRIPTOR: u32 = 1;
    const POSITION: u32 = 2;
    const CODE: u32 = 3;
    const BOX: u32 = 4;
    const COUNT: u32 = 5;
    const I: u32 = 6;
    const TREE: u32 = 7;
    const ELEMENT_CODE: u32 = 8;
    const ADDRESS: u32 = 9;
    const STRIDE: u32 = 10;
    const ARGUMENT: u32 = 11;
    const ENTRY: u32 = 12;

    /// Offset of a list's first element (after len, cap, type_id, pad).
    const LIST_DATA_OFFSET: i32 = 16;
    /// Offset of a pairs map's first entry (after count, capacity).
    const PAIRS_ENTRIES_OFFSET: i32 = 8;

    fn word(offset: u64) -> MemArg {
        AnyConvertRuntime::word(offset)
    }

    /// An 8-byte access at `offset` of an address that is only 4-aligned.
    fn unaligned_double(offset: u64) -> MemArg {
        MemArg {
            offset,
            align: 2,
            memory_index: 0,
        }
    }

    /// The descriptor code at `position + offset` into local `into`.
    fn load_code(offset: u64, into: u32) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 4 + offset,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(into),
        ]
    }

    /// Return a new box with `tag`; `payload` stores bytes 4..12 of the box
    /// in local BOX.
    fn boxed(&self, tag: i32, payload: Vec<Instruction<'static>>) -> Vec<Instruction<'static>> {
        let mut v = vec![
            Instruction::I32Const(12),
            Instruction::Call(self.malloc),
            Instruction::LocalSet(Self::BOX),
            Instruction::LocalGet(Self::BOX),
            Instruction::I32Const(tag),
            Instruction::I32Store(Self::word(0)),
        ];
        v.extend(payload);
        v.extend([Instruction::LocalGet(Self::BOX), Instruction::Return]);
        v
    }

    /// A box whose payload is the i32 `value` and a zero high word.
    fn boxed_word(&self, tag: i32) -> Vec<Instruction<'static>> {
        self.boxed(
            tag,
            vec![
                Instruction::LocalGet(Self::BOX),
                Instruction::LocalGet(Self::VALUE),
                Instruction::I32Store(Self::word(4)),
                Instruction::LocalGet(Self::BOX),
                Instruction::I32Const(0),
                Instruction::I32Store(Self::word(8)),
            ],
        )
    }

    /// `if value == 0 { return a null box }`.
    fn null_when_zero(&self) -> Vec<Instruction<'static>> {
        let mut v = vec![
            Instruction::LocalGet(Self::VALUE),
            Instruction::I32Eqz,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ];
        v.extend(self.boxed(
            JSON_TAG_NULL,
            vec![
                Instruction::LocalGet(Self::BOX),
                Instruction::I64Const(0),
                Instruction::I64Store(Self::unaligned_double(4)),
            ],
        ));
        v.push(Instruction::End);
        v
    }

    /// The loop over `count` elements: `body` runs with local I as index.
    fn each(body: Vec<Instruction<'static>>) -> Vec<Instruction<'static>> {
        use wasm_encoder::BlockType;
        let mut v = vec![
            Instruction::I32Const(0),
            Instruction::LocalSet(Self::I),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(Self::I),
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32GeS,
            Instruction::BrIf(1),
        ];
        v.extend(body);
        v.extend([
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(Self::I),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
        ]);
        v
    }

    /// `__cln_to_any(argument, descriptor, position + 1)`.
    fn convert_element(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(Self::ARGUMENT),
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::Call(self.self_index),
        ]
    }

    /// Allocate the JSON tree: `count` slots of `slot_size` bytes after the
    /// count word, which is written.
    fn allocate_tree(&self, slot_size: i32) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32Const(slot_size),
            Instruction::I32Mul,
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::Call(self.malloc),
            Instruction::LocalTee(Self::TREE),
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32Store(Self::word(0)),
        ]
    }

    /// Box the tree in local TREE with `tag` and return it.
    fn boxed_tree(&self, tag: i32) -> Vec<Instruction<'static>> {
        self.boxed(
            tag,
            vec![
                Instruction::LocalGet(Self::BOX),
                Instruction::LocalGet(Self::TREE),
                Instruction::I32Store(Self::word(4)),
                Instruction::LocalGet(Self::BOX),
                Instruction::I32Const(0),
                Instruction::I32Store(Self::word(8)),
            ],
        )
    }

    fn list(&self) -> Vec<Instruction<'static>> {
        use any_descriptor::*;
        let mut v = self.null_when_zero();
        v.extend(Self::load_code(1, Self::ELEMENT_CODE));
        // 8-byte elements are passed by address.
        v.extend([
            Instruction::LocalGet(Self::ELEMENT_CODE),
            Instruction::I32Const(NUMBER as i32),
            Instruction::I32Eq,
            Instruction::LocalGet(Self::ELEMENT_CODE),
            Instruction::I32Const(INTEGER_64 as i32),
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(8),
            Instruction::Else,
            Instruction::I32Const(4),
            Instruction::End,
            Instruction::LocalSet(Self::STRIDE),
            Instruction::LocalGet(Self::VALUE),
            Instruction::I32Load(Self::word(0)),
            Instruction::LocalSet(Self::COUNT),
        ]);
        v.extend(self.allocate_tree(4));
        let mut body = vec![
            // address = list + 16 + i * stride
            Instruction::LocalGet(Self::VALUE),
            Instruction::I32Const(Self::LIST_DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(Self::I),
            Instruction::LocalGet(Self::STRIDE),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalTee(Self::ADDRESS),
            // argument = stride == 8 ? address : mem[address]
            Instruction::LocalGet(Self::ADDRESS),
            Instruction::I32Load(Self::word(0)),
            Instruction::LocalGet(Self::STRIDE),
            Instruction::I32Const(8),
            Instruction::I32Eq,
            Instruction::Select,
            Instruction::LocalSet(Self::ARGUMENT),
            // tree[4 + 4i] = element box
            Instruction::LocalGet(Self::TREE),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
        ];
        body.extend(self.convert_element());
        body.push(Instruction::I32Store(Self::word(4)));
        v.extend(Self::each(body));
        v.extend(self.boxed_tree(JSON_TAG_ARRAY));
        v
    }

    fn pairs(&self) -> Vec<Instruction<'static>> {
        let mut v = self.null_when_zero();
        v.extend([
            Instruction::LocalGet(Self::VALUE),
            Instruction::I32Load(Self::word(0)),
            Instruction::LocalSet(Self::COUNT),
        ]);
        v.extend(self.allocate_tree(8));
        let mut body = vec![
            // entry = pairs + 8 + i * 8: [key_ptr][value]
            Instruction::LocalGet(Self::VALUE),
            Instruction::I32Const(Self::PAIRS_ENTRIES_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalTee(Self::ENTRY),
            Instruction::I32Load(Self::word(4)),
            Instruction::LocalSet(Self::ARGUMENT),
            // slot = tree + 4 + i * 8: [key_ptr][boxed value]
            Instruction::LocalGet(Self::TREE),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalTee(Self::ADDRESS),
            Instruction::LocalGet(Self::ENTRY),
            Instruction::I32Load(Self::word(0)),
            Instruction::I32Store(Self::word(4)),
            Instruction::LocalGet(Self::ADDRESS),
        ];
        body.extend(self.convert_element());
        body.push(Instruction::I32Store(Self::word(8)));
        v.extend(Self::each(body));
        v.extend(self.boxed_tree(JSON_TAG_OBJECT));
        v
    }

    fn instructions(&self) -> Vec<Instruction<'static>> {
        use any_descriptor::*;
        let when = AnyConvertRuntime::when_code;
        let mut v = Self::load_code(0, Self::CODE);
        // `any`: the box itself (0 is null).
        let mut any = self.null_when_zero();
        any.extend([Instruction::LocalGet(Self::VALUE), Instruction::Return]);
        v.extend(when(Self::CODE, ANY, any));
        // integer: the i64 sign extension of the value.
        v.extend(when(
            Self::CODE,
            INTEGER,
            self.boxed(
                JSON_TAG_INTEGER,
                vec![
                    Instruction::LocalGet(Self::BOX),
                    Instruction::LocalGet(Self::VALUE),
                    Instruction::I64ExtendI32S,
                    Instruction::I64Store(Self::unaligned_double(4)),
                ],
            ),
        ));
        // integer:64 (by address): the exact value.
        let mut integer_64 = self.null_when_zero();
        integer_64.extend(self.boxed(
            JSON_TAG_INTEGER,
            vec![
                Instruction::LocalGet(Self::BOX),
                Instruction::LocalGet(Self::VALUE),
                Instruction::I64Load(Self::unaligned_double(0)),
                Instruction::I64Store(Self::unaligned_double(4)),
            ],
        ));
        v.extend(when(Self::CODE, INTEGER_64, integer_64));
        // number (by address).
        let mut number = self.null_when_zero();
        number.extend(self.boxed(
            JSON_TAG_NUMBER,
            vec![
                Instruction::LocalGet(Self::BOX),
                Instruction::LocalGet(Self::VALUE),
                Instruction::F64Load(Self::unaligned_double(0)),
                Instruction::F64Store(Self::unaligned_double(4)),
            ],
        ));
        v.extend(when(Self::CODE, NUMBER, number));
        v.extend(when(Self::CODE, BOOLEAN, self.boxed_word(JSON_TAG_BOOLEAN)));
        let mut string = self.null_when_zero();
        string.extend(self.boxed_word(JSON_TAG_STRING));
        v.extend(when(Self::CODE, STRING, string));
        // A class instance is boxed as itself (tag 6, class_json.rs);
        // `json_value_of` turns it into its object at the JSON boundary.
        let mut instance = self.null_when_zero();
        instance.extend(self.boxed_word(JSON_TAG_OBJECT));
        v.extend(when(Self::CODE, INSTANCE, instance));
        v.extend(when(Self::CODE, LIST, self.list()));
        v.extend(when(Self::CODE, PAIRS, self.pairs()));
        // The descriptor is a compiler constant with no other code.
        v.push(Instruction::Unreachable);
        v
    }
}

/// What the body of [`ANY_CONVERT`] calls.
///
/// Locals: 0 any_ptr, 1 descriptor, 2 position (parameters); 3 code,
/// 4 tag, 5 payload, 6 count, 7 result, 8 i, 9 element code, 10 element,
/// 11 element tag, 12 slot address, 13 cell (a pairs entry's 8-byte value).
struct AnyConvertRuntime {
    self_index: u32,
    malloc: u32,
    list_allocate: u32,
    pairs_new: u32,
    pairs_set: u32,
    err_raise: u32,
    empty_string: u32,
    /// The error message of each descriptor code.
    messages: Vec<(u8, u32)>,
}

impl AnyConvertRuntime {
    const ANY: u32 = 0;
    const DESCRIPTOR: u32 = 1;
    const POSITION: u32 = 2;
    const CODE: u32 = 3;
    const TAG: u32 = 4;
    const PAYLOAD: u32 = 5;
    const COUNT: u32 = 6;
    const RESULT: u32 = 7;
    const I: u32 = 8;
    const ELEMENT_CODE: u32 = 9;
    const ELEMENT: u32 = 10;
    const ELEMENT_TAG: u32 = 11;
    const SLOT: u32 = 12;
    const CELL: u32 = 13;

    fn word(offset: u64) -> MemArg {
        MemArg {
            offset,
            align: 2,
            memory_index: 0,
        }
    }

    fn double(offset: u64) -> MemArg {
        MemArg {
            offset,
            align: 3,
            memory_index: 0,
        }
    }

    /// Raise the error of descriptor `code` and return 0.
    fn raise(&self, code: u8) -> Vec<Instruction<'static>> {
        let message = self
            .messages
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, offset)| *offset)
            .expect("every descriptor code has a message");
        vec![
            Instruction::I32Const(message as i32),
            Instruction::Call(self.err_raise),
            Instruction::I32Const(0),
            Instruction::Return,
        ]
    }

    /// `if <local> == code { body }`.
    fn when_code(
        local: u32,
        code: u8,
        body: Vec<Instruction<'static>>,
    ) -> Vec<Instruction<'static>> {
        let mut v = vec![
            Instruction::LocalGet(local),
            Instruction::I32Const(code as i32),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
        ];
        v.extend(body);
        v.push(Instruction::End);
        v
    }

    /// Return 0 when an error raised in a nested conversion is pending.
    fn return_if_raised() -> Vec<Instruction<'static>> {
        vec![
            Instruction::GlobalGet(crate::codegen::native_stdlib::ERROR_STATE_GLOBAL),
            Instruction::I32Const(1),
            Instruction::I32And,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
        ]
    }

    /// Leave 1 when the payload is a class instance (its first word is an
    /// object header, class_json.rs).
    fn payload_is_instance() -> Vec<Instruction<'static>> {
        use crate::mir::mir_types::{CLASS_HEADER_MARK, CLASS_HEADER_MARK_MASK};
        vec![
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::I32Load(Self::word(0)),
            Instruction::I32Const(CLASS_HEADER_MARK_MASK as i32),
            Instruction::I32And,
            Instruction::I32Const(CLASS_HEADER_MARK as i32),
            Instruction::I32Eq,
        ]
    }

    /// `return payload` when the tag is `tag`, otherwise raise.
    fn scalar(&self, code: u8, tag: i32) -> Vec<Instruction<'static>> {
        let mut body = vec![
            Instruction::LocalGet(Self::TAG),
            Instruction::I32Const(tag),
            Instruction::I32Eq,
            Instruction::If(wasm_encoder::BlockType::Empty),
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::Return,
            Instruction::End,
        ];
        body.extend(self.raise(code));
        Self::when_code(Self::CODE, code, body)
    }

    fn instructions(&self) -> Vec<Instruction<'static>> {
        use any_descriptor::*;
        use wasm_encoder::BlockType;
        let mut v = vec![
            // code = descriptor[4 + position]
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(Self::CODE),
        ];
        // `any`: the box itself.
        v.extend(Self::when_code(
            Self::CODE,
            ANY,
            vec![Instruction::LocalGet(Self::ANY), Instruction::Return],
        ));
        // A null pointer reads as the null tag. Null converts to null ("" for
        // a string), as the scalar conversions give.
        v.extend([
            Instruction::LocalGet(Self::ANY),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(JSON_TAG_NULL),
            Instruction::Else,
            Instruction::LocalGet(Self::ANY),
            Instruction::I32Load(Self::word(0)),
            Instruction::End,
            Instruction::LocalTee(Self::TAG),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
        ]);
        v.extend(Self::when_code(
            Self::CODE,
            STRING,
            vec![
                Instruction::I32Const(self.empty_string as i32),
                Instruction::Return,
            ],
        ));
        v.extend([
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(Self::ANY),
            Instruction::I32Load(Self::word(4)),
            Instruction::LocalSet(Self::PAYLOAD),
        ]);
        // The 4-byte scalars: the payload word of a value of that type. An
        // integer target takes the low word of the i64 (as `integer x =
        // value` does).
        v.extend(self.scalar(INTEGER, JSON_TAG_INTEGER));
        v.extend(self.scalar(BOOLEAN, JSON_TAG_BOOLEAN));
        v.extend(self.scalar(STRING, JSON_TAG_STRING));
        // A class instance (an object whose payload has an object header).
        let mut instance = vec![
            Instruction::LocalGet(Self::TAG),
            Instruction::I32Const(JSON_TAG_OBJECT),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
        ];
        instance.extend(Self::payload_is_instance());
        instance.extend([
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::Return,
            Instruction::End,
            Instruction::End,
        ]);
        instance.extend(self.raise(INSTANCE));
        v.extend(Self::when_code(Self::CODE, INSTANCE, instance));
        v.extend(self.list_conversion());
        v.extend(self.pairs_conversion());
        // The descriptor is a compiler constant: any other code is a
        // compiler bug.
        v.push(Instruction::Unreachable);
        v
    }

    /// `list<T>`: a JSON array, each element converted to `T`.
    fn list_conversion(&self) -> Vec<Instruction<'static>> {
        use any_descriptor::*;
        use wasm_encoder::BlockType;
        let mut body = vec![
            Instruction::LocalGet(Self::TAG),
            Instruction::I32Const(JSON_TAG_ARRAY),
            Instruction::I32Ne,
            Instruction::If(BlockType::Empty),
        ];
        body.extend(self.raise(LIST));
        body.extend([
            Instruction::End,
            // count = array[0]; element code = descriptor[5 + position]
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::I32Load(Self::word(0)),
            Instruction::LocalSet(Self::COUNT),
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(Self::ELEMENT_CODE),
            // The list keeps room for 8 elements at least, as a list literal
            // does, so `.add()` has space. Capacity counts 4-byte slots: a
            // `number` or `integer:64` element takes two.
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32Const(8),
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32Const(8),
            Instruction::I32GtU,
            Instruction::Select,
        ]);
        body.extend(Self::wide_element_test());
        body.extend([
            Instruction::If(BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(2),
            Instruction::Else,
            Instruction::I32Const(1),
            Instruction::End,
            Instruction::I32Mul,
            Instruction::Call(self.list_allocate),
            Instruction::LocalTee(Self::RESULT),
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32Store(Self::word(0)),
            Instruction::I32Const(0),
            Instruction::LocalSet(Self::I),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(Self::I),
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // element = array[4 + i*4]
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(Self::word(4)),
            Instruction::LocalSet(Self::ELEMENT),
        ]);
        // An 8-byte element is converted here; any other through a nested
        // conversion of the element.
        body.extend(Self::wide_element_test());
        body.extend([Instruction::If(BlockType::Empty)]);
        body.extend(self.wide_element());
        body.extend([
            Instruction::Else,
            // slot = list + 16 + i*4
            Instruction::LocalGet(Self::RESULT),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(Self::SLOT),
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::Call(self.self_index),
            Instruction::LocalSet(Self::ELEMENT),
        ]);
        body.extend(Self::return_if_raised());
        body.extend([
            Instruction::LocalGet(Self::SLOT),
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::I32Store(Self::word(16)),
            Instruction::End,
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(Self::I),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(Self::RESULT),
            Instruction::Return,
        ]);
        Self::when_code(Self::CODE, LIST, body)
    }

    /// Leave 1 when the element code is `number` or `integer:64`.
    fn wide_element_test() -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(Self::ELEMENT_CODE),
            Instruction::I32Const(any_descriptor::NUMBER as i32),
            Instruction::I32Eq,
            Instruction::LocalGet(Self::ELEMENT_CODE),
            Instruction::I32Const(any_descriptor::INTEGER_64 as i32),
            Instruction::I32Eq,
            Instruction::I32Or,
        ]
    }

    /// Store element `i` (a `number` or `integer:64`) at `list + 16 + i*8`.
    /// A number element takes a number or an integer (widened); an
    /// `integer:64` element an integer; both take null as 0.
    fn wide_element(&self) -> Vec<Instruction<'static>> {
        let mut v = vec![
            // slot = list + i*8 (the elements start at offset 16)
            Instruction::LocalGet(Self::RESULT),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(Self::SLOT),
        ];
        v.extend(self.wide_value(Self::SLOT, 16));
        v
    }

    /// Store the element in local ELEMENT as the `number` or `integer:64`
    /// of the element code at `local + offset`.
    fn wide_value(&self, local: u32, offset: u64) -> Vec<Instruction<'static>> {
        use any_descriptor::*;
        use wasm_encoder::BlockType;
        let mut v = vec![
            // element tag (a null pointer is null)
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(wasm_encoder::ValType::I32)),
            Instruction::I32Const(JSON_TAG_NULL),
            Instruction::Else,
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::I32Load(Self::word(0)),
            Instruction::End,
            Instruction::LocalSet(Self::ELEMENT_TAG),
        ];
        let tag_is = |tag: i32| {
            vec![
                Instruction::LocalGet(Self::ELEMENT_TAG),
                Instruction::I32Const(tag),
                Instruction::I32Eq,
            ]
        };
        // number
        let mut number = tag_is(JSON_TAG_NUMBER);
        number.extend([
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(local),
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::F64Load(Self::double(4)),
            Instruction::F64Store(Self::double(offset)),
            Instruction::Else,
        ]);
        number.extend(tag_is(JSON_TAG_INTEGER));
        number.extend([
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(local),
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::I64Load(Self::double(4)),
            Instruction::F64ConvertI64S,
            Instruction::F64Store(Self::double(offset)),
            Instruction::Else,
        ]);
        number.extend(tag_is(JSON_TAG_NULL));
        number.extend([
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(local),
            Instruction::F64Const(0.0),
            Instruction::F64Store(Self::double(offset)),
            Instruction::Else,
        ]);
        number.extend(self.raise(NUMBER));
        number.extend([Instruction::End, Instruction::End, Instruction::End]);
        v.extend(Self::when_code(Self::ELEMENT_CODE, NUMBER, number));
        // integer:64
        let mut wide_integer = tag_is(JSON_TAG_INTEGER);
        wide_integer.extend([
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(local),
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::I64Load(Self::double(4)),
            Instruction::I64Store(Self::double(offset)),
            Instruction::Else,
        ]);
        wide_integer.extend(tag_is(JSON_TAG_NULL));
        wide_integer.extend([
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(local),
            Instruction::I64Const(0),
            Instruction::I64Store(Self::double(offset)),
            Instruction::Else,
        ]);
        wide_integer.extend(self.raise(INTEGER_64));
        wide_integer.extend([Instruction::End, Instruction::End]);
        v.extend(Self::when_code(
            Self::ELEMENT_CODE,
            INTEGER_64,
            wide_integer,
        ));
        v
    }

    /// `pairs<string, V>`: a JSON object (not a class instance), each value
    /// converted to `V`; the keys are the object's keys.
    fn pairs_conversion(&self) -> Vec<Instruction<'static>> {
        use any_descriptor::*;
        use wasm_encoder::BlockType;
        // The payload is read as an instance only when the value is an
        // object (another payload need not be an address).
        let mut body = vec![
            Instruction::LocalGet(Self::TAG),
            Instruction::I32Const(JSON_TAG_OBJECT),
            Instruction::I32Ne,
            Instruction::If(BlockType::Empty),
        ];
        body.extend(self.raise(PAIRS));
        body.push(Instruction::End);
        body.extend(Self::payload_is_instance());
        body.push(Instruction::If(BlockType::Empty));
        body.extend(self.raise(PAIRS));
        body.extend([
            Instruction::End,
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::I32Load(Self::word(0)),
            Instruction::LocalTee(Self::COUNT),
            Instruction::Call(self.pairs_new),
            Instruction::LocalSet(Self::RESULT),
            Instruction::I32Const(0),
            Instruction::LocalSet(Self::I),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(Self::I),
            Instruction::LocalGet(Self::COUNT),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // slot = object + i*8: the entry `[key_ptr @4][value_ptr @8]`
            Instruction::LocalGet(Self::PAYLOAD),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalTee(Self::SLOT),
            Instruction::I32Load(Self::word(8)),
            Instruction::LocalSet(Self::ELEMENT),
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 5,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalSet(Self::ELEMENT_CODE),
        ]);
        // A `number` or `integer:64` value is stored in an 8-byte cell the
        // entry points to; any other through a nested conversion.
        body.extend(Self::wide_element_test());
        body.extend([
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(8),
            Instruction::Call(self.malloc),
            Instruction::LocalSet(Self::CELL),
        ]);
        body.extend(self.wide_value(Self::CELL, 0));
        body.extend([
            Instruction::LocalGet(Self::CELL),
            Instruction::LocalSet(Self::ELEMENT),
            Instruction::Else,
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::LocalGet(Self::DESCRIPTOR),
            Instruction::LocalGet(Self::POSITION),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::Call(self.self_index),
            Instruction::LocalSet(Self::ELEMENT),
            Instruction::End,
        ]);
        body.extend(Self::return_if_raised());
        body.extend([
            // result = pairs.set(result, key, value): the map moves when it
            // grows.
            Instruction::LocalGet(Self::RESULT),
            Instruction::LocalGet(Self::SLOT),
            Instruction::I32Load(Self::word(4)),
            Instruction::LocalGet(Self::ELEMENT),
            Instruction::Call(self.pairs_set),
            Instruction::LocalSet(Self::RESULT),
            Instruction::LocalGet(Self::I),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(Self::I),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(Self::RESULT),
            Instruction::Return,
        ]);
        Self::when_code(Self::CODE, PAIRS, body)
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
