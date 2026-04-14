//! Native WASM stdlib implementations
//!
//! This module provides pure WASM bytecode implementations of stdlib functions,
//! enabling standalone WASM execution without host runtime dependencies.
//!
//! Functions implemented natively:
//! - String operations: length, concat, substring, startsWith, endsWith, etc.
//! - List operations: length, get, set, push, pop, etc.
//! - Type conversions: int_to_string, bool_to_string, string_to_int
//!
//! Functions that remain as imports (require platform access):
//! - I/O: print, printl, input
//! - File system: file_read, file_write, etc.
//! - HTTP: http_get, http_post, etc.

pub mod list_ops;
pub mod memory;
pub mod string_ops;
pub mod type_conversions;

use wasm_encoder::{Instruction, MemArg, ValType};

/// Memory layout constants
/// HEAP_START must be after all static data sections to avoid memory overlap.
/// Set to 1MB (16 WASM pages) to allow generous compile-time data sections for
/// programs with many string literals (SQL queries, HTML templates, etc.).
/// Must match MAX_MEMORY_SIZE in src/codegen/memory.rs.
pub const HEAP_START: u32 = 1048576;
pub const ALIGNMENT: u32 = 8;

/// String memory layout:
/// - Offset 0: length (i32, 4 bytes)
/// - Offset 4: data (bytes)
pub const STRING_LENGTH_OFFSET: u32 = 0;
pub const STRING_DATA_OFFSET: u32 = 4;

/// List/Array memory layout (matches list_ops.rs and MIR codegen):
/// - Offset 0: size (i32, 4 bytes)
/// - Offset 4: capacity (i32, 4 bytes)
/// - Offset 8: type_id (i32, 4 bytes)
/// - Offset 12: padding (i32, 4 bytes)
/// - Offset 16: elements (i32 or f64 depending on type)
/// CRITICAL: This 16-byte header format MUST match:
///   - list_ops.rs generate_list_allocate()
///   - list_class.rs generate_push()
///   - mir_codegen.rs GetElementPtr for list access (offset 16)
pub const LIST_LENGTH_OFFSET: u32 = 0;
pub const LIST_CAPACITY_OFFSET: u32 = 4;
pub const LIST_TYPE_ID_OFFSET: u32 = 8;
pub const LIST_DATA_OFFSET: u32 = 16; // Elements start after 16-byte header
pub const LIST_ELEMENT_SIZE_I32: u32 = 4;
pub const LIST_ELEMENT_SIZE_F64: u32 = 8;

/// Global variable indices
pub const HEAP_PTR_GLOBAL: u32 = 0;

/// Generate instructions for reading string length
pub fn gen_string_length() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0), // string pointer
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2, // 4-byte alignment
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for reading list length
pub fn gen_list_length() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Load(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2, // 4-byte alignment
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for getting character at index (returns byte value)
pub fn gen_string_char_at() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + index
        Instruction::LocalGet(0), // string pointer
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Add,
        // Load single byte
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for getting list element (i32)
pub fn gen_list_get_i32() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + (index * 4)
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // Load i32 value
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for setting list element (i32)
pub fn gen_list_set_i32() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + (index * 4)
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // Store value
        Instruction::LocalGet(2), // value
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Return list pointer for chaining
        Instruction::LocalGet(0),
    ]
}

/// Helper function to get the function parameters for native stdlib functions
pub fn get_native_function_signature(name: &str) -> Option<(Vec<ValType>, Vec<ValType>)> {
    match name {
        // String functions
        "native_string_length" => Some((vec![ValType::I32], vec![ValType::I32])),
        "native_string_char_at" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_string_char_code_at" => {
            Some((vec![ValType::I32, ValType::I32], vec![ValType::I32]))
        }
        "native_string_starts_with" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_string_ends_with" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_string_contains" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_string_index_of" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_string_concat" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_string_substring" => Some((
            vec![ValType::I32, ValType::I32, ValType::I32],
            vec![ValType::I32],
        )),

        // List functions
        "native_list_length" => Some((vec![ValType::I32], vec![ValType::I32])),
        "native_list_get_i32" => Some((vec![ValType::I32, ValType::I32], vec![ValType::I32])),
        "native_list_set_i32" => Some((
            vec![ValType::I32, ValType::I32, ValType::I32],
            vec![ValType::I32],
        )),

        // Memory functions
        "native_malloc" => Some((vec![ValType::I32], vec![ValType::I32])),
        "native_memcpy" => Some((vec![ValType::I32, ValType::I32, ValType::I32], vec![])),

        // Type conversion functions
        "native_int_to_string" => Some((vec![ValType::I32], vec![ValType::I32])),
        "native_bool_to_string" => Some((vec![ValType::I32], vec![ValType::I32])),
        "native_string_to_int" => Some((vec![ValType::I32], vec![ValType::I32])),

        _ => None,
    }
}
