//! Module for memory operations during code generation.

use crate::ast::Value;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_encoder::{ConstExpr, DataSection, Instruction, ValType};

// Essential constants
pub const HEADER_SIZE: u32 = 16; // 16-byte header for memory blocks
pub const ALIGNMENT: usize = 8;

// Memory type IDs (keep only the ones actually used)
pub const STRING_TYPE_ID: u32 = 3;
pub const ARRAY_TYPE_ID: u32 = 4;
pub const MATRIX_TYPE_ID: u32 = 5;

// Memory pool sizes for efficient allocation
const SMALL_POOL_SIZE: usize = 64;
const MEDIUM_POOL_SIZE: usize = 256;
const LARGE_POOL_SIZE: usize = 1024;

/// Memory block header layout in WASM memory
/// Offset 0-3: Size (u32)
/// Offset 4-7: Reference count (u32)
/// Offset 8-11: Type ID (u32)
/// Offset 12-15: Next free block pointer (u32, 0 if not free)
#[derive(Debug, Clone)]
pub struct MemoryBlock {
    #[allow(dead_code)]
    pub address: usize,
    pub size: usize,
    pub is_free: bool,
    pub type_id: u32,
    pub ref_count: usize,
    pub next_free: Option<usize>,
}

/// Memory pool for size-segregated allocation
#[derive(Debug, Clone)]
struct MemoryPool {
    block_size: usize,
    free_blocks: Vec<usize>,
    total_blocks: usize,
}

impl MemoryPool {
    fn new(block_size: usize) -> Self {
        Self {
            block_size,
            free_blocks: Vec::new(),
            total_blocks: 0,
        }
    }

    fn allocate(&mut self, _heap_start: usize, current_address: &mut usize) -> Option<usize> {
        if let Some(address) = self.free_blocks.pop() {
            Some(address)
        } else {
            // Allocate new block
            let address = *current_address;
            *current_address += self.block_size;
            self.total_blocks += 1;
            Some(address)
        }
    }

    fn deallocate(&mut self, address: usize) {
        self.free_blocks.push(address);
    }
}

/// Enhanced memory management utilities with ARC and memory pools
pub(crate) struct MemoryUtils {
    data_section: DataSection,
    heap_start: usize,
    current_address: usize,
    memory_blocks: HashMap<usize, MemoryBlock>,
    free_blocks: Vec<usize>,

    // Memory pools for efficient allocation
    small_pool: MemoryPool,  // <= 64 bytes
    medium_pool: MemoryPool, // <= 256 bytes
    large_pool: MemoryPool,  // <= 1024 bytes

    // ARC tracking
    root_objects: Vec<usize>, // Objects that should never be collected
    gc_threshold: usize,      // Trigger GC when this many objects are allocated
    allocated_objects: usize,

    // String pool for deduplication
    string_pool: HashMap<String, usize>,

    // Shared memory manager for stdlib integration
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl MemoryUtils {
    /// Create a new MemoryUtils instance with memory pools
    pub(crate) fn new(heap_start: usize) -> Self {
        // Create a shared memory manager for stdlib integration
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, None)));

        Self {
            data_section: DataSection::new(),
            heap_start,
            current_address: heap_start,
            memory_blocks: HashMap::new(),
            free_blocks: Vec::new(),
            small_pool: MemoryPool::new(SMALL_POOL_SIZE),
            medium_pool: MemoryPool::new(MEDIUM_POOL_SIZE),
            large_pool: MemoryPool::new(LARGE_POOL_SIZE),
            root_objects: Vec::new(),
            gc_threshold: 1000,
            allocated_objects: 0,
            string_pool: HashMap::new(),
            memory_manager,
        }
    }

    /// Add a data segment
    pub(crate) fn add_data_segment(&mut self, offset: u32, data: &[u8]) {
        let offset_expr = ConstExpr::i32_const(offset as i32);
        let data_vec: Vec<u8> = data.to_vec();
        self.data_section.active(0, &offset_expr, data_vec);
    }

    /// Add string data to the data section with proper memory layout
    #[allow(dead_code)]
    pub(crate) fn add_string_data(&mut self, data: &[u8]) -> u32 {
        let string_content = std::str::from_utf8(data).unwrap_or("");

        // Check if string already exists in pool
        if let Some(&existing_ptr) = self.string_pool.get(string_content) {
            // Increment reference count for existing string
            if let Some(block) = self.memory_blocks.get_mut(&existing_ptr) {
                block.ref_count += 1;
            }
            return existing_ptr as u32;
        }

        // Allocate new string with proper layout
        let string_len = data.len();
        let total_size = HEADER_SIZE as usize + 4 + string_len; // header + length + content

        let address = match self.allocate_from_pool(total_size, STRING_TYPE_ID) {
            Ok(addr) => addr,
            Err(_) => {
                // Fallback to direct allocation
                let addr = self.current_address;
                self.current_address += Self::align_size(total_size);
                addr
            }
        };

        // Create memory block
        let block = MemoryBlock {
            address,
            size: total_size,
            is_free: false,
            type_id: STRING_TYPE_ID,
            ref_count: 1,
            next_free: None,
        };
        self.memory_blocks.insert(address, block);
        self.allocated_objects += 1;

        // Add string to pool
        self.string_pool.insert(string_content.to_string(), address);

        // Create data segments for the string
        // Header (size, ref_count, type_id, next_free)
        let header_data = [
            (total_size as u32).to_le_bytes(),
            1u32.to_le_bytes(), // ref_count
            STRING_TYPE_ID.to_le_bytes(),
            0u32.to_le_bytes(), // next_free
        ]
        .concat();
        self.add_data_segment(address as u32, &header_data);

        // String length
        let len_data = (string_len as u32).to_le_bytes();
        self.add_data_segment((address + HEADER_SIZE as usize) as u32, &len_data);

        // String content
        self.add_data_segment((address + HEADER_SIZE as usize + 4) as u32, data);

        // Return pointer to the string length field (after header)
        // The caller can then read the length and access the content
        (address + HEADER_SIZE as usize) as u32
    }

    /// Get the data section
    pub(crate) fn get_data_section(&self) -> &DataSection {
        &self.data_section
    }

    /// Align a size to the required alignment
    fn align_size(size: usize) -> usize {
        (size + ALIGNMENT - 1) & !(ALIGNMENT - 1)
    }

    /// Allocate from appropriate memory pool
    fn allocate_from_pool(&mut self, size: usize, type_id: u32) -> Result<usize, CompilerError> {
        let aligned_size = Self::align_size(size);

        let address = if aligned_size <= SMALL_POOL_SIZE {
            self.small_pool
                .allocate(self.heap_start, &mut self.current_address)
        } else if aligned_size <= MEDIUM_POOL_SIZE {
            self.medium_pool
                .allocate(self.heap_start, &mut self.current_address)
        } else if aligned_size <= LARGE_POOL_SIZE {
            self.large_pool
                .allocate(self.heap_start, &mut self.current_address)
        } else {
            // Large allocation - allocate directly
            let addr = self.current_address;
            self.current_address += aligned_size;
            Some(addr)
        };

        match address {
            Some(addr) => {
                // Create memory block
                let block = MemoryBlock {
                    address: addr,
                    size: aligned_size,
                    is_free: false,
                    type_id,
                    ref_count: 1,
                    next_free: None,
                };
                self.memory_blocks.insert(addr, block);
                self.allocated_objects += 1;

                // Check if we need to trigger GC
                if self.allocated_objects > self.gc_threshold {
                    self.collect_garbage();
                }

                Ok(addr)
            }
            None => Err(CompilerError::memory_allocation_error(
                "Memory pool allocation failed",
                aligned_size,
                None,
                None,
            )),
        }
    }

    /// Record memory allocation
    pub(crate) fn record_allocation(&mut self, size: usize, type_id: u32) -> usize {
        let address = self.current_address;
        let block = MemoryBlock {
            address,
            size,
            is_free: false,
            type_id,
            ref_count: 1,
            next_free: None,
        };
        self.memory_blocks.insert(address, block);
        self.current_address += size;
        self.allocated_objects += 1;
        address
    }

    /// Find a free block of sufficient size
    fn find_free_block(&self, size: usize) -> Option<usize> {
        for &block_addr in &self.free_blocks {
            if let Some(block) = self.memory_blocks.get(&block_addr) {
                if block.is_free && block.size >= size {
                    return Some(block_addr);
                }
            }
        }
        None
    }

    /// Allocate memory for a block with ARC
    pub(crate) fn allocate(&mut self, size: usize, type_id: u32) -> Result<usize, CompilerError> {
        let aligned_size = Self::align_size(size + HEADER_SIZE as usize);

        // Try pool allocation first
        match self.allocate_from_pool(aligned_size, type_id) {
            Ok(addr) => Ok(addr + HEADER_SIZE as usize),
            Err(_) => {
                // Fallback to finding free block
                if let Some(block_addr) = self.find_free_block(aligned_size) {
                    if let Some(block) = self.memory_blocks.get_mut(&block_addr) {
                        block.is_free = false;
                        block.type_id = type_id;
                        block.ref_count = 1;
                        block.next_free = None;

                        // Remove from free list
                        self.free_blocks.retain(|&addr| addr != block_addr);

                        return Ok(block_addr + HEADER_SIZE as usize);
                    }
                }

                // Check memory limits
                let total_memory = self.heap_start + 16 * 1024 * 1024; // 16MB limit
                if self.current_address + aligned_size > total_memory {
                    return Err(CompilerError::memory_allocation_error(
                        "Memory allocation failed: not enough memory",
                        aligned_size,
                        Some(total_memory - self.current_address),
                        None,
                    ));
                }

                // Allocate new memory
                let address = self.record_allocation(aligned_size, type_id);
                Ok(address + HEADER_SIZE as usize)
            }
        }
    }

    /// Increase reference count for a block (ARC retain)
    pub(crate) fn retain(&mut self, address: usize) -> Result<(), CompilerError> {
        let header_address = address - HEADER_SIZE as usize;

        if let Some(block) = self.memory_blocks.get_mut(&header_address) {
            block.ref_count += 1;
            Ok(())
        } else {
            Err(CompilerError::runtime_error(
                format!("Attempt to retain invalid memory address: {address}"),
                None,
                None,
            ))
        }
    }

    /// Decrease reference count for a block (ARC release)
    pub(crate) fn release(&mut self, address: usize) -> Result<(), CompilerError> {
        let header_address = address - HEADER_SIZE as usize;

        if let Some(block) = self.memory_blocks.get_mut(&header_address) {
            if block.ref_count > 0 {
                block.ref_count -= 1;
            }

            // If reference count is zero, mark as free and return to pool
            if block.ref_count == 0 {
                self.deallocate_block(header_address);
            }

            Ok(())
        } else {
            Err(CompilerError::runtime_error(
                format!("Attempt to release invalid memory address: {address}"),
                None,
                None,
            ))
        }
    }

    /// Deallocate a block and return it to the appropriate pool
    fn deallocate_block(&mut self, address: usize) {
        if let Some(mut block) = self.memory_blocks.remove(&address) {
            block.is_free = true;
            block.ref_count = 0;

            // Return to appropriate pool
            if block.size <= SMALL_POOL_SIZE {
                self.small_pool.deallocate(address);
            } else if block.size <= MEDIUM_POOL_SIZE {
                self.medium_pool.deallocate(address);
            } else if block.size <= LARGE_POOL_SIZE {
                self.large_pool.deallocate(address);
            }
            // Large blocks are not pooled

            self.allocated_objects -= 1;

            // Remove from string pool if it's a string
            if block.type_id == STRING_TYPE_ID {
                self.string_pool.retain(|_, &mut addr| addr != address);
            }
        }
    }

    /// Mark an object as a root (never collected)
    #[allow(dead_code)]
    pub(crate) fn add_root_object(&mut self, address: usize) {
        if !self.root_objects.contains(&address) {
            self.root_objects.push(address);
        }
    }

    /// Remove an object from roots
    #[allow(dead_code)]
    pub(crate) fn remove_root_object(&mut self, address: usize) {
        self.root_objects.retain(|&addr| addr != address);
    }

    /// Garbage collection - mark and sweep for circular references
    pub(crate) fn collect_garbage(&mut self) {
        // Mark phase: mark all reachable objects
        let mut marked = std::collections::HashSet::new();

        // Mark all root objects
        for &root_addr in &self.root_objects {
            self.mark_object(root_addr, &mut marked);
        }

        // Mark all objects with ref_count > 0
        for (&addr, block) in &self.memory_blocks {
            if block.ref_count > 0 {
                self.mark_object(addr, &mut marked);
            }
        }

        // Sweep phase: deallocate unmarked objects
        let addresses_to_remove: Vec<usize> = self
            .memory_blocks
            .keys()
            .filter(|&&addr| !marked.contains(&addr))
            .copied()
            .collect();

        for addr in addresses_to_remove {
            self.deallocate_block(addr);
        }

        // Reset GC threshold
        self.gc_threshold = (self.allocated_objects * 2).max(1000);
    }

    /// Mark an object and its references as reachable
    fn mark_object(&self, address: usize, marked: &mut std::collections::HashSet<usize>) {
        if marked.contains(&address) {
            return;
        }

        marked.insert(address);

        // For now, we don't traverse object references
        // In a full implementation, we would examine the object's fields
        // and recursively mark any referenced objects
    }

    /// Generate memory initialization instructions
    #[allow(dead_code)]
    pub(crate) fn generate_init_memory(&self) -> Vec<Instruction> {
        // Initialize memory manager's heap pointer
        let instructions = vec![
            Instruction::I32Const(self.heap_start as i32),
            Instruction::GlobalSet(0), // Assuming global 0 is the heap pointer
        ];

        instructions
    }

    /// Check if the memory utils has any allocated data
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.memory_blocks.is_empty() && self.current_address == self.heap_start
    }

    /// Allocates memory for a string with proper ARC and layout
    pub(crate) fn allocate_string(&mut self, s: &str) -> Result<usize, CompilerError> {
        // Check if string already exists in pool
        if let Some(&existing_ptr) = self.string_pool.get(s) {
            // Increment reference count for existing string
            self.retain(existing_ptr + HEADER_SIZE as usize)?;
            return Ok(existing_ptr);
        }

        let bytes = s.as_bytes();
        let len = bytes.len();

        // Check for very large strings
        if len > 1024 * 1024 {
            // 1MB limit for strings
            return Err(CompilerError::memory_allocation_error(
                "String allocation failed: string is too large",
                len + 4,
                None,
                None,
            ));
        }

        // Allocate memory for the string (length + content)
        let ptr = self.allocate(len + 4, STRING_TYPE_ID)?;

        // The string pointer should point to the length field, not after the header
        let string_ptr = ptr - HEADER_SIZE as usize;

        // Add to string pool
        self.string_pool.insert(s.to_string(), string_ptr);

        // Create data segment for length
        let len_bytes = (len as u32).to_le_bytes();
        self.add_data_segment(string_ptr as u32, &len_bytes);

        // Create data segment for the string content
        self.add_data_segment((string_ptr + 4) as u32, bytes);

        Ok(string_ptr)
    }

    /// Force allocate a string at a specific address for type conversion functions
    #[allow(dead_code)]
    pub(crate) fn force_string_at_address(
        &mut self,
        s: &str,
        target_addr: u32,
    ) -> Result<(), CompilerError> {
        let bytes = s.as_bytes();
        let len = bytes.len();

        // Debug: Log what we're allocating
        eprintln!("DEBUG: Allocating '{s}' (len={len}) at address {target_addr}");

        // Create data segment for the string length (4 bytes, little-endian)
        let len_bytes = (len as u32).to_le_bytes().to_vec();
        self.add_data_segment(target_addr, &len_bytes);

        // Create data segment for the string content
        self.add_data_segment(target_addr + 4, bytes);

        // Debug: Log the bytes being written
        eprintln!("DEBUG: Length bytes: {len_bytes:?}, Content bytes: {bytes:?}");

        // Add to string pool for consistency
        self.string_pool.insert(s.to_string(), target_addr as usize);

        Ok(())
    }

    /// Allocates memory for an array with proper ARC
    pub(crate) fn allocate_array(&mut self, elements: &[Value]) -> Result<usize, CompilerError> {
        self.allocate_array_with_target_type(elements, None)
    }

    pub(crate) fn allocate_array_with_target_type(
        &mut self,
        elements: &[Value],
        target_element_type: Option<&crate::ast::Type>,
    ) -> Result<usize, CompilerError> {
        use crate::ast::Type;
        use crate::types::WasmType;

        let element_type = if let Some(target_type) = target_element_type {
            // If we have a target type, use it to determine the storage format
            match target_type {
                Type::Number => WasmType::F64,
                Type::Integer => WasmType::I32,
                Type::Boolean => WasmType::I32,
                Type::String => WasmType::I32,
                _ => WasmType::I32,
            }
        } else if elements.is_empty() {
            WasmType::I32
        } else {
            // Fallback to existing logic if no target type provided
            match &elements[0] {
                Value::Integer(_) => WasmType::I32,
                Value::Boolean(_) => WasmType::I32,
                Value::String(_) => WasmType::I32,
                Value::Number(_) => WasmType::F64,
                _ => WasmType::I32,
            }
        };

        let element_size = element_type.size_in_bytes();
        let total_size = 4 + (elements.len() * element_size); // length + elements

        let ptr = self.allocate(total_size, ARRAY_TYPE_ID)?;

        // Create data segment for array length
        let len_bytes = (elements.len() as u32).to_le_bytes();
        self.add_data_segment((ptr - HEADER_SIZE as usize) as u32, &len_bytes);

        // Create data segments for array elements
        let mut offset = 4;
        for element in elements {
            let element_bytes = match element_type {
                WasmType::F64 => {
                    // Convert all values to F64 for number arrays
                    let f64_value = match element {
                        Value::Integer(i) => *i as f64,
                        Value::Number(f) => *f,
                        Value::Boolean(b) => {
                            if *b {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        _ => {
                            return Err(CompilerError::type_error(
                                format!("Cannot convert {element:?} to number for array storage"),
                                None,
                                None,
                            ))
                        }
                    };
                    f64_value.to_le_bytes().to_vec()
                }
                WasmType::I32 => {
                    // Convert all values to I32 for integer/boolean/string arrays
                    let i32_value = match element {
                        Value::Integer(i) => *i as u32,
                        Value::Boolean(b) => *b as u32,
                        Value::Number(f) => *f as u32,
                        Value::String(s) => {
                            // For string elements, allocate string and store pointer
                            let str_ptr = self.allocate_string(s)?;
                            str_ptr as u32
                        }
                        _ => {
                            return Err(CompilerError::type_error(
                                format!("Cannot convert {element:?} to I32 for array storage"),
                                None,
                                None,
                            ))
                        }
                    };
                    i32_value.to_le_bytes().to_vec()
                }
                _ => {
                    // Fallback to original logic for other types
                    match element {
                        Value::Integer(i) => (*i as u32).to_le_bytes().to_vec(),
                        Value::Boolean(b) => (*b as u32).to_le_bytes().to_vec(),
                        Value::Number(f) => f.to_le_bytes().to_vec(),
                        Value::String(s) => {
                            // For string elements, store pointer to string
                            let str_ptr = self.allocate_string(s)?;
                            (str_ptr as u32).to_le_bytes().to_vec()
                        }
                        _ => vec![0; element_size],
                    }
                }
            };

            self.add_data_segment((ptr + offset - HEADER_SIZE as usize) as u32, &element_bytes);
            offset += element_size;
        }

        Ok(ptr)
    }

    /// Allocates memory for a matrix with proper ARC
    pub(crate) fn allocate_matrix(&mut self, rows: &[Vec<f64>]) -> Result<usize, CompilerError> {
        if rows.is_empty() {
            return Err(CompilerError::memory_allocation_error(
                "Matrix allocation failed: empty matrix",
                0,
                None,
                None,
            ));
        }

        let num_rows = rows.len();
        let num_cols = rows[0].len();

        // Validate matrix dimensions
        for row in rows {
            if row.len() != num_cols {
                return Err(CompilerError::memory_allocation_error(
                    "Matrix allocation failed: inconsistent row lengths",
                    0,
                    None,
                    None,
                ));
            }
        }

        let total_elements = num_rows * num_cols;
        let total_size = 8 + (total_elements * 8); // rows + cols + elements (f64)

        let ptr = self.allocate(total_size, MATRIX_TYPE_ID)?;

        // Create data segment for matrix dimensions
        let dims_bytes = [
            (num_rows as u32).to_le_bytes(),
            (num_cols as u32).to_le_bytes(),
        ]
        .concat();
        self.add_data_segment((ptr - HEADER_SIZE as usize) as u32, &dims_bytes);

        // Create data segment for matrix elements (row-major order)
        let mut element_bytes = Vec::new();
        for row in rows {
            for &element in row {
                element_bytes.extend_from_slice(&element.to_le_bytes());
            }
        }

        self.add_data_segment((ptr + 8 - HEADER_SIZE as usize) as u32, &element_bytes);

        Ok(ptr)
    }

    /// Generate memory management functions for WASM
    #[allow(dead_code)]
    pub(crate) fn generate_memory_functions(
        &self,
    ) -> Vec<(Vec<ValType>, Option<ValType>, Vec<Instruction<'static>>)> {
        vec![
            // malloc(size: i32, type_id: i32) -> i32
            (
                vec![ValType::I32, ValType::I32],
                Some(ValType::I32),
                vec![
                    Instruction::LocalGet(0), // size
                    Instruction::LocalGet(1), // type_id
                    Instruction::Call(0),     // Call internal allocate function
                ],
            ),
            // retain(ptr: i32) -> void
            (
                vec![ValType::I32],
                None,
                vec![
                    Instruction::LocalGet(0), // ptr
                    Instruction::Call(1),     // Call internal retain function
                ],
            ),
            // release(ptr: i32) -> void
            (
                vec![ValType::I32],
                None,
                vec![
                    Instruction::LocalGet(0), // ptr
                    Instruction::Call(2),     // Call internal release function
                ],
            ),
        ]
    }

    /// Get memory statistics
    #[allow(dead_code)]
    pub(crate) fn get_stats(&self) -> (usize, usize, usize) {
        (
            self.allocated_objects,
            self.memory_blocks.len(),
            self.current_address - self.heap_start,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn get_memory_manager_ref(&self) -> Rc<RefCell<MemoryManager>> {
        // Return the shared memory manager instance for stdlib integration
        self.memory_manager.clone()
    }
}
