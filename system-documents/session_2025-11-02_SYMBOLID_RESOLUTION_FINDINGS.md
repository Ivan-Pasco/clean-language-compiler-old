# Session 2025-11-02: SymbolId Resolution Findings and Path Forward

## Current Status

**CRITICAL ISSUE**: SymbolId resolution system is fundamentally broken due to hardcoded mappings that don't match runtime values.

### Test Results
- **Total test files**: 297
- **Successfully compiling**: Unknown (regression from expected 93.6%)
- **Primary error**: "Cannot resolve SymbolId(X) to function name during code generation"

## Attempted Solutions

### 1. Minimal Symbol Map Population ✅ IMPLEMENTED
**What**: Added `symbol_name_map` to MirProgram and populated it with user-defined functions in mir_builder.rs

**Result**: Compiles successfully, but only captures user-defined functions, not builtin functions like `print()`, `math.*`, etc.

**Files modified**:
- `src/mir/mir_types.rs` - Added `symbol_name_map` field
- `src/mir/mir_builder.rs` - Populate map for user functions, start function, and class methods
- `src/codegen/mir_codegen.rs` - Initialize from symbol_name_map

### 2. Revert Conditional Operations Registration ❌ NO EFFECT
**What**: Removed lines 191-196 from mir_codegen.rs that registered conditional operations

**Result**: Did NOT fix the issue. SymbolId(162) error persisted.

**Conclusion**: The regression was NOT caused by conditional operations registration.

### 3. Update Hardcoded SymbolId Mappings ⚠️ PARTIALLY HELPED
**What**: Updated hardcoded mappings in `get_function_name_by_symbol()`:
- `print` from SymbolId(0) → SymbolId(162)
- Other functions offset by +162

**Result**: Fixed SymbolId(162) error, but exposed new SymbolId(0) error. There are too many different SymbolIds to manually map.

**Conclusion**: Manual mapping is not feasible - need complete dynamic system.

## Root Cause Analysis

### The Real Problem
SymbolIds are dynamically assigned during semantic analysis/resolution phase. The hardcoded mappings in `get_function_name_by_symbol()` assume a specific assignment order that no longer matches reality.

### Why It's Broken
1. Builtin functions (`print`, `math.*`, `string.*`, etc.) are registered in the resolver/symbol table
2. They get dynamic SymbolIds during registration
3. MIR and codegen don't have access to the symbol table
4. Code tries to use hardcoded SymbolId→name mappings
5. Mappings are wrong → compilation fails

### Evidence
- `print()` expected to be SymbolId(0), actually SymbolId(162)
- Same file has other functions with SymbolId(0)
- Different functions have different, unpredictable SymbolIds

## The Path Forward

### REQUIRED: Complete Dynamic Resolution System

**Goal**: Eliminate ALL hardcoded SymbolId mappings and use purely dynamic resolution.

**Approach**: Pass symbol table through the entire compiler pipeline.

#### Phase 1: Add Symbol Table to TAST
```rust
// In src/typechecker/tast.rs
pub struct TastProgram {
    // ... existing fields
    pub symbol_table: Arc<SymbolTable>,  // ADD THIS
}
```

#### Phase 2: Pass Symbol Table to MIR Builder
```rust
// In src/mir/mir_builder.rs
pub struct MirBuilder {
    // ... existing fields
    symbol_table: Arc<SymbolTable>,  // ADD THIS
}
```

#### Phase 3: Populate symbol_name_map from Symbol Table
```rust
// In mir_builder.rs build_program()
for (symbol_id, symbol) in &tast.symbol_table.symbols {
    mir_program.symbol_name_map.insert(*symbol_id, symbol.name.clone());
}
```

#### Phase 4: Remove Hardcoded Mappings
```rust
// In mir_codegen.rs get_function_name_by_symbol()
fn get_function_name_by_symbol(&self, symbol_id: SymbolId) -> Option<String> {
    // REMOVE all match arms with hardcoded IDs
    // ONLY use function_symbol_map lookup
    self.function_symbol_map.get(&symbol_id).cloned()
}
```

### Alternative: Revert to Last Known Good State

If dynamic system is too complex, could revert to commit before regression:
```bash
git log --oneline | grep -E "(93|fix|stdlib)"
git reset --hard <commit-hash>
```

But this loses other important fixes.

## Files Requiring Changes

### High Priority
1. `src/typechecker/tast.rs` - Add symbol_table field to TastProgram
2. `src/typechecker/type_inference.rs` - Pass symbol_table when creating TAST
3. `src/mir/mir_builder.rs` - Accept and use symbol_table, populate symbol_name_map
4. `src/codegen/mir_codegen.rs` - Remove hardcoded SymbolId mappings

### Supporting Files
5. Anywhere TastProgram is constructed - pass symbol_table
6. Anywhere MirBuilder is constructed - pass symbol_table

## Estimated Impact

**Complexity**: Medium-High (need to thread symbol_table through multiple phases)

**Risk**: Medium (changing core data structures)

**Benefit**: HIGH - Eliminates entire class of fragile hardcoded mappings

**Time**: 2-3 hours of focused work

## Current Session Files Modified

1. `src/mir/mir_types.rs` - Added symbol_name_map ✅
2. `src/mir/mir_builder.rs` - Populate map for user functions ✅
3. `src/codegen/mir_codegen.rs` - Updated some SymbolId mappings (partially)

## Recommendation

**IMPLEMENT THE COMPLETE DYNAMIC SYSTEM** as outlined above. The hardcoded approach is fundamentally flawed and will continue to break as the compiler evolves.

## Next Session Actions

1. Add `symbol_table: Arc<SymbolTable>` to TastProgram
2. Thread symbol_table through type checker
3. Populate symbol_name_map from symbol_table in MIR builder
4. Remove ALL hardcoded SymbolId mappings
5. Test with full test suite
6. Verify 90%+ success rate restored

---

**Session Date**: 2025-11-02
**Issue Severity**: 🔴 CRITICAL - Blocks 16.8% of test suite (50 files)
