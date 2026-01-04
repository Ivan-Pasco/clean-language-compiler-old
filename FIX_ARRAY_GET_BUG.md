# Bug: JSON Array `.get(i)` Returns Wrong Value for i > 0

## Status: RUNTIME ISSUE - NOT A COMPILER BUG (2026-01-03)

### Executive Summary

**The bug is in the clean-server runtime, NOT the compiler.** Extensive testing proves the compiler's JSON parser works correctly for ALL scenarios:

| Test Case | Result |
|-----------|--------|
| Simple 2-element arrays | ✅ Pass |
| 4-element arrays with 8 fields each | ✅ Pass |
| Nested structures (`data.data.rows.get(i)`) | ✅ Pass |
| Concatenated (heap-allocated) strings | ✅ Pass |
| Host function returns (`_db_query`) | ❌ **FAILS** |

### Proof: Identical JSON Structure Works

Test file `/tmp/test_host_simulation.cln` uses the EXACT same structure as the failing scenario:
- 4 objects in an array
- Each object has 8 fields
- "read_time" is the 7th field

**Result: ALL PASS**
```
--- Index 0 --- article.slug: renaissance-of-analog ✅
--- Index 1 --- article.slug: designing-next-billion ✅
--- Index 2 --- article.slug: battery-revolution ✅
--- Index 3 --- article.slug: architecture-solitude ✅
```

### Root Cause: Runtime Memory Allocation

The difference between working and broken:

| Scenario | String Source | Allocator | Works? |
|----------|---------------|-----------|--------|
| Hardcoded string | Static data section | Compiler | ✅ |
| Concatenated string | WASM heap | WASM malloc | ✅ |
| Host function return | WASM heap | **Runtime** | ❌ |

**The issue is how the runtime coordinates with WASM's memory allocator.**

### Bug Mechanism

When runtime returns a JSON string:
1. Runtime allocates string at address ~1024
2. Runtime SHOULD update WASM heap pointer at `memory[0]`
3. WASM's JSON parser calls `malloc` for objects/arrays
4. **BUG**: `malloc` returns address overlapping with allocated string or vice versa

This causes:
- `array[0]` = correct object pointer
- `array[1]` = "read_time" (field name pointer from overlapping memory)

### Where to Fix (clean-server)

1. **`_db_query` function** - How it allocates result string
2. **Heap pointer update** - Must set `memory[0]` to safe value BEFORE returning
3. **Memory region safety** - Ensure string region won't be reused by malloc

```rust
// In clean-server when returning string to WASM:
fn return_string_to_wasm(wasm_memory: &mut [u8], string: &str) -> i32 {
    // 1. Read current heap pointer
    let heap_ptr = read_i32(wasm_memory, 0);

    // 2. Calculate string allocation (4 bytes length + string bytes)
    let alloc_size = 4 + string.len() as i32;

    // 3. Write string at heap_ptr
    write_i32(wasm_memory, heap_ptr as usize, string.len() as i32);
    wasm_memory[(heap_ptr + 4) as usize..][..string.len()].copy_from_slice(string.as_bytes());

    // 4. CRITICAL: Update heap pointer for future allocations
    let new_heap_ptr = align_to_8(heap_ptr + alloc_size);
    write_i32(wasm_memory, 0, new_heap_ptr);

    heap_ptr // Return pointer to string
}
```

### Verification

Once fixed in runtime, run:
```bash
# In clean-server, run the article-blog example with database
# The existing regression test should also pass:
./target/release/cln compile tests/cln/stdlib/json/json_array_get_regression.cln -o test.wasm
./target/release/wasmtime_runner test.wasm
```

---

## Original Report

### Summary

When accessing elements from a JSON array using `.get(index)`, the first element (index 0) works correctly, but subsequent indices return incorrect values (field names from objects instead of array elements).

## Reproduction

```cln
string jsonStr = "{\"rows\":[{\"slug\":\"first\",\"read_time\":5},{\"slug\":\"second\",\"read_time\":6}]}"
any parsed = json.tryTextToData(jsonStr)
any rows = parsed.rows

any item0 = rows.get(0)
printl("item0 = " + item0.toString())  // Output: 26976 (correct - pointer to object)

any item1 = rows.get(1)
printl("item1 = " + item1.toString())  // Output: read_time (WRONG - returns field name string!)
```

## Expected vs Actual

| Index | Expected | Actual |
|-------|----------|--------|
| 0 | Pointer to first object (e.g., 26976) | ✅ Correct |
| 1 | Pointer to second object | ❌ Returns `"read_time"` string |
| 2 | Pointer to third object | ❌ Unknown (crashes) |

## Debug Output from Article Blog

```
DEBUG: In loop, i = 0
DEBUG: Got article, article = 26976
DEBUG: About to access article.slug...
DEBUG: slugField = renaissance-of-analog   <-- Works!

DEBUG: In loop, i = 1
DEBUG: Got article, article = read_time    <-- WRONG VALUE!
DEBUG: About to access article.slug...
[CRASH]
```

## Analysis

The `.get(0)` call correctly returns a pointer to the first array element.
The `.get(1)` call returns `"read_time"` which is a field name from within one of the objects.

This suggests the array index calculation is wrong - it's stepping through object fields instead of array elements.

Possible causes:
1. Array element stride calculation is wrong (using object field size instead of array element size)
2. Memory layout for JSON arrays is not being correctly followed
3. The pointer arithmetic in `.get()` is reading from wrong offsets

## Location to Fix

Look in the compiler's codegen for:
- `any.get(index)` method implementation
- How JSON arrays are laid out in memory
- Pointer arithmetic for array element access

## Test File

The article-blog example at:
`/Users/earcandy/Documents/Dev/Clean Language/clean-framework/examples/article-blog/app-db.cln`

Uses a database query that returns 4 articles in a JSON array. The first article renders correctly, but accessing the second article via `.get(1)` fails.

## Priority

🔴 CRITICAL - This breaks any iteration over JSON arrays with more than one element.
