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
/// Returns: i32 (pointer to new concatenated string)
///
/// Uses locals:
///   - local 2: len1
///   - local 3: len2
///   - local 4: new_ptr
///   - local 5: i (loop counter)
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
        // Allocate new string: malloc(4 + len1 + len2)
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(4), // new_ptr
        // Store combined length at new_ptr
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Add,
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

/// Generate instructions for string.substring (requires malloc)
///
/// Parameters:
///   - local 0: str_ptr
///   - local 1: start
///   - local 2: end
///
/// Returns: i32 (pointer to new substring)
///
/// Uses locals:
///   - local 3: new_len
///   - local 4: new_ptr
///   - local 5: i (loop counter)
pub fn gen_substring(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Calculate new_len = end - start -> local 3
        Instruction::LocalGet(2), // end
        Instruction::LocalGet(1), // start
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
        // Copy bytes from str_ptr[4 + start] to new_ptr[4]
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
        // new_ptr[4 + i] = str_ptr[4 + start + i]
        Instruction::LocalGet(4),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // start
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
        _ => vec![],
    }
}
