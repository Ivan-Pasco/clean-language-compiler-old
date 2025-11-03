# Session 2025-10-24: Constructor SymbolId Investigation

## Date: 2025-10-24 (Final phase)

## Executive Summary

**Problem**: 68 files fail compilation with "Cannot resolve SymbolId(X) to function name during code generation"
**Root Cause**: Constructor calls use class SymbolId instead of constructor function SymbolId
**Status**: ✅ Root cause identified, 🎯 Fix location determined, ⏳ Implementation pending

## Investigation Results

### The Problem

**Error Pattern**:
```
Error: Cannot resolve SymbolId(202) to function name during code generation
Error: Cannot resolve SymbolId(203) to function name during code generation
```

**Failing File Example** (`tests/cln/debug/test_cat_only.cln`):
```clean
class Cat is Animal
    constructor(string catName, integer catAge, boolean indoorFlag)
        base(catName, catAge)

start()
    Cat test = Cat("test", 5, true)  // <-- This line fails
```

### Root Cause Analysis

#### The Data Flow

1. **Parser/HIR**: Constructor call `Cat(...)` is parsed as a function call
2. **Resolver**: Resolves `Cat` to the class's SymbolId (e.g., 202)
3. **Typechecker** (src/typechecker/type_inference.rs:1723-1772):
   - Creates `TastExpressionKind::FunctionCall`
   - Uses class's SymbolId in the Variable expression: **Lines 1753-1756**
   ```rust
   function: Box::new(TastExpression {
       kind: TastExpressionKind::Variable {
           symbol_id: *function_symbol_id,  // <-- class's SymbolId!
           name: function.clone(),
       },
       ...
   }),
   ```
4. **MIR Builder**: Extracts SymbolId from FunctionCall's Variable expression
   - Location: src/mir/mir_builder.rs:1443-1450
   - Gets class's SymbolId (202)
5. **MIR Program**: Constructors stored with constructor function's SymbolId
   - Constructor function has different SymbolId (e.g., 999)
   - Stored in `mir_program.functions: {999 => MirFunction{name: "Cat", ...}}`
6. **Codegen**: Builds function_symbol_map from MIR functions
   - Location: src/codegen/mir_codegen.rs:188-203
   - Map contains: `{999 => "Cat"}`
7. **Codegen Resolution**: Tries to resolve class SymbolId (202)
   - Location: src/codegen/mir_codegen.rs:1878-1910
   - Lookup fails: 202 not in `{999 => "Cat"}`
   - Returns None → Error: "Cannot resolve SymbolId(202)"

#### Why This Happens

**TastClass has both**:
- `symbol_id`: The class's SymbolId (202)
- `constructors: Vec<TastFunction>`: Each with its own SymbolId (999)

**The typechecker uses the WRONG SymbolId** - it uses the class's SymbolId for constructor calls instead of the actual constructor function's SymbolId.

### Failed/Partial Fixes Attempted

#### 1. Codegen Fallback (Partial)
**What**: Added heuristic to search for constructors when SymbolId not found
**Why Failed**: Can't reliably identify which constructor without the class-constructor mapping
**Code**: src/codegen/mir_codegen.rs:1896-1910 (now documented as TODO)

#### 2. Reverse Name Mapping (Ineffective)
**What**: Added `function_name_to_symbol` HashMap
**Why Ineffective**: Still don't have the function name from just the class SymbolId
**Code**: src/codegen/mir_codegen.rs:58, 194

## The Proper Fix

### Location: src/typechecker/type_inference.rs:1723-1772

**Current Code** (INCORRECT):
```rust
ResolvedHirExpression::Call {
    function,
    function_symbol_id,  // <-- This is the class's SymbolId
    arguments,
    location,
} => {
    // ...
    (
        TastExpressionKind::FunctionCall {
            function: Box::new(TastExpression {
                kind: TastExpressionKind::Variable {
                    symbol_id: *function_symbol_id,  // WRONG: Uses class SymbolId
                    name: function.clone(),
                },
                ...
            }),
            ...
        },
        return_type,
        location.clone(),
    )
}
```

**Required Fix**:
1. Detect if this is a constructor call (check if `function_symbol_id` refers to a class)
2. If so, look up the class's constructor's SymbolId from the type_env or symbol table
3. Use the constructor's SymbolId instead of the class's SymbolId

**Pseudocode**:
```rust
let actual_symbol_id = if self.is_class_symbol(*function_symbol_id) {
    // Look up the default constructor for this class
    self.get_constructor_symbol_id(*function_symbol_id)?
} else {
    *function_symbol_id
};

// Then use actual_symbol_id instead of function_symbol_id
```

### Alternative Fix Locations

#### Option 2: Resolver Layer
**Location**: src/resolver/*.rs where constructor calls are resolved
**Approach**: Make the resolver return the constructor's SymbolId instead of the class's SymbolId
**Complexity**: Lower - resolver already has access to class and constructor information

#### Option 3: MIR Builder
**Location**: src/mir/mir_builder.rs:1434-1495
**Approach**: When building FunctionCall MIR, check if SymbolId refers to a class and substitute constructor's SymbolId
**Complexity**: Medium - needs access to class→constructor mapping

### Recommended Approach

**Best**: Fix in Resolver (Option 2)
- Resolver already has class and constructor information
- Cleanest separation of concerns
- Prevents the wrong SymbolId from propagating through the pipeline

**Why not Typechecker**: Type inference shouldn't know about constructors vs classes - that's resolved semantics

**Why not MIR Builder**: Same reason - symbol resolution should happen earlier

**Why not Codegen**: Too late - we've lost the connection between class and constructor

## Files Modified (Investigation Only)

### Production Code
1. **src/codegen/mir_codegen.rs**:
   - Line 58: Added `function_name_to_symbol` HashMap (ineffective for this fix)
   - Lines 194: Populated reverse mapping
   - Lines 1896-1910: Added TODO comment documenting proper fix location

### Documentation
- `session_2025-10-24_CONSTRUCTOR_SYMBOLID_INVESTIGATION.md` (this file)

## Impact Analysis

**Files Affected**: 68 out of 295 (23%)
**Expected Improvement After Fix**:
- Compilation: 76.9% → ~99% (+22.1 percentage points)
- Validation: 56.3% → ~80% (+23.7 percentage points)

**Why Such Large Impact**: Almost all Clean Language programs use classes and constructors

## Test Cases to Verify Fix

### Simple Constructor Call
```clean
class Test
    boolean flag
    constructor(boolean value)
        flag = value

start()
    Test test = Test(true)  // Should compile
```

### Inherited Constructor Call
```clean
class Animal
    string name
    constructor(string animalName)
        name = animalName

class Cat is Animal
    boolean isIndoor
    constructor(string catName, boolean indoorFlag)
        base(catName)
        isIndoor = indoorFlag

start()
    Cat test = Cat("Felix", true)  // Should compile
```

### Multiple Constructors
```clean
class Point
    integer x
    integer y

    constructor()
        x = 0
        y = 0

    constructor(integer px, integer py)
        x = px
        y = py

start()
    Point p1 = Point()           // Should compile
    Point p2 = Point(10, 20)     // Should compile
```

## Next Session Action Plan

### Priority 1: Implement Resolver Fix
1. Search for constructor call resolution in src/resolver/*.rs
2. Identify where SymbolIds are assigned to constructor calls
3. Modify to return constructor's SymbolId instead of class's SymbolId
4. Ensure all code paths are covered (default constructors, parameterized, inherited)

### Priority 2: Test and Validate
1. Compile test_cat_only.cln and test_boolean_assignment.cln
2. Run comprehensive test suite
3. Verify 68 files now compile successfully
4. Check validation rate improvement

### Priority 3: Clean Up
1. Remove ineffective `function_name_to_symbol` mapping if no longer needed
2. Update TODO comments
3. Document the fix

## Technical Details

### SymbolId Mapping Example

**Before Fix**:
```
Class "Cat": SymbolId(202)
Constructor "Cat": SymbolId(999)

FunctionCall("Cat", ...):
  Variable.symbol_id = 202  ❌ class's SymbolId

function_symbol_map:
  {999 => "Cat"}

Resolution: 202 not in map → Error
```

**After Fix**:
```
Class "Cat": SymbolId(202)
Constructor "Cat": SymbolId(999)

FunctionCall("Cat", ...):
  Variable.symbol_id = 999  ✅ constructor's SymbolId

function_symbol_map:
  {999 => "Cat"}

Resolution: 999 found in map → Success
```

### Key Data Structures

**TastClass** (src/typechecker/tast.rs:50-62):
```rust
pub struct TastClass {
    pub symbol_id: SymbolId,           // Class's SymbolId
    pub name: String,
    pub fields: Vec<TastField>,
    pub methods: Vec<TastFunction>,
    pub constructors: Vec<TastFunction>,  // Each has own SymbolId!
    ...
}
```

**ResolvedHirExpression::Call** (from resolver):
```rust
Call {
    function: String,                  // "Cat"
    function_symbol_id: SymbolId,      // Currently: class's SymbolId (WRONG)
    arguments: Vec<...>,
    location: SourceLocation,
}
```

## Conclusion

This investigation successfully identified the exact root cause of 68 compilation failures:
- ✅ Located the bug: Typechecker uses class SymbolId instead of constructor SymbolId
- ✅ Determined proper fix location: Resolver layer (best) or Typechecker (acceptable)
- ✅ Identified all affected code paths
- ✅ Created reproducible test cases
- ✅ Quantified expected impact

**Status**: Ready for implementation
**Difficulty**: Medium (requires understanding resolver and constructor resolution)
**Expected Time**: 1-2 hours
**Expected Result**: +22% compilation rate, +24% validation rate

---

**Next**: Implement resolver fix to use constructor SymbolId for constructor calls
