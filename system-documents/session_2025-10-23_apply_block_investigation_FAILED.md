# Apply Block HIR Lowering Investigation - FAILED

## Date: 2025-10-23 (continued from ROOT_CAUSE_FOUND.md)

## Summary

Attempted to fix apply block code generation by adding HIR lowering, but the fix **made things worse**.

**Results**:
- Baseline: 69.7% validation (207/297 files)
- After fix: 67.7% validation (201/297 files)
- **Delta: -2.0 percentage points** ❌

## What Was Attempted

### Fix #1: Initial HIR Lowering (FAILED)
**File**: `src/hir/hir_builder.rs` lines 384-424

**Logic**:
- For each expression in apply block, create a single-argument function call
- Example: `func: expr1, expr2` → `func(expr1); func(expr2);`

**Problem**: Created multiple calls with one argument each, but apply blocks might need different semantics

### Fix #2: Corrected HIR Lowering (ALSO FAILED)
**Changes**:
- Print functions: Each expression becomes separate print statement
- Other functions: All expressions become arguments to single call
- Example: `func: expr1, expr2` → `func(expr1, expr2)`

**Result**: Validation rate dropped even more (67.7%)

## Why It Failed

### Evidence
1. test_single_boolean.wasm **does validate** with the fix ✅
2. But 6 other files that previously validated now **fail validation** ❌
3. Net result: -6 files validating

### Error Categories After Fix
- local_errors: 38 files (was 31-40 before)
- call_mismatch: 17 files
- function_out_of_range: 17 files
- implicit_return: 13 files
- explicit_return: 5 files
- end_of_function: 4 files

### Type Mismatch Errors
Example error: `type mismatch in local.set, expected [i32] but got [f64]`

This suggests the HIR lowering is:
- Changing function call semantics
- Breaking type inference
- Creating incorrect argument passing

## Root Cause Analysis

### The Real Problem

Apply blocks were **intentionally NOT lowered to HIR** in the original design. Reasons:

1. **Semantic Complexity**: Apply blocks have special semantics that don't map cleanly to simple function calls
2. **Type Information**: The semantic analyzer validates apply blocks in AST form, preserving type information
3. **Code Generation**: Original codegen had specific handlers for apply blocks that understood their semantics

### What Went Wrong

By lowering apply blocks to HIR:
- Lost semantic information about the apply block structure
- Broke type inference (HIR expressions don't carry same type info as AST)
- Created function calls that don't match expected signatures
- Forced all apply blocks into same pattern (multiple calls OR single call with all args)

### The Empty Function Issue

The original bug (empty wrapper functions) is NOT caused by missing HIR lowering. It's caused by something else in the code generation pipeline that we haven't identified yet.

## Correct Next Steps

### 1. Revert HIR Lowering Changes ✅
The HIR lowering approach is wrong. Revert to baseline 69.7%.

### 2. Investigate Actual Codegen Path
Need to trace:
- Where are apply blocks being processed?
- Why do they generate empty functions?
- What is calling the empty functions?

### 3. Check Semantic Analysis Output
- Does semantic analyzer output contain apply block info?
- Is type information preserved?
- Are function signatures correct?

### 4. Compare with Working Features
Find a feature that generates proper WASM and trace its path:
- How does it go from AST → HIR → MIR → WASM?
- What happens to its type information?
- Where does actual code generation occur?

## Key Insights

1. **Don't assume HIR lowering is needed**: Some AST nodes might be intentionally kept for direct codegen
2. **Preserve semantics**: Lowering should not change meaning or lose information
3. **Test incrementally**: Check validation after each small change
4. **Understand original design**: There are reasons for architectural decisions

## Files Modified (TO BE REVERTED)

1. `src/hir/hir_builder.rs` lines 384-430 - Added apply block lowering
2. Need to restore original code that was there

## Lessons Learned

1. **Root cause was wrong**: The issue isn't "missing HIR lowering", it's "incorrect code generation"
2. **Fix symptoms, break other things**: Making one test pass while breaking 6 others is net negative
3. **Respect original architecture**: The design had apply blocks in AST for good reasons
4. **Need deeper investigation**: Must understand WHY empty functions are generated

## Next Session TODO

1. Revert hir_builder.rs changes
2. Find where empty wrapper functions are created
3. Trace a working println statement through entire pipeline
4. Compare with broken apply block path
5. Fix actual code generation issue (not HIR lowering)
