# Clean Language Memory Management Documentation

This document provides comprehensive guidance for Claude on Clean Language's memory management system, including garbage collection, reference counting, WebAssembly memory model integration, and performance optimization strategies. This knowledge is essential for maintaining memory safety while achieving optimal performance.

> 🔗 **Related Documentation**: [WebAssembly Generation](./webassembly.md) • [Standard Library](./standard-library.md) • [IR Documentation](./intermediate-representation.md) • [Development Guide](./development-guide.md)

## Overview

Clean Language employs a hybrid memory management approach combining reference counting for immediate deallocation with mark-and-sweep garbage collection for cycle detection. This system is designed specifically for WebAssembly's linear memory model and provides both safety and performance for Clean Language programs.

## Memory Architecture

### 1. WebAssembly Memory Layout (`src/memory/layout.rs`)

```rust
/// WebAssembly linear memory layout for Clean Language
pub struct MemoryLayout {
    pub heap_start: u32,
    pub heap_size: u32,
    pub stack_start: u32,
    pub stack_size: u32,
    pub globals_start: u32,
    pub globals_size: u32,
    pub string_pool_start: u32,
    pub string_pool_size: u32,
}

impl MemoryLayout {
    pub const DEFAULT_LAYOUT: MemoryLayout = MemoryLayout {
        heap_start: 0x10000,        // 64KB offset
        heap_size: 0x100000,        // 1MB heap
        stack_start: 0x110000,      // After heap
        stack_size: 0x10000,        // 64KB stack
        globals_start: 0x120000,    // After stack
        globals_size: 0x1000,       // 4KB globals
        string_pool_start: 0x121000, // After globals
        string_pool_size: 0x10000,  // 64KB string pool
    };
    
    pub fn validate(&self) -> Result<(), MemoryLayoutError> {
        // Check for overlaps
        let regions = vec![
            ("heap", self.heap_start, self.heap_size),
            ("stack", self.stack_start, self.stack_size),
            ("globals", self.globals_start, self.globals_size),
            ("string_pool", self.string_pool_start, self.string_pool_size),
        ];
        
        for (i, (name1, start1, size1)) in regions.iter().enumerate() {
            let end1 = start1 + size1;
            for (name2, start2, size2) in regions.iter().skip(i + 1) {
                let end2 = start2 + size2;
                
                if (start1 < &end2 && start2 < &end1) {
                    return Err(MemoryLayoutError::OverlappingRegions {
                        region1: name1.to_string(),
                        region2: name2.to_string(),
                    });
                }
            }
        }
        
        Ok(())
    }
}
```

### 2. Object Header Structure

```rust
/// Universal object header for all Clean Language objects
#[repr(C)]
pub struct ObjectHeader {
    pub ref_count: u32,      // Reference count for ARC
    pub type_id: u32,        // Object type identifier
    pub size: u32,           // Object size in bytes (including header)
    pub gc_flags: u8,        // GC mark bits and flags
    pub padding: [u8; 3],    // Alignment padding
}

impl ObjectHeader {
    pub const SIZE: usize = std::mem::size_of::<ObjectHeader>();
    pub const ALIGNMENT: usize = 16;  // 16-byte alignment for performance
    
    pub const GC_MARK_MASK: u8 = 0x01;
    pub const GC_ROOT_MASK: u8 = 0x02;
    pub const GC_PINNED_MASK: u8 = 0x04;
    pub const GC_GENERATION_MASK: u8 = 0x30;  // 2 bits for generation
    
    pub fn new(type_id: u32, size: u32) -> Self {
        Self {
            ref_count: 1,
            type_id,
            size,
            gc_flags: 0,
            padding: [0; 3],
        }
    }
    
    pub fn is_marked(&self) -> bool {
        (self.gc_flags & Self::GC_MARK_MASK) != 0
    }
    
    pub fn mark(&mut self) {
        self.gc_flags |= Self::GC_MARK_MASK;
    }
    
    pub fn unmark(&mut self) {
        self.gc_flags &= !Self::GC_MARK_MASK;
    }
    
    pub fn is_root(&self) -> bool {
        (self.gc_flags & Self::GC_ROOT_MASK) != 0
    }
    
    pub fn set_root(&mut self, is_root: bool) {
        if is_root {
            self.gc_flags |= Self::GC_ROOT_MASK;
        } else {
            self.gc_flags &= !Self::GC_ROOT_MASK;
        }
    }
    
    pub fn generation(&self) -> u8 {
        (self.gc_flags & Self::GC_GENERATION_MASK) >> 4
    }
    
    pub fn set_generation(&mut self, generation: u8) {
        self.gc_flags = (self.gc_flags & !Self::GC_GENERATION_MASK) | ((generation & 0x03) << 4);
    }
}

/// Type identifiers for different object types
pub mod type_ids {
    pub const STRING: u32 = 1;
    pub const LIST: u32 = 2;
    pub const MATRIX: u32 = 3;
    pub const OBJECT: u32 = 4;
    pub const FUNCTION: u32 = 5;
    pub const CLOSURE: u32 = 6;
}
```

### 3. Memory Manager Core (`src/memory/manager.rs`)

```rust
/// Central memory manager for Clean Language
pub struct MemoryManager {
    layout: MemoryLayout,
    heap_allocator: HeapAllocator,
    gc_state: GarbageCollector,
    string_pool: StringPool,
    statistics: MemoryStatistics,
    roots: HashSet<u32>,  // Root object addresses
}

impl MemoryManager {
    pub fn new(layout: MemoryLayout) -> Result<Self, MemoryError> {
        layout.validate()?;
        
        Ok(Self {
            layout,
            heap_allocator: HeapAllocator::new(layout.heap_start, layout.heap_size),
            gc_state: GarbageCollector::new(),
            string_pool: StringPool::new(layout.string_pool_start, layout.string_pool_size),
            statistics: MemoryStatistics::new(),
            roots: HashSet::new(),
        })
    }
    
    pub fn allocate(&mut self, size: usize, type_id: u32) -> Result<u32, MemoryError> {
        let aligned_size = align_up(size + ObjectHeader::SIZE, ObjectHeader::ALIGNMENT);
        
        // Try allocation
        let ptr = self.heap_allocator.allocate(aligned_size)?;
        
        // Initialize object header
        unsafe {
            let header = &mut *(ptr as *mut ObjectHeader);
            *header = ObjectHeader::new(type_id, aligned_size as u32);
        }
        
        // Update statistics
        self.statistics.record_allocation(aligned_size);
        
        // Return pointer to object data (after header)
        Ok(ptr + ObjectHeader::SIZE as u32)
    }
    
    pub fn allocate_with_gc(&mut self, size: usize, type_id: u32) -> Result<u32, MemoryError> {
        match self.allocate(size, type_id) {
            Ok(ptr) => Ok(ptr),
            Err(MemoryError::OutOfMemory) => {
                // Trigger garbage collection and try again
                let collected = self.gc_state.collect(self)?;
                self.statistics.record_gc_run(collected);
                
                self.allocate(size, type_id)
            }
            Err(e) => Err(e),
        }
    }
    
    pub fn retain(&mut self, ptr: u32) -> Result<(), MemoryError> {
        let header_ptr = ptr - ObjectHeader::SIZE as u32;
        let header = self.get_header_mut(header_ptr)?;
        
        if header.ref_count == u32::MAX {
            return Err(MemoryError::RefCountOverflow);
        }
        
        header.ref_count += 1;
        Ok(())
    }
    
    pub fn release(&mut self, ptr: u32) -> Result<(), MemoryError> {
        let header_ptr = ptr - ObjectHeader::SIZE as u32;
        let header = self.get_header_mut(header_ptr)?;
        
        if header.ref_count == 0 {
            return Err(MemoryError::InvalidRefCount);
        }
        
        header.ref_count -= 1;
        
        if header.ref_count == 0 {
            self.deallocate_object(header_ptr)?;
        }
        
        Ok(())
    }
    
    pub fn add_root(&mut self, ptr: u32) {
        self.roots.insert(ptr);
        if let Ok(header) = self.get_header_mut(ptr - ObjectHeader::SIZE as u32) {
            header.set_root(true);
        }
    }
    
    pub fn remove_root(&mut self, ptr: u32) {
        self.roots.remove(&ptr);
        if let Ok(header) = self.get_header_mut(ptr - ObjectHeader::SIZE as u32) {
            header.set_root(false);
        }
    }
    
    fn deallocate_object(&mut self, header_ptr: u32) -> Result<(), MemoryError> {
        let header = self.get_header(header_ptr)?;
        let size = header.size as usize;
        let type_id = header.type_id;
        
        // Call type-specific destructor
        self.call_destructor(header_ptr, type_id)?;
        
        // Free memory
        self.heap_allocator.deallocate(header_ptr, size)?;
        
        // Update statistics
        self.statistics.record_deallocation(size);
        
        Ok(())
    }
    
    fn call_destructor(&mut self, header_ptr: u32, type_id: u32) -> Result<(), MemoryError> {
        match type_id {
            type_ids::STRING => {
                // String objects don't need special cleanup
                Ok(())
            }
            type_ids::LIST => {
                // Release references to list elements
                self.destruct_list(header_ptr + ObjectHeader::SIZE as u32)
            }
            type_ids::OBJECT => {
                // Release references to object fields
                self.destruct_object_fields(header_ptr + ObjectHeader::SIZE as u32)
            }
            _ => Ok(()),
        }
    }
    
    fn destruct_list(&mut self, list_ptr: u32) -> Result<(), MemoryError> {
        let list_info = self.get_list_info(list_ptr)?;
        
        for i in 0..list_info.length {
            let element_ptr = self.get_list_element(list_ptr, i)?;
            if element_ptr != 0 {
                self.release(element_ptr)?;
            }
        }
        
        Ok(())
    }
    
    fn get_header(&self, header_ptr: u32) -> Result<&ObjectHeader, MemoryError> {
        if !self.is_valid_pointer(header_ptr) {
            return Err(MemoryError::InvalidPointer(header_ptr));
        }
        
        unsafe {
            Ok(&*(header_ptr as *const ObjectHeader))
        }
    }
    
    fn get_header_mut(&mut self, header_ptr: u32) -> Result<&mut ObjectHeader, MemoryError> {
        if !self.is_valid_pointer(header_ptr) {
            return Err(MemoryError::InvalidPointer(header_ptr));
        }
        
        unsafe {
            Ok(&mut *(header_ptr as *mut ObjectHeader))
        }
    }
    
    fn is_valid_pointer(&self, ptr: u32) -> bool {
        ptr >= self.layout.heap_start && 
        ptr < self.layout.heap_start + self.layout.heap_size &&
        (ptr % ObjectHeader::ALIGNMENT as u32) == 0
    }
}

fn align_up(size: usize, alignment: usize) -> usize {
    (size + alignment - 1) & !(alignment - 1)
}
```

## Garbage Collection System

### 1. Mark-and-Sweep Collector (`src/memory/gc.rs`)

```rust
/// Mark-and-sweep garbage collector with generational support
pub struct GarbageCollector {
    generations: [Generation; 3],  // Young, mature, old
    current_generation: u8,
    collection_threshold: usize,
    total_collections: u64,
    last_collection_time: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct Generation {
    pub id: u8,
    pub objects: HashSet<u32>,     // Object pointers in this generation
    pub allocation_count: usize,
    pub collection_count: u64,
    pub survival_rate: f64,
}

impl GarbageCollector {
    pub fn new() -> Self {
        Self {
            generations: [
                Generation { id: 0, objects: HashSet::new(), allocation_count: 0, collection_count: 0, survival_rate: 0.1 },
                Generation { id: 1, objects: HashSet::new(), allocation_count: 0, collection_count: 0, survival_rate: 0.8 },
                Generation { id: 2, objects: HashSet::new(), allocation_count: 0, collection_count: 0, survival_rate: 0.95 },
            ],
            current_generation: 0,
            collection_threshold: 1000,  // Objects allocated before GC
            total_collections: 0,
            last_collection_time: std::time::Instant::now(),
        }
    }
    
    pub fn collect(&mut self, memory_manager: &mut MemoryManager) -> Result<usize, MemoryError> {
        let start_time = std::time::Instant::now();
        let mut collected_objects = 0;
        
        // Determine which generation to collect
        let generation_to_collect = self.select_generation_for_collection();
        
        collected_objects += self.collect_generation(generation_to_collect, memory_manager)?;
        
        // Promote survivors to next generation
        self.promote_survivors(generation_to_collect, memory_manager)?;
        
        // Update statistics
        let collection_time = start_time.elapsed();
        self.generations[generation_to_collect as usize].collection_count += 1;
        self.total_collections += 1;
        self.last_collection_time = start_time;
        
        Ok(collected_objects)
    }
    
    fn select_generation_for_collection(&self) -> u8 {
        // Simple heuristic: collect young generation most frequently
        match self.total_collections % 10 {
            0..=6 => 0,  // Young generation (70% of collections)
            7..=8 => 1,  // Mature generation (20% of collections)  
            9 => 2,      // Old generation (10% of collections)
            _ => 0,
        }
    }
    
    fn collect_generation(&mut self, generation: u8, memory_manager: &mut MemoryManager) -> Result<usize, MemoryError> {
        let mut collected = 0;
        
        // Mark phase
        self.mark_roots(memory_manager)?;
        self.mark_reachable_objects(generation, memory_manager)?;
        
        // Sweep phase
        let objects_to_collect: Vec<u32> = self.generations[generation as usize]
            .objects
            .iter()
            .cloned()
            .filter(|&ptr| {
                memory_manager.get_header(ptr - ObjectHeader::SIZE as u32)
                    .map(|h| !h.is_marked())
                    .unwrap_or(true)
            })
            .collect();
        
        for object_ptr in objects_to_collect {
            memory_manager.deallocate_object(object_ptr - ObjectHeader::SIZE as u32)?;
            self.generations[generation as usize].objects.remove(&object_ptr);
            collected += 1;
        }
        
        // Unmark all objects for next collection
        self.unmark_all_objects(memory_manager)?;
        
        Ok(collected)
    }
    
    fn mark_roots(&self, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        for &root_ptr in &memory_manager.roots {
            self.mark_object(root_ptr, memory_manager)?;
        }
        Ok(())
    }
    
    fn mark_reachable_objects(&self, generation: u8, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        let mut worklist: Vec<u32> = memory_manager.roots.iter().cloned().collect();
        
        while let Some(object_ptr) = worklist.pop() {
            let header_ptr = object_ptr - ObjectHeader::SIZE as u32;
            let header = memory_manager.get_header(header_ptr)?;
            
            if header.is_marked() {
                continue;
            }
            
            // Skip objects not in the generation being collected (and newer)
            if header.generation() > generation {
                continue;
            }
            
            // Mark this object
            memory_manager.get_header_mut(header_ptr)?.mark();
            
            // Add referenced objects to worklist
            let referenced_objects = self.get_object_references(object_ptr, header.type_id, memory_manager)?;
            worklist.extend(referenced_objects);
        }
        
        Ok(())
    }
    
    fn get_object_references(&self, object_ptr: u32, type_id: u32, memory_manager: &MemoryManager) -> Result<Vec<u32>, MemoryError> {
        match type_id {
            type_ids::LIST => {
                let list_info = memory_manager.get_list_info(object_ptr)?;
                let mut references = Vec::new();
                
                for i in 0..list_info.length {
                    let element_ptr = memory_manager.get_list_element(object_ptr, i)?;
                    if element_ptr != 0 {
                        references.push(element_ptr);
                    }
                }
                
                Ok(references)
            }
            type_ids::OBJECT => {
                // Get object field references
                memory_manager.get_object_field_references(object_ptr)
            }
            type_ids::CLOSURE => {
                // Get captured variable references
                memory_manager.get_closure_references(object_ptr)
            }
            _ => Ok(Vec::new()),
        }
    }
    
    fn promote_survivors(&mut self, collected_generation: u8, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        if collected_generation >= 2 {
            return Ok();  // Already in oldest generation
        }
        
        let next_generation = collected_generation + 1;
        let survivors: Vec<u32> = self.generations[collected_generation as usize]
            .objects
            .iter()
            .cloned()
            .filter(|&ptr| {
                memory_manager.get_header(ptr - ObjectHeader::SIZE as u32)
                    .map(|h| h.is_marked())
                    .unwrap_or(false)
            })
            .collect();
        
        for survivor_ptr in survivors {
            // Move to next generation
            self.generations[collected_generation as usize].objects.remove(&survivor_ptr);
            self.generations[next_generation as usize].objects.insert(survivor_ptr);
            
            // Update object generation
            let header_ptr = survivor_ptr - ObjectHeader::SIZE as u32;
            memory_manager.get_header_mut(header_ptr)?.set_generation(next_generation);
        }
        
        Ok(())
    }
    
    fn unmark_all_objects(&self, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        for generation in &self.generations {
            for &object_ptr in &generation.objects {
                let header_ptr = object_ptr - ObjectHeader::SIZE as u32;
                memory_manager.get_header_mut(header_ptr)?.unmark();
            }
        }
        Ok(())
    }
    
    fn mark_object(&self, object_ptr: u32, memory_manager: &mut MemoryManager) -> Result<(), MemoryError> {
        let header_ptr = object_ptr - ObjectHeader::SIZE as u32;
        memory_manager.get_header_mut(header_ptr)?.mark();
        Ok(())
    }
    
    pub fn should_collect(&self) -> bool {
        self.generations[0].allocation_count > self.collection_threshold
    }
    
    pub fn record_allocation(&mut self, object_ptr: u32, generation: u8) {
        self.generations[generation as usize].objects.insert(object_ptr);
        self.generations[generation as usize].allocation_count += 1;
    }
}
```

### 2. Heap Allocator (`src/memory/allocator.rs`)

```rust
/// Heap allocator using segregated free lists
pub struct HeapAllocator {
    base_address: u32,
    heap_size: u32,
    free_lists: [FreeList; 8],  // Different size classes
    large_objects: BTreeMap<u32, usize>,  // For objects > 1KB
    allocated_bytes: usize,
    free_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct FreeList {
    size_class: usize,  // Max object size for this list
    blocks: Vec<FreeBlock>,
}

#[derive(Debug, Clone)]
pub struct FreeBlock {
    address: u32,
    size: usize,
}

impl HeapAllocator {
    pub fn new(base_address: u32, heap_size: u32) -> Self {
        let size_classes = [32, 64, 128, 256, 512, 1024, 2048, 4096];
        let mut free_lists = [
            FreeList { size_class: 0, blocks: Vec::new() }; 8
        ];
        
        for (i, &size) in size_classes.iter().enumerate() {
            free_lists[i].size_class = size;
        }
        
        // Initialize with one large free block
        free_lists[7].blocks.push(FreeBlock {
            address: base_address,
            size: heap_size as usize,
        });
        
        Self {
            base_address,
            heap_size,
            free_lists,
            large_objects: BTreeMap::new(),
            allocated_bytes: 0,
            free_bytes: heap_size as usize,
        }
    }
    
    pub fn allocate(&mut self, size: usize) -> Result<u32, MemoryError> {
        let aligned_size = align_up(size, ObjectHeader::ALIGNMENT);
        
        if aligned_size > 4096 {
            // Large object allocation
            self.allocate_large_object(aligned_size)
        } else {
            // Small object allocation using free lists
            let size_class_index = self.find_size_class(aligned_size);
            self.allocate_from_free_list(size_class_index, aligned_size)
        }
    }
    
    pub fn deallocate(&mut self, address: u32, size: usize) -> Result<(), MemoryError> {
        let aligned_size = align_up(size, ObjectHeader::ALIGNMENT);
        
        if aligned_size > 4096 {
            // Large object deallocation
            self.deallocate_large_object(address, aligned_size)
        } else {
            // Small object deallocation
            self.deallocate_to_free_list(address, aligned_size)
        }
    }
    
    fn find_size_class(&self, size: usize) -> usize {
        for (i, free_list) in self.free_lists.iter().enumerate() {
            if size <= free_list.size_class {
                return i;
            }
        }
        self.free_lists.len() - 1  // Fallback to largest size class
    }
    
    fn allocate_from_free_list(&mut self, size_class_index: usize, size: usize) -> Result<u32, MemoryError> {
        // Try exact size class first
        for class_index in size_class_index..self.free_lists.len() {
            if let Some(block) = self.free_lists[class_index].blocks.pop() {
                if block.size >= size {
                    let allocated_address = block.address;
                    
                    // Split block if necessary
                    if block.size > size + ObjectHeader::ALIGNMENT {
                        let remaining_block = FreeBlock {
                            address: block.address + size as u32,
                            size: block.size - size,
                        };
                        
                        let remaining_class = self.find_size_class(remaining_block.size);
                        self.free_lists[remaining_class].blocks.push(remaining_block);
                    }
                    
                    self.allocated_bytes += size;
                    self.free_bytes -= size;
                    
                    return Ok(allocated_address);
                }
            }
        }
        
        Err(MemoryError::OutOfMemory)
    }
    
    fn allocate_large_object(&mut self, size: usize) -> Result<u32, MemoryError> {
        // Find a large enough free block
        for free_list in &mut self.free_lists {
            for (i, block) in free_list.blocks.iter().enumerate() {
                if block.size >= size {
                    let allocated_address = block.address;
                    let block = free_list.blocks.remove(i);
                    
                    // Split block if necessary
                    if block.size > size + ObjectHeader::ALIGNMENT {
                        let remaining_block = FreeBlock {
                            address: block.address + size as u32,
                            size: block.size - size,
                        };
                        
                        let remaining_class = self.find_size_class(remaining_block.size);
                        if remaining_class < self.free_lists.len() {
                            self.free_lists[remaining_class].blocks.push(remaining_block);
                        }
                    }
                    
                    self.large_objects.insert(allocated_address, size);
                    self.allocated_bytes += size;
                    self.free_bytes -= size;
                    
                    return Ok(allocated_address);
                }
            }
        }
        
        Err(MemoryError::OutOfMemory)
    }
    
    fn deallocate_to_free_list(&mut self, address: u32, size: usize) -> Result<(), MemoryError> {
        // Add block back to appropriate free list
        let size_class = self.find_size_class(size);
        let free_block = FreeBlock { address, size };
        
        // Try to coalesce with adjacent blocks
        let coalesced_block = self.coalesce_blocks(free_block);
        
        let final_size_class = self.find_size_class(coalesced_block.size);
        self.free_lists[final_size_class].blocks.push(coalesced_block);
        
        self.allocated_bytes -= size;
        self.free_bytes += size;
        
        Ok(())
    }
    
    fn deallocate_large_object(&mut self, address: u32, size: usize) -> Result<(), MemoryError> {
        if self.large_objects.remove(&address).is_none() {
            return Err(MemoryError::InvalidPointer(address));
        }
        
        // Add back to free lists for future allocations
        let free_block = FreeBlock { address, size };
        let coalesced_block = self.coalesce_blocks(free_block);
        
        if coalesced_block.size > 4096 {
            // Keep as large block in largest size class
            self.free_lists[self.free_lists.len() - 1].blocks.push(coalesced_block);
        } else {
            let size_class = self.find_size_class(coalesced_block.size);
            self.free_lists[size_class].blocks.push(coalesced_block);
        }
        
        self.allocated_bytes -= size;
        self.free_bytes += size;
        
        Ok(())
    }
    
    fn coalesce_blocks(&mut self, mut block: FreeBlock) -> FreeBlock {
        let mut changed = true;
        
        while changed {
            changed = false;
            
            for free_list in &mut self.free_lists {
                let mut i = 0;
                while i < free_list.blocks.len() {
                    let other_block = &free_list.blocks[i];
                    
                    // Check if blocks are adjacent
                    if block.address + block.size as u32 == other_block.address {
                        // Coalesce: extend current block
                        let other_block = free_list.blocks.remove(i);
                        block.size += other_block.size;
                        changed = true;
                        break;
                    } else if other_block.address + other_block.size as u32 == block.address {
                        // Coalesce: extend current block backward
                        let other_block = free_list.blocks.remove(i);
                        block.address = other_block.address;
                        block.size += other_block.size;
                        changed = true;
                        break;
                    } else {
                        i += 1;
                    }
                }
                
                if changed {
                    break;
                }
            }
        }
        
        block
    }
    
    pub fn fragmentation_ratio(&self) -> f64 {
        if self.free_bytes == 0 {
            return 0.0;
        }
        
        let total_free_blocks: usize = self.free_lists.iter()
            .map(|list| list.blocks.len())
            .sum();
        
        if total_free_blocks <= 1 {
            0.0  // No fragmentation
        } else {
            // Simple fragmentation metric: number of free blocks vs. ideal (1)
            (total_free_blocks as f64 - 1.0) / self.free_bytes as f64 * 1000.0
        }
    }
}
```

### 3. String Pool Management (`src/memory/string_pool.rs`)

```rust
/// String pool for deduplicating string literals
pub struct StringPool {
    base_address: u32,
    pool_size: u32,
    interned_strings: HashMap<u64, u32>,  // hash -> pointer
    string_data: Vec<u8>,
    next_offset: usize,
}

impl StringPool {
    pub fn new(base_address: u32, pool_size: u32) -> Self {
        Self {
            base_address,
            pool_size,
            interned_strings: HashMap::new(),
            string_data: vec![0; pool_size as usize],
            next_offset: 0,
        }
    }
    
    pub fn intern(&mut self, string_data: &[u8]) -> Result<u32, MemoryError> {
        let hash = self.hash_string(string_data);
        
        // Check if string is already interned
        if let Some(&ptr) = self.interned_strings.get(&hash) {
            return Ok(ptr);
        }
        
        // Allocate space for new string
        let string_size = string_data.len();
        let total_size = ObjectHeader::SIZE + 4 + string_size;  // header + length + data
        
        if self.next_offset + total_size > self.pool_size as usize {
            return Err(MemoryError::StringPoolExhausted);
        }
        
        let string_ptr = self.base_address + self.next_offset as u32;
        
        // Write string header
        let header = ObjectHeader::new(type_ids::STRING, total_size as u32);
        unsafe {
            std::ptr::write(string_ptr as *mut ObjectHeader, header);
        }
        
        // Write string length
        let length_ptr = string_ptr + ObjectHeader::SIZE as u32;
        unsafe {
            std::ptr::write(length_ptr as *mut u32, string_size as u32);
        }
        
        // Write string data
        let data_ptr = length_ptr + 4;
        unsafe {
            std::ptr::copy_nonoverlapping(
                string_data.as_ptr(),
                data_ptr as *mut u8,
                string_size
            );
        }
        
        self.next_offset += total_size;
        
        // Store in intern table
        let user_ptr = string_ptr + ObjectHeader::SIZE as u32;
        self.interned_strings.insert(hash, user_ptr);
        
        Ok(user_ptr)
    }
    
    pub fn get_string_data(&self, ptr: u32) -> Result<&[u8], MemoryError> {
        let length_ptr = ptr;
        let length = unsafe { *(length_ptr as *const u32) } as usize;
        let data_ptr = length_ptr + 4;
        
        if data_ptr + length as u32 > self.base_address + self.pool_size {
            return Err(MemoryError::InvalidPointer(ptr));
        }
        
        unsafe {
            Ok(std::slice::from_raw_parts(data_ptr as *const u8, length))
        }
    }
    
    pub fn get_string_length(&self, ptr: u32) -> Result<u32, MemoryError> {
        let length_ptr = ptr;
        Ok(unsafe { *(length_ptr as *const u32) })
    }
    
    fn hash_string(&self, data: &[u8]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }
    
    pub fn usage_stats(&self) -> StringPoolStats {
        StringPoolStats {
            total_size: self.pool_size as usize,
            used_size: self.next_offset,
            free_size: self.pool_size as usize - self.next_offset,
            interned_count: self.interned_strings.len(),
            utilization: self.next_offset as f64 / self.pool_size as f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StringPoolStats {
    pub total_size: usize,
    pub used_size: usize,
    pub free_size: usize,
    pub interned_count: usize,
    pub utilization: f64,
}
```

## Memory Statistics and Monitoring

### 1. Memory Statistics (`src/memory/statistics.rs`)

```rust
/// Comprehensive memory statistics for monitoring and debugging
#[derive(Debug, Clone)]
pub struct MemoryStatistics {
    pub total_allocated: u64,
    pub total_freed: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
    pub allocation_count: u64,
    pub deallocation_count: u64,
    pub gc_runs: u64,
    pub gc_time_total: std::time::Duration,
    pub gc_objects_collected: u64,
    pub fragmentation_events: u64,
    pub out_of_memory_events: u64,
    pub string_pool_stats: StringPoolStats,
    pub generation_stats: [GenerationStats; 3],
}

#[derive(Debug, Clone)]
pub struct GenerationStats {
    pub generation: u8,
    pub objects_allocated: u64,
    pub objects_promoted: u64,
    pub objects_collected: u64,
    pub collection_count: u64,
    pub average_survival_rate: f64,
}

impl MemoryStatistics {
    pub fn new() -> Self {
        Self {
            total_allocated: 0,
            total_freed: 0,
            current_usage: 0,
            peak_usage: 0,
            allocation_count: 0,
            deallocation_count: 0,
            gc_runs: 0,
            gc_time_total: std::time::Duration::default(),
            gc_objects_collected: 0,
            fragmentation_events: 0,
            out_of_memory_events: 0,
            string_pool_stats: StringPoolStats {
                total_size: 0,
                used_size: 0,
                free_size: 0,
                interned_count: 0,
                utilization: 0.0,
            },
            generation_stats: [
                GenerationStats { generation: 0, objects_allocated: 0, objects_promoted: 0, objects_collected: 0, collection_count: 0, average_survival_rate: 0.0 },
                GenerationStats { generation: 1, objects_allocated: 0, objects_promoted: 0, objects_collected: 0, collection_count: 0, average_survival_rate: 0.0 },
                GenerationStats { generation: 2, objects_allocated: 0, objects_promoted: 0, objects_collected: 0, collection_count: 0, average_survival_rate: 0.0 },
            ],
        }
    }
    
    pub fn record_allocation(&mut self, size: usize) {
        self.total_allocated += size as u64;
        self.current_usage += size as u64;
        self.allocation_count += 1;
        
        if self.current_usage > self.peak_usage {
            self.peak_usage = self.current_usage;
        }
    }
    
    pub fn record_deallocation(&mut self, size: usize) {
        self.total_freed += size as u64;
        self.current_usage = self.current_usage.saturating_sub(size as u64);
        self.deallocation_count += 1;
    }
    
    pub fn record_gc_run(&mut self, objects_collected: usize) {
        self.gc_runs += 1;
        self.gc_objects_collected += objects_collected as u64;
    }
    
    pub fn record_gc_time(&mut self, gc_time: std::time::Duration) {
        self.gc_time_total += gc_time;
    }
    
    pub fn record_out_of_memory(&mut self) {
        self.out_of_memory_events += 1;
    }
    
    pub fn record_fragmentation_event(&mut self) {
        self.fragmentation_events += 1;
    }
    
    pub fn update_string_pool_stats(&mut self, stats: StringPoolStats) {
        self.string_pool_stats = stats;
    }
    
    pub fn update_generation_stats(&mut self, generation: u8, stats: GenerationStats) {
        if generation < 3 {
            self.generation_stats[generation as usize] = stats;
        }
    }
    
    pub fn memory_efficiency(&self) -> f64 {
        if self.total_allocated == 0 {
            return 1.0;
        }
        
        let useful_memory = self.current_usage;
        let total_allocated = self.total_allocated;
        
        useful_memory as f64 / total_allocated as f64
    }
    
    pub fn average_gc_time(&self) -> std::time::Duration {
        if self.gc_runs == 0 {
            std::time::Duration::default()
        } else {
            self.gc_time_total / self.gc_runs as u32
        }
    }
    
    pub fn gc_effectiveness(&self) -> f64 {
        if self.gc_runs == 0 {
            return 0.0;
        }
        
        self.gc_objects_collected as f64 / self.gc_runs as f64
    }
    
    pub fn print_report(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        writeln!(output, "Memory Statistics Report")?;
        writeln!(output, "========================")?;
        writeln!(output, "Total allocated: {} bytes", self.total_allocated)?;
        writeln!(output, "Total freed: {} bytes", self.total_freed)?;
        writeln!(output, "Current usage: {} bytes", self.current_usage)?;
        writeln!(output, "Peak usage: {} bytes", self.peak_usage)?;
        writeln!(output, "Allocations: {}", self.allocation_count)?;
        writeln!(output, "Deallocations: {}", self.deallocation_count)?;
        writeln!(output, "Memory efficiency: {:.2}%", self.memory_efficiency() * 100.0)?;
        writeln!(output)?;
        
        writeln!(output, "Garbage Collection:")?;
        writeln!(output, "GC runs: {}", self.gc_runs)?;
        writeln!(output, "Objects collected: {}", self.gc_objects_collected)?;
        writeln!(output, "Total GC time: {:.2}ms", self.gc_time_total.as_millis())?;
        writeln!(output, "Average GC time: {:.2}ms", self.average_gc_time().as_millis())?;
        writeln!(output, "GC effectiveness: {:.2} objects/run", self.gc_effectiveness())?;
        writeln!(output)?;
        
        writeln!(output, "String Pool:")?;
        writeln!(output, "Total size: {} bytes", self.string_pool_stats.total_size)?;
        writeln!(output, "Used size: {} bytes", self.string_pool_stats.used_size)?;
        writeln!(output, "Interned strings: {}", self.string_pool_stats.interned_count)?;
        writeln!(output, "Utilization: {:.2}%", self.string_pool_stats.utilization * 100.0)?;
        writeln!(output)?;
        
        writeln!(output, "Generational Statistics:")?;
        for stats in &self.generation_stats {
            writeln!(output, "Generation {}:", stats.generation)?;
            writeln!(output, "  Allocated: {}", stats.objects_allocated)?;
            writeln!(output, "  Promoted: {}", stats.objects_promoted)?;
            writeln!(output, "  Collected: {}", stats.objects_collected)?;
            writeln!(output, "  Collections: {}", stats.collection_count)?;
            writeln!(output, "  Survival rate: {:.2}%", stats.average_survival_rate * 100.0)?;
        }
        
        if self.out_of_memory_events > 0 {
            writeln!(output)?;
            writeln!(output, "Out of memory events: {}", self.out_of_memory_events)?;
            writeln!(output, "Fragmentation events: {}", self.fragmentation_events)?;
        }
        
        Ok(())
    }
}
```

## Memory Testing Framework

### 1. Memory Test Utilities (`src/memory/testing.rs`)

```rust
/// Testing utilities for memory management
pub mod testing {
    use super::*;
    
    pub struct MemoryTester {
        memory_manager: MemoryManager,
        test_objects: Vec<u32>,
    }
    
    impl MemoryTester {
        pub fn new() -> Self {
            let layout = MemoryLayout::DEFAULT_LAYOUT;
            let memory_manager = MemoryManager::new(layout).unwrap();
            
            Self {
                memory_manager,
                test_objects: Vec::new(),
            }
        }
        
        pub fn test_allocation_deallocation(&mut self) -> TestResult {
            // Test basic allocation and deallocation
            let sizes = [32, 64, 128, 256, 512, 1024];
            let mut allocated_objects = Vec::new();
            
            // Allocate objects of different sizes
            for &size in &sizes {
                match self.memory_manager.allocate(size, type_ids::OBJECT) {
                    Ok(ptr) => allocated_objects.push((ptr, size)),
                    Err(e) => return TestResult::Failed(format!("Allocation failed: {}", e)),
                }
            }
            
            // Verify allocations
            for (ptr, size) in &allocated_objects {
                let header_ptr = ptr - ObjectHeader::SIZE as u32;
                match self.memory_manager.get_header(header_ptr) {
                    Ok(header) => {
                        if header.size < *size as u32 + ObjectHeader::SIZE as u32 {
                            return TestResult::Failed(format!("Invalid object size: expected >= {}, got {}", 
                                                             size + ObjectHeader::SIZE, header.size));
                        }
                    }
                    Err(e) => return TestResult::Failed(format!("Invalid header: {}", e)),
                }
            }
            
            // Deallocate objects
            for (ptr, _) in allocated_objects {
                if let Err(e) = self.memory_manager.release(ptr) {
                    return TestResult::Failed(format!("Deallocation failed: {}", e));
                }
            }
            
            TestResult::Passed
        }
        
        pub fn test_reference_counting(&mut self) -> TestResult {
            // Test reference counting behavior
            let ptr = match self.memory_manager.allocate(64, type_ids::OBJECT) {
                Ok(ptr) => ptr,
                Err(e) => return TestResult::Failed(format!("Allocation failed: {}", e)),
            };
            
            // Initial ref count should be 1
            let header_ptr = ptr - ObjectHeader::SIZE as u32;
            let initial_ref_count = self.memory_manager.get_header(header_ptr).unwrap().ref_count;
            if initial_ref_count != 1 {
                return TestResult::Failed(format!("Expected initial ref count 1, got {}", initial_ref_count));
            }
            
            // Retain should increment ref count
            if let Err(e) = self.memory_manager.retain(ptr) {
                return TestResult::Failed(format!("Retain failed: {}", e));
            }
            
            let ref_count_after_retain = self.memory_manager.get_header(header_ptr).unwrap().ref_count;
            if ref_count_after_retain != 2 {
                return TestResult::Failed(format!("Expected ref count 2 after retain, got {}", ref_count_after_retain));
            }
            
            // First release should decrement ref count
            if let Err(e) = self.memory_manager.release(ptr) {
                return TestResult::Failed(format!("First release failed: {}", e));
            }
            
            let ref_count_after_first_release = self.memory_manager.get_header(header_ptr).unwrap().ref_count;
            if ref_count_after_first_release != 1 {
                return TestResult::Failed(format!("Expected ref count 1 after first release, got {}", ref_count_after_first_release));
            }
            
            // Second release should deallocate object
            if let Err(e) = self.memory_manager.release(ptr) {
                return TestResult::Failed(format!("Second release failed: {}", e));
            }
            
            // Object should no longer be valid
            if self.memory_manager.get_header(header_ptr).is_ok() {
                return TestResult::Failed("Object should have been deallocated".to_string());
            }
            
            TestResult::Passed
        }
        
        pub fn test_garbage_collection(&mut self) -> TestResult {
            // Create objects with circular references
            let obj1 = match self.memory_manager.allocate(64, type_ids::OBJECT) {
                Ok(ptr) => ptr,
                Err(e) => return TestResult::Failed(format!("Allocation 1 failed: {}", e)),
            };
            
            let obj2 = match self.memory_manager.allocate(64, type_ids::OBJECT) {
                Ok(ptr) => ptr,
                Err(e) => return TestResult::Failed(format!("Allocation 2 failed: {}", e)),
            };
            
            // Add as roots so GC doesn't collect them immediately
            self.memory_manager.add_root(obj1);
            self.memory_manager.add_root(obj2);
            
            // Simulate circular reference by adding references
            if let Err(e) = self.memory_manager.retain(obj2) {
                return TestResult::Failed(format!("Failed to create reference: {}", e));
            }
            if let Err(e) = self.memory_manager.retain(obj1) {
                return TestResult::Failed(format!("Failed to create reference: {}", e));
            }
            
            // Remove roots to make objects eligible for collection
            self.memory_manager.remove_root(obj1);
            self.memory_manager.remove_root(obj2);
            
            // Run garbage collection
            match self.memory_manager.gc_state.collect(&mut self.memory_manager) {
                Ok(collected) => {
                    if collected == 0 {
                        return TestResult::Failed("GC should have collected circular references".to_string());
                    }
                }
                Err(e) => return TestResult::Failed(format!("GC failed: {}", e)),
            }
            
            TestResult::Passed
        }
        
        pub fn test_string_pool(&mut self) -> TestResult {
            let test_strings = [
                "hello",
                "world", 
                "Clean Language",
                "hello",  // Duplicate
                "WebAssembly",
                "world",  // Duplicate
            ];
            
            let mut interned_ptrs = Vec::new();
            
            // Intern strings
            for test_string in &test_strings {
                match self.memory_manager.string_pool.intern(test_string.as_bytes()) {
                    Ok(ptr) => interned_ptrs.push(ptr),
                    Err(e) => return TestResult::Failed(format!("String interning failed: {}", e)),
                }
            }
            
            // Check deduplication
            if interned_ptrs[0] != interned_ptrs[3] {
                return TestResult::Failed("String deduplication failed for 'hello'".to_string());
            }
            
            if interned_ptrs[1] != interned_ptrs[5] {
                return TestResult::Failed("String deduplication failed for 'world'".to_string());
            }
            
            // Verify string data
            for (i, test_string) in test_strings.iter().enumerate() {
                match self.memory_manager.string_pool.get_string_data(interned_ptrs[i]) {
                    Ok(data) => {
                        if data != test_string.as_bytes() {
                            return TestResult::Failed(format!("String data mismatch: expected {:?}, got {:?}", 
                                                             test_string.as_bytes(), data));
                        }
                    }
                    Err(e) => return TestResult::Failed(format!("Failed to get string data: {}", e)),
                }
            }
            
            TestResult::Passed
        }
        
        pub fn test_memory_pressure(&mut self) -> TestResult {
            // Test behavior under memory pressure
            let mut allocated_objects = Vec::new();
            let object_size = 1024;
            let max_objects = 500;  // Should exceed available memory
            
            for i in 0..max_objects {
                match self.memory_manager.allocate_with_gc(object_size, type_ids::OBJECT) {
                    Ok(ptr) => {
                        allocated_objects.push(ptr);
                        // Keep every 10th object to create memory pressure
                        if i % 10 != 0 {
                            let _ = self.memory_manager.release(ptr);
                            allocated_objects.pop();
                        }
                    }
                    Err(MemoryError::OutOfMemory) => {
                        // Expected when memory is exhausted
                        break;
                    }
                    Err(e) => return TestResult::Failed(format!("Unexpected error: {}", e)),
                }
            }
            
            // Verify GC ran during pressure
            if self.memory_manager.statistics.gc_runs == 0 {
                return TestResult::Failed("GC should have run under memory pressure".to_string());
            }
            
            // Clean up remaining objects
            for ptr in allocated_objects {
                let _ = self.memory_manager.release(ptr);
            }
            
            TestResult::Passed
        }
        
        pub fn run_all_tests(&mut self) -> Vec<(String, TestResult)> {
            vec![
                ("allocation_deallocation".to_string(), self.test_allocation_deallocation()),
                ("reference_counting".to_string(), self.test_reference_counting()),
                ("garbage_collection".to_string(), self.test_garbage_collection()),
                ("string_pool".to_string(), self.test_string_pool()),
                ("memory_pressure".to_string(), self.test_memory_pressure()),
            ]
        }
    }
    
    #[derive(Debug)]
    pub enum TestResult {
        Passed,
        Failed(String),
    }
    
    impl std::fmt::Display for TestResult {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                TestResult::Passed => write!(f, "PASSED"),
                TestResult::Failed(reason) => write!(f, "FAILED: {}", reason),
            }
        }
    }
    
    pub fn run_memory_tests() {
        let mut tester = MemoryTester::new();
        let results = tester.run_all_tests();
        
        println!("Memory Management Test Results");
        println!("==============================");
        
        let mut passed = 0;
        let mut failed = 0;
        
        for (test_name, result) in results {
            println!("{}: {}", test_name, result);
            match result {
                TestResult::Passed => passed += 1,
                TestResult::Failed(_) => failed += 1,
            }
        }
        
        println!("\nSummary: {} passed, {} failed", passed, failed);
    }
}
```

## Error Types and Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Out of memory")]
    OutOfMemory,
    
    #[error("Invalid pointer: {0:#x}")]
    InvalidPointer(u32),
    
    #[error("Reference count overflow")]
    RefCountOverflow,
    
    #[error("Invalid reference count")]
    InvalidRefCount,
    
    #[error("Memory layout error: {0}")]
    MemoryLayoutError(String),
    
    #[error("String pool exhausted")]
    StringPoolExhausted,
    
    #[error("GC error: {message}")]
    GcError { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryLayoutError {
    #[error("Overlapping memory regions: {region1} and {region2}")]
    OverlappingRegions {
        region1: String,
        region2: String,
    },
    
    #[error("Invalid region size: {region}")]
    InvalidRegionSize { region: String },
    
    #[error("Total memory exceeds WebAssembly limit")]
    ExceedsWasmLimit,
}
```

## Best Practices for Claude

When working with Clean Language's memory management system:

1. **Safety First**: Always validate pointers and handle memory errors gracefully
2. **Reference Counting**: Use retain/release consistently for object lifetime management
3. **Garbage Collection**: Design object graphs to minimize circular references
4. **Memory Layout**: Respect alignment requirements for WebAssembly compatibility
5. **String Pooling**: Leverage string interning for memory efficiency
6. **Testing**: Comprehensive testing under various memory pressure scenarios
7. **Monitoring**: Use memory statistics for performance analysis and debugging
8. **WebAssembly Integration**: Consider WebAssembly memory model limitations

This memory management documentation provides the foundation for implementing efficient and safe memory handling in the Clean Language compiler and runtime system.