# Find: Where is the Heap Pointer Stored in WASM Memory?

## Context

The clean-server runtime needs to coordinate memory allocation with WASM modules compiled by Clean Language. Currently:

- **Server reads**: `memory[0]` (bytes 0-3) as heap pointer
- **Server gets**: `-1` (0xFFFFFFFF)
- **Expected**: A valid heap pointer like `1024` or higher

This mismatch causes memory corruption when the server allocates strings for `_db_query` responses.

## Task

Find where the Clean Language compiler stores the heap pointer in WASM linear memory.

### Search for:

1. **Memory initialization** - Where is the initial heap pointer set?
   ```rust
   // Look for patterns like:
   memory[0] = heap_start;
   // or
   global $heap_ptr
   ```

2. **malloc/alloc implementation** - Where does it read/write the heap pointer?
   ```rust
   // Look for:
   let heap_ptr = memory.read_i32(HEAP_PTR_OFFSET);
   heap_ptr += size;
   memory.write_i32(HEAP_PTR_OFFSET, heap_ptr);
   ```

3. **Memory layout constants** - Any defined offsets?
   ```rust
   const HEAP_PTR_OFFSET: usize = 0;  // or some other value?
   ```

### Files to check:

```
src/codegen/memory.rs        - Memory management codegen
src/codegen/wasm_codegen.rs  - WASM code generation
src/stdlib/memory.rs         - Memory stdlib functions
platform-architecture/MEMORY_MODEL.md - Memory layout docs
```

### Possible locations:

| Location | Description |
|----------|-------------|
| `memory[0..4]` | First 4 bytes of linear memory |
| WASM Global | A `(global $heap_ptr (mut i32))` |
| Fixed offset | Some constant like `memory[1024]` |
| Data section | Initialized in data segment |

## Expected Output

Please provide:

1. **Exact location** where heap pointer is stored (memory offset or global name)
2. **Code snippet** showing how it's read/written
3. **Initial value** set at module startup

## Why This Matters

The server needs to:
1. Read the current heap pointer before allocating
2. Write the new heap pointer after allocating
3. Ensure WASM's malloc doesn't overlap with server allocations

Once we know the correct location, we can fix the server to read from there.
