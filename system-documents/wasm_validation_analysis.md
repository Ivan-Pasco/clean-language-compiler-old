# WASM Validation Error Analysis

## Summary

**Current Status**: 247/768 files validating (32.1% success rate)
**Total Errors**: ~6,000+ validation errors across 521 failing files

## Key Finding

The MIR builder implicit return fix only addressed 22 errors out of 6000+ total errors (~0.4% of all errors). This explains why the validation rate only improved from 31% to 32%.

## Error Categories (By Frequency)

### 1. Type System Errors (4,500+ errors - 75% of total)

#### Integer Type Confusion (i32/i64)
- **1,678 errors**: `type mismatch in local.set, expected [i32] but got [i64]`
- **928 errors**: `type mismatch in i64.add, expected [i64, i64] but got [i32, i32]`
- **110 errors**: `type mismatch in return, expected [i64] but got [i32]`
- **28 errors**: `type mismatch in i64.eq, expected [i64, i64] but got [i32, i32]`
- **Total**: ~2,800 i32/i64 confusion errors

**Root Cause**: The WASM codegen is generating i64 instructions for integer operations but using i32 types for variables, or vice versa. This suggests inconsistent type mapping between Clean Language's `integer` type and WASM's integer types.

#### Float/Integer Type Confusion
- **233 errors**: `type mismatch in local.set, expected [i32] but got [f64]`
- **26 errors**: `type mismatch in return, expected [f64] but got [i32]`
- **Total**: ~260 float/int errors

**Root Cause**: Type coercion between integers and floats is not being handled properly in WASM codegen.

#### Empty Stack Errors
- **1,589 errors**: `type mismatch in local.set, expected [i32] but got []`
- **Total**: 1,589 empty-stack errors

**Root Cause**: Expressions that should produce values are leaving nothing on the stack. This indicates missing value generation in expression codegen.

### 2. Function Call Errors (2,200+ errors - 37% of total)

- **2,042 errors**: `type mismatch in call, expected [i32, i32] but got [i32]`
- **151 errors**: `type mismatch in call, expected [i32, i32] but got []`
- **24 errors**: `type mismatch in call, expected [i64, i64] but got [i32, i32]`
- **13 errors**: `type mismatch in call, expected [i64] but got [i32]`
- **10 errors**: `type mismatch in call, expected [i32] but got []`

**Root Cause**: Function call argument generation is not properly emitting all required arguments, or is emitting them with wrong types.

### 3. Function Termination Errors (400+ errors)

#### Wrong Stack State at Function End
- **366 errors**: `type mismatch at end of function, expected [] but got [i32]`
- **22 errors**: `type mismatch in implicit return, expected [i32] but got []` (FIXED!)

**Root Cause**: Functions that return void are leaving values on the stack, or vice versa.

#### Invalid Control Flow
- **127 errors**: `invalid depth: 1 (max 0)`
- **125 errors**: `invalid depth: 2 (max 0)`
- **86 errors**: `invalid depth: 3 (max 0)`

**Root Cause**: Branch instructions are referencing non-existent block depths. This indicates incorrect block nesting or Jump/Branch target calculation in WASM codegen.

### 4. Memory Access Errors
- **139 errors**: `memory variable out of range: 0 (max 0)`

**Root Cause**: Attempting to access linear memory when no memory has been allocated/declared.

### 5. Control Flow Errors
- **30 errors**: `type mismatch in br, expected [i32] but got []`
- **21 errors**: `type mismatch in br_if, expected [i32] but got []`
- **11 errors**: `type mismatch in br_if, expected [i64] but got []`

**Root Cause**: Branch instructions expect values on the stack for block results, but nothing is there.

## Root Cause Analysis

The fundamental issues are in **WASM codegen layer**, not MIR:

1. **Type Mapping Inconsistency**: Clean Language's `integer` type is being mapped to both i32 and i64 inconsistently across different code paths (expression evaluation vs variable storage vs function signatures)

2. **Function Call Codegen**: The code that generates WASM call instructions is not properly emitting all arguments or is using wrong types

3. **Expression Evaluation**: Many expressions are not leaving values on the stack when they should

4. **Block Management**: WASM block structure generation has issues with depth calculation and termination

## Affected Files in Codebase

Primary files needing fixes:
- `src/codegen/instruction_generator.rs` - Expression and instruction generation
- `src/codegen/statement_generator.rs` - Statement codegen including function calls
- `src/codegen/type_manager.rs` - Type mapping between Clean Language and WASM types
- `src/codegen/pipeline/generation.rs` - Main codegen pipeline

## Recommended Fix Priority

### 🔴 CRITICAL (Must fix first)
1. **Fix integer type consistency** (2,800 errors)
   - Establish clear mapping: `integer` → i64 throughout
   - Audit all integer operations to use consistent types

2. **Fix function call argument generation** (2,200 errors)
   - Ensure all function call arguments are emitted
   - Ensure argument types match function signatures

### 🟡 HIGH (Fix next)
3. **Fix expression evaluation** (1,589 errors)
   - Ensure all expressions leave expected values on stack
   - Add stack validation after each expression

4. **Fix function termination** (366 errors)
   - Ensure void functions clean up stack
   - Ensure non-void functions leave correct value

### 🟢 MEDIUM (Fix after above)
5. **Fix control flow depth** (338 errors)
   - Audit block structure generation
   - Fix depth calculation for branches

6. **Fix memory access** (139 errors)
   - Ensure linear memory is declared when needed
   - Fix memory variable references

## Testing Strategy

1. **Create minimal test cases** for each error category
2. **Fix one category at a time** starting with highest priority
3. **Validate incrementally** after each fix
4. **Target 100% validation rate** - no compromises

## Previous Session Success

The MIR builder fix DID work correctly:
- Fixed `ensure_function_termination()` to check ALL blocks
- Added proper type-specific default returns
- Both test cases (simple if/else and if/else if/else) now validate

This demonstrates the fix methodology is sound - we just need to apply it to the larger WASM codegen issues.
