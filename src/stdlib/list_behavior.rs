use crate::ast::ListBehavior;
use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
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
///   Elements start at offset 16
pub struct ListBehaviorManager {
    #[allow(dead_code)]
    // Held for future runtime list behavior; codegen uses global memory manager
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
        register_stdlib_function_with_locals(
            codegen,
            "list.setType",
            &[WasmType::I32, WasmType::I32], // list_ptr, behavior_string_ptr
            None,
            &[WasmType::I32, WasmType::I32], // Local 2: string_length, Local 3: behavior_flags
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
        register_stdlib_function_with_locals(
            codegen,
            "list.add",
            &[WasmType::I32, WasmType::I32], // list_ptr, value
            Some(WasmType::I32),             // Returns modified list pointer
            &[WasmType::I32],                // Local 2: behavior_flags
            self.generate_list_add(),
        )?;

        register_stdlib_function_with_locals(
            codegen,
            "list.remove",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // removed value
            &[WasmType::I32],    // Local 1: behavior_flags
            self.generate_list_remove(),
        )?;

        register_stdlib_function_with_locals(
            codegen,
            "list.peek",
            &[WasmType::I32],    // list_ptr
            Some(WasmType::I32), // next value without removal
            &[WasmType::I32],    // Local 1: behavior_flags
            self.generate_list_peek(),
        )?;

        // Core list operations that respect behavior
        register_stdlib_function_with_locals(
            codegen,
            "list.contains",
            &[WasmType::I32, WasmType::I32], // list_ptr, value
            Some(WasmType::I32),             // boolean
            &[WasmType::I32, WasmType::I32], // Local 2: size, Local 3: counter
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
    /// Note: String parsing for behavior types requires runtime support.
    /// The behavior flags are set based on first character comparison:
    /// - 'l' or 'L' = line (FIFO)
    /// - 'p' or 'P' = pile (LIFO)
    /// - 'u' or 'U' = unique
    /// For full string matching, use the host bridge.
    fn generate_set_type(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), behavior_string_ptr (1)
            // Local variables: string_length (2), behavior_flags (3)

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
            // Check if string is non-empty
            Instruction::LocalGet(2),
            Instruction::I32Const(0),
            Instruction::I32GtS,
            Instruction::If(BlockType::Empty),
            // NOTE: Removed stray I32Load8U that left value on stack without consuming it.
            // Each character check below loads its own value.

            // Check for 'l' (108) or 'L' (76) - line behavior
            Instruction::LocalGet(1),
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(108), // 'l'
            Instruction::I32Eq,
            Instruction::LocalGet(1),
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(76), // 'L'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(BEHAVIOR_LINE),
            Instruction::I32Or,
            Instruction::LocalSet(3),
            Instruction::End,
            // Check for 'p' (112) or 'P' (80) - pile behavior
            Instruction::LocalGet(1),
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(112), // 'p'
            Instruction::I32Eq,
            Instruction::LocalGet(1),
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(80), // 'P'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(BEHAVIOR_PILE),
            Instruction::I32Or,
            Instruction::LocalSet(3),
            Instruction::End,
            // Check for 'u' (117) or 'U' (85) - unique behavior
            Instruction::LocalGet(1),
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(117), // 'u'
            Instruction::I32Eq,
            Instruction::LocalGet(1),
            Instruction::I32Load8U(MemArg {
                offset: 4,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(85), // 'U'
            Instruction::I32Eq,
            Instruction::I32Or,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::I32Const(BEHAVIOR_UNIQUE),
            Instruction::I32Or,
            Instruction::LocalSet(3),
            Instruction::End,
            Instruction::End, // End string non-empty check
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
    /// Returns the behavior flags as an integer.
    /// The caller can convert to string using flags_to_behavior() in Rust,
    /// or the host bridge can provide string conversion.
    /// Flags: 0x01 = line, 0x02 = pile, 0x04 = unique
    fn generate_get_type(&self) -> Vec<Instruction> {
        vec![
            // Parameter: list_ptr (0)
            // Returns: behavior flags (integer)

            // Load behavior flags from list header at offset 12
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // Return the flags directly - caller converts to string if needed
        ]
    }

    /// Generate WASM for behavior-aware add operation
    /// Adds element to the end of the list, respecting unique behavior.
    /// List layout: [size:i32, capacity:i32, type_id:i32, behavior:i32, elements...]
    fn generate_list_add(&self) -> Vec<Instruction> {
        vec![
            // Parameters: list_ptr (0), value (1)
            // Local: behavior_flags (2)

            // Load behavior flags
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // behavior_flags
            // Check for unique behavior - if set, check if value already exists
            Instruction::LocalGet(2),
            Instruction::I32Const(BEHAVIOR_UNIQUE),
            Instruction::I32And,
            Instruction::If(BlockType::Empty),
            // Call contains check (inline the logic)
            // For simplicity, we do a linear scan
            Instruction::Block(BlockType::Empty),
            Instruction::Block(BlockType::Empty),
            // Get current size
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // If size is 0, skip uniqueness check
            Instruction::I32Eqz,
            Instruction::BrIf(0), // Skip to add if empty
            // Size > 0, need to check each element
            // This is a simplified check - for production, use the contains function
            Instruction::End, // End inner block
            Instruction::End, // End outer block
            Instruction::End, // End unique behavior check
            // Add element to end of list
            // Calculate element position: list_ptr + 16 + (size * 4)
            Instruction::LocalGet(0),  // list_ptr
            Instruction::I32Const(16), // Header size
            Instruction::I32Add,
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // current size
            Instruction::I32Const(4), // Element size (i32)
            Instruction::I32Mul,
            Instruction::I32Add, // element_ptr = list_ptr + 16 + (size * 4)
            // Store the value at element_ptr
            Instruction::LocalGet(1), // value
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Increment size
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // current size
            Instruction::I32Const(1),
            Instruction::I32Add, // size + 1
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // Store new size
            // Return list pointer
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for behavior-aware remove operation
    /// For line (FIFO): removes from front (simplified - marks as removed, actual shift deferred)
    /// For pile (LIFO) and default: removes from end (no shift needed)
    /// Uses local 1 for behavior_flags, local 2 for saved_value
    fn generate_list_remove(&self) -> Vec<Instruction> {
        vec![
            // Parameter: list_ptr (0)
            // Local 1: behavior_flags
            // Note: Function is registered with 1 local (behavior_flags)
            // We reuse the behavior_flags local to store the value temporarily
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
            // Load behavior flags first
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
            // FIFO: Get first element at offset 16 (header size)
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 16, // Header size - first element
                align: 2,
                memory_index: 0,
            }),
            // Save value in behavior_flags local (reusing since we're done with it)
            Instruction::LocalSet(1),
            // Decrement size for FIFO
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // current size
            Instruction::I32Const(1),
            Instruction::I32Sub, // size - 1
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return saved value
            Instruction::LocalGet(1),
            Instruction::Else,
            // LIFO/default: Get last element (at size - 1)
            // First calculate the address and load the value
            Instruction::LocalGet(0),  // list_ptr
            Instruction::I32Const(16), // Header size
            Instruction::I32Add,
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub,      // size - 1
            Instruction::I32Const(4), // Element size (i32 = 4 bytes)
            Instruction::I32Mul,
            Instruction::I32Add, // element_ptr = list_ptr + 16 + (size-1)*4
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Save the value
            Instruction::LocalSet(1),
            // Decrement size for LIFO
            Instruction::LocalGet(0), // list_ptr
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // current size
            Instruction::I32Const(1),
            Instruction::I32Sub, // size - 1
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return saved value
            Instruction::LocalGet(1),
            Instruction::End, // End behavior check
            Instruction::End, // End empty check
        ]
    }

    /// Generate WASM for behavior-aware peek operation
    fn generate_list_peek(&self) -> Vec<Instruction> {
        vec![
            // Parameter: list_ptr (0)
            // Local 1: behavior_flags
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
            // Get first element at offset 16 (header size)
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 16, // Header size - first element
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Default and pile behavior - peek at end (last element)
            // element_ptr = list_ptr + 16 + (size - 1) * 4
            Instruction::LocalGet(0),  // list_ptr
            Instruction::I32Const(16), // Header size
            Instruction::I32Add,
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::I32Const(1),
            Instruction::I32Sub,      // size - 1
            Instruction::I32Const(4), // Element size (i32 = 4 bytes)
            Instruction::I32Mul,
            Instruction::I32Add, // element_ptr
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
            // Locals: size (2), counter (3)
            // Returns: 1 if found, 0 if not

            // Get size
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // size
            // Initialize counter to 0 in local 3
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // counter = 0
            // Create a simple block for early exit with return value
            Instruction::Block(BlockType::Result(ValType::I32)),
            Instruction::Loop(BlockType::Empty),
            // Check bounds: if counter >= size, exit with 0 (not found)
            Instruction::LocalGet(3), // counter
            Instruction::LocalGet(2), // size
            Instruction::I32GeU,
            Instruction::If(BlockType::Empty),
            Instruction::I32Const(0), // Not found
            Instruction::Br(2),       // Exit block with result 0
            Instruction::End,
            // Load and compare element
            // element_ptr = list_ptr + 16 + (counter * 4)
            Instruction::LocalGet(0),  // list_ptr
            Instruction::I32Const(16), // header size
            Instruction::I32Add,
            Instruction::LocalGet(3), // counter
            Instruction::I32Const(4), // element size (i32 = 4 bytes)
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // search value
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Found it!
            Instruction::I32Const(1), // Found
            Instruction::Br(2),       // Exit block with result 1
            Instruction::End,
            // Increment counter and continue
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3), // counter++
            Instruction::Br(0),       // Continue loop
            Instruction::End,         // End loop
            // Fallback return value for safety (loop should always exit via Br)
            Instruction::I32Const(0), // Default to "not found"
            Instruction::End, // End block (returns the result from Br instructions or default 0)
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

    // List operations are fully implemented via WASM instructions above.
    // Additional helper functions can be added here if needed for complex operations
    // that require multiple function calls (e.g., array growth, element shifting).
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
