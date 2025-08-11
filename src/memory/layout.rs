//! Memory layout management

use super::*;

/// Memory region descriptor
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start: MemoryAddress,
    pub size: u32,
    pub name: String,
    pub permissions: MemoryPermissions,
}

/// Memory permissions
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl MemoryPermissions {
    pub const READ_WRITE: Self = Self { read: true, write: true, execute: false };
    pub const READ_ONLY: Self = Self { read: true, write: false, execute: false };
    pub const EXECUTE: Self = Self { read: true, write: false, execute: true };
}

/// Memory layout manager
pub struct MemoryLayoutManager {
    regions: Vec<MemoryRegion>,
    total_size: u32,
}

impl MemoryLayoutManager {
    pub fn new(layout: &MemoryLayout) -> Self {
        let mut manager = Self {
            regions: Vec::new(),
            total_size: layout.total_size,
        };

        // Define standard regions
        manager.add_region(MemoryRegion {
            start: layout.stack_start,
            size: layout.heap_start - layout.stack_start,
            name: "Stack".to_string(),
            permissions: MemoryPermissions::READ_WRITE,
        });

        manager.add_region(MemoryRegion {
            start: layout.heap_start,
            size: layout.string_pool_start - layout.heap_start,
            name: "Heap".to_string(),
            permissions: MemoryPermissions::READ_WRITE,
        });

        manager.add_region(MemoryRegion {
            start: layout.string_pool_start,
            size: layout.globals_start - layout.string_pool_start,
            name: "String Pool".to_string(),
            permissions: MemoryPermissions::READ_ONLY,
        });

        manager.add_region(MemoryRegion {
            start: layout.globals_start,
            size: layout.total_size - layout.globals_start,
            name: "Globals".to_string(),
            permissions: MemoryPermissions::READ_WRITE,
        });

        manager
    }

    pub fn add_region(&mut self, region: MemoryRegion) {
        self.regions.push(region);
    }

    pub fn get_region(&self, address: MemoryAddress) -> Option<&MemoryRegion> {
        self.regions.iter().find(|region| {
            address >= region.start && address < region.start + region.size
        })
    }

    pub fn validate_access(&self, address: MemoryAddress, is_write: bool) -> MemoryResult<()> {
        if let Some(region) = self.get_region(address) {
            if !region.permissions.read {
                return Err(CompilerError::memory_error("Read access denied", None, None));
            }
            if is_write && !region.permissions.write {
                return Err(CompilerError::memory_error("Write access denied", None, None));
            }
            Ok(())
        } else {
            Err(CompilerError::memory_error("Invalid memory address", None, None))
        }
    }

    pub fn get_regions(&self) -> &[MemoryRegion] {
        &self.regions
    }
}