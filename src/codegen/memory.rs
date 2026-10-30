//! Static data placement for code generation.
//!
//! `MemoryUtils` owns the WASM data section. Every compile-time constant
//! (string literals, test-runner labels, `bool_to_string`'s "true"/"false")
//! is placed through `CodeGenerator::get_or_create_string_offset`, which
//! assigns the address and calls `add_data_segment` here. Runtime memory is
//! managed in WASM by the bump allocator (`__heap_ptr`), not by the compiler.

use wasm_encoder::{ConstExpr, DataSection};

/// Maximum data-section size (1 MB).  All static data (string literals, globals)
/// must fit within this region.  The runtime heap (`__heap_ptr`) starts
/// immediately after at `native_stdlib::HEAP_START` (also 1 MB).
const MAX_MEMORY_SIZE: usize = 1048576;

/// Rejected data-segment placement. Fields carry diagnostic payload for the
/// `{:?}` formatting done by callers.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum MemorySafetyError {
    BufferOverflow {
        address: usize,
        size: usize,
        attempted_size: usize,
    },
    InvalidPointer {
        address: usize,
    },
    UnalignedAccess {
        address: usize,
        alignment: usize,
    },
}

/// Owner of the WASM data section, with bounds validation for each segment.
pub(crate) struct MemoryUtils {
    data_section: DataSection,
    /// Lowest address static data may occupy (`DATA_SECTION_START`).
    data_start: usize,
}

impl MemoryUtils {
    /// Create an empty data section whose segments must lie in
    /// `[data_start, MAX_MEMORY_SIZE)`.
    pub(crate) fn new(data_start: usize) -> Self {
        Self {
            data_section: DataSection::new(),
            data_start,
        }
    }

    /// Add a data segment with bounds validation
    pub(crate) fn add_data_segment(
        &mut self,
        offset: u32,
        data: &[u8],
    ) -> Result<(), MemorySafetyError> {
        self.validate_memory_access(offset as usize, data.len())?;

        let offset_expr = ConstExpr::i32_const(offset as i32);
        self.data_section.active(0, &offset_expr, data.to_vec());
        Ok(())
    }

    /// Check that a segment is non-null, 4-byte aligned when larger than a
    /// word, and fully inside the static data region.
    fn validate_memory_access(&self, address: usize, size: usize) -> Result<(), MemorySafetyError> {
        if address == 0 {
            return Err(MemorySafetyError::InvalidPointer { address });
        }

        // WASM data segments only need byte alignment; 4-byte alignment is
        // enforced for multi-word segments because string pointers are
        // loaded with `i32.load` (align 2).
        if !address.is_multiple_of(4) && size > 4 {
            return Err(MemorySafetyError::UnalignedAccess {
                address,
                alignment: 4,
            });
        }

        if address < self.data_start || address >= MAX_MEMORY_SIZE {
            return Err(MemorySafetyError::InvalidPointer { address });
        }
        if address + size > MAX_MEMORY_SIZE {
            return Err(MemorySafetyError::BufferOverflow {
                address,
                size: MAX_MEMORY_SIZE - address,
                attempted_size: size,
            });
        }
        Ok(())
    }

    /// Get the data section
    pub(crate) fn get_data_section(&self) -> &DataSection {
        &self.data_section
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_segment_inside_data_region() {
        let mut mem = MemoryUtils::new(1024);
        assert!(mem.add_data_segment(4096, b"\x05\0\0\0hello").is_ok());
    }

    #[test]
    fn rejects_null_misaligned_and_out_of_region_segments() {
        let mut mem = MemoryUtils::new(1024);
        assert!(matches!(
            mem.add_data_segment(0, b"x"),
            Err(MemorySafetyError::InvalidPointer { .. })
        ));
        assert!(matches!(
            mem.add_data_segment(4098, b"\x05\0\0\0hello"),
            Err(MemorySafetyError::UnalignedAccess { .. })
        ));
        assert!(matches!(
            mem.add_data_segment(512, b"x"),
            Err(MemorySafetyError::InvalidPointer { .. })
        ));
        assert!(matches!(
            mem.add_data_segment((MAX_MEMORY_SIZE - 4) as u32, b"\x05\0\0\0hello"),
            Err(MemorySafetyError::BufferOverflow { .. })
        ));
    }
}
