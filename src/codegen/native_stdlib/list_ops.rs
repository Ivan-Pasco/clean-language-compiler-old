//! Native WASM list append used by list literals.
//!
//! Lists use the standard layout
//! `[size: i32][capacity: i32][type_id: i32][flags: i32][elements...]`
//! (16-byte header, elements start at offset 16). The list method
//! implementations live in `src/stdlib/list_class.rs` and
//! `src/stdlib/list_behavior.rs`.

use wasm_encoder::{Instruction, MemArg};

use super::{LIST_DATA_OFFSET, LIST_ELEMENT_SIZE_I32, LIST_LENGTH_OFFSET};

/// Append an i32 element in place, without a capacity check.
///
/// Only list literals use this: `list.allocate` sizes their capacity up front.
/// Growing appends go through `list.add`, which moves a full list.
///
/// Parameters: local 0 = list_ptr, local 1 = value. Returns the same list_ptr.
pub fn gen_push_i32_inplace() -> Vec<Instruction<'static>> {
    let size = MemArg {
        offset: LIST_LENGTH_OFFSET as u64,
        align: 2,
        memory_index: 0,
    };
    vec![
        // list_ptr + 16 + size * 4
        Instruction::LocalGet(0),
        Instruction::I32Load(size),
        Instruction::I32Const(LIST_ELEMENT_SIZE_I32 as i32),
        Instruction::I32Mul,
        Instruction::I32Const(LIST_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Add,
        Instruction::LocalGet(1),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // size += 1
        Instruction::LocalGet(0),
        Instruction::LocalGet(0),
        Instruction::I32Load(size),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::I32Store(size),
        Instruction::LocalGet(0),
    ]
}
