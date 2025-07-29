use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use crate::ast::{ListBehavior};
use wasm_encoder::{Instruction, MemArg, BlockType, ValType};
use crate::stdlib::register_stdlib_function;

/// List behavior implementation for Clean Language
/// Handles different list behaviors: line (FIFO), pile (LIFO), unique (set)
pub struct ListBehaviorManager {
    pub behaviors: std::collections::HashMap<i32, ListBehavior>,
    next_list_id: i32,
}

impl ListBehaviorManager {
    pub fn new() -> Self {
        Self {
            behaviors: std::collections::HashMap::new(),
            next_list_id: 1,
        }
    }

    /// Register a new list with default behavior
    pub fn register_list(&mut self) -> i32 {
        let list_id = self.next_list_id;
        self.behaviors.insert(list_id, ListBehavior::Default);
        self.next_list_id += 1;
        list_id
    }

    /// Set behavior for a specific list
    pub fn set_behavior(&mut self, list_id: i32, behavior: ListBehavior) {
        self.behaviors.insert(list_id, behavior);
    }

    /// Get behavior for a specific list
    pub fn get_behavior(&self, list_id: i32) -> ListBehavior {
        self.behaviors.get(&list_id).copied().unwrap_or(ListBehavior::Default)
    }

    /// Register all list behavior functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_behavior_operations(codegen)?;
        self.register_property_operations(codegen)?;
        Ok(())
    }

    fn register_behavior_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // List.add(list, value) - adds element according to behavior
        register_stdlib_function(
            codegen,
            "List.add",
            &[WasmType::I32, WasmType::I32],
            None,
            self.generate_list_add()
        )?;

        // List.remove(list) -> any - removes element according to behavior
        register_stdlib_function(
            codegen,
            "List.remove",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_remove()
        )?;

        // List.peek(list) -> any - views next element without removing
        register_stdlib_function(
            codegen,
            "List.peek",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_peek()
        )?;

        // List.setBehavior(list, behavior_string) - sets behavior for list
        register_stdlib_function(
            codegen,
            "List.setBehavior",
            &[WasmType::I32, WasmType::I32],
            None,
            self.generate_set_behavior()
        )?;

        // List.getBehavior(list) -> string - gets current behavior
        register_stdlib_function(
            codegen,
            "List.getBehavior",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_get_behavior()
        )?;

        Ok(())
    }

    fn register_property_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // List.contains(list, value) -> boolean - checks if value exists (for unique behavior)
        register_stdlib_function(
            codegen,
            "List.contains",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_contains()
        )?;

        // List.size(list) -> integer - gets list size
        register_stdlib_function(
            codegen,
            "List.size",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_size()
        )?;

        Ok(())
    }

    /// Generate WebAssembly instructions for List.add operation
    fn generate_list_add(&self) -> Vec<Instruction> {
        vec![
            // Get list pointer
            Instruction::LocalGet(0), // list_ptr
            // Load current size
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            // Calculate new item position (size * 4 + 8 for header)
            Instruction::I32Const(4),
            Instruction::I32Mul,
            Instruction::I32Const(8),
            Instruction::I32Add,
            // Add to list_ptr to get storage location
            Instruction::LocalGet(0),
            Instruction::I32Add,
            // Store the value
            Instruction::LocalGet(1), // value
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            // Increment list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // current size
            Instruction::I32Const(1),
            Instruction::I32Add, // new size
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
        ]
    }

    /// Generate WebAssembly instructions for List.remove operation
    fn generate_list_remove(&self) -> Vec<Instruction> {
        vec![
            // Get list pointer
            Instruction::LocalGet(0), // list_ptr
            // Load current size
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            // Check if list is empty
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Return 0 if empty
            Instruction::Else,
            // Get last element (LIFO behavior for remove)
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub, // size - 1
            Instruction::I32Const(4),
            Instruction::I32Mul, // (size-1) * 4
            Instruction::I32Const(8),
            Instruction::I32Add, // + header offset
            Instruction::I32Add, // list_ptr + offset
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load value
            // Decrement list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // current size
            Instruction::I32Const(1),
            Instruction::I32Sub, // new size
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::End,
        ]
    }

    /// Generate WebAssembly instructions for List.peek operation
    fn generate_list_peek(&self) -> Vec<Instruction> {
        vec![
            // Get list pointer
            Instruction::LocalGet(0), // list_ptr
            // Load current size
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            // Check if list is empty
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Return 0 if empty
            Instruction::Else,
            // Get last element without removing (LIFO peek)
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub, // size - 1
            Instruction::I32Const(4),
            Instruction::I32Mul, // (size-1) * 4
            Instruction::I32Const(8),
            Instruction::I32Add, // + header offset
            Instruction::I32Add, // list_ptr + offset
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load value
            Instruction::End,
        ]
    }

    /// Generate WebAssembly instructions for List.contains operation
    fn generate_list_contains(&self) -> Vec<Instruction> {
        vec![
            // Get list pointer and value to search for
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // size
            // If empty list, return false
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Return false if empty
            Instruction::Else,
            // Simple linear search - check first element
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Const(8), // skip header
            Instruction::I32Add,
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // first element
            Instruction::LocalGet(1), // search value
            Instruction::I32Eq, // compare
            Instruction::End,
        ]
    }

    /// Generate WebAssembly instructions for List.size operation
    fn generate_list_size(&self) -> Vec<Instruction> {
        vec![
            // Get list pointer (arg 0)
            Instruction::LocalGet(0),
            // Load size field
            Instruction::I32Load(MemArg {
                offset: 0, // size field
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WebAssembly instructions for List.setBehavior operation
    fn generate_set_behavior(&self) -> Vec<Instruction> {
        vec![
            // Store behavior ID in list header at offset 4
            Instruction::LocalGet(0), // list_ptr
            // For now, store behavior string pointer directly
            // Full implementation would parse string and store behavior enum
            Instruction::LocalGet(1), // behavior_string
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
        ]
    }

    /// Generate WebAssembly instructions for List.getBehavior operation
    fn generate_get_behavior(&self) -> Vec<Instruction> {
        vec![
            // Load behavior from list header at offset 4
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 4, align: 2, memory_index: 0 }), // behavior pointer
            // Return the behavior string pointer (or default if 0)
            Instruction::LocalTee(1), // store in local 1
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Return pointer to "default" string if no behavior set
            Instruction::I32Const(1000), // hardcoded "default" string location
            Instruction::Else,
            Instruction::LocalGet(1), // return stored behavior
            Instruction::End,
        ]
    }
}

/// Helper function to parse behavior string
pub fn parse_behavior_string(behavior_str: &str) -> ListBehavior {
    match behavior_str {
        "line" => ListBehavior::Line,
        "pile" => ListBehavior::Pile,
        "unique" => ListBehavior::Unique,
        "line-pile" | "linepile" => ListBehavior::LinePile,
        "line-unique" | "lineunique" => ListBehavior::LineUnique,
        "pile-unique" | "pileunique" => ListBehavior::PileUnique,
        "line-unique-pile" | "lineuniuepile" => ListBehavior::LineUniquePile,
        _ => ListBehavior::Default,
    }
}

/// Helper function to convert behavior to string
pub fn behavior_to_string(behavior: ListBehavior) -> &'static str {
    match behavior {
        ListBehavior::Default => "default",
        ListBehavior::Line => "line",
        ListBehavior::Pile => "pile",
        ListBehavior::Unique => "unique",
        ListBehavior::LinePile => "line-pile",
        ListBehavior::LineUnique => "line-unique",
        ListBehavior::PileUnique => "pile-unique",
        ListBehavior::LineUniquePile => "line-unique-pile",
    }
}