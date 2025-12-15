//! Native WASM list/array operations
//!
//! Provides pure WASM bytecode implementations of list functions
//! that operate on the standard Clean Language list format:
//! [size: i32][capacity: i32][type_id: i32][padding: i32][elements...]
//! (16-byte header, elements start at offset 16)

use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

use super::{
    LIST_CAPACITY_OFFSET, LIST_DATA_OFFSET, LIST_ELEMENT_SIZE_F64, LIST_ELEMENT_SIZE_I32,
    LIST_LENGTH_OFFSET, LIST_TYPE_ID_OFFSET,
};

/// Generate instructions for list.length
pub fn gen_length() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Load(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for list.get (i32 elements)
pub fn gen_get_i32() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + (index * 4)
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // Load i32 value
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for list.get (f64 elements)
pub fn gen_get_f64() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + (index * 8)
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Const(LIST_ELEMENT_SIZE_F64 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // Load f64 value
        Instruction::F64Load(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for list.set (i32 elements)
/// Returns the list pointer for chaining
pub fn gen_set_i32() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + (index * 4)
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // Store value
        Instruction::LocalGet(2), // value
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Return list pointer for chaining
        Instruction::LocalGet(0),
    ]
}

/// Generate instructions for list.set (f64 elements)
pub fn gen_set_f64() -> Vec<Instruction<'static>> {
    vec![
        // Calculate address: ptr + 4 + (index * 8)
        Instruction::LocalGet(0), // list pointer
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // index
        Instruction::I32Const(LIST_ELEMENT_SIZE_F64 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        // Store value
        Instruction::LocalGet(2), // value (f64)
        Instruction::F64Store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }),
        // Return list pointer for chaining
        Instruction::LocalGet(0),
    ]
}

/// Generate instructions for list.indexOf (i32 elements)
///
/// Parameters:
///   - local 0: list_ptr
///   - local 1: value (to search for)
///
/// Returns: i32 (index of first occurrence, or -1 if not found)
///
/// Uses locals:
///   - local 2: len
///   - local 3: i (loop counter)
pub fn gen_index_of_i32() -> Vec<Instruction<'static>> {
    vec![
        // Get length -> local 2
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2),
        // Initialize i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // Loop to find element
        Instruction::Block(BlockType::Result(ValType::I32)),
        Instruction::Loop(BlockType::Empty),
        // If i >= len, not found -> return -1
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32GeU,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(-1),
        Instruction::Br(2),
        Instruction::End,
        // Load list[i]
        Instruction::LocalGet(0),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
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
        Instruction::LocalGet(3), // Found -> return i
        Instruction::Br(2),
        Instruction::End,
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,          // End loop
        Instruction::I32Const(-1), // Default: not found
        Instruction::End,          // End block
    ]
}

/// Generate instructions for list.contains (i32 elements)
/// Returns 1 if indexOf >= 0, else 0
pub fn gen_contains_i32(index_of_func: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
        Instruction::Call(index_of_func),
        Instruction::I32Const(0),
        Instruction::I32GeS,
    ]
}

/// Generate instructions for list.push (i32 elements, requires malloc)
///
/// This creates a new list with the element appended.
/// New list has 16-byte header: [size|capacity|type_id|padding|elements...]
///
/// Parameters:
///   - local 0: list_ptr
///   - local 1: value
///
/// Returns: i32 (pointer to new list)
///
/// Uses locals:
///   - local 2: old_len
///   - local 3: new_ptr
///   - local 4: i (loop counter)
pub fn gen_push_i32(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get old length -> local 2
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2),
        // Allocate new list: malloc(16 + (old_len + 1) * 4)
        // 16-byte header + space for elements
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Const(LIST_DATA_OFFSET as i32), // 16-byte header
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(3), // new_ptr
        // Store new length (old_len + 1) at offset 0
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::I32Store(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy capacity from old list (offset 4)
        Instruction::LocalGet(3),
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: LIST_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy type_id from old list (offset 8)
        Instruction::LocalGet(3),
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_TYPE_ID_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: LIST_TYPE_ID_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy old elements (from offset 16)
        Instruction::I32Const(0),
        Instruction::LocalSet(4), // i = 0
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= old_len, exit
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[16 + i*4] = list_ptr[16 + i*4]
        Instruction::LocalGet(3),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(4),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Store new element at index old_len (offset 16 + old_len * 4)
        Instruction::LocalGet(3),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(2), // old_len
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalGet(1), // value
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Return new pointer
        Instruction::LocalGet(3),
    ]
}

/// Generate instructions for list.pop (i32 elements)
/// Returns the last element (does not modify the list)
pub fn gen_pop_i32() -> Vec<Instruction<'static>> {
    vec![
        // Get length
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Save to local 1
        Instruction::LocalSet(1),
        // Load element at index (len - 1)
        Instruction::LocalGet(0),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
    ]
}

/// Generate instructions for list.slice (i32 elements, requires malloc)
///
/// Parameters:
///   - local 0: list_ptr
///   - local 1: start
///   - local 2: end
///
/// Returns: i32 (pointer to new sliced list)
///
/// Uses locals:
///   - local 3: new_len
///   - local 4: new_ptr
///   - local 5: i (loop counter)
pub fn gen_slice_i32(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Calculate new_len = end - start -> local 3
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Sub,
        Instruction::LocalSet(3),
        // Allocate new list: malloc(4 + new_len * 4)
        Instruction::LocalGet(3),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(4),
        // Store new length at offset 0
        Instruction::LocalGet(4),
        Instruction::LocalGet(3),
        Instruction::I32Store(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy capacity from source list at offset 4
        Instruction::LocalGet(4),
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: LIST_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy type_id from source list at offset 8
        Instruction::LocalGet(4),
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_TYPE_ID_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: LIST_TYPE_ID_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy elements
        Instruction::I32Const(0),
        Instruction::LocalSet(5), // i = 0
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= new_len, exit
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + i*4] = list_ptr[4 + (start+i)*4]
        Instruction::LocalGet(4),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(5),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // start
        Instruction::LocalGet(5), // i
        Instruction::I32Add,
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new pointer
        Instruction::LocalGet(4),
    ]
}

/// Generate instructions for list.reverse (i32 elements, requires malloc)
///
/// Parameters:
///   - local 0: list_ptr
///
/// Returns: i32 (pointer to new reversed list)
///
/// Uses locals:
///   - local 1: len
///   - local 2: new_ptr
///   - local 3: i (loop counter)
pub fn gen_reverse_i32(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Get length -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        // Allocate new list: malloc(16 + len * 4) for 16-byte header
        Instruction::LocalGet(1),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalSet(2),
        // Store length at offset 0
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Store(MemArg {
            offset: LIST_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy capacity from source list at offset 4
        Instruction::LocalGet(2),
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: LIST_CAPACITY_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy type_id from source list at offset 8
        Instruction::LocalGet(2),
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: LIST_TYPE_ID_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: LIST_TYPE_ID_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        // Copy elements in reverse
        Instruction::I32Const(0),
        Instruction::LocalSet(3), // i = 0
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= len, exit
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_ptr[4 + i*4] = list_ptr[4 + (len-1-i)*4]
        Instruction::LocalGet(2),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(1), // len
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalGet(3), // i
        Instruction::I32Sub,
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new pointer
        Instruction::LocalGet(2),
    ]
}

/// Get local variables needed for each list function
pub fn get_locals(func_name: &str) -> Vec<(u32, ValType)> {
    match func_name {
        "index_of_i32" => vec![
            (1, ValType::I32), // len
            (1, ValType::I32), // i
        ],
        "push_i32" => vec![
            (1, ValType::I32), // old_len
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
        ],
        "pop_i32" => vec![
            (1, ValType::I32), // len
        ],
        "slice_i32" => vec![
            (1, ValType::I32), // new_len
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
        ],
        "reverse_i32" => vec![
            (1, ValType::I32), // len
            (1, ValType::I32), // new_ptr
            (1, ValType::I32), // i
        ],
        _ => vec![],
    }
}
