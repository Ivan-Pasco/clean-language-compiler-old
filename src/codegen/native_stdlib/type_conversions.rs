//! Native WASM type conversion operations
//!
//! Provides pure WASM bytecode implementations of type conversions
//! like int_to_string, bool_to_string and the text-to-number conversions.

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

/// Generate instructions for int64_to_string
/// Converts a 64-bit integer to its decimal string representation.
///
/// Parameter `local 0` is i64; everything else mirrors `gen_int_to_string` but
/// using i64 arithmetic so values outside the i32 range round-trip correctly.
///
/// Local layout:
///   0: value (i64, param)
///   1: is_negative (i32)
///   2: abs_value (i64)
///   3: digit_count (i32)
///   4: temp (i64, used during digit extraction)
///   5: new_ptr (i32, malloc result)
///   6: write_pos (i32)
///   7: digit (i32, low byte after %10)
pub fn gen_int64_to_string(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // is_negative = value < 0
        Instruction::LocalGet(0),
        Instruction::I64Const(0),
        Instruction::I64LtS,
        Instruction::LocalSet(1),
        // abs_value = is_negative ? 0 - value : value
        Instruction::LocalGet(1),
        Instruction::If(BlockType::Result(ValType::I64)),
        Instruction::I64Const(0),
        Instruction::LocalGet(0),
        Instruction::I64Sub,
        Instruction::Else,
        Instruction::LocalGet(0),
        Instruction::End,
        Instruction::LocalSet(2),
        // value == 0 → return "0" early
        Instruction::LocalGet(2),
        Instruction::I64Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(5),
        Instruction::Call(malloc_func),
        Instruction::LocalTee(5),
        Instruction::I32Const(1),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::I32Const(48), // '0'
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalGet(5),
        Instruction::Else,
        // Count digits
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        Instruction::LocalGet(2),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::I64Eqz,
        Instruction::BrIf(1),
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::LocalGet(4),
        Instruction::I64Const(10),
        Instruction::I64DivU,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // total_len = digit_count + (is_negative ? 1 : 0)
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(1),
        Instruction::Else,
        Instruction::I32Const(0),
        Instruction::End,
        Instruction::I32Add,
        Instruction::LocalSet(3),
        // malloc(STRING_DATA_OFFSET + total_len)
        Instruction::LocalGet(3),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(5),
        // store length
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // write_pos = total_len - 1
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(6),
        // temp = abs_value
        Instruction::LocalGet(2),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::I64Eqz,
        Instruction::BrIf(1),
        // digit_i32 = (temp % 10) wrapped to i32
        Instruction::LocalGet(4),
        Instruction::I64Const(10),
        Instruction::I64RemU,
        Instruction::I32WrapI64,
        Instruction::LocalSet(7),
        // store digit char
        Instruction::LocalGet(5),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(6),
        Instruction::I32Add,
        Instruction::LocalGet(7),
        Instruction::I32Const(48),
        Instruction::I32Add,
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(6),
        Instruction::LocalGet(4),
        Instruction::I64Const(10),
        Instruction::I64DivU,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // minus sign
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
        Instruction::LocalGet(5),
        Instruction::End,
    ]
}

/// The errors a text-to-number conversion raises (`__conversion_error`).
///
/// `raise` is the index of `__conversion_error(text, suffix)`; the other
/// fields are length-prefixed suffix strings completing the message that
/// starts with the quoted text: `'abc' is not a valid integer`.
#[derive(Clone, Copy)]
pub struct ConversionErrors {
    pub raise: u32,
    pub invalid: u32,
    pub too_large: u32,
    pub too_small: u32,
}

/// `__conversion_error(text, suffix)`: raise the error `'<text><suffix>`
/// (`__err_raise`), which an `onError` region catches.
///
/// Parameters: local 0 the text, local 1 the message suffix (it starts with
/// the closing quote).
pub fn gen_conversion_error(
    concat_idx: u32,
    err_raise_idx: u32,
    quote_ptr: u32,
) -> Vec<Instruction<'static>> {
    vec![
        Instruction::I32Const(quote_ptr as i32),
        Instruction::LocalGet(0),
        Instruction::Call(concat_idx),
        Instruction::LocalGet(1),
        Instruction::Call(concat_idx),
        Instruction::Call(err_raise_idx),
    ]
}

/// Load the byte at `text[local i]` (local 0 is the text).
fn byte_at(i: u32) -> [Instruction<'static>; 6] {
    [
        Instruction::LocalGet(0),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(i),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
    ]
}

/// Raise the error whose suffix `suffix` pushes, for the text in local 0,
/// and return `zero`.
fn raise_and_return(
    raise: u32,
    suffix: Vec<Instruction<'static>>,
    zero: Instruction<'static>,
) -> Vec<Instruction<'static>> {
    let mut v = vec![Instruction::LocalGet(0)];
    v.extend(suffix);
    v.extend([Instruction::Call(raise), zero, Instruction::Return]);
    v
}

/// `__string_to_int(text) -> i32` (`wide == false`) and
/// `__string_to_int64(text) -> i64` (`wide == true`): `.toInteger()` of a
/// string (stdlib-reference.md §3).
///
/// The text is an optional `+` or `-` followed by one or more decimal
/// digits, nothing else. Other text raises `'<text>' is not a valid
/// integer`; a value outside the target's range raises `... is too large
/// for integer` (`too small` when negative). The function then returns 0.
///
/// Locals: 0 text (parameter), 1 len, 2 negative, 3 i, 4 magnitude (i64),
/// 5 byte, then its digit value, 6 limit (i64, the largest magnitude
/// allowed), 7 digit (i64).
pub fn gen_string_to_integer(wide: bool, errors: ConversionErrors) -> Vec<Instruction<'static>> {
    let zero = if wide {
        Instruction::I64Const(0)
    } else {
        Instruction::I32Const(0)
    };
    // The negative limit of integer:64 is 2^63, i64::MIN read unsigned.
    let (max, negative_limit) = if wide {
        (i64::MAX, i64::MIN)
    } else {
        (i32::MAX as i64, -(i32::MIN as i64))
    };
    let invalid = || vec![Instruction::I32Const(errors.invalid as i32)];
    let out_of_range = vec![
        Instruction::LocalGet(2),
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(errors.too_small as i32),
        Instruction::Else,
        Instruction::I32Const(errors.too_large as i32),
        Instruction::End,
    ];

    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        Instruction::I32Const(0),
        Instruction::LocalSet(2),
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        Instruction::I64Const(0),
        Instruction::LocalSet(4),
        // A leading sign.
        Instruction::LocalGet(1),
        Instruction::If(BlockType::Empty),
    ];
    v.extend(byte_at(3));
    v.extend([
        Instruction::LocalTee(5),
        Instruction::I32Const(b'-' as i32),
        Instruction::I32Eq,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1),
        Instruction::LocalSet(2),
        Instruction::I32Const(1),
        Instruction::LocalSet(3),
        Instruction::End,
        Instruction::LocalGet(5),
        Instruction::I32Const(b'+' as i32),
        Instruction::I32Eq,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(1),
        Instruction::LocalSet(3),
        Instruction::End,
        Instruction::End,
        // No digits at all.
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::If(BlockType::Empty),
    ]);
    v.extend(raise_and_return(errors.raise, invalid(), zero.clone()));
    v.extend([
        Instruction::End,
        Instruction::LocalGet(2),
        Instruction::If(BlockType::Result(ValType::I64)),
        Instruction::I64Const(negative_limit),
        Instruction::Else,
        Instruction::I64Const(max),
        Instruction::End,
        Instruction::LocalSet(6),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
    ]);
    v.extend(byte_at(3));
    v.extend([
        Instruction::I32Const(b'0' as i32),
        Instruction::I32Sub,
        Instruction::LocalTee(5),
        Instruction::I32Const(9),
        Instruction::I32GtU,
        Instruction::If(BlockType::Empty),
    ]);
    v.extend(raise_and_return(errors.raise, invalid(), zero.clone()));
    v.extend([
        Instruction::End,
        Instruction::LocalGet(5),
        Instruction::I64ExtendI32U,
        Instruction::LocalSet(7),
        // magnitude * 10 + digit must not pass the limit (unsigned).
        Instruction::LocalGet(4),
        Instruction::LocalGet(6),
        Instruction::LocalGet(7),
        Instruction::I64Sub,
        Instruction::I64Const(10),
        Instruction::I64DivU,
        Instruction::I64GtU,
        Instruction::If(BlockType::Empty),
    ]);
    v.extend(raise_and_return(errors.raise, out_of_range, zero));
    v.extend([
        Instruction::End,
        Instruction::LocalGet(4),
        Instruction::I64Const(10),
        Instruction::I64Mul,
        Instruction::LocalGet(7),
        Instruction::I64Add,
        Instruction::LocalSet(4),
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(2),
        Instruction::If(BlockType::Result(ValType::I64)),
        Instruction::I64Const(0),
        Instruction::LocalGet(4),
        Instruction::I64Sub,
        Instruction::Else,
        Instruction::LocalGet(4),
        Instruction::End,
    ]);
    if !wide {
        v.push(Instruction::I32WrapI64);
    }
    v
}

/// Count the decimal digits starting at `text[local 2]`: advance local 2
/// past them and add their number to `count_local` (locals as in
/// [`gen_string_to_number`]).
fn skip_digits(count_local: u32) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
    ];
    v.extend(byte_at(2));
    v.extend([
        Instruction::I32Const(b'0' as i32),
        Instruction::I32Sub,
        Instruction::I32Const(9),
        Instruction::I32GtU,
        Instruction::BrIf(1),
        Instruction::LocalGet(count_local),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(count_local),
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(2),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
    ]);
    v
}

/// When `text[local 2]` exists and is one of `chars`, advance local 2.
/// Leaves 1 on the stack when it advanced, 0 otherwise.
fn skip_one_of(chars: &[u8]) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32LtU,
        Instruction::If(BlockType::Result(ValType::I32)),
    ];
    v.extend(byte_at(2));
    v.push(Instruction::LocalSet(3));
    for (n, c) in chars.iter().enumerate() {
        v.extend([
            Instruction::LocalGet(3),
            Instruction::I32Const(*c as i32),
            Instruction::I32Eq,
        ]);
        if n > 0 {
            v.push(Instruction::I32Or);
        }
    }
    v.extend([
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(2),
        Instruction::I32Const(1),
        Instruction::Else,
        Instruction::I32Const(0),
        Instruction::End,
        Instruction::Else,
        Instruction::I32Const(0),
        Instruction::End,
    ]);
    v
}

/// `__string_to_number(text) -> f64`: `.toNumber()` of a string
/// (stdlib-reference.md §3).
///
/// The text is a decimal number: an optional sign, digits with an optional
/// fraction (`12`, `1.5`, `.5`, `5.`; at least one digit), and an optional
/// exponent (`e` or `E`, an optional sign, one or more digits). Other text
/// raises `'<text>' is not a valid number`. Valid text is converted by the
/// host's `string_to_float`; a result beyond the `number` range raises
/// `... is too large for number` (`too small` when negative). The function
/// then returns 0.
///
/// Locals: 0 text (parameter), 1 len, 2 i, 3 byte, 4 mantissa digits,
/// 5 exponent digits, 6 result (f64).
pub fn gen_string_to_number(
    string_to_float_idx: u32,
    errors: ConversionErrors,
) -> Vec<Instruction<'static>> {
    let invalid = || {
        raise_and_return(
            errors.raise,
            vec![Instruction::I32Const(errors.invalid as i32)],
            Instruction::F64Const(0.0),
        )
    };
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        Instruction::I32Const(0),
        Instruction::LocalSet(2),
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        Instruction::I32Const(0),
        Instruction::LocalSet(5),
    ];
    // Sign, integer digits, fraction.
    v.extend(skip_one_of(b"+-"));
    v.push(Instruction::Drop);
    v.extend(skip_digits(4));
    v.extend(skip_one_of(b"."));
    v.push(Instruction::If(BlockType::Empty));
    v.extend(skip_digits(4));
    v.push(Instruction::End);
    v.extend([
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
    ]);
    v.extend(invalid());
    v.push(Instruction::End);
    // Exponent.
    v.extend(skip_one_of(b"eE"));
    v.push(Instruction::If(BlockType::Empty));
    v.extend(skip_one_of(b"+-"));
    v.push(Instruction::Drop);
    v.extend(skip_digits(5));
    v.extend([
        Instruction::LocalGet(5),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
    ]);
    v.extend(invalid());
    v.extend([Instruction::End, Instruction::End]);
    // Anything after the number.
    v.extend([
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
    ]);
    v.extend(invalid());
    v.extend([
        Instruction::End,
        Instruction::LocalGet(0),
        Instruction::Call(string_to_float_idx),
        Instruction::LocalTee(6),
        Instruction::F64Abs,
        Instruction::F64Const(f64::INFINITY),
        Instruction::F64Eq,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(0),
        Instruction::LocalGet(6),
        Instruction::F64Const(0.0),
        Instruction::F64Lt,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(errors.too_small as i32),
        Instruction::Else,
        Instruction::I32Const(errors.too_large as i32),
        Instruction::End,
        Instruction::Call(errors.raise),
        Instruction::F64Const(0.0),
        Instruction::Return,
        Instruction::End,
        Instruction::LocalGet(6),
    ]);
    v
}
