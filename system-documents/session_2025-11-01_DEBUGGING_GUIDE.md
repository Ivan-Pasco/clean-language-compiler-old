# Debugging Guide for Remaining 4 WASM Validation Errors

## Test Results

### Simple base() Case - ✅ WORKS
**File**: `/tmp/test_base_minimal.cln`
```clean
class Parent
    integer value
    constructor(integer v)
        value = v

class Child is Parent
    constructor(integer v)
        base(v)

start()
    Child c = Child(42)
```
**Result**: Compiles and validates successfully!

### Complex Polymorphism Case - ❌ FAILS
**File**: `tests/cln/language/classes/16_classes_polymorphism_fixed.cln`
**Error**: `type mismatch in call, expected [i32] but got []` at offset 0x0da5
**Second Error**: `type mismatch at end of function, expected [] but got [i32]`

## Key Difference

**Simple case**: Single inheritance level, minimal parameters
**Complex case**: Multiple inheritance levels, multiple parameters, polymorphic methods

## Debugging Steps for Next Session

###Step 1: Add Debug Output to Constructor Calls

Add debug output in the codegen to see what arguments are being passed:

**File to modify**: `src/codegen/wasm_codegen.rs` or `src/mir/mir_builder.rs`

Search for where constructor calls are generated and add:
```rust
eprintln!("DEBUG CONSTRUCTOR CALL: function_name={}, args={:?}", function_name, args.len());
```

### Step 2: Compare WASM Output

Compile both test cases with debug output enabled:
```bash
./target/release/clean-language-compiler compile -i /tmp/test_base_minimal.cln -o /tmp/simple.wasm 2>&1 | grep "CONSTRUCTOR CALL"
./target/release/clean-language-compiler compile -i tests/cln/language/classes/16_classes_polymorphism_fixed.cln -o /tmp/complex.wasm 2>&1 | grep "CONSTRUCTOR CALL"
```

Look for differences in argument counts.

### Step 3: Examine MIR Representation

Search for how base() calls are represented in MIR:

**Files to search**:
- `src/mir/mod.rs` - MIR instruction definitions
- `src/mir/mir_builder.rs` - MIR construction from TAST

**Search for**:
- "base"
- "BaseConstructor"
- "parent"
- "super"

### Step 4: Check Function Signature Generation

The error "expected [i32] but got []" suggests the function signature expects 1 parameter (the `this` pointer) but the call site provides 0 arguments.

**Possible causes**:
1. **Base constructor signature is wrong** - Should have `this` + parameters
2. **Call site is wrong** - Should pass `this` + user arguments
3. **Mismatch between signature and call** - One has `this`, other doesn't

**Files to check**:
- Where constructor signatures are generated
- Where base() calls translate to WASM `call` instructions
- Parameter counting logic

### Step 5: Search for "demonstratePolymorphism"

The file has a function `demonstratePolymorphism(Vehicle vehicle)` that accepts a parent class type. This might be related to the error since it's dealing with polymorphism.

Check if the error is in:
- The function definition
- The function calls
- The method calls on the polymorphic parameter

##Recommended Fix Approach

### Option A: Fix base() Call Site

If the issue is that base() calls aren't passing `this`:

**Location**: Wherever constructor calls are generated in codegen
**Fix**: When generating a call to a parent constructor via base(), prepend the current `this` value

```rust
// BEFORE (wrong)
let args = /* user arguments */;
builder.call(parent_constructor_index, args);

// AFTER (correct)
let mut args_with_this = vec![current_this];
args_with_this.extend(user_arguments);
builder.call(parent_constructor_index, args_with_this);
```

### Option B: Fix Constructor Signature

If the issue is that constructor signatures don't include `this`:

**Location**: Where function signatures are generated
**Fix**: Ensure all constructors have `this` as first parameter

```rust
// Constructor signature should be: (this: i32, ...user_params) -> void
```

### Option C: Both Need Fixing

Most likely both the signature AND the call site need to be consistent.

## Quick Test to Verify Fix

After making changes, run:
```bash
# Rebuild compiler
cargo build --release

# Test simple case (should still work)
./target/release/clean-language-compiler compile -i /tmp/test_base_minimal.cln -o /tmp/test1.wasm
wasm-validate /tmp/test1.wasm

# Test complex case (should now work)
./target/release/clean-language-compiler compile -i tests/cln/language/classes/16_classes_polymorphism_fixed.cln -o /tmp/test2.wasm
wasm-validate /tmp/test2.wasm

# If successful, test all files
./validate_all.sh
```

## Expected Outcome

After fixing the base() issue:
- **2 files fixed**: 16_classes_polymorphism_fixed.cln, 16_classes_polymorphism_new.cln
- **Remaining errors**: 2 (calculator return type, static method parameter)
- **Success rate**: 99.3% (231/233 compiled files)

## Code Locations to Investigate

### Primary suspects:
1. `src/codegen/wasm_codegen.rs` - WASM call instruction generation
2. `src/mir/mir_builder.rs` - MIR construction, especially constructor handling
3. `src/codegen/wasm_codegen.rs` - Function signature generation

### Search terms:
- "constructor"
- "base"
- "parent"
- "call" (in context of function calls)
- "signature"
- "parameters"

## Notes from Investigation

- Simple inheritance with base() works correctly
- Complex polymorphism with base() fails
- Error suggests missing `this` parameter in call
- Two errors in same file: call mismatch + return type mismatch
- Second error might be cascading from first error

## Success Criteria

The fix is complete when:
1. `/tmp/test_base_minimal.cln` still validates ✅
2. `tests/cln/language/classes/16_classes_polymorphism_fixed.cln` validates ✅
3. `tests/cln/language/classes/16_classes_polymorphism_new.cln` validates ✅
4. No new validation errors introduced ✅
