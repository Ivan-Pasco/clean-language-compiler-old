//! Automatic Reference Counting implementation

use super::*;

/// ARC manager for deterministic memory management
pub struct ARCManager {
    ref_counts: HashMap<MemoryAddress, RefCount>,
}

impl ARCManager {
    pub fn new() -> Self {
        Self {
            ref_counts: HashMap::new(),
        }
    }

    /// Increment reference count
    pub fn retain(&mut self, address: MemoryAddress) -> MemoryResult<()> {
        let count = self.ref_counts.entry(address).or_insert(0);
        *count += 1;
        Ok(())
    }

    /// Decrement reference count
    pub fn release(&mut self, address: MemoryAddress) -> MemoryResult<bool> {
        if let Some(count) = self.ref_counts.get_mut(&address) {
            *count -= 1;
            if *count == 0 {
                self.ref_counts.remove(&address);
                Ok(true) // Should deallocate
            } else {
                Ok(false) // Keep alive
            }
        } else {
            Err(CompilerError::memory_error("Invalid memory address for release", None, None))
        }
    }

    /// Get reference count
    pub fn get_ref_count(&self, address: MemoryAddress) -> RefCount {
        self.ref_counts.get(&address).copied().unwrap_or(0)
    }
}

impl Default for ARCManager {
    fn default() -> Self {
        Self::new()
    }
}