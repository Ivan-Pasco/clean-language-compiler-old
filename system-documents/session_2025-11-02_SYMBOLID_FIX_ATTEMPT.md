# Session 2025-11-02: SymbolId Resolution Fix Attempt

## Summary

Attempted to fix the SymbolId resolution system to resolve "Cannot resolve SymbolId(X) to function name" errors affecting 50 files.

## Root Cause

The conditional operations registration added in this session (lines 191-196 of `mir_codegen.rs`) MAY have changed function registration order, but the real issue is:

**Hardcoded SymbolId mappings in `get_function_name_by_symbol()` don't match runtime values**

Example:
- Code expects: `print` = SymbolId(0)
- Runtime actual: `print` = SymbolId(162)

## Attempted Solution

Added `symbol_name_map` to MirProgram to dynamically capture ALL function names:

### Changes Made
1. **mir_types.rs**: Added `symbol_name_map: HashMap<SymbolId, String>` field to MirProgram
2. **mir_builder.rs**: Attempted to extract function names from TAST by scanning expressions
3. **mir_codegen.rs**: Initialize `function_symbol_map` from MirProgram's `symbol_name_map`

### Why It Failed
Compilation errors (23 total) due to:
- Complex TAST structure (TastBlock, TastStatement, TastExpression)
- Mismatched field names (`constructor` vs `constructors`, `value` vs `initializer`)
- Type mismatches (expecting expressions but getting blocks)

## Recommendation

**REVERT** the conditional operations registration and use a simpler approach:

### Option 1: Just Revert (RECOMMENDED FOR NOW)
```bash
git checkout src/codegen/mir_codegen.rs  # Remove lines 191-196
```
This restores 93.6% success rate.

### Option 2: Simpler SymbolId Fix (FUTURE)
Instead of scanning TAST, just:
1. Run one test file with debug output to see actual SymbolIds
2. Update hardcoded mappings in `get_function_name_by_symbol()`
3. Much simpler, less error-prone

### Option 3: Complete Dynamic System (FUTURE, COMPLEX)
Finish the symbol_name_map implementation by:
1. Fixing all TAST structure issues in extract functions
2. Handling TastBlock.statements properly
3. Using correct field names for each variant

## Files Modified
- `src/mir/mir_types.rs` - Added symbol_name_map field ✅
- `src/mir/mir_builder.rs` - Added extraction methods (REVERTED due to errors)
- `src/codegen/mir_codegen.rs` - Uses symbol_name_map ✅

## Current Status
- ✅ MirProgram has symbol_name_map field
- ❌ Extraction from TAST has compilation errors  
- ✅ Code generator ready to use symbol_name_map
- ⚠️  Need to revert or complete the fix

## Next Session Actions
1. Decide: Revert or fix?
2. If revert: Remove conditional ops registration, restore 93.6%
3. If fix: Complete TAST extraction (fix 23 compilation errors)
