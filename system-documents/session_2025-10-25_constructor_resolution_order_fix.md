# Constructor Resolution Order Fix - Session 2025-10-25

## Summary

Fixed constructor symbol resolution order issue that prevented global functions from referencing class constructors. Improved compilation success rate from **96% to 97%** (282/289 files).

## Problem

When a Clean Language program had both classes and a global `functions:` block, constructor calls would fail with:
```
Error: Constructor for class 'Calculator' not found
```

### Affected Files
- tests/cln/integration/real_world/calculator_application.cln
- tests/cln/parser_compliance/05_expressions.cln (forward reference)

### Root Cause

The resolver processes the HIR in two passes:

**First Pass** (`register_top_level_symbols`):
1. Register builtin functions
2. Register global functions ← **Function symbols created**
3. Register start function
4. Register classes ← **Class symbols created, but NOT constructor symbols**

**Second Pass** (`resolve_program`):
1. `resolve_functions()` ← **Function bodies resolved, Constructor() calls happen here**
2. `resolve_classes()` ← **Constructor symbols created** ← ❌ **Too late!**
3. `resolve_start_function()`

When global functions tried to call constructors (e.g., `Calculator()`), the constructor symbols hadn't been created yet because `resolve_classes()` hadn't run.

## Solution

### Modified File: `src/resolver/resolver_impl.rs`

#### Part 1: Register Constructor Symbols in First Pass (lines 158-175)

**Location**: Inside `register_top_level_symbols()`, in the class registration loop

```rust
// Register classes
for class in &hir.classes {
    // ... create class symbol ...

    // CRITICAL FIX: Register constructor symbol in first pass
    // This allows global functions to reference constructors before classes are fully resolved
    let constructor_params = if let Some(constructor) = &class.constructor {
        constructor.parameters.iter().map(|p| p.param_type.clone()).collect()
    } else {
        vec![] // Default constructor has no parameters
    };

    let _constructor_symbol_id = self.symbol_table.create_symbol(
        format!("{}.constructor", class.name),
        SymbolKind::Constructor {
            class_id: class_symbol_id,
            parameters: constructor_params,
        },
        self.symbol_table.current_scope_id(),
        class.location.clone(),
    );
}
```

**Key changes:**
- Extract constructor parameters from class.constructor (or use empty vec for default)
- Create constructor symbol immediately after class symbol
- Use same scope as class symbol (global scope)

#### Part 2: Lookup Constructor Symbol in Second Pass (lines 310-321)

**Location**: Inside `resolve_class()`, before class scope is created

```rust
// Lookup constructor symbol - it was already created in the first pass (register_top_level_symbols)
// This ensures constructors are available before global functions are resolved
let constructor_name = format!("{}.constructor", class.name);
let constructor_symbol_id = self
    .symbol_table
    .lookup_symbol(&constructor_name)
    .ok_or_else(|| {
        self.error(
            &format!("Constructor symbol for class '{}' not found - this is an internal compiler error", class.name),
            class.location.clone(),
        );
    })?;
```

**Key changes:**
- Replaced `create_symbol()` with `lookup_symbol()`
- Constructor symbol was already created in first pass
- Error message indicates internal compiler error if lookup fails

## Results

### Before Fix
```
Success: 280/289 (96%)
Failed: 9/289

Constructor not found: 2 files
- 05_expressions.cln
- calculator_application.cln
```

### After Fix
```
Success: 282/289 (97%)
Failed: 7/289

Constructor not found: 0 files ✅
```

### Files Fixed: +2
1. ✅ **calculator_application.cln** - Uses Calculator() in global functions
2. ✅ **05_expressions.cln** - Forward reference (TestData used before defined)

### Remaining Issues: 7 files

All remaining errors are **loop iterator variable scoping** issues:
- 20_async_parallel.cln - `result` undefined
- 10_comprehensive_features.cln - `item` undefined
- 16_classes_polymorphism.cln - `vehicle` undefined (2 occurrences)
- 13_functions_generics.cln - `name` undefined
- 18_control_flow_loops.cln - `num` undefined
- 32_comprehensive_stdlib.cln - `num` undefined
- 73_console_input_comprehensive.cln - `item` undefined

These files use `iterate item in collection` syntax where iterator variables aren't being registered in scope.

## Technical Details

### Two-Pass Resolution Design

The resolver intentionally uses two passes for symbol resolution:

**Pass 1**: Register all top-level symbols
- Allows forward references (classes/functions used before defined)
- Creates symbol table entries without resolving bodies
- **Now includes constructor symbols**

**Pass 2**: Resolve symbol references in bodies
- Process function bodies
- Process class methods and constructors
- Lookup symbols from pass 1

### Why Constructor Symbols Need Early Registration

1. **Global function bodies** are resolved before class bodies
2. **Constructor calls** in global functions need to lookup `{ClassName}.constructor`
3. **Symbol must exist** before lookup happens
4. **Solution**: Create constructor symbols in pass 1 alongside class symbols

### Constructor Symbol Naming Convention

- **Symbol name**: `{ClassName}.constructor`
- **Example**: `Calculator.constructor`
- **Scope**: Global (same as class symbol)
- **Parameters**: Extracted from class.constructor or empty vec for default

## Impact

**Before:**
- Classes with explicit constructors couldn't be instantiated from global functions
- Forward references to constructors failed
- 96% success rate

**After:**
- All constructor references work correctly
- Global functions can instantiate classes
- Forward references resolved
- **97% success rate** ✅

## Compilation Status

```bash
✅ cargo build --lib                 # Success (15.31s)
✅ cargo build --release              # Success (2m 10s)
✅ All constructor tests passing      # calculator_application.cln, 05_expressions.cln
```

## Testing

### Test Case 1: Global Functions Block
```clean
class Calculator
    number memory
    constructor()
        memory = 0.0

functions:
    void test()
        Calculator calc = Calculator()  // Now works! ✅
```

### Test Case 2: Forward Reference
```clean
class TestObject
    constructor()
        TestData data = TestData()  // Used before defined ✅

class TestData  // Defined after use
    constructor()
        // ...
```

## Next Steps

1. **Fix loop iterator variable scoping** (7 files)
   - `iterate item in collection` - register `item` in loop scope
   - `iterate num in range` - register `num` in loop scope

2. **Target 100% success rate**
   - Currently at 97% (282/289)
   - 7 files remaining (all iterator scoping)

## Lessons Learned

**For Two-Pass Resolution:**
- First pass must register ALL symbols that can be referenced
- Constructor symbols are special - they're method-like but globally accessible
- Processing order matters: functions before classes can cause issues

**For Clean Language Compiler:**
- Constructor symbols follow naming pattern: `{ClassName}.constructor`
- Default constructors (no explicit constructor) still need symbols
- Symbol registration must happen before any body resolution

**For AI-Assisted Development:**
- Systematic testing reveals processing order issues
- Minimal test cases isolate root causes effectively
- Two-pass architecture is common in compilers for forward references

## Related Work

- Session 2025-10-25: Default constructor generation fix (+2 files)
- Session 2025-10-25: Architectural refactoring (component-based design)
- Session 2025-10-25: Math namespace functions verified working

## Progress Tracking

**Session Start**: 95% success rate (281/295 including fail/)
**Session End**: 97% success rate (282/289 excluding fail/)
**Net Improvement**: +1 file fixed (excluding intentional failures)
**Major Fix**: Constructor resolution order (+2 files)
