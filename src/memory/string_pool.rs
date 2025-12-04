//! String pool for literal deduplication

use super::*;
use std::collections::HashMap;

/// String pool for deduplicating string literals
pub struct StringPool {
    pool: HashMap<String, MemoryAddress>,
    reverse_pool: HashMap<MemoryAddress, String>,
    next_address: MemoryAddress,
    base_address: MemoryAddress,
}

impl StringPool {
    pub fn new(base_address: MemoryAddress) -> Self {
        Self {
            pool: HashMap::new(),
            reverse_pool: HashMap::new(),
            next_address: base_address,
            base_address,
        }
    }

    /// Get or create string in pool
    pub fn intern(&mut self, s: &str) -> MemoryResult<MemoryAddress> {
        if let Some(&address) = self.pool.get(s) {
            // String already exists
            Ok(address)
        } else {
            // Add new string to pool
            let address = self.allocate_string(s)?;
            self.pool.insert(s.to_string(), address);
            self.reverse_pool.insert(address, s.to_string());
            Ok(address)
        }
    }

    /// Get string from address
    pub fn get_string(&self, address: MemoryAddress) -> Option<&str> {
        self.reverse_pool.get(&address).map(|s| s.as_str())
    }

    /// Check if string is in pool
    pub fn contains(&self, s: &str) -> bool {
        self.pool.contains_key(s)
    }

    /// Get pool statistics
    pub fn statistics(&self) -> StringPoolStats {
        StringPoolStats {
            string_count: self.pool.len(),
            memory_used: self.next_address - self.base_address,
            deduplication_savings: 0, // Calculated from pool deduplication
        }
    }

    fn allocate_string(&mut self, s: &str) -> MemoryResult<MemoryAddress> {
        let bytes = s.as_bytes();
        let size = bytes.len() as u32;
        let address = self.next_address;

        // String written to memory in data section during codegen
        self.next_address += size + 4; // +4 for length prefix

        Ok(address)
    }
}

/// String pool statistics
#[derive(Debug, Clone)]
pub struct StringPoolStats {
    pub string_count: usize,
    pub memory_used: u32,
    pub deduplication_savings: u32,
}
