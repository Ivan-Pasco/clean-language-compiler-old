//! Native WASM type conversion operations
//!
//! Provides pure WASM bytecode implementations of type conversions
//! like int_to_string, bool_to_string, string_to_int.

use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

use super::STRING_DATA_OFFSET;

/// Generate instructions for bool_to_string
/// Returns a pointer to pre-allocated "true" or "false" string
///
/// Parameters:
///   - local 0: bool_value (i32, 0 or non-zero)
///
/// Returns: i32 (pointer to string "true" or "false")
///
/// Note: Requires true_ptr and false_ptr to be constants pointing to
/// pre-allocated strings in the data section
pub fn gen_bool_to_string(true_ptr: u32, false_ptr: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0), // bool value
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(true_ptr as i32),
        Instruction::Else,
        Instruction::I32Const(false_ptr as i32),
        Instruction::End,
    ]
}

/// Generate instructions for int_to_string
/// Converts an integer to its decimal string representation
///
/// Parameters:
///   - local 0: value (i32)
///
/// Returns: i32 (pointer to new string)
///
/// Uses locals:
///   - local 1: is_negative
///   - local 2: abs_value
///   - local 3: digit_count
///   - local 4: temp (for digit extraction)
///   - local 5: new_ptr
///   - local 6: write_pos
///   - local 7: digit
///
/// Algorithm:
///   1. Handle negative: if value < 0, set flag and negate
///   2. Count digits
///   3. Allocate string
///   4. Write digits from end to start
///   5. Add minus sign if negative
pub fn gen_int_to_string(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Check if value is negative
        Instruction::LocalGet(0), // value
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::LocalSet(1), // is_negative
        // Get absolute value: if is_negative then negate, else keep original
        Instruction::LocalGet(1), // is_negative (condition for if)
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0),
        Instruction::LocalGet(0),
        Instruction::I32Sub, // negate
        Instruction::Else,
        Instruction::LocalGet(0),
        Instruction::End,
        Instruction::LocalSet(2), // abs_value
        // Handle special case: value = 0
        Instruction::LocalGet(2),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        // Allocate string for "0" (length 1)
        Instruction::I32Const(5), // 4 bytes length + 1 byte data
        Instruction::Call(malloc_func),
        Instruction::LocalTee(5), // new_ptr
        // Store length = 1
        Instruction::I32Const(1),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Store '0'
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::I32Const(48), // '0'
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Return pointer
        Instruction::LocalGet(5),
        Instruction::Else, // value != 0
        // Count digits
        Instruction::I32Const(0),
        Instruction::LocalSet(3), // digit_count = 0
        Instruction::LocalGet(2),
        Instruction::LocalSet(4), // temp = abs_value
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if temp == 0, exit
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // digit_count++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        // temp = temp / 10
        Instruction::LocalGet(4),
        Instruction::I32Const(10),
        Instruction::I32DivU,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Add 1 for minus sign if negative
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(1),
        Instruction::Else,
        Instruction::I32Const(0),
        Instruction::End,
        Instruction::I32Add,
        Instruction::LocalSet(3), // total_len = digit_count + (is_negative ? 1 : 0)
        // Allocate string: malloc(4 + total_len)
        Instruction::LocalGet(3),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(5), // new_ptr
        // Store length
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Write digits from end to start
        // write_pos = total_len - 1
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(6),
        Instruction::LocalGet(2),
        Instruction::LocalSet(4), // temp = abs_value
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if temp == 0, exit
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::BrIf(1),
        // digit = temp % 10
        Instruction::LocalGet(4),
        Instruction::I32Const(10),
        Instruction::I32RemU,
        Instruction::LocalSet(7),
        // Store digit char at write_pos
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(6), // write_pos
        Instruction::I32Add,
        Instruction::LocalGet(7),  // digit
        Instruction::I32Const(48), // '0'
        Instruction::I32Add,
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // write_pos--
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(6),
        // temp = temp / 10
        Instruction::LocalGet(4),
        Instruction::I32Const(10),
        Instruction::I32DivU,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Add minus sign if negative
        Instruction::LocalGet(1),
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::I32Const(45), // '-'
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::End,
        // Return pointer
        Instruction::LocalGet(5),
        Instruction::End, // End if (value == 0)
    ]
}

/// Generate instructions for string_to_int
/// Parses a decimal string to integer
///
/// Parameters:
///   - local 0: str_ptr
///
/// Returns: i32 (parsed integer)
///
/// Uses locals:
///   - local 1: len
///   - local 2: is_negative
///   - local 3: i (loop counter)
///   - local 4: result
///   - local 5: char
pub fn gen_string_to_int() -> Vec<Instruction<'static>> {
    vec![
        // Get length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        // Initialize
        Instruction::I32Const(0),
        Instruction::LocalSet(2), // is_negative = false
        Instruction::I32Const(0),
        Instruction::LocalSet(3), // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(4), // result = 0
        // Check for empty string
        Instruction::LocalGet(1),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0),
        Instruction::Else,
        // Check for minus sign at position 0
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Const(45), // '-'
        Instruction::I32Eq,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1),
        Instruction::LocalSet(2), // is_negative = true
        Instruction::I32Const(1),
        Instruction::LocalSet(3), // i = 1 (skip minus)
        Instruction::End,
        // Parse digits
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= len, exit
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // Load char
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
        Instruction::LocalSet(5), // char
        // Check if char is digit (48-57 = '0'-'9')
        Instruction::LocalGet(5),
        Instruction::I32Const(48),
        Instruction::I32GeU,
        Instruction::LocalGet(5),
        Instruction::I32Const(57),
        Instruction::I32LeU,
        Instruction::I32And,
        Instruction::If(BlockType::Empty),
        // result = result * 10 + (char - '0')
        Instruction::LocalGet(4),
        Instruction::I32Const(10),
        Instruction::I32Mul,
        Instruction::LocalGet(5),
        Instruction::I32Const(48),
        Instruction::I32Sub,
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::End,
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Apply negative sign if needed
        Instruction::LocalGet(2),
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0),
        Instruction::LocalGet(4),
        Instruction::I32Sub, // negate
        Instruction::Else,
        Instruction::LocalGet(4),
        Instruction::End,
        Instruction::End, // End empty string check
    ]
}

/// Get local variables needed for each type conversion function
pub fn get_locals(func_name: &str) -> Vec<(u32, ValType)> {
    match func_name {
        "int_to_string" => vec![
            (1, ValType::I32), // is_negative
            (1, ValType::I32), // abs_value
            (1, ValType::I32), // digit_count / total_len
            (1, ValType::I32), // temp
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // write_pos
            (1, ValType::I32), // digit
        ],
        "string_to_int" => vec![
            (1, ValType::I32), // len
            (1, ValType::I32), // is_negative
            (1, ValType::I32), // i
            (1, ValType::I32), // result
            (1, ValType::I32), // char
        ],
        _ => vec![],
    }
}
