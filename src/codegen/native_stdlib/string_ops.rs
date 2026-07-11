//! Native WASM string operations
//!
//! Provides pure WASM bytecode implementations of string functions
//! that operate on the standard Clean Language string format:
//! [length: i32][data: bytes...]

use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

use super::{STRING_DATA_OFFSET, STRING_LENGTH_OFFSET};

/// Generate instructions for string.length
/// Simply reads the length field at offset 0
pub fn gen_length() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0), // string pointer
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2, // 4-byte alignment
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for string.charAt
/// Returns the byte value at the given index
pub fn gen_char_at() -> Vec<Instruction<'static>> {
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

/// Generate instructions for string.charCodeAt (same as charAt)
pub fn gen_char_code_at() -> Vec<Instruction<'static>> {
    gen_char_at()
}

/// Generate instructions for string.fromCharCode
///
/// Converts an integer code point to a single-character string.
///
/// Parameters:
///   - local 0: code_point (i32)
///
/// Returns: i32 (pointer to new 1-char length-prefixed string)
///
/// Uses locals:
///   - local 1: new_ptr
pub fn gen_from_char_code(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Allocate STRING_DATA_OFFSET + 1 bytes (4-byte length header + 1 byte)
        Instruction::I32Const((STRING_DATA_OFFSET as i32) + 1),
        Instruction::Call(malloc_func),
        Instruction::LocalSet(1), // new_ptr
        // Write length = 1
        Instruction::LocalGet(1),
        Instruction::I32Const(1),
        Instruction::I32Store(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Write byte value: new_ptr[4] = code_point & 0xFF
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(0), // code_point
        Instruction::I32Const(0xFF),
        Instruction::I32And,
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Return new_ptr
        Instruction::LocalGet(1),
    ]
}

/// Generate instructions for string.startsWith
///
/// Parameters:
///   - local 0: str_ptr (string to check)
///   - local 1: prefix_ptr (prefix to look for)
///
/// Returns: i32 (1 if starts with prefix, 0 otherwise)
///
/// Uses locals:
///   - local 2: prefix_len
///   - local 3: str_len
///   - local 4: i (loop counter)
pub fn gen_starts_with() -> Vec<Instruction<'static>> {
    vec![
        // Get prefix length -> local 2
        Instruction::LocalGet(1), // prefix_ptr
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2), // prefix_len
        // Get string length -> local 3
        Instruction::LocalGet(0), // str_ptr
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3), // str_len
        // If prefix_len > str_len, return 0
        Instruction::LocalGet(2), // prefix_len
        Instruction::LocalGet(3), // str_len
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0), // false
        Instruction::Else,
        // Initialize i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        // Loop to compare bytes
        Instruction::Block(BlockType::Result(ValType::I32)),
        Instruction::Loop(BlockType::Empty),
        // If i >= prefix_len, all bytes matched -> return 1
        Instruction::LocalGet(4), // i
        Instruction::LocalGet(2), // prefix_len
        Instruction::I32GeU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1), // true
        Instruction::Br(2),       // Exit block with 1
        Instruction::End,
        // Compare str[i] vs prefix[i]
        // Load str[i]
        Instruction::LocalGet(0), // str_ptr
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4), // i
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Load prefix[i]
        Instruction::LocalGet(1), // prefix_ptr
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4), // i
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Compare bytes
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0), // Mismatch, return false
        Instruction::Br(2),       // Exit block with 0
        Instruction::End,
        // i++
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        // Continue loop
        Instruction::Br(0),
        Instruction::End, // End loop
        // Default: all matched
        Instruction::I32Const(1),
        Instruction::End, // End block
        Instruction::End, // End if/else
    ]
}

/// Generate instructions for string.endsWith
///
/// Parameters:
///   - local 0: str_ptr (string to check)
///   - local 1: suffix_ptr (suffix to look for)
///
/// Returns: i32 (1 if ends with suffix, 0 otherwise)
///
/// Uses locals:
///   - local 2: suffix_len
///   - local 3: str_len
///   - local 4: offset (str_len - suffix_len)
///   - local 5: i (loop counter)
pub fn gen_ends_with() -> Vec<Instruction<'static>> {
    vec![
        // Get suffix length -> local 2
        Instruction::LocalGet(1), // suffix_ptr
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2), // suffix_len
        // Get string length -> local 3
        Instruction::LocalGet(0), // str_ptr
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3), // str_len
        // If suffix_len > str_len, return 0
        Instruction::LocalGet(2), // suffix_len
        Instruction::LocalGet(3), // str_len
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0), // false
        Instruction::Else,
        // Calculate offset = str_len - suffix_len -> local 4
        Instruction::LocalGet(3), // str_len
        Instruction::LocalGet(2), // suffix_len
        Instruction::I32Sub,
        Instruction::LocalSet(4), // offset
        // Initialize i = 0 -> local 5
        Instruction::I32Const(0),
        Instruction::LocalSet(5),
        // Loop to compare bytes
        Instruction::Block(BlockType::Result(ValType::I32)),
        Instruction::Loop(BlockType::Empty),
        // If i >= suffix_len, all bytes matched -> return 1
        Instruction::LocalGet(5), // i
        Instruction::LocalGet(2), // suffix_len
        Instruction::I32GeU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1),
        Instruction::Br(2),
        Instruction::End,
        // Compare str[offset + i] vs suffix[i]
        // Load str[offset + i]
        Instruction::LocalGet(0), // str_ptr
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4), // offset
        Instruction::I32Add,
        Instruction::LocalGet(5), // i
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Load suffix[i]
        Instruction::LocalGet(1), // suffix_ptr
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5), // i
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Compare bytes
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Br(2),
        Instruction::End,
        // i++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        // Continue loop
        Instruction::Br(0),
        Instruction::End, // End loop
        Instruction::I32Const(1),
        Instruction::End, // End block
        Instruction::End, // End if/else
    ]
}

/// Generate instructions for string.indexOf
///
/// Parameters:
///   - local 0: str_ptr (string to search in)
///   - local 1: search_ptr (string to search for)
///
/// Returns: i32 (index of first occurrence, or -1 if not found)
///
/// Uses locals:
///   - local 2: str_len
///   - local 3: search_len
///   - local 4: i (outer loop counter)
///   - local 5: j (inner loop counter)
///   - local 6: match (flag)
pub fn gen_index_of() -> Vec<Instruction<'static>> {
    vec![
        // Get str_len -> local 2
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2),
        // Get search_len -> local 3
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // If search_len == 0, return 0
        Instruction::LocalGet(3),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0),
        Instruction::Else,
        // If search_len > str_len, return -1
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(-1),
        Instruction::Else,
        // Initialize i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        // Outer loop: try each starting position
        Instruction::Block(BlockType::Result(ValType::I32)),
        Instruction::Loop(BlockType::Empty),
        // If i > str_len - search_len, not found -> return -1
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Sub,
        Instruction::I32GtU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(-1),
        Instruction::Br(2),
        Instruction::End,
        // Check if substring matches at position i
        // Initialize j = 0, match = 1
        Instruction::I32Const(0),
        Instruction::LocalSet(5), // j
        Instruction::I32Const(1),
        Instruction::LocalSet(6), // match
        // Inner loop: compare characters
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // If j >= search_len, done checking
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // If match == 0, skip remaining comparisons
        Instruction::LocalGet(6),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // Compare str[i+j] vs search[j]
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(6), // match = false
        Instruction::End,
        // j++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End, // End inner loop
        Instruction::End, // End inner block
        // If match == 1, found -> return i
        Instruction::LocalGet(6),
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(4), // return i
        Instruction::Br(2),
        Instruction::End,
        // i++
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,          // End outer loop
        Instruction::I32Const(-1), // default: not found
        Instruction::End,          // End outer block
        Instruction::End,          // End if (search_len > str_len)
        Instruction::End,          // End if (search_len == 0)
    ]
}

/// Generate instructions for string.indexOfFrom (indexOf with start index)
///
/// Parameters:
///   - local 0: str_ptr (string to search in)
///   - local 1: search_ptr (string to search for)
///   - local 2: start_index (position to start searching from)
///
/// Returns: i32 (index of first occurrence from start_index, or -1 if not found)
///
/// Uses locals:
///   - local 3: str_len
///   - local 4: search_len
///   - local 5: i (outer loop counter)
///   - local 6: j (inner loop counter)
///   - local 7: match (flag)
pub fn gen_index_of_from() -> Vec<Instruction<'static>> {
    vec![
        // Get str_len -> local 3
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // Get search_len -> local 4
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(4),
        // If search_len == 0, return start_index (or 0 if start_index < 0)
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::LocalGet(2), // Return start_index
        Instruction::I32Const(0),
        Instruction::LocalGet(2),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::Select,
        Instruction::Else,
        // If search_len > str_len, return -1
        Instruction::LocalGet(4),
        Instruction::LocalGet(3),
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(-1),
        Instruction::Else,
        // Clamp start_index to [0, str_len - search_len]
        // If start_index < 0, start from 0
        Instruction::LocalGet(2),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(2),
        Instruction::End,
        // Initialize i = start_index
        Instruction::LocalGet(2),
        Instruction::LocalSet(5),
        // Outer loop: try each starting position
        Instruction::Block(BlockType::Result(ValType::I32)),
        Instruction::Loop(BlockType::Empty),
        // If i > str_len - search_len, not found -> return -1
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::LocalGet(4),
        Instruction::I32Sub,
        Instruction::I32GtU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(-1),
        Instruction::Br(2),
        Instruction::End,
        // Check if substring matches at position i
        // Initialize j = 0, match = 1
        Instruction::I32Const(0),
        Instruction::LocalSet(6), // j
        Instruction::I32Const(1),
        Instruction::LocalSet(7), // match
        // Inner loop: compare characters
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // If j >= search_len, done checking
        Instruction::LocalGet(6),
        Instruction::LocalGet(4),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // If match == 0, skip remaining comparisons
        Instruction::LocalGet(7),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // Compare str[i+j] vs search[j]
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(7), // match = false
        Instruction::End,
        // j++
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(6),
        Instruction::Br(0),
        Instruction::End, // End inner loop
        Instruction::End, // End inner block
        // If match == 1, found -> return i
        Instruction::LocalGet(7),
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(5), // return i
        Instruction::Br(2),
        Instruction::End,
        // i++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End,          // End outer loop
        Instruction::I32Const(-1), // default: not found
        Instruction::End,          // End outer block
        Instruction::End,          // End if (search_len > str_len)
        Instruction::End,          // End if (search_len == 0)
    ]
}

/// Generate instructions for string.lastIndexOfFrom (lastIndexOf with start index)
///
/// Parameters:
///   - local 0: str_ptr (string to search in)
///   - local 1: search_ptr (string to search for)
///   - local 2: start_index (search backwards from this position, inclusive)
///
/// Returns: i32 (index of last occurrence at or before start_index, or -1 if not found)
///
/// Uses locals:
///   - local 3: str_len
///   - local 4: search_len
///   - local 5: i (outer loop counter)
///   - local 6: j (inner loop counter)
///   - local 7: match (flag)
///   - local 8: result (stores last found index)
pub fn gen_last_index_of_from() -> Vec<Instruction<'static>> {
    vec![
        // Get str_len -> local 3
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // Get search_len -> local 4
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(4),
        // Initialize result to -1 (not found)
        Instruction::I32Const(-1),
        Instruction::LocalSet(8),
        // If search_len == 0, return start_index (clamped to str_len)
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        // Return min(start_index, str_len)
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32LtS,
        Instruction::Select,
        Instruction::Else,
        // If search_len > str_len, return -1
        Instruction::LocalGet(4),
        Instruction::LocalGet(3),
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(-1),
        Instruction::Else,
        // Clamp start_index: i = min(start_index, str_len - search_len)
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::LocalGet(4),
        Instruction::I32Sub,
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::LocalGet(4),
        Instruction::I32Sub,
        Instruction::I32LtS,
        Instruction::Select,
        Instruction::LocalSet(5),
        // If start_index was negative, return -1
        Instruction::LocalGet(2),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(-1),
        Instruction::Else,
        // Outer loop: try each starting position from start to beginning
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // Check if substring matches at position i
        // Initialize j = 0, match = 1
        Instruction::I32Const(0),
        Instruction::LocalSet(6), // j
        Instruction::I32Const(1),
        Instruction::LocalSet(7), // match
        // Inner loop: compare characters
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // If j >= search_len, done checking
        Instruction::LocalGet(6),
        Instruction::LocalGet(4),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // If match == 0, skip remaining comparisons
        Instruction::LocalGet(7),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // Compare str[i+j] vs search[j]
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(7), // match = false
        Instruction::End,
        // j++
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(6),
        Instruction::Br(0),
        Instruction::End, // End inner loop
        Instruction::End, // End inner block
        // If match == 1, we found it at position i - return immediately
        // (searching backwards, so first found is the last occurrence at or before start)
        Instruction::LocalGet(7),
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(5), // Store result = i
        Instruction::LocalSet(8),
        Instruction::Br(2), // Found! Exit outer Block (not Loop)
        Instruction::End,
        // If i == 0, we've checked all positions, exit loop
        Instruction::LocalGet(5),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // i--
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End, // End outer loop
        Instruction::End, // End outer block
        // Return result (either the found index or -1)
        Instruction::LocalGet(8),
        Instruction::End, // End if (start_index < 0)
        Instruction::End, // End if (search_len > str_len)
        Instruction::End, // End if (search_len == 0)
    ]
}

/// Generate instructions for string.lastIndexOf
///
/// Parameters:
///   - local 0: str_ptr (string to search in)
///   - local 1: search_ptr (string to search for)
///
/// Returns: i32 (index of last occurrence, or -1 if not found)
///
/// Uses locals:
///   - local 2: str_len
///   - local 3: search_len
///   - local 4: i (loop counter - starts from end)
///   - local 5: j (inner loop counter)
///   - local 6: match (flag)
///   - local 7: result (stores last found index)
pub fn gen_last_index_of() -> Vec<Instruction<'static>> {
    vec![
        // Get str_len -> local 2
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2),
        // Get search_len -> local 3
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // Initialize result to -1 (not found)
        Instruction::I32Const(-1),
        Instruction::LocalSet(7),
        // If search_len == 0, return str_len (empty string is at the end)
        Instruction::LocalGet(3),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::LocalGet(2), // Return str_len for empty search
        Instruction::Else,
        // If search_len > str_len, return -1
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(-1),
        Instruction::Else,
        // Initialize i = str_len - search_len (start from the end)
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Sub,
        Instruction::LocalSet(4),
        // Outer loop: try each starting position from end to beginning
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // Check if substring matches at position i
        // Initialize j = 0, match = 1
        Instruction::I32Const(0),
        Instruction::LocalSet(5), // j
        Instruction::I32Const(1),
        Instruction::LocalSet(6), // match
        // Inner loop: compare characters
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // If j >= search_len, done checking
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // If match == 0, skip remaining comparisons
        Instruction::LocalGet(6),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // Compare str[i+j] vs search[j]
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(6), // match = false
        Instruction::End,
        // j++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End, // End inner loop
        Instruction::End, // End inner block
        // If match == 1, we found it at position i - this is the last occurrence
        // so we can return immediately (since we're searching from the end)
        Instruction::LocalGet(6),
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(4), // Store result = i
        Instruction::LocalSet(7),
        Instruction::Br(2), // Found! Exit outer Block (not Loop)
        Instruction::End,
        // If i == 0, we've checked all positions, exit loop
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // i--
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End, // End outer loop
        Instruction::End, // End outer block
        // Return result (either the found index or -1)
        Instruction::LocalGet(7),
        Instruction::End, // End if (search_len > str_len)
        Instruction::End, // End if (search_len == 0)
    ]
}

/// Generate instructions for string.contains
/// Returns 1 if indexOf >= 0, else 0
pub fn gen_contains(index_of_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Call indexOf
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
        Instruction::Call(index_of_func),
        // Check if result >= 0
        Instruction::I32Const(0),
        Instruction::I32GeS,
    ]
}

/// Generate instructions for string.concat (requires malloc)
///
/// Parameters:
///   - local 0: str1_ptr
///   - local 1: str2_ptr
///
/// Returns: i32 (pointer to new concatenated string, or 0 on overflow)
///
/// Uses locals:
///   - local 2: len1
///   - local 3: len2
///   - local 4: new_ptr
///   - local 5: i (loop counter)
///   - local 6: combined_len (for overflow checking)
pub fn gen_concat(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get len1 -> local 2
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2),
        // Get len2 -> local 3
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // OVERFLOW CHECK: Calculate combined_len = len1 + len2 -> local 6
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::LocalSet(6),
        // Check for overflow: if combined_len < len1, overflow occurred (unsigned comparison)
        // If overflow, return 0 (null pointer) immediately
        Instruction::LocalGet(6),
        Instruction::LocalGet(2),
        Instruction::I32LtU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // Check: combined_len > MAX_SAFE_SIZE (0x7FFFFFF8, leaves room for length prefix + alignment)
        // If would overflow when adding 4, return 0 immediately
        Instruction::LocalGet(6),
        Instruction::I32Const(0x7FFFFFF8_u32 as i32),
        Instruction::I32GtU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // No overflow - allocate new string: malloc(4 + combined_len)
        Instruction::LocalGet(6),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(4),
        // If allocation returned 0 (null), just return 0
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // Store combined length at new_ptr (use pre-computed local 6)
        Instruction::LocalGet(4),
        Instruction::LocalGet(6), // combined_len already computed with overflow check
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Copy first string (manual loop instead of memcpy call to avoid circular dependency)
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(5),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= len1, exit
        Instruction::LocalGet(5),
        Instruction::LocalGet(2),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + i] = str1_ptr[4 + i]
        Instruction::LocalGet(4),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Copy second string
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(5),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= len2, exit
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + len1 + i] = str2_ptr[4 + i]
        Instruction::LocalGet(4),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2), // len1
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new pointer
        Instruction::LocalGet(4),
    ]
}

/// Generate instructions for string.substring (requires malloc).
///
/// Per `foundation/spec/stdlib-reference.md` §"Error semantics" (line 165):
///
/// > `.substring(start, end)` clamps to valid range rather than halting.
///
/// The pre-0.33.52 implementation trusted its callers and did `end - start`
/// as an unsigned length. When `end < start` (produced by upstream code
/// such as `path.length() - 4` on an empty string, or a `indexOf`-derived
/// index that failed to find its needle), the subtraction underflowed to
/// ~4 billion; the malloc-then-memcpy loop then walked off the end of
/// linear memory and trapped. That failure was reported as CODEGEN000
/// (fingerprint 74837f51f68b) in frame.ui's `assemble_typed`, but the
/// same trap fires for every substring call anywhere in the language
/// when its indices go out of range.
///
/// The fix clamps both endpoints to `[0, str_len]` and swaps them so
/// that `end < start` returns an empty string. Every code path continues
/// to produce a valid `String` value with a valid length prefix — no
/// user code needs to guard indices.
///
/// Parameters:
///   - local 0: str_ptr
///   - local 1: start (signed i32; clamped in the body)
///   - local 2: end   (signed i32; clamped in the body)
///
/// Returns: i32 (pointer to new substring)
///
/// Uses locals:
///   - local 3: str_len
///   - local 4: new_len
///   - local 5: new_ptr
///   - local 6: i (loop counter)
pub fn gen_substring(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // str_len = str_ptr[0] -> local 3
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // Clamp start into [0, str_len].
        // Signed compare so negative starts land at 0.
        //   if start < 0 { start = 0 }
        Instruction::LocalGet(1),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(1),
        Instruction::End,
        //   if start > str_len { start = str_len }
        Instruction::LocalGet(1),
        Instruction::LocalGet(3),
        Instruction::I32GtS,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalSet(1),
        Instruction::End,
        // Clamp end into [0, str_len].
        //   if end < 0 { end = 0 }
        Instruction::LocalGet(2),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::LocalSet(2),
        Instruction::End,
        //   if end > str_len { end = str_len }
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32GtS,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalSet(2),
        Instruction::End,
        // If end < start, treat as empty range (end = start).
        // Spec: "clamps to valid range rather than halting" — the
        // language-level guarantee is a well-formed empty String
        // rather than a trap.
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32LtS,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(1),
        Instruction::LocalSet(2),
        Instruction::End,
        // new_len = end - start -> local 4
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Sub,
        Instruction::LocalSet(4),
        // Allocate new string: malloc(STRING_DATA_OFFSET + new_len)
        Instruction::LocalGet(4),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(5),
        // Store length prefix.
        Instruction::LocalGet(5),
        Instruction::LocalGet(4),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Copy bytes from str_ptr[STRING_DATA_OFFSET + start] to new_ptr[STRING_DATA_OFFSET].
        Instruction::I32Const(0),
        Instruction::LocalSet(6),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= new_len, exit
        Instruction::LocalGet(6),
        Instruction::LocalGet(4),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[STRING_DATA_OFFSET + i] = str_ptr[STRING_DATA_OFFSET + start + i]
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(6),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new pointer
        Instruction::LocalGet(5),
    ]
}

/// Generate instructions for string.trimStart
///
/// Trims whitespace characters from the beginning of a string.
/// Whitespace: space (32), tab (9), newline (10), carriage return (13)
///
/// Parameters:
///   - local 0: str_ptr (input string)
///
/// Returns: i32 (pointer to new trimmed string)
///
/// Uses locals:
///   - local 1: str_len
///   - local 2: start_idx (first non-whitespace index)
///   - local 3: new_len
///   - local 4: new_ptr
///   - local 5: i (loop counter)
///   - local 6: temp byte
pub fn gen_trim_start(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get string length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1), // str_len
        // Find first non-whitespace character
        // start_idx = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(2), // start_idx
        // Loop to find first non-whitespace
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if start_idx >= str_len, break (all whitespace)
        Instruction::LocalGet(2), // start_idx
        Instruction::LocalGet(1), // str_len
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // Load byte at start_idx
        Instruction::LocalGet(0), // str_ptr
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2), // start_idx
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalSet(6), // temp byte
        // Check if whitespace
        Instruction::LocalGet(6),
        Instruction::I32Const(32), // space
        Instruction::I32Eq,
        Instruction::LocalGet(6),
        Instruction::I32Const(9), // tab
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(6),
        Instruction::I32Const(10), // newline
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(6),
        Instruction::I32Const(13), // carriage return
        Instruction::I32Eq,
        Instruction::I32Or,
        // If not whitespace, break
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // start_idx++
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(2),
        Instruction::Br(0), // continue loop
        Instruction::End,   // end loop
        Instruction::End,   // end block
        // Calculate new_len = str_len - start_idx
        Instruction::LocalGet(1), // str_len
        Instruction::LocalGet(2), // start_idx
        Instruction::I32Sub,
        Instruction::LocalSet(3), // new_len
        // Allocate new string: malloc(4 + new_len)
        Instruction::LocalGet(3),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(4), // new_ptr
        // Store length
        Instruction::LocalGet(4),
        Instruction::LocalGet(3),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Copy bytes from str_ptr[4 + start_idx] to new_ptr[4]
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(5),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= new_len, exit
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + i] = str_ptr[4 + start_idx + i]
        Instruction::LocalGet(4),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2), // start_idx
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new pointer
        Instruction::LocalGet(4),
    ]
}

/// Generate instructions for string.trimEnd
///
/// Trims whitespace characters from the end of a string.
///
/// Parameters:
///   - local 0: str_ptr (input string)
///
/// Returns: i32 (pointer to new trimmed string)
///
/// Uses locals:
///   - local 1: str_len
///   - local 2: end_idx (last non-whitespace index + 1 = new length)
///   - local 3: new_ptr
///   - local 4: i (loop counter)
///   - local 5: temp byte
pub fn gen_trim_end(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get string length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1), // str_len
        // end_idx = str_len (will be decremented)
        Instruction::LocalGet(1),
        Instruction::LocalSet(2), // end_idx
        // Loop to find last non-whitespace (search from end)
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if end_idx <= 0, break (all whitespace)
        Instruction::LocalGet(2), // end_idx
        Instruction::I32Const(0),
        Instruction::I32LeS,
        Instruction::BrIf(1),
        // Load byte at end_idx - 1
        Instruction::LocalGet(0), // str_ptr
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2), // end_idx
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalSet(5), // temp byte
        // Check if whitespace
        Instruction::LocalGet(5),
        Instruction::I32Const(32), // space
        Instruction::I32Eq,
        Instruction::LocalGet(5),
        Instruction::I32Const(9), // tab
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(5),
        Instruction::I32Const(10), // newline
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(5),
        Instruction::I32Const(13), // carriage return
        Instruction::I32Eq,
        Instruction::I32Or,
        // If not whitespace, break
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // end_idx--
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(2),
        Instruction::Br(0), // continue loop
        Instruction::End,   // end loop
        Instruction::End,   // end block
        // new_len = end_idx (already the count of non-trailing-whitespace chars)
        // Allocate new string: malloc(4 + end_idx)
        Instruction::LocalGet(2),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(3), // new_ptr
        // Store length
        Instruction::LocalGet(3),
        Instruction::LocalGet(2), // end_idx = new length
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Copy bytes from str_ptr[4] to new_ptr[4], for end_idx bytes
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= end_idx, exit
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + i] = str_ptr[4 + i]
        Instruction::LocalGet(3),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new pointer
        Instruction::LocalGet(3),
    ]
}

/// Generate instructions for string.trim
///
/// Trims whitespace characters from both ends of a string.
///
/// Parameters:
///   - local 0: str_ptr (input string)
///
/// Returns: i32 (pointer to new trimmed string)
///
/// Uses locals:
///   - local 1: str_len
///   - local 2: start_idx
///   - local 3: end_idx
///   - local 4: new_len
///   - local 5: new_ptr
///   - local 6: i (loop counter)
///   - local 7: temp byte
pub fn gen_trim(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get string length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1), // str_len
        // Find first non-whitespace (start_idx)
        Instruction::I32Const(0),
        Instruction::LocalSet(2), // start_idx = 0
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if start_idx >= str_len, break
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // Load byte
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalSet(7), // temp byte
        // Check whitespace
        Instruction::LocalGet(7),
        Instruction::I32Const(32),
        Instruction::I32Eq,
        Instruction::LocalGet(7),
        Instruction::I32Const(9),
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(7),
        Instruction::I32Const(10),
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(7),
        Instruction::I32Const(13),
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::I32Eqz,
        Instruction::BrIf(1), // not whitespace, break
        // start_idx++
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(2),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Find last non-whitespace (end_idx)
        Instruction::LocalGet(1),
        Instruction::LocalSet(3), // end_idx = str_len
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if end_idx <= start_idx, break
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32LeS,
        Instruction::BrIf(1),
        // Load byte at end_idx - 1
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalSet(7), // temp byte
        // Check whitespace
        Instruction::LocalGet(7),
        Instruction::I32Const(32),
        Instruction::I32Eq,
        Instruction::LocalGet(7),
        Instruction::I32Const(9),
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(7),
        Instruction::I32Const(10),
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::LocalGet(7),
        Instruction::I32Const(13),
        Instruction::I32Eq,
        Instruction::I32Or,
        Instruction::I32Eqz,
        Instruction::BrIf(1), // not whitespace, break
        // end_idx--
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Calculate new_len = end_idx - start_idx
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32Sub,
        Instruction::LocalSet(4), // new_len
        // Handle case where new_len <= 0 (all whitespace)
        Instruction::LocalGet(4),
        Instruction::I32Const(0),
        Instruction::I32LeS,
        Instruction::If(BlockType::Result(ValType::I32)),
        // Allocate empty string
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::Call(malloc_func),
        Instruction::LocalSet(5),
        Instruction::LocalGet(5),
        Instruction::I32Const(0),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalGet(5),
        Instruction::Else,
        // Allocate new string: malloc(4 + new_len)
        Instruction::LocalGet(4),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(5), // new_ptr
        // Store length
        Instruction::LocalGet(5),
        Instruction::LocalGet(4),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Copy bytes
        Instruction::I32Const(0),
        Instruction::LocalSet(6), // i = 0
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= new_len, exit
        Instruction::LocalGet(6),
        Instruction::LocalGet(4),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + i] = str_ptr[4 + start_idx + i]
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2), // start_idx
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(6),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new_ptr
        Instruction::LocalGet(5),
        Instruction::End, // end if/else
    ]
}

/// Generate WASM instructions for string to uppercase conversion.
/// Parameters: local 0 = str_ptr
/// Locals: 1=str_len, 2=new_ptr, 3=i (loop counter), 4=ch
pub fn gen_to_upper(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get string length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        // Allocate new string: len + STRING_DATA_OFFSET bytes -> local 2
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(2),
        // Write length to new string header
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Store(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // Loop over each character
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= str_len, exit
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // Load byte from original string at data offset + i
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalSet(4),
        // If 'a'(97) <= ch <= 'z'(122): ch -= 32
        Instruction::LocalGet(4),
        Instruction::I32Const(97),
        Instruction::I32GeU,
        Instruction::LocalGet(4),
        Instruction::I32Const(122),
        Instruction::I32LeU,
        Instruction::I32And,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::I32Const(32),
        Instruction::I32Sub,
        Instruction::LocalSet(4),
        Instruction::End,
        // Store converted byte to new string at data offset + i
        Instruction::LocalGet(2),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
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
        // Return new string ptr
        Instruction::LocalGet(2),
    ]
}

/// Generate WASM instructions for string to lowercase conversion.
/// Parameters: local 0 = str_ptr
/// Locals: 1=str_len, 2=new_ptr, 3=i (loop counter), 4=ch
pub fn gen_to_lower(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get string length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        // Allocate new string: len + STRING_DATA_OFFSET bytes -> local 2
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(2),
        // Write length to new string header
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Store(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // Loop over each character
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= str_len, exit
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // Load byte from original string at data offset + i
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalSet(4),
        // If 'A'(65) <= ch <= 'Z'(90): ch += 32
        Instruction::LocalGet(4),
        Instruction::I32Const(65),
        Instruction::I32GeU,
        Instruction::LocalGet(4),
        Instruction::I32Const(90),
        Instruction::I32LeU,
        Instruction::I32And,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::I32Const(32),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::End,
        // Store converted byte to new string at data offset + i
        Instruction::LocalGet(2),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
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
        // Return new string ptr
        Instruction::LocalGet(2),
    ]
}

/// Get local variables needed for each string function
pub fn get_locals(func_name: &str) -> Vec<(u32, ValType)> {
    match func_name {
        "starts_with" => vec![
            (1, ValType::I32), // prefix_len
            (1, ValType::I32), // str_len
            (1, ValType::I32), // i
        ],
        "ends_with" => vec![
            (1, ValType::I32), // suffix_len
            (1, ValType::I32), // str_len
            (1, ValType::I32), // offset
            (1, ValType::I32), // i
        ],
        "index_of" => vec![
            (1, ValType::I32), // str_len
            (1, ValType::I32), // search_len
            (1, ValType::I32), // i
            (1, ValType::I32), // j
            (1, ValType::I32), // match
        ],
        "concat" => vec![
            (1, ValType::I32), // len1
            (1, ValType::I32), // len2
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
        ],
        "substring" => vec![
            (1, ValType::I32), // new_len
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
        ],
        "trim_start" => vec![
            (1, ValType::I32), // str_len
            (1, ValType::I32), // start_idx
            (1, ValType::I32), // new_len
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
            (1, ValType::I32), // temp byte
        ],
        "trim_end" => vec![
            (1, ValType::I32), // str_len
            (1, ValType::I32), // end_idx
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
            (1, ValType::I32), // temp byte
        ],
        "trim" => vec![
            (1, ValType::I32), // str_len
            (1, ValType::I32), // start_idx
            (1, ValType::I32), // end_idx
            (1, ValType::I32), // new_len
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
            (1, ValType::I32), // temp byte
        ],
        _ => vec![],
    }
}
