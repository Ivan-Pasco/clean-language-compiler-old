//! Module for memory operations during code generation with comprehensive safety validation.
//!
//! Note: Much of the memory safety infrastructure is currently unused but kept for future
//! enhancements. The basic MemoryUtils is actively used for data segment management.

#![allow(dead_code)]

use crate::ast::Value;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_encoder::{ConstExpr, DataSection};

/// Memory safety configuration constants
#[allow(dead_code)]
const GUARD_PAGE_SIZE: usize = 512; // 512 bytes guard pages (reduced for single-page WASM)
const MAX_MEMORY_SIZE: usize = 262144; // 256KB limit (4 WASM pages) — matches native_stdlib::HEAP_START
const MEMORY_ALIGNMENT: usize = 8; // 8-byte alignment
#[allow(dead_code)]
const POISON_PATTERN: u8 = 0xDE; // Pattern for poisoned memory
const CANARY_VALUE: u32 = 0xDEADBEEF; // Canary for overflow detection

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

/// Enhanced memory block header layout in WASM memory with safety validation
/// Offset 0-3: Size (u32)
/// Offset 4-7: Reference count (u32)
/// Offset 8-11: Type ID (u32)
/// Offset 12-15: Next free block pointer (u32, 0 if not free)
/// Safety extensions:
/// - Guard bytes before/after allocation
/// - Canary values for overflow detection
/// - Allocation metadata for bounds checking
#[derive(Debug, Clone)]
pub struct MemoryBlock {
    pub address: usize,
    pub size: usize,
    pub is_free: bool,
    pub type_id: u32,
    pub ref_count: usize,
    pub next_free: Option<usize>,
    // Memory safety fields
    pub allocation_id: u64,
    pub canary_start: u32,
    pub canary_end: u32,
    pub is_poisoned: bool,
    pub stack_trace: Vec<String>, // For debugging use-after-free
}

/// Memory safety validation errors
#[derive(Debug, Clone)]
pub enum MemorySafetyError {
    BufferOverflow {
        address: usize,
        size: usize,
        attempted_size: usize,
    },
    UseAfterFree {
        address: usize,
        allocation_id: u64,
    },
    DoubleFree {
        address: usize,
        allocation_id: u64,
    },
    InvalidPointer {
        address: usize,
    },
    CorruptedCanary {
        address: usize,
        expected: u32,
        found: u32,
    },
    OutOfBounds {
        address: usize,
        base: usize,
        size: usize,
    },
    UnalignedAccess {
        address: usize,
        alignment: usize,
    },
}

/// Memory region for bounds checking
#[derive(Debug, Clone)]
struct MemoryRegion {
    start: usize,
    end: usize,
    permissions: MemoryPermissions,
    name: String,
}

/// Memory access permissions
#[derive(Debug, Clone, PartialEq)]
enum MemoryPermissions {
    ReadWrite,
    ReadOnly,
    NoAccess, // Guard regions
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

    /// Validate pool integrity
    fn validate_integrity(&self) -> Result<(), MemorySafetyError> {
        // Ensure all free blocks are unique
        let mut sorted_blocks = self.free_blocks.clone();
        sorted_blocks.sort_unstable();
        sorted_blocks.dedup();

        if sorted_blocks.len() != self.free_blocks.len() {
            return Err(MemorySafetyError::CorruptedCanary {
                address: 0,
                expected: self.free_blocks.len() as u32,
                found: sorted_blocks.len() as u32,
            });
        }

        Ok(())
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

/// Enhanced memory management utilities with comprehensive safety validation
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

    // Memory safety validation
    memory_regions: Vec<MemoryRegion>,
    allocation_counter: u64,
    guard_regions: HashMap<usize, usize>, // Guard page mappings
    poisoned_memory: HashMap<usize, u64>, // Address -> allocation_id for freed memory
    bounds_check_enabled: bool,

    // Shadow memory for tracking allocation status
    shadow_memory: HashMap<usize, AllocationStatus>,

    // Shared memory manager for stdlib integration
    memory_manager: Rc<RefCell<MemoryManager>>,
}

/// Allocation status for shadow memory tracking
#[derive(Debug, Clone, PartialEq)]
enum AllocationStatus {
    Allocated {
        size: usize,
        type_id: u32,
        allocation_id: u64,
    },
    Free,
    Poisoned {
        allocation_id: u64,
    },
    Guard,
}

impl MemoryUtils {
    /// Create a new MemoryUtils instance with comprehensive memory safety validation
    pub(crate) fn new(heap_start: usize) -> Self {
        // Create a shared memory manager for stdlib integration
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, None)));

        // Ensure heap_start is properly aligned (simplified for single WASM page)
        let aligned_heap_start = Self::align_size(heap_start);
        let aligned_current_address = aligned_heap_start; // Start directly at heap start

        let mut memory_utils = Self {
            data_section: DataSection::new(),
            heap_start: aligned_heap_start,
            current_address: aligned_current_address, // Start after guard page with proper alignment
            memory_blocks: HashMap::new(),
            free_blocks: Vec::new(),
            small_pool: MemoryPool::new(SMALL_POOL_SIZE),
            medium_pool: MemoryPool::new(MEDIUM_POOL_SIZE),
            large_pool: MemoryPool::new(LARGE_POOL_SIZE),
            root_objects: Vec::new(),
            gc_threshold: 1000,
            allocated_objects: 0,
            string_pool: HashMap::new(),
            memory_regions: Vec::new(),
            allocation_counter: 1,
            guard_regions: HashMap::new(),
            poisoned_memory: HashMap::new(),
            bounds_check_enabled: true,
            shadow_memory: HashMap::new(),
            memory_manager,
        };

        // Initialize memory regions with guard pages
        memory_utils.setup_memory_regions();
        memory_utils
    }

    /// Setup initial memory regions with simplified layout for single WASM page
    fn setup_memory_regions(&mut self) {
        // Single main heap region covering most of the WASM page
        // Start after some static data area (0-1024), end before 64KB page limit
        self.memory_regions.push(MemoryRegion {
            start: self.heap_start, // 1024
            end: MAX_MEMORY_SIZE,   // 65536
            permissions: MemoryPermissions::ReadWrite,
            name: "main_heap".to_string(),
        });

        // CRITICAL: Initialize the global heap pointer at address 0
        // This is used by list.allocate and other allocation functions
        // The heap pointer stores the next available address for allocation
        // Initial value = heap_start (1024) where allocations begin
        let heap_ptr_bytes = (self.heap_start as u32).to_le_bytes();
        let offset_expr = ConstExpr::i32_const(0);
        self.data_section
            .active(0, &offset_expr, heap_ptr_bytes.to_vec());
    }

    /// Add a data segment with bounds validation
    pub(crate) fn add_data_segment(
        &mut self,
        offset: u32,
        data: &[u8],
    ) -> Result<(), MemorySafetyError> {
        // Validate bounds before adding data segment
        self.validate_memory_access(offset as usize, data.len())?;

        let offset_expr = ConstExpr::i32_const(offset as i32);
        let data_vec: Vec<u8> = data.to_vec();
        self.data_section.active(0, &offset_expr, data_vec);
        Ok(())
    }

    /// Legacy add_data_segment for compatibility (unsafe)
    pub(crate) fn unsafe_add_data_segment(&mut self, offset: u32, data: &[u8]) {
        let offset_expr = ConstExpr::i32_const(offset as i32);
        let data_vec: Vec<u8> = data.to_vec();
        self.data_section.active(0, &offset_expr, data_vec);
    }

    /// Comprehensive memory access validation
    fn validate_memory_access(&self, address: usize, size: usize) -> Result<(), MemorySafetyError> {
        if !self.bounds_check_enabled {
            return Ok(());
        }

        // Check for null pointer
        if address == 0 {
            return Err(MemorySafetyError::InvalidPointer { address });
        }

        // Check alignment - WASM data sections only require 1-byte alignment
        // The alignment check for 4/8 bytes is overly strict for data segments
        // Only check that address is not completely misaligned (allow any natural alignment)
        // For data segments, WASM doesn't enforce strict alignment requirements
        if address % 4 != 0 && size > 4 {
            // Only enforce 4-byte alignment for larger allocations
            // This is a relaxed check compared to the previous 8-byte requirement
            return Err(MemorySafetyError::UnalignedAccess {
                address,
                alignment: 4,
            });
        }

        // Check if accessing poisoned memory (use-after-free detection)
        if let Some(&allocation_id) = self.poisoned_memory.get(&address) {
            return Err(MemorySafetyError::UseAfterFree {
                address,
                allocation_id,
            });
        }

        // Validate against memory regions
        for region in &self.memory_regions {
            if address >= region.start && address < region.end {
                match region.permissions {
                    MemoryPermissions::NoAccess => {
                        return Err(MemorySafetyError::OutOfBounds {
                            address,
                            base: region.start,
                            size: region.end - region.start,
                        });
                    }
                    MemoryPermissions::ReadWrite | MemoryPermissions::ReadOnly => {
                        // Check if access goes beyond region bounds
                        if address + size > region.end {
                            return Err(MemorySafetyError::BufferOverflow {
                                address,
                                size: region.end - address,
                                attempted_size: size,
                            });
                        }
                        return Ok(());
                    }
                }
            }
        }

        // Address not in any defined region
        Err(MemorySafetyError::InvalidPointer { address })
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
                // Create memory block with safety metadata
                let allocation_id = self.allocation_counter;
                self.allocation_counter += 1;

                let block = MemoryBlock {
                    address: addr,
                    size: aligned_size,
                    is_free: false,
                    type_id,
                    ref_count: 1,
                    next_free: None,
                    allocation_id,
                    canary_start: CANARY_VALUE,
                    canary_end: CANARY_VALUE,
                    is_poisoned: false,
                    stack_trace: Vec::new(),
                };

                // Update shadow memory
                self.shadow_memory.insert(
                    addr,
                    AllocationStatus::Allocated {
                        size: aligned_size,
                        type_id,
                        allocation_id,
                    },
                );

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
        let allocation_id = self.allocation_counter;
        self.allocation_counter += 1;

        let block = MemoryBlock {
            address,
            size,
            is_free: false,
            type_id,
            ref_count: 1,
            next_free: None,
            allocation_id,
            canary_start: CANARY_VALUE,
            canary_end: CANARY_VALUE,
            is_poisoned: false,
            stack_trace: Vec::new(),
        };

        // Update shadow memory
        self.shadow_memory.insert(
            address,
            AllocationStatus::Allocated {
                size,
                type_id,
                allocation_id,
            },
        );

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

    /// Allocates memory for a string with proper ARC and layout
    pub(crate) fn allocate_string(&mut self, s: &str) -> Result<usize, CompilerError> {
        // Check if string already exists in pool
        if let Some(&existing_ptr) = self.string_pool.get(s) {
            // Verify the memory block still exists (safety check for GC issues)
            if self.memory_blocks.contains_key(&existing_ptr) {
                // Increment reference count for existing string
                self.retain(existing_ptr + HEADER_SIZE as usize)?;
                return Ok(existing_ptr);
            } else {
                // String pool entry is stale, remove it
                self.string_pool.remove(s);
            }
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

        // Create data segment for length with safety validation
        let len_bytes = (len as u32).to_le_bytes();
        self.add_data_segment(string_ptr as u32, &len_bytes)
            .map_err(|e| {
                CompilerError::memory_allocation_error(
                    &format!("String length allocation failed: {:?}", e),
                    4,
                    None,
                    None,
                )
            })?;

        // Create data segment for the string content with safety validation
        self.add_data_segment((string_ptr + 4) as u32, bytes)
            .map_err(|e| {
                CompilerError::memory_allocation_error(
                    &format!("String content allocation failed: {:?}", e),
                    len,
                    None,
                    None,
                )
            })?;

        Ok(string_ptr)
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

        // Create data segment for array length with safety validation
        let len_bytes = (elements.len() as u32).to_le_bytes();
        self.add_data_segment((ptr - HEADER_SIZE as usize) as u32, &len_bytes)
            .map_err(|e| {
                CompilerError::memory_allocation_error(
                    &format!("Array length allocation failed: {:?}", e),
                    4,
                    None,
                    None,
                )
            })?;

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

            self.add_data_segment((ptr + offset - HEADER_SIZE as usize) as u32, &element_bytes)
                .map_err(|e| {
                    CompilerError::memory_allocation_error(
                        &format!("Array element allocation failed: {:?}", e),
                        element_bytes.len(),
                        None,
                        None,
                    )
                })?;
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

        // Create data segment for matrix dimensions with safety validation
        let dims_bytes = [
            (num_rows as u32).to_le_bytes(),
            (num_cols as u32).to_le_bytes(),
        ]
        .concat();
        self.add_data_segment((ptr - HEADER_SIZE as usize) as u32, &dims_bytes)
            .map_err(|e| {
                CompilerError::memory_allocation_error(
                    &format!("Matrix dimensions allocation failed: {:?}", e),
                    8,
                    None,
                    None,
                )
            })?;

        // Create data segment for matrix elements (row-major order) with safety validation
        let mut element_bytes = Vec::new();
        for row in rows {
            for &element in row {
                element_bytes.extend_from_slice(&element.to_le_bytes());
            }
        }

        self.add_data_segment((ptr + 8 - HEADER_SIZE as usize) as u32, &element_bytes)
            .map_err(|e| {
                CompilerError::memory_allocation_error(
                    &format!("Matrix elements allocation failed: {:?}", e),
                    element_bytes.len(),
                    None,
                    None,
                )
            })?;

        Ok(ptr)
    }

    /// Get the current allocation address (first available address for new allocations)
    pub(crate) fn get_current_address(&self) -> usize {
        self.current_address
    }
}
