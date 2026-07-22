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

pub mod carryover;
pub mod list_ops;
pub mod memory;
pub mod pairs_ops;
pub mod string_builder;
pub mod string_ops;
pub mod transient_arena;
pub mod type_conversions;

use wasm_encoder::{Instruction, MemArg, ValType};

/// Runtime heap start offset — the initial value of the `__heap_ptr` WASM global.
///
/// This is the byte offset where the bump allocator begins at runtime.
/// It is set to 1 MB (16 WASM pages) so that the entire first megabyte is
/// reserved for compile-time static data (the WASM data section containing
/// string literals, SQL queries, HTML templates, etc.).
///
/// **Not to be confused with `codegen::DATA_SECTION_START` (1 KB)**, which is
/// the offset where the *data section layout engine* begins placing static data.
/// The relationship:
///
///   [0 .. 1 KB]           Null-pointer guard / reserved
///   [1 KB .. 1 MB]        Data section (string pool, globals) — layout starts
///                          at `DATA_SECTION_START` (1024), up to `MAX_MEMORY_SIZE`
///   [1 MB .. top]         Runtime heap — bump allocator starts at `HEAP_START`
///
/// Must match `MAX_MEMORY_SIZE` in `src/codegen/memory.rs` (both are 1 MB).
/// Must match the `NEXT_ALLOCATION_OFFSET` in `src/bin/wasmtime_runner.rs`.
/// The host bridge reads `__heap_ptr` from WASM exports to know where the
/// heap begins (see MEMORY_POLICY.md section 7.2).
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
///
/// CRITICAL: This 16-byte header format MUST match:
///   - list_ops.rs generate_list_allocate()
///   - list_class.rs generate_push()
///   - mir_codegen.rs GetElementPtr for list access (offset 16)
pub const PAIRS_COUNT_OFFSET: u32 = 0;
pub const PAIRS_CAPACITY_OFFSET: u32 = 4;
pub const PAIRS_HEADER_SIZE: u32 = 8;
pub const PAIRS_ENTRY_SIZE: u32 = 8;
pub const PAIRS_KEY_OFFSET: u32 = 0;
pub const PAIRS_VAL_OFFSET: u32 = 4;

pub const LIST_LENGTH_OFFSET: u32 = 0;
pub const LIST_CAPACITY_OFFSET: u32 = 4;
pub const LIST_TYPE_ID_OFFSET: u32 = 8;
pub const LIST_DATA_OFFSET: u32 = 16; // Elements start after 16-byte header
pub const LIST_ELEMENT_SIZE_I32: u32 = 4;
pub const LIST_ELEMENT_SIZE_F64: u32 = 8;

/// Global variable indices.
///
/// Layout (compiler-owned; user state globals start at `RESERVED_GLOBAL_COUNT`):
/// - 0: `HEAP_PTR_GLOBAL` — main bump heap pointer
/// - 1-12: `__json_get` parse-result cache — 4-entry round-robin LRU.
///   Each entry is 3 slots: (src ptr, parsed ptr, heap floor). Widened
///   from a single-entry cache in fingerprint `#19cd8092` — the single-
///   entry cache thrashed when a page rendered rows via two interleaved
///   JSON sources (outer `list_result` + inner per-row `components_json`),
///   causing every outer-loop iteration to re-parse the large source.
///   Entries are keyed by source ptr and validated against the heap floor
///   (same invariant as before). Eviction is round-robin via the insert
///   cursor in global 13. See `mir_codegen/utilities.rs` and the json.get
///   shim in `src/stdlib/json_class.rs`.
/// - 13: `__json_get` LRU insert cursor (values 0..3). Advances mod 4 on
///   every cache miss so a run of >4 distinct sources evicts oldest.
/// - 14-15: transient arena (`TRANSIENT_BASE_GLOBAL`, `TRANSIENT_PTR_GLOBAL`).
///   See `transient_arena.rs`. The matching `__transient_scope_enter` /
///   `__transient_scope_exit` pair saves and restores `TRANSIENT_PTR_GLOBAL`
///   to release per-iteration intermediates without touching the main bump
///   heap (and therefore without risking the "free a live pointer" failure
///   mode that bit `CMP-SSR-RECLAIM-FREES-LIVE-POINTER`).
/// - 16-17: carryover slot pools (`CARRYOVER_A_BASE_GLOBAL`,
///   `CARRYOVER_B_BASE_GLOBAL`). See `carryover.rs`. Two independent
///   fixed-position pools used to ping-pong an outer-scope string
///   variable that is reassigned once per iteration inside an
///   accumulator-rewritten loop (bug `CODEGEN-LOOP-OUTER-STRING-REASSIGN-LEAK`,
///   fingerprint `88dc6aeb0f8e`). Each pool is single-tenanted per
///   iteration; lazy `__malloc`-allocated on first write.
pub const HEAP_PTR_GLOBAL: u32 = 0;
/// Base of the `__json_get` 4-entry LRU. Entry N occupies globals
/// (JSON_CACHE_BASE + 3*N .. JSON_CACHE_BASE + 3*N + 2).
pub const JSON_CACHE_BASE_GLOBAL: u32 = 1;
pub const JSON_CACHE_ENTRIES: u32 = 4;
/// Round-robin insert cursor for the `__json_get` LRU.
pub const JSON_CACHE_CURSOR_GLOBAL: u32 = 13;
pub const TRANSIENT_BASE_GLOBAL: u32 = 14;
pub const TRANSIENT_PTR_GLOBAL: u32 = 15;
pub const CARRYOVER_A_BASE_GLOBAL: u32 = 16;
pub const CARRYOVER_B_BASE_GLOBAL: u32 = 17;

/// Number of reserved compiler-owned globals (heap ptr + json cache +
/// transient arena + carryover pool bases). User-level state globals
/// are assigned indices starting at `RESERVED_GLOBAL_COUNT`.
pub const RESERVED_GLOBAL_COUNT: u32 = 18;

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
