//! Native WASM stdlib implementations
//!
//! This module provides pure WASM bytecode implementations of stdlib functions,
//! enabling standalone WASM execution without host runtime dependencies.
//!
//! Functions implemented natively:
//! - String operations: length, concat, substring, startsWith, endsWith, etc.
//! - List literal append (list methods live in `src/stdlib/list_class.rs`)
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

/// Heap allocation alignment (MEMORY_MODEL.md § Alignment). The heap has no
/// fixed start: it begins after the string literals and the transient arena
/// pool (memory layout: `codegen::DATA_SECTION_START`).
pub const ALIGNMENT: u32 = 8;

/// String memory layout:
/// - Offset 0: length (i32, 4 bytes)
/// - Offset 4: data (bytes)
pub const STRING_LENGTH_OFFSET: u32 = 0;
pub const STRING_DATA_OFFSET: u32 = 4;

pub const PAIRS_COUNT_OFFSET: u32 = 0;
pub const PAIRS_CAPACITY_OFFSET: u32 = 4;
pub const PAIRS_HEADER_SIZE: u32 = 8;
pub const PAIRS_ENTRY_SIZE: u32 = 8;
pub const PAIRS_KEY_OFFSET: u32 = 0;
pub const PAIRS_VAL_OFFSET: u32 = 4;

/// List/Array memory layout (matches list_ops.rs and MIR codegen):
/// - Offset 0: size (i32, 4 bytes)
/// - Offset 4: capacity (i32, 4 bytes)
/// - Offset 8: type_id (i32, 4 bytes)
/// - Offset 12: behavior flags (i32, 4 bytes)
/// - Offset 16: elements (i32 or f64 depending on type)
///
/// CRITICAL: This 16-byte header format MUST match:
///   - stdlib/list_ops.rs generate_list_allocate()
///   - stdlib/list_class.rs and stdlib/list_behavior.rs
///   - mir_codegen GetElementPtr for list access (offset 16)
pub const LIST_LENGTH_OFFSET: u32 = 0;
pub const LIST_DATA_OFFSET: u32 = 16; // Elements start after 16-byte header
pub const LIST_ELEMENT_SIZE_I32: u32 = 4;

/// Global variable indices.
///
/// Layout (compiler-owned; user state globals start at `RESERVED_GLOBAL_COUNT`):
/// - 0: `HEAP_PTR_GLOBAL` — main bump heap pointer
/// - 1-2: transient arena (`TRANSIENT_BASE_GLOBAL`, `TRANSIENT_PTR_GLOBAL`).
///   See `transient_arena.rs`. The matching `__transient_scope_enter` /
///   `__transient_scope_exit` pair saves and restores `TRANSIENT_PTR_GLOBAL`
///   to release per-iteration intermediates without touching the main bump
///   heap (and therefore without risking the "free a live pointer" failure
///   mode that bit `CMP-SSR-RECLAIM-FREES-LIVE-POINTER`).
/// - 3-4: carryover slot pools (`CARRYOVER_A_BASE_GLOBAL`,
///   `CARRYOVER_B_BASE_GLOBAL`). See `carryover.rs`. Two independent
///   fixed-position pools used to ping-pong an outer-scope string
///   variable that is reassigned once per iteration inside an
///   accumulator-rewritten loop (bug `CODEGEN-LOOP-OUTER-STRING-REASSIGN-LEAK`,
///   fingerprint `88dc6aeb0f8e`). Each pool is single-tenanted per
///   iteration; lazy `__malloc`-allocated on first write.
pub const HEAP_PTR_GLOBAL: u32 = 0;
pub const TRANSIENT_BASE_GLOBAL: u32 = 1;
pub const TRANSIENT_PTR_GLOBAL: u32 = 2;
pub const CARRYOVER_A_BASE_GLOBAL: u32 = 3;
pub const CARRYOVER_B_BASE_GLOBAL: u32 = 4;
/// 5-6: `onError` state (`codegen_registration.rs`,
/// `register_error_handling_operations`). `ERROR_DEPTH_GLOBAL` counts the
/// `onError` regions being evaluated; `ERROR_PENDING_GLOBAL` is 1 while an
/// `error(...)` raised inside one is unwinding to its handler.
pub const ERROR_DEPTH_GLOBAL: u32 = 5;
pub const ERROR_PENDING_GLOBAL: u32 = 6;

/// Number of reserved compiler-owned globals (heap ptr + transient arena +
/// carryover pool bases + error state). User-level state globals are
/// assigned indices starting at `RESERVED_GLOBAL_COUNT`.
pub const RESERVED_GLOBAL_COUNT: u32 = 7;
