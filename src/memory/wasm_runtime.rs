//! WebAssembly Memory Management Runtime
//!
//! This module provides the runtime implementation for memory management functions
//! that are imported by Clean Language WebAssembly modules.

use super::*;
use std::sync::Mutex;

/// Global memory manager for WebAssembly runtime
static GLOBAL_MEMORY_MANAGER: Mutex<Option<MemoryManager>> = Mutex::new(None);

/// Initialize the global memory manager
pub fn init_memory_manager(layout: MemoryLayout) -> MemoryResult<()> {
    let mut manager = GLOBAL_MEMORY_MANAGER.lock().map_err(|_| {
        CompilerError::memory_error("Failed to acquire memory manager lock", None, None)
    })?;

    *manager = Some(MemoryManager::new(layout));
    Ok(())
}

/// Get a reference to the global memory manager
fn with_memory_manager<F, R>(f: F) -> MemoryResult<R>
where
    F: FnOnce(&mut MemoryManager) -> MemoryResult<R>,
{
    let mut manager = GLOBAL_MEMORY_MANAGER.lock().map_err(|_| {
        CompilerError::memory_error("Failed to acquire memory manager lock", None, None)
    })?;

    let manager_ref = manager
        .as_mut()
        .ok_or_else(|| CompilerError::memory_error("Memory manager not initialized", None, None))?;

    f(manager_ref)
}

/// WebAssembly runtime function: Allocate memory
/// Function signature: (type_id: i32, size: i32) -> address: i32
pub fn mem_alloc(type_id: i32, size: i32) -> i32 {
    match with_memory_manager(|manager| manager.allocate(type_id as TypeId, size as u32)) {
        Ok(address) => address as i32,
        Err(_) => 0, // Return null on allocation failure
    }
}

/// WebAssembly runtime function: Increment reference count
/// Function signature: (address: i32) -> ()
pub fn mem_retain(address: i32) {
    let _ = with_memory_manager(|manager| {
        if address != 0 {
            manager.retain(address as MemoryAddress)?;
        }
        Ok(())
    });
}

/// WebAssembly runtime function: Decrement reference count and deallocate if needed
/// Function signature: (address: i32) -> ()
pub fn mem_release(address: i32) {
    let _ = with_memory_manager(|manager| {
        if address != 0 {
            manager.release(address as MemoryAddress)?;
        }
        Ok(())
    });
}

/// WebAssembly runtime function: Trigger garbage collection
/// Function signature: () -> freed_count: i32
pub fn mem_collect() -> i32 {
    match with_memory_manager(|manager| manager.collect_garbage()) {
        Ok(freed) => freed as i32,
        Err(_) => 0,
    }
}

/// WebAssembly runtime function: Get reference count
/// Function signature: (address: i32) -> ref_count: i32
pub fn mem_get_ref_count(address: i32) -> i32 {
    match with_memory_manager(|manager| {
        if address != 0 {
            Ok(manager.get_ref_count(address as MemoryAddress))
        } else {
            Ok(0)
        }
    }) {
        Ok(count) => count as i32,
        Err(_) => 0,
    }
}

/// Runtime memory statistics
#[derive(Debug, Clone)]
pub struct RuntimeMemoryStats {
    pub total_allocated: u32,
    pub allocation_count: usize,
    pub gc_cycles: u32,
    pub objects_freed: u32,
}

/// Get memory statistics for debugging
pub fn get_memory_stats() -> MemoryResult<RuntimeMemoryStats> {
    with_memory_manager(|manager| {
        let stats = manager.get_total_allocated();
        Ok(RuntimeMemoryStats {
            total_allocated: stats,
            allocation_count: 0, // TODO: Track this
            gc_cycles: 0,        // TODO: Track this
            objects_freed: 0,    // TODO: Track this
        })
    })
}

/// Memory management configuration for WebAssembly
#[derive(Debug, Clone)]
pub struct WasmMemoryConfig {
    /// Initial WebAssembly memory pages (64KB each)
    pub initial_pages: u32,
    /// Maximum WebAssembly memory pages
    pub max_pages: Option<u32>,
    /// Enable garbage collection
    pub enable_gc: bool,
    /// GC trigger threshold (bytes)
    pub gc_threshold: u32,
}

impl Default for WasmMemoryConfig {
    fn default() -> Self {
        Self {
            initial_pages: 16,    // 1MB initial
            max_pages: Some(256), // 16MB max
            enable_gc: true,
            gc_threshold: 1024 * 1024, // 1MB
        }
    }
}

/// Initialize Clean Language memory runtime with WebAssembly
pub fn init_clean_memory_runtime(config: WasmMemoryConfig) -> MemoryResult<()> {
    let layout = MemoryLayout {
        stack_start: 0,
        heap_start: 1024,                                    // After stack
        string_pool_start: config.initial_pages * 65536 / 2, // Middle of memory
        globals_start: config.initial_pages * 65536 * 3 / 4, // Near end
        total_size: config.initial_pages * 65536,
    };

    init_memory_manager(layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_runtime_basic() {
        let config = WasmMemoryConfig::default();
        init_clean_memory_runtime(config).expect("Failed to initialize memory runtime");

        // Test allocation
        let addr = mem_alloc(1, 64);
        assert_ne!(addr, 0, "Allocation should succeed");

        // Test reference counting
        mem_retain(addr);
        let ref_count = mem_get_ref_count(addr);
        assert_eq!(ref_count, 1, "Reference count should be 1");

        // Test deallocation
        mem_release(addr);

        // Test garbage collection
        let freed = mem_collect();
        assert!(freed >= 0, "GC should return non-negative freed count");
    }

    #[test]
    fn test_memory_stats() {
        let config = WasmMemoryConfig::default();
        init_clean_memory_runtime(config).expect("Failed to initialize memory runtime");

        let stats = get_memory_stats().expect("Failed to get memory stats");
        assert!(
            stats.total_allocated >= 0,
            "Total allocated should be non-negative"
        );
    }
}
