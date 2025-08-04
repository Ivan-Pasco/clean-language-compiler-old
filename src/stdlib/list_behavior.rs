use crate::ast::ListBehavior;
use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// List behavior implementation for Clean Language
/// Handles different list behaviors: line (FIFO), pile (LIFO), unique (set)
///
/// List Header Layout (16 bytes):
/// - Offset 0: Size (i32) - current number of elements
/// - Offset 4: Capacity (i32) - allocated capacity
/// - Offset 8: Type ID (i32) - LIST_TYPE_ID
/// - Offset 12: Behavior (i32) - behavior flags
/// Elements start at offset 16
pub struct ListBehaviorManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

// Behavior flag bits
const BEHAVIOR_LINE: i32 = 0x01; // FIFO queue behavior
const BEHAVIOR_PILE: i32 = 0x02; // LIFO stack behavior
const BEHAVIOR_UNIQUE: i32 = 0x04; // Unique elements only

impl ListBehaviorManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all list behavior functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Property access: list.type = "behavior"
        register_stdlib_function(
            codegen,
            "list.setType",
            &[WasmType::I32, WasmType::I32], // list_ptr, behavior_string_ptr
            None,
            self.generate_set_type(),
        )?;

        // Property access: list.type
        register_stdlib_function(
            codegen,
            "list.getType",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // string_ptr
            self.generate_get_type(),
        )?;

        // Behavior-aware operations
        register_stdlib_function(
            codegen,
            "list.add",
            &[WasmType::I32, WasmType::I32], // list_ptr, value
            None,
            self.generate_list_add(),
        )?;

        register_stdlib_function(
            codegen,
            "list.remove",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // removed value
            self.generate_list_remove(),
        )?;

        register_stdlib_function(
            codegen,
            "list.peek",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // next value without removal
            self.generate_list_peek(),
        )?;

        // Core list operations that respect behavior
        register_stdlib_function(
            codegen,
            "list.contains",
            &[WasmType::I32, WasmType::I32], // list_ptr, value
            Some(WasmType::I32),             // boolean
            self.generate_list_contains(),
        )?;

        register_stdlib_function(
            codegen,
            "list.size",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // size
            self.generate_list_size(),
        )?;

        register_stdlib_function(
            codegen,
            "list.isEmpty",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // boolean
            self.generate_list_is_empty(),
        )?;

        register_stdlib_function(
            codegen,
            "list.isNotEmpty",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // boolean
            self.generate_list_is_not_empty(),
        )?;

        Ok(())
    }

    /// Generate WASM for setting list type property
    fn generate_set_type(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), behavior_string_ptr (1)
            // Parse behavior string and set flags

            // Load string length
            Instruction::LocalGet(1), // behavior_string_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // string_length
            // Initialize behavior flags to 0
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // behavior_flags
            // Check for "line" in string
            Instruction::LocalGet(1), // behavior_string_ptr
            Instruction::Call(self.string_contains_line_index()), // Check if contains "line"
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(BEHAVIOR_LINE),
            Instruction::I32Or,
            Instruction::LocalSet(3),
            Instruction::End,
            // Check for "pile" in string
            Instruction::LocalGet(1), // behavior_string_ptr
            Instruction::Call(self.string_contains_pile_index()), // Check if contains "pile"
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(BEHAVIOR_PILE),
            Instruction::I32Or,
            Instruction::LocalSet(3),
            Instruction::End,
            // Check for "unique" in string
            Instruction::LocalGet(1), // behavior_string_ptr
            Instruction::Call(self.string_contains_unique_index()), // Check if contains "unique"
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(BEHAVIOR_UNIQUE),
            Instruction::I32Or,
            Instruction::LocalSet(3),
            Instruction::End,
            // Store behavior flags in list header at offset 12
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(3), // behavior_flags
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WASM for getting list type property
    fn generate_get_type(&self) -> Vec<Instruction> {
        vec![
            // Parameter: list_ptr (0)
            // Returns: string pointer describing behavior

            // Load behavior flags from list header
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // behavior_flags
            // Build behavior string based on flags
            // For now, return appropriate pre-allocated string
            Instruction::LocalGet(1),
            Instruction::Call(self.behavior_flags_to_string_index()),
        ]
    }

    /// Generate WASM for behavior-aware add operation
    fn generate_list_add(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), value (1)

            // Load behavior flags
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // behavior_flags
            // Check for unique behavior
            Instruction::LocalGet(2),
            Instruction::I32Const(BEHAVIOR_UNIQUE),
            Instruction::I32And,
            Instruction::If(BlockType::Empty),
            // Check if value already exists
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::Call(self.list_contains_internal_index()),
            Instruction::If(BlockType::Empty),
            // Value exists, return without adding
            Instruction::Return,
            Instruction::End,
            Instruction::End,
            // Add element based on behavior
            // For line (FIFO) and default: add to end
            // For pile (LIFO): also add to end (remove from end)
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::Call(self.list_push_internal_index()),
        ]
    }

    /// Generate WASM for behavior-aware remove operation
    fn generate_list_remove(&self) -> Vec<Instruction> {
        vec![
            // Parameter: list_ptr (0)
            // Returns: removed value (0 if empty)

            // Check if list is empty
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Return 0 if empty
            Instruction::Else,
            // Load behavior flags
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // behavior_flags
            // Check for line behavior (FIFO - remove from front)
            Instruction::LocalGet(1),
            Instruction::I32Const(BEHAVIOR_LINE),
            Instruction::I32And,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::Call(self.list_shift_internal_index()),
            Instruction::Else,
            // Default and pile behavior - remove from end
            Instruction::LocalGet(0),
            Instruction::Call(self.list_pop_internal_index()),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for behavior-aware peek operation
    fn generate_list_peek(&self) -> Vec<Instruction> {
        vec![
            // Parameter: list_ptr (0)
            // Returns: next value without removal (0 if empty)

            // Check if list is empty
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Return 0 if empty
            Instruction::Else,
            // Load behavior flags
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // behavior_flags
            // Check for line behavior (FIFO - peek at front)
            Instruction::LocalGet(1),
            Instruction::I32Const(BEHAVIOR_LINE),
            Instruction::I32And,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Get first element
            Instruction::LocalGet(0),
            Instruction::I32Const(16), // Header size
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Default and pile behavior - peek at end
            Instruction::LocalGet(0),
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Const(8), // Element size
            Instruction::I32Mul,
            Instruction::I32Const(16), // Header size
            Instruction::I32Add,
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for list.contains operation
    fn generate_list_contains(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), value (1)
            // Returns: 1 if found, 0 if not

            // Get size
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // size
            // Initialize counter
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // i = 0
            // Loop through elements
            Instruction::Block(BlockType::Result(ValType::I32)),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= size
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0), // Not found
            Instruction::Br(2),
            Instruction::End,
            // Load element at index i
            Instruction::LocalGet(0),
            Instruction::I32Const(16),
            Instruction::I32Add,
            Instruction::LocalGet(3),
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Compare with value
            Instruction::LocalGet(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(1), // Found
            Instruction::Br(2),
            Instruction::End,
            // Increment counter
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0), // Continue loop
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for list.size operation
    fn generate_list_size(&self) -> Vec<Instruction> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate WASM for list.isEmpty operation
    fn generate_list_is_empty(&self) -> Vec<Instruction> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Eqz,
        ]
    }

    /// Generate WASM for list.isNotEmpty operation
    fn generate_list_is_not_empty(&self) -> Vec<Instruction> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(0),
            Instruction::I32Ne,
        ]
    }

    // Helper function indices (would be resolved from function table)
    fn string_contains_line_index(&self) -> u32 {
        300
    }
    fn string_contains_pile_index(&self) -> u32 {
        301
    }
    fn string_contains_unique_index(&self) -> u32 {
        302
    }
    fn behavior_flags_to_string_index(&self) -> u32 {
        303
    }
    fn list_contains_internal_index(&self) -> u32 {
        304
    }
    fn list_push_internal_index(&self) -> u32 {
        305
    }
    fn list_shift_internal_index(&self) -> u32 {
        306
    }
    fn list_pop_internal_index(&self) -> u32 {
        307
    }
}

/// Convert behavior enum to flags
pub fn behavior_to_flags(behavior: ListBehavior) -> i32 {
    match behavior {
        ListBehavior::Default => 0,
        ListBehavior::Line => BEHAVIOR_LINE,
        ListBehavior::Pile => BEHAVIOR_PILE,
        ListBehavior::Unique => BEHAVIOR_UNIQUE,
        ListBehavior::LinePile => BEHAVIOR_LINE | BEHAVIOR_PILE,
        ListBehavior::LineUnique => BEHAVIOR_LINE | BEHAVIOR_UNIQUE,
        ListBehavior::PileUnique => BEHAVIOR_PILE | BEHAVIOR_UNIQUE,
        ListBehavior::LineUniquePile => BEHAVIOR_LINE | BEHAVIOR_PILE | BEHAVIOR_UNIQUE,
    }
}

/// Convert flags to behavior enum
pub fn flags_to_behavior(flags: i32) -> ListBehavior {
    match flags {
        0 => ListBehavior::Default,
        f if f == BEHAVIOR_LINE => ListBehavior::Line,
        f if f == BEHAVIOR_PILE => ListBehavior::Pile,
        f if f == BEHAVIOR_UNIQUE => ListBehavior::Unique,
        f if f == (BEHAVIOR_LINE | BEHAVIOR_PILE) => ListBehavior::LinePile,
        f if f == (BEHAVIOR_LINE | BEHAVIOR_UNIQUE) => ListBehavior::LineUnique,
        f if f == (BEHAVIOR_PILE | BEHAVIOR_UNIQUE) => ListBehavior::PileUnique,
        f if f == (BEHAVIOR_LINE | BEHAVIOR_PILE | BEHAVIOR_UNIQUE) => ListBehavior::LineUniquePile,
        _ => ListBehavior::Default,
    }
}

/// Parse behavior string to enum
pub fn parse_behavior_string(behavior_str: &str) -> ListBehavior {
    match behavior_str {
        "default" => ListBehavior::Default,
        "line" => ListBehavior::Line,
        "pile" => ListBehavior::Pile,
        "unique" => ListBehavior::Unique,
        "line-pile" | "linepile" => ListBehavior::LinePile,
        "line-unique" | "lineunique" => ListBehavior::LineUnique,
        "pile-unique" | "pileunique" => ListBehavior::PileUnique,
        "line-unique-pile" | "lineuniquepile" => ListBehavior::LineUniquePile,
        _ => ListBehavior::Default,
    }
}

/// Convert behavior to string
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behavior_conversion() {
        // Test behavior to flags
        assert_eq!(behavior_to_flags(ListBehavior::Default), 0);
        assert_eq!(behavior_to_flags(ListBehavior::Line), BEHAVIOR_LINE);
        assert_eq!(behavior_to_flags(ListBehavior::Pile), BEHAVIOR_PILE);
        assert_eq!(behavior_to_flags(ListBehavior::Unique), BEHAVIOR_UNIQUE);
        assert_eq!(
            behavior_to_flags(ListBehavior::LineUnique),
            BEHAVIOR_LINE | BEHAVIOR_UNIQUE
        );

        // Test flags to behavior
        assert_eq!(flags_to_behavior(0), ListBehavior::Default);
        assert_eq!(flags_to_behavior(BEHAVIOR_LINE), ListBehavior::Line);
        assert_eq!(flags_to_behavior(BEHAVIOR_PILE), ListBehavior::Pile);
        assert_eq!(flags_to_behavior(BEHAVIOR_UNIQUE), ListBehavior::Unique);
        assert_eq!(
            flags_to_behavior(BEHAVIOR_LINE | BEHAVIOR_UNIQUE),
            ListBehavior::LineUnique
        );
    }

    #[test]
    fn test_behavior_string_parsing() {
        assert_eq!(parse_behavior_string("line"), ListBehavior::Line);
        assert_eq!(parse_behavior_string("pile"), ListBehavior::Pile);
        assert_eq!(parse_behavior_string("unique"), ListBehavior::Unique);
        assert_eq!(
            parse_behavior_string("line-unique"),
            ListBehavior::LineUnique
        );
        assert_eq!(
            parse_behavior_string("lineunique"),
            ListBehavior::LineUnique
        );
        assert_eq!(parse_behavior_string("invalid"), ListBehavior::Default);
    }

    #[test]
    fn test_behavior_to_string() {
        assert_eq!(behavior_to_string(ListBehavior::Default), "default");
        assert_eq!(behavior_to_string(ListBehavior::Line), "line");
        assert_eq!(behavior_to_string(ListBehavior::Pile), "pile");
        assert_eq!(behavior_to_string(ListBehavior::Unique), "unique");
        assert_eq!(behavior_to_string(ListBehavior::LineUnique), "line-unique");
    }
}
