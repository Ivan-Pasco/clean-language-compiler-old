//! Native WASM pairs/map operations
//!
//! Provides pure WASM bytecode implementations of pairs<K,V> functions.
//!
//! Pairs memory layout:
//!   [count: i32 at offset 0]       — number of key/value entries currently stored
//!   [capacity: i32 at offset 4]    — maximum entries before reallocation is needed
//!   [entries at offset 8, each 8 bytes: key_ptr (i32) + val_ptr (i32)]
//!
//! Offsets within each entry (relative to entry base):
//!   [key_ptr: i32 at offset 0]
//!   [val_ptr: i32 at offset 4]
//!
//! String memory layout (for key comparison):
//!   [length: i32 at offset 0]
//!   [data bytes at offset 4]

use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Offset of the entry count field from the map base pointer.
pub const PAIRS_COUNT_OFFSET: u32 = 0;
/// Offset of the capacity field from the map base pointer.
pub const PAIRS_CAPACITY_OFFSET: u32 = 4;
/// Total header size in bytes before the first entry.
pub const PAIRS_HEADER_SIZE: u32 = 8;
/// Size of one key/value entry in bytes.
pub const PAIRS_ENTRY_SIZE: u32 = 8;
/// Offset of the key pointer within an entry.
pub const PAIRS_KEY_OFFSET: u32 = 0;
/// Offset of the value pointer within an entry.
pub const PAIRS_VAL_OFFSET: u32 = 4;

/// Generate WASM instructions for __pairs_str_eq(ptr_a: i32, ptr_b: i32) -> i32
///
/// Compares two Clean Language strings for byte-equality.
/// Returns 1 if equal, 0 otherwise.
///
/// Parameters:
///   local 0 = ptr_a (i32)
///   local 1 = ptr_b (i32)
///
/// Extra locals (must be declared via register_function_with_locals):
///   local 2 = len_a (i32)
///   local 3 = i     (i32) — loop counter
///
/// Block structure (following list_ops.rs gen_index_of_i32 pattern):
///   block $result (result i32)    -- depth 0 from inside
///     ... (compare lengths, return 0 early via Br(0))
///     block $byte_cmp_outer       -- wraps loop; depth 0=this, 1=result
///       loop $byte_cmp_loop       -- depth 0=loop, 1=outer, 2=result
///         ... (compare bytes, return 0 via Br(2) on mismatch)
///         ... (exit loop via Br(1) when done)
///       end
///     end
///     i32.const 1   -- fell through means all bytes matched
///   end
pub fn gen_str_eq() -> Vec<Instruction<'static>> {
    vec![
        // Outer result block — produces i32
        Instruction::Block(BlockType::Result(ValType::I32)), // label=0 (from outermost)
        // --- Fast path: pointer equality ---
        // if ptr_a == ptr_b, immediately return 1
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
        Instruction::I32Eq,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1),
        Instruction::Br(1), // break to outer result block, carrying 1
        Instruction::End,
        // --- Length check ---
        // len_a = mem[ptr_a + 0]
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2), // len_a
        // len_b = mem[ptr_b + 0]
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // if len_a != len_b, return 0
        Instruction::LocalGet(2),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Br(1), // break to outer result block, carrying 0
        Instruction::End,
        // --- i = 0 ---
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // --- Byte comparison loop ---
        // Block wrapping loop so we can break out of both at once
        Instruction::Block(BlockType::Empty), // $byte_loop_outer; from inside: 0=this, 1=result
        Instruction::Loop(BlockType::Empty),  // $byte_loop; from inside: 0=loop, 1=outer, 2=result
        // if i >= len_a, break out of loop (all bytes matched)
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32GeU,
        Instruction::BrIf(1), // break to $byte_loop_outer
        // byte_a = mem8[ptr_a + 4 + i]
        Instruction::LocalGet(0),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // byte_b = mem8[ptr_b + 4 + i]
        Instruction::LocalGet(1),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // if byte_a != byte_b, return 0
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Br(3), // break to outer result block, carrying 0 (0=if, 1=loop, 2=outer_block, 3=result)
        Instruction::End,
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0), // continue loop
        Instruction::End,   // end $byte_loop
        Instruction::End,   // end $byte_loop_outer
        // All bytes matched — return 1
        Instruction::I32Const(1),
        Instruction::End, // end outer result block
    ]
}

/// Generate WASM instructions for __pairs_new(capacity: i32) -> i32
///
/// Allocates a new empty pairs map with the given initial capacity.
/// If capacity == 0, uses a default of 8 slots.
///
/// Parameters:
///   local 0 = capacity (i32)
///
/// Extra locals (declared via register_function_with_locals):
///   local 1 = ptr (i32) — scratch register for the allocated pointer
pub(crate) fn gen_pairs_new_impl(malloc_idx: u32) -> Vec<Instruction<'static>> {
    // Parameters:   local 0 = capacity (i32)
    // Extra locals: local 1 = ptr (i32)
    vec![
        // effective_capacity = capacity > 0 ? capacity : 8
        Instruction::LocalGet(0),
        Instruction::I32Const(0),
        Instruction::I32GtS,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::LocalGet(0),
        Instruction::Else,
        Instruction::I32Const(8),
        Instruction::End,
        Instruction::LocalSet(0), // local 0 = effective_capacity
        // size = 8 + effective_capacity * 8
        Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
        Instruction::LocalGet(0),
        Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // ptr = malloc(size)
        Instruction::Call(malloc_idx),
        Instruction::LocalSet(1), // local 1 = ptr
        // mem[ptr + PAIRS_COUNT_OFFSET] = 0
        Instruction::LocalGet(1),
        Instruction::I32Const(0),
        Instruction::I32Store(MemArg {
            offset: PAIRS_COUNT_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // mem[ptr + PAIRS_CAPACITY_OFFSET] = effective_capacity
        Instruction::LocalGet(1),
        Instruction::LocalGet(0),
        Instruction::I32Store(MemArg {
            offset: PAIRS_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // return ptr
        Instruction::LocalGet(1),
    ]
}

/// Generate WASM instructions for __pairs_set(map_ptr: i32, key_ptr: i32, val_ptr: i32) -> void
///
/// Sets (or inserts) a key/value pair in the map.
/// If the key already exists (by byte-equality), the value is updated in place.
/// If not found and count < capacity, appends a new entry.
///
/// Parameters:
///   local 0 = map_ptr (i32)
///   local 1 = key_ptr (i32)
///   local 2 = val_ptr (i32)
///
/// Extra locals (declared via register_function_with_locals):
///   local 3 = count     (i32)
///   local 4 = i         (i32) — loop counter
///   local 5 = entry_ptr (i32)
///
/// Block structure:
///   block $outer (void)          -- early exit; depth from inside: 0=this
///     ...count = mem[map+0]...
///     ...i = 0...
///     block $scan_outer (void)   -- wraps scan loop; depth: 0=this, 1=outer
///       loop $scan (void)        -- depth: 0=loop, 1=scan_outer, 2=outer
///         ... (if key match: store val, br 2 to $outer)
///         ... (if i >= count: br 1 to $scan_outer, fall through to append)
///       end
///     end
///     ... append logic ...
///   end
pub fn gen_pairs_set(str_eq_idx: u32) -> Vec<Instruction<'static>> {
    vec![
        // count = mem[map_ptr + PAIRS_COUNT_OFFSET]
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: PAIRS_COUNT_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3), // count
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        // $outer block — break here when done (either key found+updated, or appended)
        Instruction::Block(BlockType::Empty), // $outer; from inside scan: 0=scan, 1=scan_outer, 2=outer
        // $scan_outer wraps the scan loop; breaking it skips to append logic
        Instruction::Block(BlockType::Empty), // $scan_outer; from inside scan: 0=scan, 1=scan_outer
        // $scan loop
        Instruction::Loop(BlockType::Empty), // $scan; from inside: 0=scan, 1=scan_outer, 2=outer
        // if i >= count, exit scan loop (key not found during scan)
        Instruction::LocalGet(4),
        Instruction::LocalGet(3),
        Instruction::I32GeS,
        Instruction::BrIf(1), // break to $scan_outer (fall through to append)
        // entry_ptr = map_ptr + 8 + i * 8
        Instruction::LocalGet(0),
        Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalSet(5), // entry_ptr
        // entry_key = mem[entry_ptr + PAIRS_KEY_OFFSET]
        Instruction::LocalGet(5),
        Instruction::I32Load(MemArg {
            offset: PAIRS_KEY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // if str_eq(entry_key, key_ptr) == 1, update value and exit
        Instruction::LocalGet(1),
        Instruction::Call(str_eq_idx),
        Instruction::If(BlockType::Empty),
        // mem[entry_ptr + PAIRS_VAL_OFFSET] = val_ptr
        Instruction::LocalGet(5),
        Instruction::LocalGet(2),
        Instruction::I32Store(MemArg {
            offset: PAIRS_VAL_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::Br(3), // break to $outer (0=if, 1=scan, 2=scan_outer, 3=outer)
        Instruction::End,   // end if
        // i++
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0), // continue $scan
        Instruction::End,   // end $scan loop
        Instruction::End,   // end $scan_outer
        // --- Append new entry (key not found in scan) ---
        // capacity = mem[map_ptr + PAIRS_CAPACITY_OFFSET]
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: PAIRS_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // if count >= capacity, skip append (map is full) — br to $outer
        Instruction::LocalGet(3),
        Instruction::I32GeS,
        Instruction::If(BlockType::Empty),
        Instruction::Br(1), // break to $outer (0=if, 1=outer)
        Instruction::End,
        // entry_ptr = map_ptr + 8 + count * 8
        Instruction::LocalGet(0),
        Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalSet(5), // entry_ptr
        // mem[entry_ptr + PAIRS_KEY_OFFSET] = key_ptr
        Instruction::LocalGet(5),
        Instruction::LocalGet(1),
        Instruction::I32Store(MemArg {
            offset: PAIRS_KEY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // mem[entry_ptr + PAIRS_VAL_OFFSET] = val_ptr
        Instruction::LocalGet(5),
        Instruction::LocalGet(2),
        Instruction::I32Store(MemArg {
            offset: PAIRS_VAL_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // mem[map_ptr + PAIRS_COUNT_OFFSET] = count + 1
        Instruction::LocalGet(0),
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::I32Store(MemArg {
            offset: PAIRS_COUNT_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::End, // end $outer
    ]
}

/// Generate WASM instructions for __pairs_get(map_ptr: i32, key_ptr: i32) -> i32
///
/// Returns the value pointer associated with key_ptr, or 0 if not found.
///
/// Parameters:
///   local 0 = map_ptr (i32)
///   local 1 = key_ptr (i32)
///
/// Extra locals (declared via register_function_with_locals):
///   local 2 = count     (i32)
///   local 3 = i         (i32)
///   local 4 = entry_ptr (i32)
///
/// Block structure (following list_ops.rs gen_index_of_i32 pattern):
///   block $result (result i32)   -- produces the return value
///     block $scan_outer (void)   -- wraps scan loop; depth from inside loop: 0=loop, 1=this, 2=result
///       loop $scan (void)
///         if i >= count: br 1 to $scan_outer (not found, fall through to i32.const 0)
///         if key match: load val, br 2 to $result carrying val
///       end
///     end
///     i32.const 0   -- not found default
///   end
pub fn gen_pairs_get(str_eq_idx: u32) -> Vec<Instruction<'static>> {
    vec![
        // count = mem[map_ptr + PAIRS_COUNT_OFFSET]
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: PAIRS_COUNT_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2), // count
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // $result block — produces i32 return value
        Instruction::Block(BlockType::Result(ValType::I32)), // from inside: 0=scan, 1=scan_outer, 2=result
        // $scan_outer wraps the scan loop
        Instruction::Block(BlockType::Empty), // $scan_outer; from inside: 0=scan, 1=scan_outer, 2=result
        // $scan loop
        Instruction::Loop(BlockType::Empty), // $scan; from inside: 0=scan, 1=scan_outer, 2=result
        // if i >= count, exit scan (not found)
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32GeS,
        Instruction::BrIf(1), // break to $scan_outer
        // entry_ptr = map_ptr + 8 + i * 8
        Instruction::LocalGet(0),
        Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalSet(4), // entry_ptr
        // entry_key = mem[entry_ptr + PAIRS_KEY_OFFSET]
        Instruction::LocalGet(4),
        Instruction::I32Load(MemArg {
            offset: PAIRS_KEY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // if str_eq(entry_key, key_ptr) == 1, return val
        Instruction::LocalGet(1),
        Instruction::Call(str_eq_idx),
        Instruction::If(BlockType::Empty),
        // push val_ptr = mem[entry_ptr + PAIRS_VAL_OFFSET]
        Instruction::LocalGet(4),
        Instruction::I32Load(MemArg {
            offset: PAIRS_VAL_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::Br(3), // break to $result carrying val (0=if, 1=scan, 2=scan_outer, 3=result)
        Instruction::End,   // end if
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0), // continue $scan
        Instruction::End,   // end $scan loop
        Instruction::End,   // end $scan_outer
        // Not found — push default 0
        Instruction::I32Const(0),
        Instruction::End, // end $result block
    ]
}

/// Generate WASM instructions for __pairs_has(map_ptr: i32, key_ptr: i32) -> i32
///
/// Returns 1 if key_ptr exists in the map, 0 otherwise.
///
/// Parameters:
///   local 0 = map_ptr (i32)
///   local 1 = key_ptr (i32)
///
/// Extra locals (declared via register_function_with_locals):
///   local 2 = count     (i32)
///   local 3 = i         (i32)
///   local 4 = entry_ptr (i32)
pub fn gen_pairs_has(str_eq_idx: u32) -> Vec<Instruction<'static>> {
    vec![
        // count = mem[map_ptr + PAIRS_COUNT_OFFSET]
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: PAIRS_COUNT_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2), // count
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // $result block — produces i32 return value
        Instruction::Block(BlockType::Result(ValType::I32)), // from inside: 0=scan, 1=scan_outer, 2=result
        // $scan_outer wraps the scan loop
        Instruction::Block(BlockType::Empty), // $scan_outer; from inside: 0=scan, 1=scan_outer, 2=result
        // $scan loop
        Instruction::Loop(BlockType::Empty), // $scan; from inside: 0=scan, 1=scan_outer, 2=result
        // if i >= count, exit scan (not found)
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32GeS,
        Instruction::BrIf(1), // break to $scan_outer
        // entry_ptr = map_ptr + 8 + i * 8
        Instruction::LocalGet(0),
        Instruction::I32Const(PAIRS_HEADER_SIZE as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Const(PAIRS_ENTRY_SIZE as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalSet(4), // entry_ptr
        // entry_key = mem[entry_ptr + PAIRS_KEY_OFFSET]
        Instruction::LocalGet(4),
        Instruction::I32Load(MemArg {
            offset: PAIRS_KEY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // if str_eq(entry_key, key_ptr) == 1, return 1 (found)
        Instruction::LocalGet(1),
        Instruction::Call(str_eq_idx),
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1), // found
        Instruction::Br(3), // break to $result carrying 1 (0=if, 1=scan, 2=scan_outer, 3=result)
        Instruction::End,   // end if
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0), // continue $scan
        Instruction::End,   // end $scan loop
        Instruction::End,   // end $scan_outer
        // Not found — push 0
        Instruction::I32Const(0),
        Instruction::End, // end $result block
    ]
}

/// Generate WASM instructions for __pairs_len(map_ptr: i32) -> i32
///
/// Returns the number of key/value pairs currently stored in the map.
///
/// Parameters:
///   local 0 = map_ptr (i32)
pub fn gen_pairs_len() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: PAIRS_COUNT_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
    ]
}
