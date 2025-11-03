# Default Constructor Generation Fix - Session 2025-10-25

## Summary

Fixed missing default constructor generation for Clean Language classes, resolving compilation errors for 2 out of 3 affected files.

## Problem

Files were failing with errors like:
```
Constructor for class 'Animal' not found
Constructor for class 'DataProcessor' not found
Constructor for class 'TestData' not found
```

Classes without explicit constructors couldn't be instantiated because the compiler didn't auto-generate default constructors.

## Root Cause

In `src/resolver/resolver_impl.rs` lines 295-312, when a class lacked an explicit constructor:

```rust
let constructor_symbol_id_opt = if let Some(constructor) = &class.constructor {
    // Create constructor symbol
    Some(constructor_symbol_id)
} else {
    None  // ❌ NO DEFAULT CONSTRUCTOR!
};
```

This meant:
1. No constructor symbol registered in symbol table
2. Later, when code calls `Animal()`, lookup for `"Animal.constructor"` fails
3. Compilation error: "Constructor for class 'Animal' not found"

## Solution

**Modified** `src/resolver/resolver_impl.rs` **lines 295-322 and 375-389**:

### Part 1: Always Create Constructor Symbol (lines 295-322)

```rust
// CRITICAL FIX: Always create a constructor symbol, even for classes without explicit constructors
// Generate default constructor if class doesn't have one
let constructor_symbol_id = if let Some(constructor) = &class.constructor {
    self.symbol_table.create_symbol(
        format!("{}.constructor", class.name),
        SymbolKind::Constructor {
            class_id: class_symbol_id,
            parameters: constructor
                .parameters
                .iter()
                .map(|p| p.param_type.clone())
                .collect(),
        },
        global_scope,
        constructor.location.clone(),
    )
} else {
    // Generate default constructor (no parameters, empty body)
    self.symbol_table.create_symbol(
        format!("{}.constructor", class.name),
        SymbolKind::Constructor {
            class_id: class_symbol_id,
            parameters: vec![],  // No parameters for default constructor
        },
        global_scope,
        class.location.clone(),
    )
};
```

**Changed:** `constructor_symbol_id_opt: Option<SymbolId>` → `constructor_symbol_id: SymbolId`

### Part 2: Generate Default Constructor Body (lines 375-389)

```rust
let resolved_constructor = if let Some(constructor) = &class.constructor {
    // Use explicit constructor
    Some(self.resolve_constructor(constructor, class_symbol_id, constructor_symbol_id)?)
} else {
    // CRITICAL FIX: Generate default constructor with empty body
    Some(ResolvedHirConstructor {
        symbol_id: constructor_symbol_id,
        parameters: vec![],
        body: ResolvedHirBlock {
            statements: vec![],  // Empty body for default constructor
            location: class.location.clone(),
        },
        location: class.location.clone(),
    })
};
```

**Changed:** Removed `None` case, always generate `Some(ResolvedHirConstructor)`

## Results

### Fixed Files: +2

1. ✅ **tests/cln/debug/simple_method_test.cln** - Animal class (no explicit constructor)
2. ✅ **tests/cln/debug/test_apply_blocks.cln** - DataProcessor class (no explicit constructor)

### Remaining Issue: 1 file (Forward Reference)

❌ **tests/cln/parser_compliance/05_expressions.cln** - TestData constructor not found

**Different Issue:** This file has TestData class WITH an explicit constructor, but it's used before it's defined:
- Line 94: `data = TestData()` (inside TestObject constructor)
- Line 96: `class TestData` (class definition)
- Line 99: `constructor()` (explicit constructor)

This is a **forward reference** problem requiring two-pass symbol resolution (register all classes first, then resolve bodies).

## Impact

**Before:**
- Classes without constructors: Cannot instantiate ❌
- Compilation fails with "Constructor not found"

**After:**
- Classes without constructors: Auto-generate default constructor ✅
- Can instantiate with `ClassName()`
- Clean Language behaves like modern languages (Java, C#, Python)

## Technical Details

### Default Constructor Behavior

When a class has no explicit constructor, the compiler now automatically generates:

```clean
class Animal
    functions:
        string getName()
            return "Animal"

// Auto-generated default constructor (invisible to user):
constructor()
    // Empty body - just allocates instance
```

### Symbol Table Registration

- **Symbol Name:** `{ClassName}.constructor`
- **Symbol Kind:** `SymbolKind::Constructor { class_id, parameters: vec![] }`
- **Scope:** Global (same as class symbol)
- **Accessibility:** Can be called from anywhere

### MIR/WASM Generation

The empty default constructor:
1. **Allocates memory** for class instance
2. **Initializes field defaults** (if any)
3. **Returns instance pointer**

No explicit code needed - memory allocation handles it.

## Files Modified

- **src/resolver/resolver_impl.rs** (lines 295-322, 375-389) - Default constructor generation

## Related Issues

### Forward Reference Problem (Separate Fix Needed)

**Issue:** Classes used before definition fail to resolve
**Example:** 05_expressions.cln uses TestData before it's defined
**Solution:** Two-pass resolution:
1. **Pass 1:** Register all class symbols + constructor symbols
2. **Pass 2:** Resolve constructor/method bodies

This requires architectural changes to the resolver and is tracked as a separate task.

## Testing

```bash
# Test files with default constructor fix
./target/release/clean-language-compiler compile \
    -i tests/cln/debug/simple_method_test.cln \
    -o /tmp/test_animal.wasm
# ✅ Successfully compiled

./target/release/clean-language-compiler compile \
    -i tests/cln/debug/test_apply_blocks.cln \
    -o /tmp/test_dataprocessor.wasm
# ✅ Successfully compiled

# Forward reference issue (different problem)
./target/release/clean-language-compiler compile \
    -i tests/cln/parser_compliance/05_expressions.cln \
    -o /tmp/test_testdata.wasm
# ❌ Constructor for class 'TestData' not found (forward reference)
```

## Compilation Status

```bash
✅ cargo build --lib      # Success
✅ cargo build --release  # Success (2m 11s)
```

## Next Steps

1. **Fix forward references** - Two-pass class resolution
2. **Register math namespace functions** - math_sin, math_cos, etc.
3. **Fix SymbolId resolution errors** - 12 files remaining
4. **Investigate WASM validation errors** - 35 files

## Lessons Learned

**For Classes in Clean Language:**
- Always generate default constructors (like Java, C#, Python)
- Constructor symbols must be in global scope
- Empty body works fine (memory allocation is implicit)
- Forward references need special handling (separate issue)

**For Compiler Development:**
- Symbol resolution order matters for forward references
- Two-pass resolution common in class-based languages
- Default constructor generation is standard practice
- Empty bodies are valid for constructors
