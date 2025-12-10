//! Native WASM memory allocation functions
//!
//! Provides a simple bump allocator and memory copy operations
//! that run entirely in WASM without host imports.

use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

use super::{ALIGNMENT, HEAP_PTR_GLOBAL};

/// Generate instructions for native malloc (bump allocator)
///
/// Parameters:
///   - local 0: size (i32) - requested allocation size
///
/// Returns:
///   - i32: pointer to allocated memory
///
/// Algorithm:
///   1. Read current heap pointer from global 0
///   2. Align the requested size to 8 bytes
///   3. Calculate new heap pointer
///   4. Store new heap pointer
///   5. Return original pointer
pub fn gen_malloc() -> Vec<Instruction<'static>> {
    vec![
        // local 0: size (parameter)
        // local 1: current_ptr (to be allocated)
        // local 2: aligned_size

        // Read current heap pointer -> save to local 1
        Instruction::GlobalGet(HEAP_PTR_GLOBAL),
        Instruction::LocalSet(1),
        // Calculate aligned size: (size + 7) & ~7 -> local 2
        Instruction::LocalGet(0),                      // size
        Instruction::I32Const((ALIGNMENT - 1) as i32), // 7
        Instruction::I32Add,
        Instruction::I32Const(!(ALIGNMENT - 1) as i32), // 0xFFFFFFF8
        Instruction::I32And,
        Instruction::LocalSet(2), // aligned_size
        // Calculate new heap pointer: current_ptr + aligned_size
        Instruction::LocalGet(1), // current_ptr
        Instruction::LocalGet(2), // aligned_size
        Instruction::I32Add,
        // Store new heap pointer
        Instruction::GlobalSet(HEAP_PTR_GLOBAL),
        // Return original pointer
        Instruction::LocalGet(1),
    ]
}

/// Generate instructions for native memcpy
///
/// Parameters:
///   - local 0: dest (i32) - destination pointer
///   - local 1: src (i32) - source pointer
///   - local 2: len (i32) - number of bytes to copy
///
/// Returns: void
///
/// Algorithm:
///   Loop from i=0 to len, copying one byte at a time
pub fn gen_memcpy() -> Vec<Instruction<'static>> {
    vec![
        // local 0: dest
        // local 1: src
        // local 2: len
        // local 3: i (loop counter)

        // Initialize i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        // Block for loop exit
        Instruction::Block(BlockType::Empty),
        // Loop body
        Instruction::Loop(BlockType::Empty),
        // Check if i >= len
        Instruction::LocalGet(3), // i
        Instruction::LocalGet(2), // len
        Instruction::I32GeU,
        Instruction::BrIf(1), // Exit block if i >= len
        // Copy one byte: dest[i] = src[i]
        // Calculate dest + i
        Instruction::LocalGet(0), // dest
        Instruction::LocalGet(3), // i
        Instruction::I32Add,
        // Load byte from src + i
        Instruction::LocalGet(1), // src
        Instruction::LocalGet(3), // i
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // Store byte to dest + i
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        // Continue loop
        Instruction::Br(0),
        Instruction::End, // End loop
        Instruction::End, // End block
    ]
}

/// Get the local variables needed for malloc
pub fn malloc_locals() -> Vec<(u32, ValType)> {
    vec![
        (1, ValType::I32), // current_ptr
        (1, ValType::I32), // aligned_size
    ]
}

/// Get the local variables needed for memcpy
pub fn memcpy_locals() -> Vec<(u32, ValType)> {
    vec![
        (1, ValType::I32), // i (loop counter)
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_malloc_instructions_generated() {
        let instructions = gen_malloc();
        assert!(!instructions.is_empty());
        // Should start with GlobalGet to read heap pointer
        assert!(matches!(instructions[0], Instruction::GlobalGet(0)));
    }

    #[test]
    fn test_memcpy_instructions_generated() {
        let instructions = gen_memcpy();
        assert!(!instructions.is_empty());
        // Should include loop instructions
        let has_loop = instructions
            .iter()
            .any(|i| matches!(i, Instruction::Loop(_)));
        assert!(has_loop);
    }
}
