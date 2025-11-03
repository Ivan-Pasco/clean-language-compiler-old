# Function Index Out of Range - Root Cause Analysis and Fix

## Date
2025-10-25

## Problem Statement
129 WASM files failing validation with "function variable out of range" errors.
Example: `tests/output/34_list_behaviors.wasm` references function indices 42-45 but only has functions 0-42 (43 total).

## Root Cause Analysis

### Investigation Process
1. **Compiled test file**: `tests/cln/core/types/34_list_behaviors.cln`
2. **Validated WASM**: Found errors calling indices 42, 43, 44, 45 with max index 42
3. **Inspected WASM module**:
   - 1 import (print at index 0)
   - 41 defined functions (indices 1-41)
   - Total: 42 functions (valid indices 0-41)
4. **Traced invalid calls**: Found they were attempting to call list operations (`array_get`, `array_length`, `list.remove`, `list.add`)

### Root Cause
**The MIR codegen path (`src/codegen/mir_codegen.rs`) does NOT register stdlib functions, while the AST codegen path DOES.**

**Why this causes the problem**:
1. `CodeGenerator::new()` is called and creates an empty `function_map`
2. AST path would call `register_list_class_operations()`, `register_string_class_operations()`, etc.
3. MIR path skips these registrations
4. When MIR code generation encounters calls to list/string methods, it looks up function indices in `function_map`
5. Functions entries exist in the map (from older code or test initialization), but actual WASM imports/functions were never created
6. Call instructions reference non-existent function indices

### Execution Flow Comparison

**AST Path** (working):
```
CodeGenerator::new()
  → register_print_imports()
  → register_type_conversion_imports()
  → register_list_class_operations()     ← Registers list functions
  → register_string_class_operations()   ← Registers string functions
  → ... other stdlib operations
  → generate_ast()
```

**MIR Path** (broken):
```
MirCodeGenerator::new()
  → CodeGenerator::new()
  → generate()
      → register_print_imports()         ← Only basic imports
      → register_type_conversion_imports()
      → register_math_operations()
      → [MISSING: stdlib class operations]  ← BUG IS HERE
      → Pre-register MIR functions
      → Generate WASM
```

## Fix Implementation

### Changes Made
Modified `src/codegen/mir_codegen.rs` in the `generate()` method to add stdlib operations registration:

```rust
// CRITICAL FIX: Register list/array and string operations
// These were missing from MIR path, causing function index out of range errors
debug_mir!("DEBUG MIR: Registering list class operations");
self.wasm_generator
    .register_list_class_operations()
    .map_err(|e| vec![e])?;
debug_mir!("DEBUG MIR: List class operations registered");

debug_mir!("DEBUG MIR: Registering string class operations");
self.wasm_generator
    .register_string_class_operations()
    .map_err(|e| vec![e])?;
debug_mir!("DEBUG MIR: String class operations registered");
```

### Remaining Issues
After adding list and string operations, still encountering function index errors for indices 88-91 (max 88).
This indicates MORE stdlib operations are needed.

### Comprehensive Solution Needed
Instead of playing "whack-a-mole" adding individual operation registrations, need to:

1. **Identify ALL stdlib operation registration methods** used in AST path
2. **Create a single comprehensive registration method** that registers ALL stdlib operations
3. **Call this method from BOTH AST and MIR paths** to ensure consistency

### Stdlib Operations to Register
Based on code inspection, these operations need registration:
- ✅ Math operations (`register_math_operations`)
- ✅ Print/console operations (`register_print_imports`)
- ✅ Type conversions (`register_type_conversion_imports`)
- ✅ List class operations (`register_list_class_operations`)
- ✅ String class operations (`register_string_class_operations`)
- ❓ Conditional operations (`register_conditional_operations`)
- ❓ Method style operations
- ❓ List behavior operations
- ❓ File operations
- ❓ HTTP operations
- ❓ Array operations fallback

## Next Steps

### Option 1: Comprehensive Registration Method (Recommended)
Create `register_all_stdlib_operations()` that:
1. Registers ALL stdlib operations in correct order
2. Prevents duplicates
3. Is called from both AST and MIR paths

### Option 2: Match AST Path Exactly
Review AST compilation path to see exact sequence of registrations
Copy that sequence to MIR path

### Option 3: Lazy Registration
Modify `get_function_index_or_error` to automatically register stdlib functions on-demand
(More complex, harder to debug)

## Test Results

### Before Fix
- Imports: 11 (indices 0-10)
- Functions: 31 (indices 11-41)
- Total: 42 (valid indices 0-41)
- Error: Calls to indices 42, 43, 44, 45

### After Adding List Operations
- Imports: 11 (indices 0-10)
- Functions: 54 (indices 11-64)
- Total: 65 (valid indices 0-64)
- Error: Calls to indices 66, 67, 68

### After Adding List + String Operations
- Total: 89 (valid indices 0-88)
- Error: Calls to indices 88, 90, 91

Shows we're making progress but need more operations registered.

## Files Modified
- `src/codegen/mir_codegen.rs` (lines 171-184)

## Related Issues
- This affects ALL 129 WASM files that use list, string, or other stdlib operations
- MIR is the primary compilation path, so this is a critical bug

## Recommendation
Implement Option 1: Create a comprehensive `register_all_stdlib_operations()` method that can be called from both paths to ensure consistency and completeness.
