//! List runtime functions that are not method implementations: allocation,
//! raw push/pop helpers used by list literals and the `list_*` builtins, and
//! the late registration hook for `ListClass` / `StringClass` functions that
//! need the native allocator.

use crate::codegen::native_stdlib::HEAP_PTR_GLOBAL;
use crate::codegen::CodeGenerator;
use crate::codegen::LIST_TYPE_ID;
use crate::error::CompilerError;
use crate::stdlib::list_class::ListClass;
use crate::stdlib::register_stdlib_function;
use crate::stdlib::string_class::StringClass;
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

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
        tracing::debug!("DEBUG LIST_OPS: Starting list function registration");

        // NOTE: Register list.push as import functions with proper type signatures
        // list.push for i32 elements (integers, booleans, pointers)
        tracing::debug!("DEBUG LIST_OPS: Registering list.push...");
        let idx1 = codegen.register_import_function(
            "env",
            "list.push",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        tracing::debug!("DEBUG LIST_OPS: list.push registered at index {}", idx1);

        // NOTE: list.push_f64 for f64 elements (floats, numbers)
        // This is needed for float array literals like [1.1, 2.2, 3.3]
        // It is reachability-gated (Import Minimality Rule): only emitted when
        // the program contains float array literals. The u32::MAX sentinel means
        // the import was tree-shaken; skip the alias so nothing maps to a bogus index.
        tracing::debug!("DEBUG LIST_OPS: Registering list.push_f64...");
        let idx2 = codegen.register_import_function(
            "env",
            "list.push_f64",
            &[WasmType::I32, WasmType::F64],
            Some(WasmType::I32),
        )?;
        tracing::debug!("DEBUG LIST_OPS: list.push_f64 registered at index {}", idx2);

        // Add alias list_push_f64 -> list.push_f64 for MIR codegen compatibility.
        // Skip when the import was tree-shaken (sentinel u32::MAX) so we do not
        // insert a bogus index that would generate invalid Call instructions.
        if idx2 != u32::MAX {
            codegen.add_function_alias("list_push_f64", idx2);
            tracing::debug!(
                "DEBUG LIST_OPS: list_push_f64 alias added at index {}",
                idx2
            );
        } else {
            tracing::debug!(
                "DEBUG LIST_OPS: list.push_f64 tree-shaken (not reachable), skipping alias"
            );
        }

        // Register list allocation function
        register_stdlib_function(
            codegen,
            "list.allocate",
            &[WasmType::I32],    // Size
            Some(WasmType::I32), // Pointer
            self.generate_list_allocate(),
        )?;

        // Register list allocation function
        register_stdlib_function(
            codegen,
            "list.allocate",
            &[WasmType::I32],    // Size
            Some(WasmType::I32), // Pointer
            self.generate_list_allocate(),
        )?;

        let length = self.generate_list_length();
        register_stdlib_function(
            codegen,
            "list.length",
            &[WasmType::I32],
            Some(WasmType::I32),
            length.clone(),
        )?;
        register_stdlib_function(
            codegen,
            "list_length",
            &[WasmType::I32],
            Some(WasmType::I32),
            length,
        )?;

        // In-place append without a capacity check, used by list literals
        // whose capacity is sized up front.
        register_stdlib_function(
            codegen,
            "list_push",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_push(),
        )?;

        register_stdlib_function(
            codegen,
            "list_pop",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_pop(),
        )?;

        // `list_contains` / `list_index_of` builtins share the native bodies.
        for (alias, target) in [
            ("list_contains", "list.contains"),
            ("list_index_of", "list.indexOf"),
        ] {
            if let Some(idx) = codegen.get_function_index(target) {
                codegen.add_function_alias(alias, idx);
            }
        }

        // Method implementations that allocate (sort, slice, join, charAt, ...).
        ListClass::new().register_functions(codegen)?;
        StringClass::new().register_heap_operations(codegen)?;

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

    fn generate_list_push(&self) -> Vec<Instruction> {
        // Implementation of list push - adds element to end of list
        // Parameters: list_ptr (0), item (1)
        // Returns: list pointer (modified in place)
        vec![
            // Get current size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            // Calculate position for new element (size * 4 + 16)
            Instruction::I32Const(4), // element size (4 bytes for i32)
            Instruction::I32Mul,
            Instruction::I32Const(16), // header size
            Instruction::I32Add,
            // Add base address
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Add,      // storage address
            // Store the new item
            Instruction::LocalGet(1), // item
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Increment list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // current size
            Instruction::I32Const(1),
            Instruction::I32Add, // new size
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return list pointer
            Instruction::LocalGet(0),
        ]
    }

    fn generate_list_pop(&self) -> Vec<Instruction> {
        // Implementation of list pop - removes and returns last element
        // Parameters: list_ptr (0)
        // Returns: popped element (0 if empty)
        vec![
            // Get current size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            // Check if empty
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Return 0 if empty
            Instruction::Else,
            // Get last element before decrementing size
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub,      // size - 1
            Instruction::I32Const(4), // element size (4 bytes for i32)
            Instruction::I32Mul,
            Instruction::I32Const(16), // header size
            Instruction::I32Add,
            Instruction::I32Add, // calculate address
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load element value to stack
            // Decrement size (but keep element value on stack)
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub, // new size
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Element value is still on stack as return value
            Instruction::End,
        ]
    }
}
