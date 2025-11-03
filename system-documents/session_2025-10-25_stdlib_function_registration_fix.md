# Stdlib Function Registration Fix - Session 2025-10-25

## Summary

Fixed missing stdlib function registrations in MIR code generation, resolving 13 compilation errors.

## Problem

Files were failing to compile with errors like:
```
Function 'list_pop' (SymbolId(55)) not found in function map during code generation
Function 'string.contains' (SymbolId(67)) not found in function map during code generation
```

Investigation showed that **NO stdlib functions** were being registered during MIR-based compilation.

## Root Cause

The Clean Language compiler has two code generation paths:

1. **Legacy Path**: `CodeGenerator::generate()` (deprecated, marked since v0.10.2)
   - Located in: `src/codegen/mod.rs` line 346
   - Calls `register_stdlib_functions()` which registers all stdlib operations
   - **NEVER EXECUTED** in current compilation flow

2. **Modern Path**: `MirCodeGenerator::generate()` (active)
   - Located in: `src/codegen/mir_codegen.rs` lines 136-299
   - Only registered math operations, type conversions, and print functions
   - **MISSING**: list, string, and method-style operations

### Compilation Flow

```
compile_with_file() (lib.rs)
  → Stages 1-6: Lexer → Parser → HIR → Resolver → TypeChecker → MIR
  → Stage 7: MirCodeGenerator::generate() ← Uses this path
  → BYPASSES: Legacy CodeGenerator::generate()
```

## Investigation Process

1. Added debug logging to check function_map contents
2. Discovered function_map contained only 51 entries (math + memory + type conversion)
3. NO list or string functions present
4. Traced back to find `register_list_operations()` was NEVER called
5. Used Explore agent to trace compilation flow
6. Found MirCodeGenerator bypasses legacy CodeGenerator entirely

## Solution

Added missing stdlib function registrations to `MirCodeGenerator::generate()` in `src/codegen/mir_codegen.rs` (lines 177-210):

```rust
// CRITICAL FIX: Register list operations (list_push, list_pop, etc.)
self.wasm_generator
    .register_list_operations()
    .map_err(|e| vec![e])?;

// CRITICAL FIX: Register string operations (string.contains, string.length, etc.)
self.wasm_generator
    .register_string_operations()
    .map_err(|e| vec![e])?;

// CRITICAL FIX: Register method-style imports
self.wasm_generator
    .register_method_style_imports()
    .map_err(|e| vec![e])?;

// CRITICAL FIX: Register string class operations
self.wasm_generator
    .register_string_class_operations()
    .map_err(|e| vec![e])?;

// CRITICAL FIX: Register list class operations
self.wasm_generator
    .register_list_class_operations()
    .map_err(|e| vec![e])?;
```

## Impact

### Files Fixed: +13

**Before**:
- Not defined/found errors: 22 files

**After**:
- Not defined/found errors: 9 files ⬇️

**Net Improvement**:
- +13 files now compile successfully
- -59% reduction in "not defined/found" errors

## Remaining Issues

### Not Defined/Found Errors (9 files)

**Constructor Not Found (6 files)**:
- 05_expressions - TestData constructor
- 83_memory_management_comprehensive - LargeObject constructor
- calculator_application - Calculator constructor
- test_apply_blocks - DataProcessor constructor
- simple_method_test - Animal constructor
- (1 more)

**Function Not Found in Map (1 file)**:
- 76_math_module_comprehensive - math_sin function

**Other (2 files)**:
- Remaining validation errors

### Next Steps

1. **Fix constructor resolution** - 6 files need constructor SymbolId resolution
2. **Register math namespace functions** - math_sin, math_cos, etc.
3. **Fix SymbolId resolution errors** - 12 files (separate category)

## Files Modified

- `src/codegen/mir_codegen.rs` (lines 177-210) - Added stdlib registrations
- `src/codegen/mir_codegen.rs` (line 832) - Removed debug logging
- `src/codegen/mod.rs` (lines 4566-4573) - Removed debug logging
- `src/codegen/mod.rs` (lines 4687-4690) - Removed debug logging

## Technical Details

### Function Registration Order

The MirCodeGenerator now registers functions in this order:
1. Print imports (print, printl)
2. Type conversion imports (int_to_string, float_to_string, etc.)
3. Math operations (math.abs, math.max, math.min, math.sqrt, math.pow)
4. **List operations** (list_push, list_pop, list.get, etc.) ← NEW
5. **String operations** (string.contains, string.length, etc.) ← NEW
6. **Method-style imports** (method calls like `.toString()`) ← NEW
7. **String class operations** (string class methods) ← NEW
8. **List class operations** (list class methods) ← NEW

This matches the legacy CodeGenerator registration order, ensuring all stdlib functions are available during MIR-based code generation.

## Related Sessions

- **session_2025-10-25_constructor_implicit_return_fix.md** - Fixed constructors to return instance pointer (+25 WASM validations)
- **session_2025-10-25_symbolid_partial_fix.md** - Fixed constructors to build with class context
- **session_2025-10-25_revert_explicit_this.md** - Removed explicit `this` keyword
