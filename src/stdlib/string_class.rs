//! String methods implemented as native WASM functions.
//!
//! Strings are length-prefixed: `[length: i32][bytes...]`, content at +4.
//!
//! Registration happens in two phases because the active pipeline registers
//! `StringClass::register_functions` before the native allocator exists:
//! - [`StringClass::register_functions`] — functions that never allocate.
//! - [`StringClass::register_heap_operations`] — functions that call
//!   `__malloc`, invoked from `ListManager::register_functions` after the
//!   memory operations are registered.
//!
//! Other string methods (substring, indexOf, trim, case conversion, split,
//! replace, concat, …) are registered in `codegen_registration.rs`.
//! `string.join` is an alias of `list.join` (see `list_class.rs`).

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg};

/// String class implementation for Clean Language
pub struct StringClass;

impl Default for StringClass {
    fn default() -> Self {
        Self::new()
    }
}

impl StringClass {
    pub fn new() -> Self {
        Self
    }

    /// Register the string functions that never allocate.
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        let length = vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ];
        register_stdlib_function(
            codegen,
            "string.length",
            &[WasmType::I32],
            Some(WasmType::I32),
            length.clone(),
        )?;
        register_stdlib_function(
            codegen,
            "string.size",
            &[WasmType::I32],
            Some(WasmType::I32),
            length,
        )?;

        // string.charCodeAt(text, index) -> integer
        register_stdlib_function(
            codegen,
            "string.charCodeAt",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            Self::gen_char_code_at(),
        )?;

        // string.isEmpty(text) -> boolean
        register_stdlib_function(
            codegen,
            "string.isEmpty",
            &[WasmType::I32],
            Some(WasmType::I32),
            vec![
                Instruction::LocalGet(0),
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }),
                Instruction::I32Eqz,
            ],
        )?;

        // string.isBlank(text) -> boolean
        register_stdlib_function(
            codegen,
            "string.isBlank",
            &[WasmType::I32],
            Some(WasmType::I32),
            Self::gen_is_blank(),
        )?;

        Ok(())
    }

    /// Register the string functions that allocate their result with
    /// `__malloc`. Must run after the native memory operations.
    pub fn register_heap_operations(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        let malloc = codegen.get_function_index("__malloc").ok_or_else(|| {
            CompilerError::codegen_error(
                "string stdlib registration requires `__malloc` to be registered first".to_string(),
                None,
                None,
            )
        })?;

        // string.charAt(text, index) -> string
        register_stdlib_function(
            codegen,
            "string.charAt",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            Self::gen_char_at(malloc),
        )?;

        // string.padStart(text, width, pad) -> string
        register_stdlib_function(
            codegen,
            "string.padStart",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            Self::gen_pad(malloc, true),
        )?;

        // string.padEnd(text, width, pad) -> string
        register_stdlib_function(
            codegen,
            "string.padEnd",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            Self::gen_pad(malloc, false),
        )?;

        Ok(())
    }

    /// Halt unless `0 <= index < length` (stdlib-reference.md §4:
    /// `.charAt` and `.charCodeAt` halt on an out-of-bounds index).
    /// params (text, index).
    fn emit_halt_if_out_of_bounds() -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32LtS,
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Load(word(0)),
            Instruction::I32GeS,
            Instruction::I32Or,
            Instruction::If(BlockType::Empty),
            Instruction::Unreachable,
            Instruction::End,
        ]
    }

    /// `string.charAt(text, index)`: a new one-byte string holding the byte at
    /// `index`. params (text, index); local 2 = new string pointer.
    fn gen_char_at(malloc_func: u32) -> Vec<Instruction<'static>> {
        let mut v = Self::emit_halt_if_out_of_bounds();
        v.extend([
            Instruction::I32Const(5),
            Instruction::Call(malloc_func),
            Instruction::LocalSet(2),
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Store(word(0)),
            Instruction::LocalGet(2),
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(byte(DATA_OFFSET)),
            Instruction::I32Store8(byte(DATA_OFFSET)),
            Instruction::LocalGet(2),
        ]);
        v
    }

    /// `string.charCodeAt(text, index)`: the byte value at `index`.
    /// params (text, index).
    fn gen_char_code_at() -> Vec<Instruction<'static>> {
        let mut v = Self::emit_halt_if_out_of_bounds();
        v.extend([
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(byte(DATA_OFFSET)),
        ]);
        v
    }

    /// `string.isBlank(text)`: true when every byte is ASCII whitespace
    /// (space, tab, LF, VT, FF, CR); the empty string is blank.
    /// params (text); locals 1 = i, 2 = byte.
    fn gen_is_blank() -> Vec<Instruction<'static>> {
        vec![
            Instruction::I32Const(0),
            Instruction::LocalSet(1),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalTee(2),
            Instruction::I32Const(32),
            Instruction::I32Eq,
            // 9..=13 is \t \n \v \f \r
            Instruction::LocalGet(2),
            Instruction::I32Const(9),
            Instruction::I32Sub,
            Instruction::I32Const(4),
            Instruction::I32LeU,
            Instruction::I32Or,
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0),
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::I32Const(1),
        ]
    }
    /// `string.padStart(text, width, pad)` / `string.padEnd(text, width, pad)`:
    /// `text` extended to `width` bytes by repeating `pad` before (`at_start`)
    /// or after it. Returns `text` itself when it is already `width` long or
    /// `pad` is empty.
    ///
    /// params (text, width, pad); locals 3 = text_len, 4 = pad_len,
    /// 5 = pad_needed, 6 = result, 7 = i, 8 = pad source pointer, 9 = text
    /// destination pointer.
    fn gen_pad(malloc_func: u32, at_start: bool) -> Vec<Instruction<'static>> {
        let mut v = vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(word(0)),
            Instruction::LocalSet(3),
            Instruction::LocalGet(2),
            Instruction::I32Load(word(0)),
            Instruction::LocalSet(4),
            Instruction::LocalGet(3),
            Instruction::LocalGet(1),
            Instruction::I32GeS,
            Instruction::LocalGet(4),
            Instruction::I32Eqz,
            Instruction::I32Or,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(0),
            Instruction::Return,
            Instruction::End,
            // pad_needed = width - text_len; result = malloc(4 + width)
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Sub,
            Instruction::LocalSet(5),
            Instruction::LocalGet(1),
            Instruction::I32Const(DATA_OFFSET as i32),
            Instruction::I32Add,
            Instruction::Call(malloc_func),
            Instruction::LocalTee(6),
            // Allocation failure: keep the text unpadded.
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(0),
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(6),
            Instruction::LocalGet(1),
            Instruction::I32Store(word(0)),
        ];
        // The padding goes at result[0..pad_needed] (start) or
        // result[text_len..width] (end); the text fills the rest.
        let (pad_base, text_base) = if at_start {
            (None, Some(5))
        } else {
            (Some(3), None)
        };
        // pad source: result + pad_base; text destination: result + text_base
        v.push(Instruction::LocalGet(6));
        if let Some(offset) = pad_base {
            v.extend([Instruction::LocalGet(offset), Instruction::I32Add]);
        }
        v.push(Instruction::LocalSet(8));
        v.push(Instruction::LocalGet(6));
        if let Some(offset) = text_base {
            v.extend([Instruction::LocalGet(offset), Instruction::I32Add]);
        }
        v.push(Instruction::LocalSet(9));
        // Padding: dst[i] = pad[i % pad_len] for i in 0..pad_needed.
        v.extend(emit_byte_loop(5, |v| {
            v.extend([
                Instruction::LocalGet(8),
                Instruction::LocalGet(7),
                Instruction::I32Add,
                Instruction::LocalGet(2),
                Instruction::LocalGet(7),
                Instruction::LocalGet(4),
                Instruction::I32RemU,
                Instruction::I32Add,
                Instruction::I32Load8U(byte(DATA_OFFSET)),
                Instruction::I32Store8(byte(DATA_OFFSET)),
            ]);
        }));
        // Text: dst[i] = text[i] for i in 0..text_len.
        v.extend(emit_byte_loop(3, |v| {
            v.extend([
                Instruction::LocalGet(9),
                Instruction::LocalGet(7),
                Instruction::I32Add,
                Instruction::LocalGet(0),
                Instruction::LocalGet(7),
                Instruction::I32Add,
                Instruction::I32Load8U(byte(DATA_OFFSET)),
                Instruction::I32Store8(byte(DATA_OFFSET)),
            ]);
        }));
        v.push(Instruction::LocalGet(6));
        v
    }
}

/// Byte offset of a string's content.
const DATA_OFFSET: u64 = 4;

fn word(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

fn byte(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }
}

/// `for i (local 7) in 0..count_local { body }`.
fn emit_byte_loop(
    count_local: u32,
    body: impl FnOnce(&mut Vec<Instruction<'static>>),
) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::I32Const(0),
        Instruction::LocalSet(7),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(7),
        Instruction::LocalGet(count_local),
        Instruction::I32GeU,
        Instruction::BrIf(1),
    ];
    body(&mut v);
    v.extend([
        Instruction::LocalGet(7),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(7),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
    ]);
    v
}
