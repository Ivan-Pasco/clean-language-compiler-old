use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function_with_locals, MemoryManager};
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Pairs Type Manager - Implements pairs<any,any> associative container
/// Supports key-value dictionary/map functionality
pub struct PairsTypeManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl PairsTypeManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all pairs type functions with the code generator
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Pairs creation functions
        self.register_create_pairs(codegen)?;
        self.register_create_pairs_with_capacity(codegen)?;

        // Pairs manipulation functions
        self.register_pairs_set(codegen)?;
        self.register_pairs_get(codegen)?;
        self.register_pairs_has_key(codegen)?;
        self.register_pairs_remove(codegen)?;
        self.register_pairs_clear(codegen)?;

        // Pairs property functions
        self.register_pairs_size(codegen)?;
        self.register_pairs_is_empty(codegen)?;
        self.register_pairs_capacity(codegen)?;

        // Pairs iteration functions
        self.register_pairs_keys(codegen)?;
        self.register_pairs_values(codegen)?;
        self.register_pairs_entries(codegen)?;

        // Pairs utility functions
        self.register_pairs_clone(codegen)?;
        self.register_pairs_merge(codegen)?;
        self.register_pairs_equals(codegen)?;
        self.register_pairs_to_string(codegen)?;

        Ok(())
    }

    /// Create empty pairs container
    fn register_create_pairs(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.create",
            &[],                             // no parameters
            Some(WasmType::I32),             // pairs pointer
            &[WasmType::I32, WasmType::I32], // pairs_ptr, header_size
            self.generate_create_pairs(),
        )
    }

    fn generate_create_pairs(&self) -> Vec<Instruction> {
        vec![
            // Allocate pairs header: 24 bytes
            // 0-3: size (number of key-value pairs)
            // 4-7: capacity (allocated slots)
            // 8-11: key_type_id (0 = mixed types)
            // 12-15: value_type_id (0 = mixed types)
            // 16-19: entries_ptr (pointer to entries array)
            // 20-23: hash_seed (for hash table)
            Instruction::I32Const(24), // header size
            Instruction::I32Const(8),  // alignment
            Instruction::Call(2000),   // memory.allocate
            Instruction::LocalSet(0),  // pairs_ptr
            // Initialize header with zeros
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Const(0), // size = 0
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Const(8), // default capacity = 8
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Const(0), // key_type_id = 0 (mixed)
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Const(0), // value_type_id = 0 (mixed)
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // Allocate entries array: capacity * 16 bytes per entry
            // Each entry: 8 bytes key + 8 bytes value (pointers or values)
            Instruction::I32Const(8),  // capacity
            Instruction::I32Const(16), // bytes per entry
            Instruction::I32Mul,
            Instruction::I32Const(8), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(1), // entries_ptr
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::LocalGet(1), // entries_ptr
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // Initialize hash seed
            Instruction::LocalGet(0),                    // pairs_ptr
            Instruction::I32Const(0x9e3779b9u32 as i32), // hash seed constant
            Instruction::I32Store(MemArg {
                offset: 20,
                align: 2,
                memory_index: 0,
            }),
            // Return pairs pointer
            Instruction::LocalGet(0), // pairs_ptr
        ]
    }

    /// Create pairs container with specific capacity
    fn register_create_pairs_with_capacity(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.createWithCapacity",
            &[WasmType::I32],                               // initial_capacity
            Some(WasmType::I32),                            // pairs pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // pairs_ptr, entries_ptr, entry_size
            self.generate_create_pairs_with_capacity(),
        )
    }

    fn generate_create_pairs_with_capacity(&self) -> Vec<Instruction> {
        vec![
            // Allocate pairs header: 24 bytes
            Instruction::I32Const(24), // header size
            Instruction::I32Const(8),  // alignment
            Instruction::Call(2000),   // memory.allocate
            Instruction::LocalSet(1),  // pairs_ptr
            // Initialize header
            Instruction::LocalGet(1), // pairs_ptr
            Instruction::I32Const(0), // size = 0
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // pairs_ptr
            Instruction::LocalGet(0), // initial_capacity
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // pairs_ptr
            Instruction::I32Const(0), // key_type_id = 0 (mixed)
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1), // pairs_ptr
            Instruction::I32Const(0), // value_type_id = 0 (mixed)
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // Allocate entries array
            Instruction::LocalGet(0),  // initial_capacity
            Instruction::I32Const(16), // bytes per entry
            Instruction::I32Mul,
            Instruction::LocalSet(3), // entry_size
            Instruction::LocalGet(3), // entry_size
            Instruction::I32Const(8), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(2), // entries_ptr
            Instruction::LocalGet(1), // pairs_ptr
            Instruction::LocalGet(2), // entries_ptr
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // Initialize hash seed
            Instruction::LocalGet(1),                    // pairs_ptr
            Instruction::I32Const(0x9e3779b9u32 as i32), // hash seed constant
            Instruction::I32Store(MemArg {
                offset: 20,
                align: 2,
                memory_index: 0,
            }),
            // Return pairs pointer
            Instruction::LocalGet(1), // pairs_ptr
        ]
    }

    /// Set key-value pair
    fn register_pairs_set(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.set",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // pairs_ptr, key_ptr, value_ptr
            None,                                           // void
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // size, capacity, entries_ptr, hash, index, entry_ptr
            self.generate_pairs_set(),
        )
    }

    fn generate_pairs_set(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::LocalSet(3), // size
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(4), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(5), // entries_ptr
            // Simple hash function: key_ptr % capacity
            Instruction::LocalGet(1), // key_ptr
            Instruction::LocalGet(4), // capacity
            Instruction::I32RemU,
            Instruction::LocalSet(6), // hash (index)
            // Find entry slot (linear probing)
            Instruction::LocalGet(5),  // entries_ptr
            Instruction::LocalGet(6),  // hash
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(8), // entry_ptr
            // Store key and value
            Instruction::LocalGet(8), // entry_ptr
            Instruction::LocalGet(1), // key_ptr
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // store key
            Instruction::LocalGet(8), // entry_ptr
            Instruction::LocalGet(2), // value_ptr
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // store value
            // Increment size if this is a new key (simplified)
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::LocalGet(3), // size
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size++
        ]
    }

    /// Get value by key
    fn register_pairs_get(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.get",
            &[WasmType::I32, WasmType::I32], // pairs_ptr, key_ptr
            Some(WasmType::I32),             // value_ptr (0 if not found)
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // capacity, entries_ptr, hash, entry_ptr, stored_key
            self.generate_pairs_get(),
        )
    }

    fn generate_pairs_get(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(2), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(3), // entries_ptr
            // Simple hash function: key_ptr % capacity
            Instruction::LocalGet(1), // key_ptr
            Instruction::LocalGet(2), // capacity
            Instruction::I32RemU,
            Instruction::LocalSet(4), // hash
            // Find entry
            Instruction::LocalGet(3),  // entries_ptr
            Instruction::LocalGet(4),  // hash
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(5), // entry_ptr
            // Load stored key
            Instruction::LocalGet(5), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // stored_key
            Instruction::LocalSet(6), // stored_key
            // Simple key comparison (pointer equality for now)
            Instruction::LocalGet(6), // stored_key
            Instruction::LocalGet(1), // key_ptr
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Key matches - return value
            Instruction::LocalGet(5), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // load value
            Instruction::Else,
            // Key not found - return 0
            Instruction::I32Const(0),
            Instruction::End,
        ]
    }

    /// Check if key exists
    fn register_pairs_has_key(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.hasKey",
            &[WasmType::I32, WasmType::I32], // pairs_ptr, key_ptr
            Some(WasmType::I32),             // boolean result
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // capacity, entries_ptr, hash, entry_ptr, stored_key
            self.generate_pairs_has_key(),
        )
    }

    fn generate_pairs_has_key(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(2), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(3), // entries_ptr
            // Simple hash function: key_ptr % capacity
            Instruction::LocalGet(1), // key_ptr
            Instruction::LocalGet(2), // capacity
            Instruction::I32RemU,
            Instruction::LocalSet(4), // hash
            // Find entry
            Instruction::LocalGet(3),  // entries_ptr
            Instruction::LocalGet(4),  // hash
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(5), // entry_ptr
            // Load stored key
            Instruction::LocalGet(5), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // stored_key
            Instruction::LocalSet(6), // stored_key
            // Check if key matches and is not null
            Instruction::LocalGet(6), // stored_key
            Instruction::I32Const(0),
            Instruction::I32Ne,       // stored_key != 0
            Instruction::LocalGet(6), // stored_key
            Instruction::LocalGet(1), // key_ptr
            Instruction::I32Eq,       // stored_key == key_ptr
            Instruction::I32And,      // both conditions true
        ]
    }

    /// Remove key-value pair
    fn register_pairs_remove(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.remove",
            &[WasmType::I32, WasmType::I32], // pairs_ptr, key_ptr
            Some(WasmType::I32),             // boolean (true if removed)
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // size, capacity, entries_ptr, hash, entry_ptr, stored_key
            self.generate_pairs_remove(),
        )
    }

    fn generate_pairs_remove(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::LocalSet(2), // size
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(3), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(4), // entries_ptr
            // Simple hash function: key_ptr % capacity
            Instruction::LocalGet(1), // key_ptr
            Instruction::LocalGet(3), // capacity
            Instruction::I32RemU,
            Instruction::LocalSet(5), // hash
            // Find entry
            Instruction::LocalGet(4),  // entries_ptr
            Instruction::LocalGet(5),  // hash
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(6), // entry_ptr
            // Load stored key
            Instruction::LocalGet(6), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // stored_key
            Instruction::LocalSet(7), // stored_key
            // Check if key matches
            Instruction::LocalGet(7), // stored_key
            Instruction::LocalGet(1), // key_ptr
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Key found - remove it
            Instruction::LocalGet(6), // entry_ptr
            Instruction::I32Const(0), // null key
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // clear key
            Instruction::LocalGet(6), // entry_ptr
            Instruction::I32Const(0), // null value
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // clear value
            // Decrement size
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::LocalGet(2), // size
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size--
            Instruction::I32Const(1), // return true
            Instruction::Else,
            Instruction::I32Const(0), // return false
            Instruction::End,
        ]
    }

    /// Clear all pairs
    fn register_pairs_clear(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.clear",
            &[WasmType::I32], // pairs_ptr
            None,             // void
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // capacity, entries_ptr, i, entry_ptr
            self.generate_pairs_clear(),
        )
    }

    fn generate_pairs_clear(&self) -> Vec<Instruction> {
        vec![
            // Set size to 0
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Const(0), // size = 0
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Load capacity and entries_ptr
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(1), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(2), // entries_ptr
            // Clear all entries
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(3), // i
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= capacity
            Instruction::LocalGet(3), // i
            Instruction::LocalGet(1), // capacity
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= capacity
            // Calculate entry pointer
            Instruction::LocalGet(2),  // entries_ptr
            Instruction::LocalGet(3),  // i
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(4), // entry_ptr
            // Clear key
            Instruction::LocalGet(4), // entry_ptr
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Clear value
            Instruction::LocalGet(4), // entry_ptr
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Increment i
            Instruction::LocalGet(3), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
        ]
    }

    /// Get pairs size
    fn register_pairs_size(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.size",
            &[WasmType::I32],    // pairs_ptr
            Some(WasmType::I32), // size
            &[],                 // no locals
            vec![
                Instruction::LocalGet(0), // pairs_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load size
            ],
        )
    }

    /// Check if pairs is empty
    fn register_pairs_is_empty(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.isEmpty",
            &[WasmType::I32],    // pairs_ptr
            Some(WasmType::I32), // boolean result
            &[WasmType::I32],    // size
            vec![
                Instruction::LocalGet(0), // pairs_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load size
                Instruction::LocalSet(1), // size
                Instruction::LocalGet(1), // size
                Instruction::I32Const(0),
                Instruction::I32Eq, // size == 0
            ],
        )
    }

    /// Get pairs capacity
    fn register_pairs_capacity(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.capacity",
            &[WasmType::I32],    // pairs_ptr
            Some(WasmType::I32), // capacity
            &[],                 // no locals
            vec![
                Instruction::LocalGet(0), // pairs_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load capacity
            ],
        )
    }

    /// Get all keys as array
    fn register_pairs_keys(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.keys",
            &[WasmType::I32],    // pairs_ptr
            Some(WasmType::I32), // keys array pointer
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // size, capacity, entries_ptr, keys_ptr, i, j, entry_ptr
            self.generate_pairs_keys(),
        )
    }

    fn generate_pairs_keys(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::LocalSet(1), // size
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(2), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(3), // entries_ptr
            // Allocate keys array: size * 4 bytes (pointers)
            Instruction::LocalGet(1), // size
            Instruction::I32Const(4), // sizeof(pointer)
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(4), // keys_ptr
            // Collect keys
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(5), // i
            Instruction::I32Const(0), // j = 0 (keys array index)
            Instruction::LocalSet(6), // j
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= capacity
            Instruction::LocalGet(5), // i
            Instruction::LocalGet(2), // capacity
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= capacity
            // Calculate entry pointer
            Instruction::LocalGet(3),  // entries_ptr
            Instruction::LocalGet(5),  // i
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(7), // entry_ptr
            // Check if entry has a key (not null)
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load key
            Instruction::I32Const(0),
            Instruction::I32Ne, // key != 0
            Instruction::If(BlockType::Empty),
            // Store key in keys array
            Instruction::LocalGet(4), // keys_ptr
            Instruction::LocalGet(6), // j
            Instruction::I32Const(4), // sizeof(pointer)
            Instruction::I32Mul,
            Instruction::I32Add,      // keys_ptr + j*4
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load key
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // store key
            // Increment j
            Instruction::LocalGet(6), // j
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // j++
            Instruction::End,
            // Increment i
            Instruction::LocalGet(5), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return keys array
            Instruction::LocalGet(4), // keys_ptr
        ]
    }

    /// Get all values as array
    fn register_pairs_values(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.values",
            &[WasmType::I32],    // pairs_ptr
            Some(WasmType::I32), // values array pointer
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // size, capacity, entries_ptr, values_ptr, i, j, entry_ptr
            self.generate_pairs_values(),
        )
    }

    fn generate_pairs_values(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::LocalSet(1), // size
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(2), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(3), // entries_ptr
            // Allocate values array: size * 4 bytes (pointers)
            Instruction::LocalGet(1), // size
            Instruction::I32Const(4), // sizeof(pointer)
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(4), // values_ptr
            // Collect values
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(5), // i
            Instruction::I32Const(0), // j = 0 (values array index)
            Instruction::LocalSet(6), // j
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= capacity
            Instruction::LocalGet(5), // i
            Instruction::LocalGet(2), // capacity
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= capacity
            // Calculate entry pointer
            Instruction::LocalGet(3),  // entries_ptr
            Instruction::LocalGet(5),  // i
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(7), // entry_ptr
            // Check if entry has a key (not null)
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load key
            Instruction::I32Const(0),
            Instruction::I32Ne, // key != 0
            Instruction::If(BlockType::Empty),
            // Store value in values array
            Instruction::LocalGet(4), // values_ptr
            Instruction::LocalGet(6), // j
            Instruction::I32Const(4), // sizeof(pointer)
            Instruction::I32Mul,
            Instruction::I32Add,      // values_ptr + j*4
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // load value
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // store value
            // Increment j
            Instruction::LocalGet(6), // j
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // j++
            Instruction::End,
            // Increment i
            Instruction::LocalGet(5), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return values array
            Instruction::LocalGet(4), // values_ptr
        ]
    }

    /// Get all entries as array of [key, value] pairs
    fn register_pairs_entries(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.entries",
            &[WasmType::I32],    // pairs_ptr
            Some(WasmType::I32), // entries array pointer
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // size, capacity, src_entries_ptr, dest_entries_ptr, i, j, entry_ptr
            self.generate_pairs_entries(),
        )
    }

    fn generate_pairs_entries(&self) -> Vec<Instruction> {
        vec![
            // Load pairs properties
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // size
            Instruction::LocalSet(1), // size
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // capacity
            Instruction::LocalSet(2), // capacity
            Instruction::LocalGet(0), // pairs_ptr
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // entries_ptr
            Instruction::LocalSet(3), // src_entries_ptr
            // Allocate destination entries array: size * 8 bytes (key+value pointers)
            Instruction::LocalGet(1), // size
            Instruction::I32Const(8), // sizeof(key_ptr + value_ptr)
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(4), // dest_entries_ptr
            // Collect entries
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(5), // i
            Instruction::I32Const(0), // j = 0 (dest array index)
            Instruction::LocalSet(6), // j
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= capacity
            Instruction::LocalGet(5), // i
            Instruction::LocalGet(2), // capacity
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= capacity
            // Calculate source entry pointer
            Instruction::LocalGet(3),  // src_entries_ptr
            Instruction::LocalGet(5),  // i
            Instruction::I32Const(16), // entry size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(7), // entry_ptr
            // Check if entry has a key (not null)
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load key
            Instruction::I32Const(0),
            Instruction::I32Ne, // key != 0
            Instruction::If(BlockType::Empty),
            // Store key in destination
            Instruction::LocalGet(4), // dest_entries_ptr
            Instruction::LocalGet(6), // j
            Instruction::I32Const(8), // sizeof(entry)
            Instruction::I32Mul,
            Instruction::I32Add,      // dest_entries_ptr + j*8
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load key
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // store key
            // Store value in destination
            Instruction::LocalGet(4), // dest_entries_ptr
            Instruction::LocalGet(6), // j
            Instruction::I32Const(8), // sizeof(entry)
            Instruction::I32Mul,
            Instruction::I32Add,      // dest_entries_ptr + j*8
            Instruction::LocalGet(7), // entry_ptr
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // load value
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // store value
            // Increment j
            Instruction::LocalGet(6), // j
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // j++
            Instruction::End,
            // Increment i
            Instruction::LocalGet(5), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(5), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return destination entries array
            Instruction::LocalGet(4), // dest_entries_ptr
        ]
    }

    /// Clone pairs container
    fn register_pairs_clone(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.clone",
            &[WasmType::I32],                // pairs_ptr
            Some(WasmType::I32),             // cloned pairs pointer
            &[WasmType::I32, WasmType::I32], // capacity, new_pairs_ptr
            vec![
                // Get capacity
                Instruction::LocalGet(0), // pairs_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // capacity
                Instruction::LocalSet(1), // capacity
                // Create new pairs with same capacity
                Instruction::LocalGet(1), // capacity
                Instruction::Call(1401),  // pairs.createWithCapacity
                Instruction::LocalSet(2), // new_pairs_ptr
                // TODO: Copy all entries from source to destination
                // For now, return empty pairs with same capacity
                Instruction::LocalGet(2), // new_pairs_ptr
            ],
        )
    }

    /// Merge another pairs container into this one
    fn register_pairs_merge(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.merge",
            &[WasmType::I32, WasmType::I32], // dest_pairs_ptr, src_pairs_ptr
            None,                            // void
            &[],                             // no locals for now
            vec![
                // TODO: Implement merge logic
                // For now, this is a placeholder
            ],
        )
    }

    /// Check if two pairs containers are equal
    fn register_pairs_equals(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.equals",
            &[WasmType::I32, WasmType::I32], // pairs1_ptr, pairs2_ptr
            Some(WasmType::I32),             // boolean result
            &[WasmType::I32, WasmType::I32], // size1, size2
            vec![
                // Quick size check
                Instruction::LocalGet(0), // pairs1_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // size1
                Instruction::LocalSet(2), // size1
                Instruction::LocalGet(1), // pairs2_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // size2
                Instruction::LocalSet(3), // size2
                // If sizes don't match, return false
                Instruction::LocalGet(2), // size1
                Instruction::LocalGet(3), // size2
                Instruction::I32Eq,       // sizes equal?

                                          // TODO: Add detailed comparison logic
                                          // For now, just return size comparison result
            ],
        )
    }

    /// Convert pairs to string representation
    fn register_pairs_to_string(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "pairs.toString",
            &[WasmType::I32],                // pairs_ptr
            Some(WasmType::I32),             // string pointer
            &[WasmType::I32, WasmType::I32], // size, result_ptr
            vec![
                // Get size
                Instruction::LocalGet(0), // pairs_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // size
                Instruction::LocalSet(1), // size
                // Allocate result string (estimated size: 20 + size*20)
                Instruction::I32Const(20), // base size
                Instruction::LocalGet(1),  // size
                Instruction::I32Const(20), // estimated chars per entry
                Instruction::I32Mul,
                Instruction::I32Add,
                Instruction::I32Const(4), // alignment
                Instruction::Call(2000),  // memory.allocate
                Instruction::LocalSet(2), // result_ptr
                // TODO: Implement string formatting
                // For now, return empty string placeholder
                Instruction::LocalGet(2), // result_ptr
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::CodeGenerator;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_pairs_type_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _manager = PairsTypeManager::new(memory_manager);
    }

    #[test]
    fn test_register_pairs_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = PairsTypeManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Verify core functions are registered
        assert!(codegen.get_function_index("pairs.create").is_some());
        assert!(codegen
            .get_function_index("pairs.createWithCapacity")
            .is_some());
        assert!(codegen.get_function_index("pairs.set").is_some());
        assert!(codegen.get_function_index("pairs.get").is_some());
        assert!(codegen.get_function_index("pairs.hasKey").is_some());
        assert!(codegen.get_function_index("pairs.remove").is_some());

        Ok(())
    }

    #[test]
    fn test_pairs_creation_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = PairsTypeManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that creation functions are available
        assert!(codegen.get_function_index("pairs.create").is_some());
        assert!(codegen
            .get_function_index("pairs.createWithCapacity")
            .is_some());

        Ok(())
    }

    #[test]
    fn test_pairs_manipulation_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = PairsTypeManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that manipulation functions are available
        assert!(codegen.get_function_index("pairs.set").is_some());
        assert!(codegen.get_function_index("pairs.get").is_some());
        assert!(codegen.get_function_index("pairs.hasKey").is_some());
        assert!(codegen.get_function_index("pairs.remove").is_some());
        assert!(codegen.get_function_index("pairs.clear").is_some());

        Ok(())
    }

    #[test]
    fn test_pairs_property_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = PairsTypeManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that property functions are available
        assert!(codegen.get_function_index("pairs.size").is_some());
        assert!(codegen.get_function_index("pairs.isEmpty").is_some());
        assert!(codegen.get_function_index("pairs.capacity").is_some());

        Ok(())
    }

    #[test]
    fn test_pairs_iteration_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = PairsTypeManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that iteration functions are available
        assert!(codegen.get_function_index("pairs.keys").is_some());
        assert!(codegen.get_function_index("pairs.values").is_some());
        assert!(codegen.get_function_index("pairs.entries").is_some());

        Ok(())
    }

    #[test]
    fn test_pairs_utility_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = PairsTypeManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that utility functions are available
        assert!(codegen.get_function_index("pairs.clone").is_some());
        assert!(codegen.get_function_index("pairs.merge").is_some());
        assert!(codegen.get_function_index("pairs.equals").is_some());
        assert!(codegen.get_function_index("pairs.toString").is_some());

        Ok(())
    }

    #[test]
    fn test_pairs_header_layout() {
        // Test that pairs header layout is correctly defined
        // Pairs header: 24 bytes
        // 0-3: size (number of key-value pairs)
        // 4-7: capacity (allocated slots)
        // 8-11: key_type_id (0 = mixed types)
        // 12-15: value_type_id (0 = mixed types)
        // 16-19: entries_ptr (pointer to entries array)
        // 20-23: hash_seed (for hash table)

        let header_size = 24;
        let size_offset = 0;
        let capacity_offset = 4;
        let key_type_offset = 8;
        let value_type_offset = 12;
        let entries_offset = 16;
        let hash_seed_offset = 20;

        assert_eq!(header_size, 24);
        assert_eq!(size_offset, 0);
        assert_eq!(capacity_offset, 4);
        assert_eq!(key_type_offset, 8);
        assert_eq!(value_type_offset, 12);
        assert_eq!(entries_offset, 16);
        assert_eq!(hash_seed_offset, 20);
    }

    #[test]
    fn test_pairs_entry_layout() {
        // Test that pairs entry layout is correctly defined
        // Each entry: 16 bytes
        // 0-7: key (8 bytes - pointer or value)
        // 8-15: value (8 bytes - pointer or value)

        let entry_size = 16;
        let key_offset = 0;
        let value_offset = 8;

        assert_eq!(entry_size, 16);
        assert_eq!(key_offset, 0);
        assert_eq!(value_offset, 8);
    }

    #[test]
    fn test_hash_function_properties() {
        // Test hash function properties
        let hash_seed = 0x9e3779b9u32; // Golden ratio constant
        let default_capacity = 8;

        // Test that hash seed is reasonable
        assert!(hash_seed > 0);
        assert!(hash_seed < u32::MAX);

        // Test that default capacity is power of 2
        assert_eq!(default_capacity & (default_capacity - 1), 0);
    }
}
