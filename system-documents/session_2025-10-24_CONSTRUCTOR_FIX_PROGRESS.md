# Constructor SymbolId Fix Progress - Session 2025-10-24

## Summary

**Status**: ⚠️ Partial Fix - Still Debugging
**Files Modified**: 4 (src/resolver/mod.rs, src/resolver/resolver_impl.rs, src/typechecker/type_inference.rs, src/codegen/mir_codegen.rs)
**Build Status**: ✅ Compiles Successfully
**Test Status**: ❌ Still failing with SymbolId resolution errors

## Changes Made

### 1. Added `symbol_id` to `ResolvedHirConstructor` ✅
**File**: `src/resolver/mod.rs:93`

```rust
pub struct ResolvedHirConstructor {
    pub symbol_id: SymbolId,  // NEW
    pub parameters: Vec<ResolvedHirParameter>,
    pub body: ResolvedHirBlock,
    pub location: SourceLocation,
}
```

### 2. Created Constructor SymbolIds in Global Scope ✅
**File**: `src/resolver/resolver_impl.rs:294-312`

**Key Change**: Constructor symbols are now created in **global scope** (before entering class_scope) to ensure they're accessible from anywhere, just like class symbols.

```rust
// Create constructor symbol in global scope BEFORE entering class scope
let global_scope = self.symbol_table.current_scope_id();
let constructor_symbol_id_opt = if let Some(constructor) = &class.constructor {
    let constructor_symbol_id = self.symbol_table.create_symbol(
        format!("{}.constructor", class.name),
        SymbolKind::Constructor {
            class_id: class_symbol_id,
            parameters: /*...*/,
        },
        global_scope,  // Global scope - accessible everywhere
        constructor.location.clone(),
    );
    Some(constructor_symbol_id)
} else {
    None
};
```

### 3. Updated `ResolvedHirExpression::Constructor` ✅
**File**: `src/resolver/mod.rs:288-294`

```rust
Constructor {
    class_name: String,
    class_symbol_id: SymbolId,
    constructor_symbol_id: SymbolId,  // NEW
    arguments: Vec<ResolvedHirExpression>,
    location: SourceLocation,
},
```

### 4. Updated Constructor Lookup in Resolver ✅
**Files**: `src/resolver/resolver_impl.rs:1033-1042, 1307-1317`

Both locations where constructors are resolved now look up the constructor SymbolId by name:

```rust
let constructor_name = format!("{}.constructor", class_name);
let constructor_symbol_id = self
    .symbol_table
    .lookup_symbol(&constructor_name)
    .ok_or_else(|| {
        self.error(
            &format!("Constructor for class '{}' not found", class_name),
            location.clone(),
        );
    })?;
```

### 5. Updated Typechecker to Use Constructor SymbolId ✅
**File**: `src/typechecker/type_inference.rs:2038`

```rust
kind: TastExpressionKind::Variable {
    symbol_id: *constructor_symbol_id,  // Use constructor's SymbolId, not class's
    name: format!("{}.constructor", class_name),
},
```

## Problem Still Occurring

**Error**: `Cannot resolve SymbolId(203) to function name during code generation`

**Root Cause Hypothesis**: Constructors might not be converted to MIR functions. The constructor SymbolId is now correct, but the constructor function itself might not be in the `mir_program.functions` HashMap.

## Next Steps (Priority Order)

### 1. Verify MIR Function Generation for Constructors
**Action**: Check if constructors are being converted from TAST to MIR functions
**Location**: `src/mir/mir_builder.rs`
**Expected**: Constructors should be added to `mir_program.functions` with their SymbolId

### 2. Check TAST Constructor Generation
**Action**: Verify constructors are included in TastProgram
**Location**: `src/typechecker/mod.rs`
**Expected**: TastClass.constructors should be populated

### 3. Trace SymbolId Through Pipeline
**Action**: Add debug logging to track the constructor SymbolId(203) through:
1. Resolver (created)
2. Typechecker (used in FunctionCall)
3. MIR Builder (should create MirFunction)
4. Codegen (should find in function_symbol_map)

### 4. Alternative Fix: Extract Constructors as Separate TastFunctions
If constructors aren't being processed, they may need to be extracted from TastClass and added to TastProgram.functions

## Files Modified Summary

1. **src/resolver/mod.rs**
   - Line 93: Added `symbol_id` to `ResolvedHirConstructor`
   - Line 291: Added `constructor_symbol_id` to `ResolvedHirExpression::Constructor`

2. **src/resolver/resolver_impl.rs**
   - Lines 294-312: Create constructor symbol in global scope
   - Lines 365-369: Use pre-created constructor_symbol_id
   - Lines 425: Updated `resolve_constructor` signature
   - Lines 474: Return constructor_symbol_id in result
   - Lines 1033-1042: Look up constructor_symbol_id in first Constructor creation
   - Lines 1307-1317: Look up constructor_symbol_id in second Constructor creation

3. **src/typechecker/type_inference.rs**
   - Line 2018: Added `constructor_symbol_id` to pattern match
   - Line 2038: Use constructor_symbol_id instead of class_symbol_id

4. **src/codegen/mir_codegen.rs** (previous session)
   - Line 58: Added `function_name_to_symbol` (may be removed)
   - Lines 1896-1910: Added TODO comment (to be addressed)

## Technical Details

### Scope Hierarchy Problem (SOLVED ✅)
**Issue**: Constructor symbols were created in `class_scope`, but looked up from `function_scope` (sibling scopes).

**Solution**: Create constructor symbols in global scope before entering class_scope, making them accessible from anywhere.

**Scope Tree**:
```
global_scope
├── Test.constructor (SymbolId 203) ← Now accessible from start()
├── class_scope (Test class)
│   ├── fields
│   └── methods
└── function_scope (start function)
    └── Looking up "Test.constructor" → SUCCESS
```

### Constructor Naming Convention
Constructors are named `{ClassName}.constructor` (e.g., "Test.constructor", "Cat.constructor")

This allows unique global identification and lookup.

## Test Cases

### Failing Tests
1. `tests/cln/debug/test_boolean_assignment.cln` - Simple constructor
2. `tests/cln/debug/test_cat_only.cln` - Inherited constructor with base() call

Both fail with: `Cannot resolve SymbolId(203) to function name during code generation`

### Expected Fix Impact
**68 files** currently failing with constructor SymbolId errors should compile after this fix is complete.

**Compilation Rate**: 76.9% → ~99% (+22.1 percentage points)
**Validation Rate**: 56.3% → ~80% (+23.7 percentage points)

## Build Status

✅ Compiler builds successfully
✅ All changes compile without errors
❌ Constructor calls still fail at runtime

## Conclusion

Significant progress made on the architectural fix. The constructor SymbolId plumbing is now in place:
- ✅ Constructor symbols created with unique SymbolIds
- ✅ Created in global scope for accessibility
- ✅ Resolver passes constructor_symbol_id correctly
- ✅ Typechecker uses constructor_symbol_id instead of class_symbol_id

**Remaining Issue**: Constructors may not be converted to MIR functions, so the function_symbol_map doesn't contain them.

**Next Session**: Focus on MIR generation for constructors to complete the fix.
