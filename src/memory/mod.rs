//! Clean Language Memory Management Module
//!
//! This module implements Automatic Reference Counting (ARC) and garbage collection
//! for the Clean Language runtime in WebAssembly.
//!
//! # Features
//! - Automatic Reference Counting (ARC) for deterministic memory management
//! - Mark-and-sweep garbage collection for cycle detection
//! - String pool for literal deduplication
//! - WebAssembly memory model compliance
//! - Memory safety guarantees

use crate::error::CompilerError;
use std::collections::HashMap;

pub mod allocator;
pub mod arc;
pub mod gc;
pub mod layout;
pub mod string_pool;

pub mod wasm_runtime;

pub use allocator::*;
pub use arc::*;
pub use gc::*;
pub use layout::*;
pub use string_pool::*;
pub use wasm_runtime::*;

/// Memory management result type
pub type MemoryResult<T> = Result<T, CompilerError>;

/// Memory address in WebAssembly linear memory
pub type MemoryAddress = u32;

/// Object type identifier
pub type TypeId = u32;

/// Reference count type
pub type RefCount = u32;

/// Object header for all heap-allocated objects
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ObjectHeader {
    /// Reference count for ARC
    pub ref_count: RefCount,
    /// Type identifier
    pub type_id: TypeId,
    /// Size of object in bytes
    pub size: u32,
    /// Garbage collection flags
    pub gc_flags: u8,
}

impl ObjectHeader {
    pub fn new(type_id: TypeId, size: u32) -> Self {
        Self {
            ref_count: 1,
            type_id,
            size,
            gc_flags: 0,
        }
    }

    /// Check if object is marked for GC
    pub fn is_marked(&self) -> bool {
        self.gc_flags & 0x01 != 0
    }

    /// Mark object for GC
    pub fn mark(&mut self) {
        self.gc_flags |= 0x01;
    }

    /// Unmark object for GC
    pub fn unmark(&mut self) {
        self.gc_flags &= !0x01;
    }
}

/// Memory layout configuration for WebAssembly
#[derive(Debug, Clone)]
pub struct MemoryLayout {
    /// Stack start address
    pub stack_start: MemoryAddress,
    /// Heap start address
    pub heap_start: MemoryAddress,
    /// String pool start address
    pub string_pool_start: MemoryAddress,
    /// Global variables start address
    pub globals_start: MemoryAddress,
    /// Total memory size
    pub total_size: u32,
}

impl Default for MemoryLayout {
    fn default() -> Self {
        Self {
            stack_start: 0,
            heap_start: 1024,
            string_pool_start: 64 * 1024, // 64KB
            globals_start: 128 * 1024,    // 128KB
            total_size: 1024 * 1024,      // 1MB default
        }
    }
}

/// Main memory manager
pub struct MemoryManager {
    allocator: Allocator,
    string_pool: StringPool,
    gc: GarbageCollector,
    arc_manager: ARCManager,
}

impl MemoryManager {
    pub fn new(layout: MemoryLayout) -> Self {
        Self {
            allocator: Allocator::new(layout.heap_start),
            string_pool: StringPool::new(layout.string_pool_start),
            gc: GarbageCollector::new(),
            arc_manager: ARCManager::new(),
        }
    }

    /// Allocate memory for object
    pub fn allocate(&mut self, type_id: TypeId, size: u32) -> MemoryResult<MemoryAddress> {
        self.allocator.allocate(type_id, size)
    }

    /// Deallocate memory
    pub fn deallocate(&mut self, address: MemoryAddress) -> MemoryResult<()> {
        self.allocator.deallocate(address)
    }

    /// Get or create string in string pool
    pub fn intern_string(&mut self, s: &str) -> MemoryResult<MemoryAddress> {
        self.string_pool.intern(s)
    }

    /// Run garbage collection cycle
    pub fn collect_garbage(&mut self) -> MemoryResult<usize> {
        self.gc.collect(&mut self.allocator)
    }

    /// Get total allocated memory
    pub fn get_total_allocated(&self) -> u32 {
        self.allocator.get_total_allocated()
    }

    /// Increment reference count for an object
    pub fn retain(&mut self, address: MemoryAddress) -> MemoryResult<()> {
        self.arc_manager.retain(address)
    }

    /// Decrement reference count for an object
    pub fn release(&mut self, address: MemoryAddress) -> MemoryResult<bool> {
        let should_deallocate = self.arc_manager.release(address)?;
        if should_deallocate {
            self.deallocate(address)?;
        }
        Ok(should_deallocate)
    }

    /// Get reference count for an object
    pub fn get_ref_count(&self, address: MemoryAddress) -> RefCount {
        self.arc_manager.get_ref_count(address)
    }
}
