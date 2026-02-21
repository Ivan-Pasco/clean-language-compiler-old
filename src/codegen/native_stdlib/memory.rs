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
///   4. Grow memory if new_ptr exceeds current memory size
///   5. Store new heap pointer
///   6. Return original pointer
pub fn gen_malloc() -> Vec<Instruction<'static>> {
    vec![
        // local 0: size (parameter)
        // local 1: current_ptr (to be allocated)
        // local 2: aligned_size
        // local 3: new_ptr (used for memory.grow check)

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
        // Calculate new heap pointer: current_ptr + aligned_size -> local 3
        Instruction::LocalGet(1), // current_ptr
        Instruction::LocalGet(2), // aligned_size
        Instruction::I32Add,
        Instruction::LocalSet(3), // new_ptr
        // Grow memory if new_ptr exceeds current memory size.
        // memory.size returns pages (64KB each), so current_bytes = pages * 65536.
        // If new_ptr > current_bytes, grow by enough pages.
        Instruction::LocalGet(3),   // new_ptr
        Instruction::MemorySize(0), // current pages
        Instruction::I32Const(16),  // shift left by 16 = multiply by 65536
        Instruction::I32Shl,        // current_bytes
        Instruction::I32GtU,        // new_ptr > current_bytes?
        Instruction::If(BlockType::Empty),
        // Need to grow: pages_needed = (new_ptr - current_bytes + 65535) / 65536
        // Simplified: grow by (new_ptr >> 16) - memory.size + 1 pages (at least 1)
        Instruction::LocalGet(3), // new_ptr
        Instruction::I32Const(16),
        Instruction::I32ShrU,       // new_ptr / 65536
        Instruction::MemorySize(0), // current pages
        Instruction::I32Sub,        // pages above current
        Instruction::I32Const(1),
        Instruction::I32Add,        // +1 to ensure enough
        Instruction::MemoryGrow(0), // grow memory, returns old size or -1
        Instruction::Drop,          // ignore result (we proceed regardless)
        Instruction::End,
        // Store new heap pointer
        Instruction::LocalGet(3),
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
        (1, ValType::I32), // new_ptr (for memory.grow check)
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
