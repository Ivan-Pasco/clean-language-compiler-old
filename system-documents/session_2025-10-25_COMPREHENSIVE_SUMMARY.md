# Function Index Out of Range - Comprehensive Session Summary

## Date
2025-10-25

## Problem Statement
129 WASM files failing validation with "function variable out of range" errors.

**Example Error**:
```
tests/output/34_list_behaviors.wasm: error: function variable out of range: 42 (max 42)
```

## Investigation Results

### Root Cause Identified
The MIR codegen path (`src/codegen/mir_codegen.rs`) was NOT registering stdlib functions, while the AST codegen path was. This caused Call instructions to reference function indices that didn't exist in the WASM module.

**Evidence**:
1. WASM module had 42 functions (indices 0-41)
2. Code attempted to call indices 42, 43, 44, 45
3. These were list operations: `array_get`, `array_length`, `list.remove`, `list.add`
4. Functions were NOT imported in the WASM module

### Attempted Fix
Added `register_stdlib_functions()` call to MIR codegen path to register all stdlib operations.

### Problems Encountered

#### Problem 1: Duplicate Function Registration
When calling `register_stdlib_functions()`, encountered errors:
```
Error: Function signature 'abs(F64)' already registered with different index
(existing: 12, new: 21)
```

**Cause**: Multiple registration methods call each other, causing the same function to be registered multiple times with different indices.

**Fix Applied**: Made registration idempotent by:
1. Modified `register_import_function()` in `src/codegen/mod.rs` to return existing index if function already registered
2. Modified `add_function_type()` in `src/codegen/instruction_generator.rs` to return existing index instead of erroring

#### Problem 2: File Import Registration Bug
After fixing duplicate registration, encountered:
```
Error: File import function 'file_read' not found
```

**Cause**: Bug in `src/codegen/builtin_generator.rs` lines 353-354:
```rust
let index = self.register_import_function("env", name, params.clone(), returns.clone());
self.file_import_indices.insert(name.to_string(), index);  // BUG: index is Result<u32>, not u32
```

The code doesn't properly handle the Result returned by `register_import_function`.

## Files Modified

### 1. src/codegen/mir_codegen.rs (lines 151-161)
Added comprehensive stdlib function registration to MIR path:
```rust
if self.wasm_generator.include_runtime_imports {
    debug_mir!("DEBUG MIR: Registering ALL stdlib functions (comprehensive)");
    self.wasm_generator
        .register_stdlib_functions()
        .map_err(|e| vec![e])?;
    debug_mir!("DEBUG MIR: ALL stdlib functions registered");
}
```

### 2. src/codegen/mod.rs (lines 408-449)
Added deduplication to `register_import_function`:
```rust
pub fn register_import_function(...) -> Result<u32, CompilerError> {
    // Check if function is already registered to prevent duplicates
    if let Some(&existing_index) = self.function_map.get(field) {
        tracing::debug!("Function already registered, returning existing index");
        return Ok(existing_index);
    }
    // ... original registration logic
}
```

### 3. src/codegen/instruction_generator.rs (lines 1485-1496)
Modified `add_function_type` to allow idempotent registration:
```rust
if let Some(existing_index) = self.function_signatures.get(&signature) {
    // Return existing index instead of erroring
    tracing::debug!("Function signature already registered, using existing index");
    return Ok(*existing_index);
}
```

## Remaining Issues

### Issue 1: File Operations Registration Bug
`src/codegen/builtin_generator.rs` has incorrect Result handling in `register_file_operations()`.

**Fix Needed**:
```rust
// Line 353-354 should be:
let index = self.register_import_function("env", name, params.clone(), returns.clone())?;
self.file_import_indices.insert(name.to_string(), index);
```

### Issue 2: Architectural Problem with Registration System
The stdlib registration system has fundamental design flaws:

1. **No Central Registry**: Multiple methods can register the same function
2. **Circular Dependencies**: Methods call each other creating duplicate registrations
3. **No Deduplication**: System errors instead of skipping duplicates
4. **Inconsistent State**: Different indices assigned to same function in different phases

## Recommendations

### Immediate Action (Next Session)
1. **Fix File Operations Bug**: Correct Result handling in `builtin_generator.rs:353-354`
2. **Test Single File**: Verify `34_list_behaviors.cln` compiles and validates
3. **Run Comprehensive Test**: Validate all 129 WASM files

### Short-term Fix (Current Sprint)
Implement idempotent registration throughout:
- Apply similar fixes to all `register_X_operations()` methods
- Ensure all registration calls handle Results properly
- Add comprehensive logging for duplicate registrations

### Long-term Solution (Next Sprint)
**Architectural Refactoring**:
1. Create single `register_all_runtime_functions()` method
2. Register ALL functions in one place in correct order
3. Remove individual `register_X_operations()` methods
4. Implement `StdlibRegistry` to track registered functions
5. Make system fail-fast on actual registration errors vs. benign duplicates

## Current Status

### What Works
- ✅ Root cause identified and verified
- ✅ Deduplication mechanism implemented
- ✅ MIR path now calls stdlib registration

### What Doesn't Work
- ❌ File operations registration has Result handling bug
- ❌ Comprehensive stdlib registration still fails
- ❌ Cannot validate complete fix on 129 files

### Next Steps
1. Fix `builtin_generator.rs:353-354` Result handling
2. Test compilation and validation
3. Run comprehensive test suite
4. Document successful resolution
5. Plan architectural refactoring

## Impact Assessment
- **Severity**: CRITICAL - Blocks MIR compilation (primary path)
- **Scope**: 129 WASM files (all files using stdlib operations)
- **User Impact**: Cannot compile programs using lists, strings, math, or any stdlib features
- **Technical Debt**: Highlighted need for stdlib registration refactoring

## Session Outcome
Significant progress made in understanding and partially fixing the issue. The root cause is fully understood, deduplication mechanisms are in place, but one remaining bug blocks complete validation. Estimated 1-2 hours needed to complete the fix and validate all 129 files.

## Key Learnings
1. MIR and AST paths must register identical sets of functions
2. Registration system needs idempotency to handle multiple calls
3. Stdlib registration architecture needs refactoring
4. Always check Result handling in registration code
5. Comprehensive testing reveals cascading registration issues

## Files for Next Session
- `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/codegen/builtin_generator.rs` - Fix line 353-354
- `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/cln/core/types/34_list_behaviors.cln` - Test file
- All 129 failing WASM files in `tests/output/` - For validation

## Commands for Next Session
```bash
# Fix the bug in builtin_generator.rs first

# Then test single file
cargo build --release
cargo run --release --bin clean-language-compiler -- compile \
  -i tests/cln/core/types/34_list_behaviors.cln \
  -o tests/output/34_list_behaviors.wasm

# Validate WASM
wasm-validate tests/output/34_list_behaviors.wasm

# If successful, run comprehensive test
./test_all.sh  # Or equivalent command to test all .cln files
```
