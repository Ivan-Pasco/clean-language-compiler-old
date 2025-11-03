# Function Index Mismatch Bug Fix
## Session: 2025-10-24 (Continued)

## Problem Summary

**Error**: "Function 'start' pre-registered at index 41 but generated at index 40"

This error was blocking compilation of test files. The function index mismatch occurred when functions were pre-registered with one index but generated with a different index due to `function_count` changing between pre-registration and generation phases.

## Root Cause Analysis

The bug occurred in the MIR-to-WASM code generation pipeline (`src/codegen/mir_codegen.rs`):

### The Issue
1. **Pre-registration Phase** (line 215): Functions were pre-registered with formula:
   ```rust
   let function_index = self.wasm_generator.function_count + i as u32;
   ```
   This captured `function_count` at pre-registration time.

2. **Generation Phase** (line 1965): When generating functions, index was recalculated:
   ```rust
   let function_index = self.wasm_generator.function_count;
   ```
   This used `function_count` at generation time.

3. **The Problem**: If ANY functions (e.g., stdlib) were added between pre-registration and generation, `function_count` would increase, causing a mismatch.

### Example
- Pre-registration: `function_count = 40`, so `start` pre-registered at index 41
- A stdlib function is added: `function_count = 41`
- Generation: `start` tries to use index 41 (current `function_count`)
- But `start` was pre-registered at index 41
- Error: "Function 'start' pre-registered at index 41 but generated at index 40"

## Solution Implemented

### Changes to `add_function_to_module` in `src/codegen/mir_codegen.rs` (lines 1964-2011)

**Key Insight**: Instead of recalculating the function index during generation, USE the pre-registered index if it exists.

**Before** (line 1965):
```rust
let function_index = self.wasm_generator.function_count;
```

**After** (lines 1964-1986):
```rust
let function_index = if let Some(&pre_registered_index) = self.wasm_generator.function_map.get(&name) {
    // Function was pre-registered, use that index
    tracing::debug!(
        name = %name,
        index = pre_registered_index,
        "Using pre-registered function index"
    );
    pre_registered_index
} else {
    // New function not pre-registered (e.g., stdlib), use current count
    let new_index = self.wasm_generator.function_count;
    self.wasm_generator
        .function_map
        .insert(name.clone(), new_index);
    tracing::debug!(
        name = %name,
        index = new_index,
        "Assigning new function index"
    );
    new_index
};
```

**Function Count Update** (lines 2007-2011):
```rust
// Increment function_count to match the highest function index + 1
// This ensures function_count always reflects the next available index
if function_index >= self.wasm_generator.function_count {
    self.wasm_generator.function_count = function_index + 1;
}
```

This ensures:
1. Pre-registered functions use their assigned indices
2. New functions get the current function_count
3. function_count always reflects the next available index

## Test Results

### Function Index Mismatch Errors
**BEFORE FIX**: Unknown number (estimated ~33 based on user report)
**AFTER FIX**: 0 (100% eliminated)

### Test Suite Results (295 total .cln files)
**Compilation Success**: 227 files (76.9%)
**Validation Success**: 162 files (54.9%)
**Failed Compilation**: 68 files
**Failed Validation**: 65 files

### Verified Working Examples
- ✅ `tests/cln/debug/test_boolean_return_minimal.cln` - Compiles and validates successfully
- ✅ `tests/cln/language/functions/10_functions_basic.cln` - Compiles successfully

## Impact

### Immediate Benefits
1. **100% Elimination**: Function index mismatch errors completely resolved
2. **Consistent Indexing**: Pre-registered functions maintain their indices throughout generation
3. **Correct Function Count**: function_count properly tracks the next available index

### Architectural Improvements
1. **Robust Pre-registration**: Pre-registered indices are honored during generation
2. **Mixed Function Sources**: Supports both pre-registered MIR functions and dynamically added stdlib functions
3. **Index Consistency**: Function map and function count stay synchronized

## Remaining Error Categories (For Future Work)

### Compilation Errors (68 files)

1. **SymbolId Resolution Failures** (~40 files)
   - Error: "Cannot resolve SymbolId(202) to function name during code generation"
   - Root cause: function_symbol_map incomplete
   - Impact: Most debug test files

2. **Missing Built-in Functions** (~5 files)
   - Error: "Function 'list_pop' (SymbolId(55)) not found in function map"
   - Root cause: Stdlib functions not properly registered

### Validation Errors (65 files)

1. **Function Variable Out of Range** (69 instances)
   - Error: "function variable out of range: 44 (max 43)"
   - Root cause: Off-by-one in function call indexing

2. **Type Mismatches** (54+ instances)
   - Error: "type mismatch in local.set, expected [i32] but got []"
   - Root cause: Type system not tracking empty vs. non-empty stack

3. **Control Flow Type Issues** (14 instances)
   - Error: "type mismatch at end of function, expected [] but got [i32]"
   - Root cause: Return value handling in control flow

## Files Modified

**Single file change**: `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/codegen/mir_codegen.rs`
- Lines 1964-1986: Check for pre-registered index before calculating new index
- Lines 2007-2011: Update function_count to reflect highest index + 1

## Testing Scripts Created

Three utility scripts for comprehensive testing:
1. `test_all.sh` - Runs all tests and reports success rates
2. `test_with_errors.sh` - Specifically detects function index mismatch errors
3. `analyze_errors.sh` - Categorizes all compilation and validation errors

## Technical Details

### Index Resolution Strategy
```rust
// Check if function was pre-registered
if let Some(&pre_registered_index) = function_map.get(&name) {
    // Use pre-registered index (for MIR functions)
    function_index = pre_registered_index;
} else {
    // Assign new index (for stdlib functions)
    function_index = function_count;
    function_map.insert(name, function_index);
}

// Update function_count to next available index
if function_index >= function_count {
    function_count = function_index + 1;
}
```

### Why This Works
1. **Pre-registered functions** maintain their assigned indices regardless of when they're generated
2. **New functions** get sequential indices starting from current function_count
3. **function_count** always reflects the next available index
4. **Mixed sources** (MIR + stdlib) work correctly together

## Conclusion

The function index mismatch bug is **completely fixed**. All 295 test files now compile without this specific error. The fix ensures that pre-registered function indices are honored during generation, eliminating the mismatch between pre-registration and generation phases.

The remaining issues are different error categories that require separate fixes:
- SymbolId resolution for method calls (~40 files)
- Type system stack tracking (~54 instances)
- Function call indexing off-by-one (~69 instances)
