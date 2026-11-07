//! List runtime functions that are not method implementations (literal
//! allocation, length, the `list_*` builtin aliases) and the late
//! registration hook for `ListClass` / `StringClass` functions that need the
//! native allocator.

use crate::codegen::CodeGenerator;
use crate::codegen::LIST_TYPE_ID;
use crate::error::CompilerError;
use crate::stdlib::list_class::{missing, ListClass};
use crate::stdlib::register_stdlib_function;
use crate::stdlib::string_class::StringClass;
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg};

/// 4-byte aligned access to a list header word at `offset`.
fn header(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

pub struct ListManager;

impl Default for ListManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ListManager {
    pub fn new() -> Self {
        Self
    }

    /// Registered after the native memory operations, so `__malloc` exists.
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // List literals: `list.allocate(capacity)` then in-place `list.push`.
        let malloc = codegen
            .get_function_index("__malloc")
            .ok_or_else(|| missing("__malloc"))?;
        register_stdlib_function(
            codegen,
            "list.allocate",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_allocate(malloc),
        )?;

        let length = register_stdlib_function(
            codegen,
            "list.length",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_length(),
        )?;
        codegen.add_function_alias("list_length", length);

        // Snake-case builtin `list_push` is the in-place literal append.
        let push = codegen
            .get_function_index("__list_push_i32")
            .ok_or_else(|| missing("__list_push_i32"))?;
        codegen.add_function_alias("list_push", push);

        // Method implementations that allocate (sort, slice, join, charAt, ...).
        ListClass::new().register_functions(codegen)?;
        StringClass::new().register_heap_operations(codegen)?;

        // Snake-case builtin `list_pop` is `.pop()`: halts on an empty list.
        let pop = codegen
            .get_function_index("list.removeLast")
            .ok_or_else(|| missing("list.removeLast"))?;
        codegen.add_function_alias("list_pop", pop);

        Ok(())
    }

    pub fn generate_list_allocate(&self, malloc: u32) -> Vec<Instruction<'static>> {
        // Allocate through `__malloc` (src/codegen/native_stdlib/memory.rs
        // `gen_malloc`): it aligns the block, grows linear memory when the
        // heap reaches the end of it, and shares the `__heap_ptr` global with
        // every other allocation. Bumping `__heap_ptr` here directly wrote
        // past the end of memory once enough lists were allocated, and could
        // hand out misaligned blocks.
        //
        // Memory layout per list (see native_stdlib LIST_* constants):
        // [size:i32 @0][cap:i32 @4][type_id:i32 @8][flags:i32 @12][elements @16].
        // Every header word is written: `__malloc` does not zero memory, and
        // a stale flags word would give a plain list behaviors it never had.
        // The elements region is not zeroed; list.push / list.add write it.
        const LIST_HEADER_BYTES: i32 = 16;
        vec![
            // local 0: capacity (parameter)
            // local 1: list pointer
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Const(LIST_HEADER_BYTES),
            Instruction::I32Add,
            Instruction::Call(malloc),
            Instruction::LocalSet(1),
            // `__malloc` returns 0 when memory cannot grow: trap instead of
            // writing the header over address 0.
            Instruction::LocalGet(1),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::Unreachable,
            Instruction::End,
            // size = 0
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(header(0)),
            // capacity
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Store(header(4)),
            // type id
            Instruction::LocalGet(1),
            Instruction::I32Const(LIST_TYPE_ID as i32),
            Instruction::I32Store(header(8)),
            // behavior flags: none
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(header(12)),
            // Return the list pointer
            Instruction::LocalGet(1),
        ]
    }

    pub fn generate_list_length(&self) -> Vec<Instruction> {
        vec![
            // Get list pointer
            Instruction::LocalGet(0),
            // Load length from header
            Instruction::I32Load(MemArg {
                offset: 0, // Length is at offset 0
                align: 2,
                memory_index: 0,
            }),
        ]
    }
}
