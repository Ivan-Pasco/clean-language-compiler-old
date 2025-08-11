//! Memory allocator implementation

use super::*;
use std::collections::BTreeMap;

/// Memory allocator for heap management
pub struct Allocator {
    heap_start: MemoryAddress,
    heap_pointer: MemoryAddress,
    free_blocks: BTreeMap<usize, Vec<MemoryAddress>>, // size -> list of addresses
    allocated_blocks: HashMap<MemoryAddress, AllocationInfo>,
    total_allocated: u32,
}

/// Information about an allocated block
#[derive(Debug, Clone)]
pub struct AllocationInfo {
    pub size: u32,
    pub type_id: TypeId,
    pub header: ObjectHeader,
}

impl Allocator {
    pub fn new(heap_start: MemoryAddress) -> Self {
        Self {
            heap_start,
            heap_pointer: heap_start,
            free_blocks: BTreeMap::new(),
            allocated_blocks: HashMap::new(),
            total_allocated: 0,
        }
    }

    /// Allocate memory for an object
    pub fn allocate(&mut self, type_id: TypeId, size: u32) -> MemoryResult<MemoryAddress> {
        let total_size = size + std::mem::size_of::<ObjectHeader>() as u32;
        let address = self.allocate_raw(total_size)?;

        let header = ObjectHeader::new(type_id, size);
        let info = AllocationInfo {
            size: total_size,
            type_id,
            header,
        };

        self.allocated_blocks.insert(address, info);
        self.total_allocated += total_size;

        // Return address after header
        Ok(address + std::mem::size_of::<ObjectHeader>() as u32)
    }

    /// Deallocate memory
    pub fn deallocate(&mut self, address: MemoryAddress) -> MemoryResult<()> {
        // Adjust address to point to header
        let header_address = address - std::mem::size_of::<ObjectHeader>() as u32;

        if let Some(info) = self.allocated_blocks.remove(&header_address) {
            self.free_raw(header_address, info.size);
            self.total_allocated -= info.size;
            Ok(())
        } else {
            Err(CompilerError::memory_error(
                "Invalid deallocation address",
                None,
                None,
            ))
        }
    }

    /// Get object header
    pub fn get_header(&self, address: MemoryAddress) -> MemoryResult<&ObjectHeader> {
        let header_address = address - std::mem::size_of::<ObjectHeader>() as u32;
        self.allocated_blocks
            .get(&header_address)
            .map(|info| &info.header)
            .ok_or_else(|| CompilerError::memory_error("Invalid object address", None, None))
    }

    /// Get total allocated memory
    pub fn get_total_allocated(&self) -> u32 {
        self.total_allocated
    }

    /// Get allocation statistics
    pub fn get_statistics(&self) -> AllocationStats {
        AllocationStats {
            total_allocated: self.total_allocated,
            block_count: self.allocated_blocks.len(),
            free_block_count: self.free_blocks.values().map(|v| v.len()).sum(),
            heap_usage: ((self.heap_pointer - self.heap_start) as f64 / (1024.0 * 1024.0)),
        }
    }

    /// Get all allocated blocks for garbage collection
    pub fn get_allocated_blocks(&self) -> &HashMap<MemoryAddress, AllocationInfo> {
        &self.allocated_blocks
    }

    /// Get allocation info for a specific address
    pub fn get_allocation_info(&self, address: MemoryAddress) -> Option<&AllocationInfo> {
        let header_address = address - std::mem::size_of::<ObjectHeader>() as u32;
        self.allocated_blocks.get(&header_address)
    }

    fn allocate_raw(&mut self, size: u32) -> MemoryResult<MemoryAddress> {
        // Try to find a suitable free block
        if let Some(address) = self.find_free_block(size as usize) {
            Ok(address)
        } else {
            // Allocate from heap pointer
            let address = self.heap_pointer;
            self.heap_pointer += size;
            Ok(address)
        }
    }

    fn free_raw(&mut self, address: MemoryAddress, size: u32) {
        self.free_blocks
            .entry(size as usize)
            .or_default()
            .push(address);
    }

    fn find_free_block(&mut self, size: usize) -> Option<MemoryAddress> {
        // Find smallest suitable block
        for (&block_size, addresses) in self.free_blocks.range_mut(size..) {
            if let Some(address) = addresses.pop() {
                if addresses.is_empty() {
                    self.free_blocks.remove(&block_size);
                }
                return Some(address);
            }
        }
        None
    }
}

/// Allocation statistics
#[derive(Debug, Clone)]
pub struct AllocationStats {
    pub total_allocated: u32,
    pub block_count: usize,
    pub free_block_count: usize,
    pub heap_usage: f64, // MB
}
