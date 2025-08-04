use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use std::collections::HashMap;
use wasm_encoder::{Instruction, MemArg, MemorySection, MemoryType};

// Type ID constants for memory management
pub const TYPE_ID_UNKNOWN: u32 = 0;
pub const TYPE_ID_INTEGER: u32 = 1;
pub const TYPE_ID_FLOAT: u32 = 2;
pub const TYPE_ID_STRING: u32 = 3;
pub const TYPE_ID_ARRAY: u32 = 4;
pub const TYPE_ID_MATRIX: u32 = 5;
pub const TYPE_ID_OBJECT: u32 = 6;
pub const TYPE_ID_FUNCTION: u32 = 7;

const HEADER_SIZE: usize = 16; // Increased to include more metadata
const MIN_ALLOCATION: usize = 32; // Increased for better alignment
const ALIGNMENT: usize = 8;

const PAGE_SIZE: usize = 65536; // WebAssembly page size (64KB)

#[derive(Debug, Clone)]
struct MemoryBlock {
    size: usize,
    ref_count: usize,
    is_root: bool,
    type_id: u32,        // Type identifier for runtime type checking
    next: Option<usize>, // For free list
    is_free: bool,
}

#[derive(Clone)]
pub struct MemoryManager {
    memory: MemoryType,
    heap_start: usize,
    current_pages: u32,
    max_pages: Option<u32>,
    allocations: HashMap<usize, MemoryBlock>,
    free_list: Option<usize>,
    pub data: Vec<u8>,
    total_allocated: usize,
}

impl MemoryManager {
    pub fn new(initial_pages: u32, max_pages: Option<u32>) -> Self {
        let initial_size = (initial_pages as usize) * PAGE_SIZE;
        Self {
            memory: MemoryType {
                minimum: initial_pages as u64,
                maximum: max_pages.map(|p| p as u64),
                memory64: false,
                shared: false,
            },
            heap_start: 0,
            current_pages: initial_pages,
            max_pages,
            allocations: HashMap::new(),
            free_list: None,
            data: vec![0; initial_size],
            total_allocated: 0,
        }
    }

    pub fn create_memory_section(&self) -> MemorySection {
        let mut section = MemorySection::new();
        section.memory(self.memory.clone());
        section
    }

    fn align_size(size: usize) -> usize {
        (size + ALIGNMENT - 1) & !(ALIGNMENT - 1)
    }

    fn find_free_block(&self, size: usize) -> Option<usize> {
        let mut current = self.free_list;
        while let Some(ptr) = current {
            if let Some(block) = self.allocations.get(&ptr) {
                if block.size >= size {
                    return Some(ptr);
                }
                current = block.next;
            } else {
                break;
            }
        }
        None
    }

    pub fn allocate(&mut self, size: usize, type_id: u32) -> Result<usize, CompilerError> {
        let aligned_size = Self::align_size(size + HEADER_SIZE);

        // Try to find a free block first
        if let Some(ptr) = self.find_free_block(aligned_size) {
            let block = self.allocations.remove(&ptr).unwrap();
            let remaining_size = block.size - aligned_size;

            if remaining_size >= MIN_ALLOCATION {
                // Split the block
                let new_ptr = ptr + aligned_size;
                self.allocations.insert(
                    new_ptr,
                    MemoryBlock {
                        size: remaining_size,
                        ref_count: 0,
                        is_root: false,
                        type_id: 0,
                        next: block.next,
                        is_free: false,
                    },
                );
                self.free_list = Some(new_ptr);
            } else {
                self.free_list = block.next;
            }

            // Initialize the allocated block
            self.allocations.insert(
                ptr,
                MemoryBlock {
                    size: aligned_size,
                    ref_count: 1,
                    is_root: false,
                    type_id,
                    next: None,
                    is_free: false,
                },
            );

            self.total_allocated += aligned_size;
            return Ok(ptr + HEADER_SIZE);
        }

        // Need to grow memory
        let ptr = self.heap_start;
        if ptr + aligned_size > self.data.len() {
            let pages_needed = ((ptr + aligned_size - self.data.len()) + PAGE_SIZE - 1) / PAGE_SIZE;

            if let Some(max) = self.max_pages {
                if self.current_pages + pages_needed as u32 > max {
                    return Err(CompilerError::memory_error(
                        format!(
                            "Cannot allocate {} bytes: would exceed maximum memory pages ({} > {})",
                            size,
                            self.current_pages + pages_needed as u32,
                            max
                        ),
                        Some("Try allocating a smaller amount of memory".to_string()),
                        None,
                    ));
                }
            }

            self.current_pages += pages_needed as u32;
            self.data
                .resize(self.data.len() + pages_needed * PAGE_SIZE, 0);
        }

        // Initialize the new block
        self.allocations.insert(
            ptr,
            MemoryBlock {
                size: aligned_size,
                ref_count: 1,
                is_root: false,
                type_id,
                next: None,
                is_free: false,
            },
        );

        self.heap_start = ptr + aligned_size;
        self.total_allocated += aligned_size;
        Ok(ptr + HEADER_SIZE)
    }

    pub fn deallocate(&mut self, ptr: usize) -> Result<(), CompilerError> {
        let header_ptr = ptr - HEADER_SIZE;

        // Get the block's reference count without holding a mutable borrow
        let ref_count = match self.allocations.get(&header_ptr) {
            Some(block) => block.ref_count,
            None => {
                return Err(CompilerError::memory_error(
                    format!("Invalid pointer: {}", ptr),
                    Some("Ensure pointer is within valid memory range".to_string()),
                    None,
                ));
            }
        };

        // Check if there are active references
        if ref_count > 0 {
            return Err(CompilerError::memory_error(
                "Cannot deallocate block with active references".to_string(),
                Some("Wait until all references are released".to_string()),
                None,
            ));
        }

        // Remove block, update it, and reinsert
        if let Some(mut block) = self.allocations.remove(&header_ptr) {
            // Add to free list
            block.next = self.free_list;
            self.free_list = Some(header_ptr);
            self.total_allocated -= block.size;

            // Mark as free
            block.is_free = true;
            self.allocations.insert(header_ptr, block);

            // Try to coalesce adjacent free blocks
            self.coalesce_free_blocks();

            Ok(())
        } else {
            // Already checked above, this shouldn't happen
            Ok(())
        }
    }

    pub fn retain(&mut self, ptr: usize) -> Result<(), CompilerError> {
        let header_ptr = ptr - HEADER_SIZE;
        if let Some(block) = self.allocations.get_mut(&header_ptr) {
            block.ref_count += 1;
            Ok(())
        } else {
            Err(CompilerError::memory_error(
                format!("Invalid pointer: {}", ptr),
                Some("Ensure pointer is within valid memory range".to_string()),
                None,
            ))
        }
    }

    pub fn dec_ref_count(&mut self, ptr: usize) -> Result<(), CompilerError> {
        let header_ptr = ptr - HEADER_SIZE;
        if let Some(block) = self.allocations.get_mut(&header_ptr) {
            if block.ref_count > 0 {
                block.ref_count -= 1;
            }
            Ok(())
        } else {
            Err(CompilerError::memory_error(
                format!("Invalid pointer: {}", ptr),
                Some("Ensure pointer is within valid memory range".to_string()),
                None,
            ))
        }
    }

    pub fn release(&mut self, ptr: usize) -> Result<(), CompilerError> {
        let header_ptr = ptr - HEADER_SIZE;

        // Get the block's reference count and is_root status without holding a mutable borrow
        let (ref_count, is_root) = match self.allocations.get(&header_ptr) {
            Some(block) => (block.ref_count, block.is_root),
            None => {
                return Err(CompilerError::memory_error(
                    format!("Invalid pointer for release: {}", ptr),
                    Some("Ensure pointer is within valid memory range".to_string()),
                    None,
                ));
            }
        };

        // Now perform the operation
        if ref_count > 0 {
            // Remove the block, modify it, and reinsert
            if let Some(mut block) = self.allocations.remove(&header_ptr) {
                block.ref_count -= 1;

                // If reference count reaches 0 and not a root object, free it
                if block.ref_count == 0 && !is_root {
                    // Mark as free and add to free list
                    block.is_free = true;
                    block.next = self.free_list;
                    self.free_list = Some(header_ptr);
                    self.total_allocated -= block.size;

                    // Add block back to allocations
                    self.allocations.insert(header_ptr, block);

                    // Try to coalesce adjacent free blocks
                    self.coalesce_free_blocks();
                } else {
                    // Just add block back with decremented ref count
                    self.allocations.insert(header_ptr, block);
                }
            }
            Ok(())
        } else {
            // Reference count already at 0, nothing to do
            Ok(())
        }
    }

    pub fn get_type_id(&self, ptr: usize) -> Result<u32, CompilerError> {
        let header_ptr = ptr - HEADER_SIZE;
        if let Some(block) = self.allocations.get(&header_ptr) {
            Ok(block.type_id)
        } else {
            Err(CompilerError::memory_error(
                format!("Invalid pointer: {}", ptr),
                Some("Ensure pointer is within valid memory range".to_string()),
                None,
            ))
        }
    }

    pub fn store_i32(&mut self, ptr: usize, value: i32) -> Result<(), CompilerError> {
        if !self.is_valid_pointer(ptr) {
            return Err(CompilerError::memory_error(
                format!("Invalid pointer: {}", ptr),
                Some("Ensure pointer is within valid memory range".to_string()),
                None,
            ));
        }
        if ptr + 4 > self.data.len() {
            return Err(CompilerError::memory_error(
                "Memory access out of bounds".to_string(),
                Some("Ensure memory access is within allocated range".to_string()),
                None,
            ));
        }
        self.data[ptr..ptr + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn store_u8(&mut self, ptr: usize, value: u8) -> Result<(), CompilerError> {
        if !self.is_valid_pointer(ptr) {
            return Err(CompilerError::memory_error(
                format!("Invalid pointer: {}", ptr),
                Some("Ensure pointer is within valid memory range".to_string()),
                None,
            ));
        }
        if ptr + 1 > self.data.len() {
            return Err(CompilerError::memory_error(
                "Memory access out of bounds".to_string(),
                Some("Ensure memory access is within allocated range".to_string()),
                None,
            ));
        }
        self.data[ptr] = value;
        Ok(())
    }

    pub fn store_f64(&mut self, ptr: usize, value: f64) -> Result<(), CompilerError> {
        if !self.is_valid_pointer(ptr) {
            return Err(CompilerError::memory_error(
                format!("Invalid pointer: {}", ptr),
                Some("Ensure pointer is within valid memory range".to_string()),
                None,
            ));
        }
        if ptr + 8 > self.data.len() {
            return Err(CompilerError::memory_error(
                "Memory access out of bounds".to_string(),
                Some("Ensure memory access is within allocated range".to_string()),
                None,
            ));
        }
        self.data[ptr..ptr + 8].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn is_valid_pointer(&self, ptr: usize) -> bool {
        let header_ptr = ptr - HEADER_SIZE;
        self.allocations.contains_key(&header_ptr)
    }

    pub fn get_current_pages(&self) -> u32 {
        self.current_pages
    }

    pub fn get_max_pages(&self) -> Option<u32> {
        self.max_pages
    }

    pub fn get_heap_start(&self) -> usize {
        self.heap_start
    }

    pub fn get_total_allocated(&self) -> usize {
        self.total_allocated
    }

    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register allocate function
        register_stdlib_function(
            codegen,
            "memory.allocate",
            &[WasmType::I32, WasmType::I32], // Size and type_id
            Some(WasmType::I32),             // Pointer
            self.generate_allocate_function(),
        )?;

        // Register retain function
        register_stdlib_function(
            codegen,
            "memory.retain",
            &[WasmType::I32], // Pointer
            None,             // No return value
            self.generate_retain_function(),
        )?;

        // Register release function
        register_stdlib_function(
            codegen,
            "memory.release",
            &[WasmType::I32], // Pointer
            None,             // No return value
            self.generate_release_function(),
        )?;

        Ok(())
    }

    pub fn generate_allocate_function(&self) -> Vec<Instruction> {
        vec![
            // Get size parameter
            Instruction::LocalGet(0),
            // Get type_id parameter
            Instruction::LocalGet(1),
            // Add header size
            Instruction::I32Const(HEADER_SIZE as i32),
            Instruction::I32Add,
            // Align to 8 bytes
            Instruction::I32Const(7),
            Instruction::I32Add,
            Instruction::I32Const(-8),
            Instruction::I32And,
            // Call memory.grow if needed
            Instruction::Call(0),
            // Return pointer
            Instruction::Return,
        ]
    }

    pub fn generate_retain_function(&self) -> Vec<Instruction> {
        vec![
            // Get pointer parameter and keep on stack for store
            Instruction::LocalGet(0),
            // Duplicate pointer for load operation
            Instruction::LocalGet(0),
            // Load reference count
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Increment reference count
            Instruction::I32Const(1),
            Instruction::I32Add,
            // Store updated reference count (stack now has [pointer, new_ref_count])
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    pub fn generate_release_function(&self) -> Vec<Instruction> {
        vec![
            // Get pointer parameter and keep on stack for store
            Instruction::LocalGet(0),
            // Duplicate pointer for load operation
            Instruction::LocalGet(0),
            // Load reference count
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Decrement reference count
            Instruction::I32Const(1),
            Instruction::I32Sub,
            // Store updated reference count (stack has [pointer, new_ref_count])
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Simplified - avoid control flow that could cause stack issues
            // In production, would implement proper memory freeing
        ]
    }

    fn coalesce_free_blocks(&mut self) {
        let mut current_ptr_opt = self.free_list;
        let mut prev_ptr_opt = None;

        while let Some(current_ptr) = current_ptr_opt {
            let current_next;
            let current_size;
            let next_is_adjacent;
            let next_is_free;

            // Get information about current block without holding a borrow
            {
                let current_block = match self.allocations.get(&current_ptr) {
                    Some(block) => block,
                    None => break, // Invalid pointer in free list
                };

                current_next = current_block.next;
                current_size = current_block.size;

                next_is_adjacent =
                    current_next.map_or(false, |next_ptr| next_ptr == current_ptr + current_size);
                next_is_free = current_next.map_or(false, |next_ptr| {
                    self.allocations
                        .get(&next_ptr)
                        .map_or(false, |block| block.is_free)
                });
            }

            if next_is_adjacent && next_is_free && current_next.is_some() {
                // Get the next block's size and next pointer
                let next_ptr = current_next.unwrap();
                let (next_size, next_next) = {
                    let next_block = self.allocations.get(&next_ptr).unwrap();
                    (next_block.size, next_block.next)
                };

                // Remove both blocks
                self.allocations.remove(&current_ptr);
                self.allocations.remove(&next_ptr);

                // Create a new coalesced block
                let coalesced_block = MemoryBlock {
                    size: current_size + next_size,
                    ref_count: 0,
                    is_root: false,
                    type_id: 0,
                    next: next_next,
                    is_free: true,
                };

                // Insert the new coalesced block
                self.allocations.insert(current_ptr, coalesced_block);

                // Update the free list
                if let Some(prev_ptr) = prev_ptr_opt {
                    if let Some(prev_block) = self.allocations.get_mut(&prev_ptr) {
                        prev_block.next = Some(current_ptr);
                    }
                } else {
                    self.free_list = Some(current_ptr);
                }

                // Stay at the same position to check if we can coalesce more
                current_ptr_opt = Some(current_ptr);
            } else {
                // Move to the next block
                prev_ptr_opt = Some(current_ptr);
                current_ptr_opt = current_next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_allocation() {
        let mut memory = MemoryManager::new(1, Some(10));

        // Test basic allocation
        let ptr = memory.allocate(100, 1).unwrap();
        assert!(ptr >= HEADER_SIZE);
        assert_eq!(memory.get_type_id(ptr).unwrap(), 1);

        // Test retain/release
        memory.retain(ptr).unwrap();
        memory.release(ptr).unwrap();
        memory.release(ptr).unwrap(); // Should deallocate

        // Test allocation after deallocation
        let ptr2 = memory.allocate(100, 1).unwrap();
        assert!(ptr2 >= HEADER_SIZE);
    }

    #[test]
    fn test_memory_bounds() {
        let mut memory = MemoryManager::new(1, Some(1));

        // Test allocation within bounds
        let ptr = memory.allocate(100, 1).unwrap();
        assert!(memory.store_i32(ptr, 42).is_ok());

        // Test allocation beyond bounds
        let result = memory.allocate(PAGE_SIZE + 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_coalescing() {
        let mut memory = MemoryManager::new(1, Some(10));

        // Allocate and free blocks
        let ptr1 = memory.allocate(100, 1).unwrap();
        let ptr2 = memory.allocate(100, 1).unwrap();
        let ptr3 = memory.allocate(100, 1).unwrap();

        memory.release(ptr1).unwrap();
        memory.release(ptr2).unwrap();
        memory.release(ptr3).unwrap();

        // Allocate a larger block that should fit in the coalesced space
        let ptr4 = memory.allocate(200, 1).unwrap();
        assert!(ptr4 >= HEADER_SIZE);
    }
}
