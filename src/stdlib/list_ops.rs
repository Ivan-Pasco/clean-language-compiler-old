//! List runtime functions that are not method implementations (literal
//! allocation, length, the `list_*` builtin aliases) and the late
//! registration hook for `ListClass` / `StringClass` functions that need the
//! native allocator.

use crate::codegen::native_stdlib::HEAP_PTR_GLOBAL;
use crate::codegen::CodeGenerator;
use crate::codegen::LIST_TYPE_ID;
use crate::error::CompilerError;
use crate::stdlib::list_class::{missing, ListClass};
use crate::stdlib::register_stdlib_function;
use crate::stdlib::string_class::StringClass;
use crate::types::WasmType;
use wasm_encoder::{Instruction, MemArg};

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
        register_stdlib_function(
            codegen,
            "list.allocate",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_allocate(),
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

    pub fn generate_list_allocate(&self) -> Vec<Instruction> {
        // Bump allocator that shares heap state with `mem_alloc`
        // (src/codegen/native_stdlib/memory.rs::gen_malloc) via the
        // `__heap_ptr` global (HEAP_PTR_GLOBAL = 0).
        //
        // Historical bug (LIST-ALLOCATE-HEAP-POINTER-COLLISION, fp ab19b3e75d98):
        // this function used to read/write the heap pointer from `mem[0]`,
        // independent of the global. Because the first allocated list's count
        // field is *also* at offset 0 of the returned pointer, the count-init
        // store and the heap-pointer advance both wrote to `mem[0]` when the
        // heap pointer happened to be 0 (the initial value of an unwritten
        // mem byte). Every list therefore physically overlapped at address 0.
        // Pairs/strings escaped this because `mem_alloc` correctly used the
        // global; the two allocators were not sharing state.
        //
        // Memory layout per list: [size:i32 @0][cap:i32 @4][type_id:i32 @8][pad @12][elements @16].
        // The allocator does NOT zero the elements region — list.push / list.add
        // write into it as needed.

        vec![
            // local 0: capacity (parameter)
            // local 1: allocated_ptr (= old heap_ptr)
            // local 2: alloc_size
            // local 3: new_heap_ptr

            // local 1 = GlobalGet(__heap_ptr)
            Instruction::GlobalGet(HEAP_PTR_GLOBAL),
            Instruction::LocalSet(1),
            // alloc_size = 16 (header) + capacity * 4 (element slots)
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Const(16),
            Instruction::I32Add,
            Instruction::LocalSet(2),
            // new_heap_ptr = allocated_ptr + alloc_size
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            // GlobalSet(__heap_ptr) = new_heap_ptr
            Instruction::LocalGet(3),
            Instruction::GlobalSet(HEAP_PTR_GLOBAL),
            // Initialize list header at allocated_ptr:
            //   mem[allocated_ptr + 0] = 0       (size)
            //   mem[allocated_ptr + 4] = capacity
            //   mem[allocated_ptr + 8] = LIST_TYPE_ID
            //   (offset 12 padding is left uninitialized)
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
            Instruction::LocalGet(0),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
            Instruction::I32Const(LIST_TYPE_ID as i32),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return allocated_ptr
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
