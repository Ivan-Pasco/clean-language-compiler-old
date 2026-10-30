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
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

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
            self.generate_char_code_at(),
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
            Self::gen_pad_start(malloc),
        )?;

        // string.padEnd(text, width, pad) -> string
        register_stdlib_function(
            codegen,
            "string.padEnd",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            Self::gen_pad_end(malloc),
        )?;

        Ok(())
    }

    /// `string.charAt(text, index)`: a new one-byte string holding the byte at
    /// `index`. Halts when `index` is out of bounds (stdlib-reference.md §4).
    /// params (text, index); local 2 = new string pointer.
    fn gen_char_at(malloc_func: u32) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32LtS,
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32GeS,
            Instruction::I32Or,
            Instruction::If(BlockType::Empty),
            Instruction::Unreachable,
            Instruction::End,
            Instruction::I32Const(5),
            Instruction::Call(malloc_func),
            Instruction::LocalSet(2),
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(2),
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Store8(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::LocalGet(2),
        ]
    }

    fn generate_char_code_at(&self) -> Vec<Instruction<'static>> {
        // Full string.charCodeAt implementation with proper control flow
        // According to spec: Returns character code (integer) at specified index
        // Parameters: text string, index
        // Returns: character code as integer (or 0 if out of bounds)
        vec![
            // Get text string and index
            Instruction::LocalGet(0), // text_ptr
            Instruction::LocalSet(2), // save text_ptr
            Instruction::LocalGet(1), // index
            Instruction::LocalSet(3), // save index
            // Get text length
            Instruction::LocalGet(2), // text_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // save text_length
            // Check if index is out of bounds (index < 0 OR index >= length)
            Instruction::LocalGet(3), // index
            Instruction::I32Const(0),
            Instruction::I32LtS,      // index < 0
            Instruction::LocalGet(3), // index
            Instruction::LocalGet(4), // text_length
            Instruction::I32GeU,      // index >= text_length
            Instruction::I32Or,       // out_of_bounds = (index < 0) OR (index >= length)
            Instruction::If(BlockType::Result(ValType::I32)),
            // Index is out of bounds, return 0
            Instruction::I32Const(0),
            Instruction::Else,
            // Index is valid, load and return character code
            Instruction::LocalGet(2), // text_ptr
            Instruction::I32Const(4), // offset past length field
            Instruction::I32Add,
            Instruction::LocalGet(3), // index
            Instruction::I32Add,      // character address = text_ptr + 4 + index
            Instruction::I32Load8U(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }), // load character as unsigned byte
            Instruction::End,
        ]
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
    /// Generate WASM instructions for string.padStart.
    ///
    /// String memory layout: [4-byte length][content bytes]
    ///
    /// Parameters (locals 0-2):
    ///   - local 0: str_ptr    — source string pointer
    ///   - local 1: width      — target output length
    ///   - local 2: pad_ptr    — pad string pointer
    ///
    /// Returns: i32 — pointer to result string, or original str_ptr when no padding needed.
    ///
    /// Additional locals (3-8):
    ///   - local 3: str_len    — source string length
    ///   - local 4: pad_len    — pad string length (guard against zero-length pad)
    ///   - local 5: pad_needed — bytes to prepend (width - str_len)
    ///   - local 6: new_ptr    — newly allocated result buffer pointer
    ///   - local 7: i          — loop counter
    ///   - local 8: pad_i      — cyclic index into pad string (i mod pad_len)
    fn gen_pad_start(malloc_func: u32) -> Vec<Instruction<'static>> {
        // Data bytes start 4 bytes after the string pointer (length prefix occupies first 4 bytes).
        const DATA_OFFSET: i32 = 4;

        vec![
            // Load str_len = mem[str_ptr + 0]
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Load pad_len = mem[pad_ptr + 0]
            Instruction::LocalGet(2),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Early-out: if str_len >= width OR pad_len == 0, return str_ptr unchanged.
            Instruction::LocalGet(3),
            Instruction::LocalGet(1),
            Instruction::I32GeS,
            Instruction::LocalGet(4),
            Instruction::I32Eqz,
            Instruction::I32Or,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::Else,
            // pad_needed = width - str_len
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Sub,
            Instruction::LocalSet(5),
            // Allocate new buffer: malloc(DATA_OFFSET + width)
            Instruction::LocalGet(1),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::Call(malloc_func),
            Instruction::LocalSet(6),
            // OOM guard: if malloc returned 0, return original str_ptr.
            Instruction::LocalGet(6),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::Else,
            // Store result length = width at new_ptr[0].
            Instruction::LocalGet(6),
            Instruction::LocalGet(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Phase 1: fill pad_needed bytes at new_ptr[DATA_OFFSET..DATA_OFFSET+pad_needed].
            // Each byte cycles through the pad string: pad_i = i mod pad_len.
            Instruction::I32Const(0),
            Instruction::LocalSet(7), // i = 0
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Exit when i >= pad_needed.
            Instruction::LocalGet(7),
            Instruction::LocalGet(5),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // Compute pad_i = i mod pad_len  =>  i - (i / pad_len) * pad_len
            Instruction::LocalGet(7),
            Instruction::LocalGet(7),
            Instruction::LocalGet(4),
            Instruction::I32DivU,
            Instruction::LocalGet(4),
            Instruction::I32Mul,
            Instruction::I32Sub,
            Instruction::LocalSet(8),
            // Destination address: new_ptr + DATA_OFFSET + i
            Instruction::LocalGet(6),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            // Source byte: pad_ptr[DATA_OFFSET + pad_i]
            Instruction::LocalGet(2),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(8),
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
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
            // Phase 2: copy original string bytes to new_ptr[DATA_OFFSET+pad_needed..].
            Instruction::I32Const(0),
            Instruction::LocalSet(7), // i = 0
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Exit when i >= str_len.
            Instruction::LocalGet(7),
            Instruction::LocalGet(3),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // Destination: new_ptr[DATA_OFFSET + pad_needed + i]
            Instruction::LocalGet(6),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(5),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            // Source: str_ptr[DATA_OFFSET + i]
            Instruction::LocalGet(0),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(7),
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
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
            // Return new_ptr.
            Instruction::LocalGet(6),
            Instruction::End, // end OOM if/else
            Instruction::End, // end early-out if/else
        ]
    }

    /// Generate WASM instructions for string.padEnd.
    ///
    /// String memory layout: [4-byte length][content bytes]
    ///
    /// Parameters (locals 0-2):
    ///   - local 0: str_ptr    — source string pointer
    ///   - local 1: width      — target output length
    ///   - local 2: pad_ptr    — pad string pointer
    ///
    /// Returns: i32 — pointer to result string, or original str_ptr when no padding needed.
    ///
    /// Additional locals (3-8):
    ///   - local 3: str_len    — source string length
    ///   - local 4: pad_len    — pad string length (guard against zero-length pad)
    ///   - local 5: pad_needed — bytes to append (width - str_len)
    ///   - local 6: new_ptr    — newly allocated result buffer pointer
    ///   - local 7: i          — loop counter
    ///   - local 8: pad_i      — cyclic index into pad string (i mod pad_len)
    fn gen_pad_end(malloc_func: u32) -> Vec<Instruction<'static>> {
        const DATA_OFFSET: i32 = 4;

        vec![
            // Load str_len
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3),
            // Load pad_len
            Instruction::LocalGet(2),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4),
            // Early-out: if str_len >= width OR pad_len == 0, return str_ptr unchanged.
            Instruction::LocalGet(3),
            Instruction::LocalGet(1),
            Instruction::I32GeS,
            Instruction::LocalGet(4),
            Instruction::I32Eqz,
            Instruction::I32Or,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::Else,
            // pad_needed = width - str_len
            Instruction::LocalGet(1),
            Instruction::LocalGet(3),
            Instruction::I32Sub,
            Instruction::LocalSet(5),
            // Allocate new buffer: malloc(DATA_OFFSET + width)
            Instruction::LocalGet(1),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::Call(malloc_func),
            Instruction::LocalSet(6),
            // OOM guard.
            Instruction::LocalGet(6),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::Else,
            // Store result length = width at new_ptr[0].
            Instruction::LocalGet(6),
            Instruction::LocalGet(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Phase 1: copy original string bytes to new_ptr[DATA_OFFSET..DATA_OFFSET+str_len].
            Instruction::I32Const(0),
            Instruction::LocalSet(7),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(7),
            Instruction::LocalGet(3),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // Destination: new_ptr[DATA_OFFSET + i]
            Instruction::LocalGet(6),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            // Source: str_ptr[DATA_OFFSET + i]
            Instruction::LocalGet(0),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(7),
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
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
            // Phase 2: fill pad_needed bytes at new_ptr[DATA_OFFSET+str_len..].
            Instruction::I32Const(0),
            Instruction::LocalSet(7),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(7),
            Instruction::LocalGet(5),
            Instruction::I32GeU,
            Instruction::BrIf(1),
            // pad_i = i mod pad_len
            Instruction::LocalGet(7),
            Instruction::LocalGet(7),
            Instruction::LocalGet(4),
            Instruction::I32DivU,
            Instruction::LocalGet(4),
            Instruction::I32Mul,
            Instruction::I32Sub,
            Instruction::LocalSet(8),
            // Destination: new_ptr[DATA_OFFSET + str_len + i]
            Instruction::LocalGet(6),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Add,
            Instruction::LocalGet(7),
            Instruction::I32Add,
            // Source: pad_ptr[DATA_OFFSET + pad_i]
            Instruction::LocalGet(2),
            Instruction::I32Const(DATA_OFFSET),
            Instruction::I32Add,
            Instruction::LocalGet(8),
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
            Instruction::LocalGet(7),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7),
            Instruction::Br(0),
            Instruction::End, // end loop
            Instruction::End, // end block
            // Return new_ptr.
            Instruction::LocalGet(6),
            Instruction::End, // end OOM if/else
            Instruction::End, // end early-out if/else
        ]
    }
}
