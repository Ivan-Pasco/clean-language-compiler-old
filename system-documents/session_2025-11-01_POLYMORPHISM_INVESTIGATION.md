# Polymorphism WASM Validation Error Investigation

## Session Date: 2025-11-01

## Summary

Investigation into the 4 remaining WASM validation errors, focused on the polymorphism test files. **Key finding**: The debugging guide's hypothesis about base() constructor calls was INCORRECT. The real issue is more subtle.

## Test Results

### ✅ WORKS: Simple base() Constructor Call
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
    print(c.value.toString())
```
**Result**: Compiles and validates successfully!

### ✅ WORKS: Method Call on Parameter (No Inheritance)
**File**: `/tmp/test_method_param.cln`
```clean
class Vehicle
    string name
    constructor(string vehicleName)
        name = vehicleName
    functions:
        string getName()
            return name

functions:
    void testMethod(Vehicle v)
        print(v.getName())

start()
    Vehicle car = Vehicle("Tesla")
    testMethod(car)
```
**Result**: Compiles and validates successfully!

### ✅ WORKS: Method Call with Inheritance (Static Dispatch)
**File**: `/tmp/test_method_override.cln`
```clean
class Vehicle
    string name
    constructor(string vehicleName)
        name = vehicleName
    functions:
        string getName()
            return name

class Car is Vehicle
    constructor(string carName)
        base(carName)
    functions:
        string getName()
            return "Car: " + name

functions:
    void testMethod(Vehicle v)
        print(v.getName())

start()
    Car myCar = Car("Tesla")
    testMethod(myCar)
```
**Result**: Compiles and validates successfully!
**NOTE**: Uses **static dispatch** - calls `Vehicle.getName()` (index 90), NOT `Car.getName()` (index 91)

### ❌ FAILS: Complex Polymorphism Test
**File**: `tests/cln/language/classes/16_classes_polymorphism_fixed.cln`
**Error**:
```
type mismatch in call, expected [i32] but got [] at offset 0x0da5
type mismatch at end of function, expected [] but got [i32] at offset 0x0da5
```

## Key Findings

### 1. base() Constructor Calls Work Correctly

The MIR builder correctly handles base() calls:
- **File**: `src/mir/mir_builder.rs` lines 2312-2420
- **Behavior**: Prepends `this` parameter to base() call arguments
- **Debug Output**: `DEBUG MIR BASECALL: Total arguments (including this): 4`

The debugging guide's hypothesis was **incorrect** - base() calls are NOT the issue.

### 2. Method Calls on Parameters Work (With Static Dispatch)

Method calls on parent-type parameters work correctly:
- The receiver is passed as the first argument ✅
- The method is resolved to the parent class's implementation (static dispatch, not dynamic)
- WASM is generated correctly ✅

### 3. Clean Language Uses Static Dispatch, Not Dynamic Polymorphism

**Critical Finding**: Clean Language does NOT implement runtime polymorphism/virtual method tables.

When calling `vehicle.getName()` where `vehicle` has type `Vehicle` but runtime type `Car`:
- **Expected (polymorphic)**: Call `Car.getName()` override
- **Actual (static)**: Call `Vehicle.getName()` parent method

This is by design - there's no vtable or dynamic dispatch implementation.

### 4. The Failing Test's Actual Issue

The polymorphism test file DOES compile, but fails WASM validation with:
```
type mismatch in call, expected [i32] but got []
```

**Debug output shows**:
```
DEBUG CALL START: function=Function(SymbolId(216)), arguments_len=1
DEBUG CALL ARGS: Loading 1 arguments for function Some("getInfo")
DEBUG CALL ARGS:   Arg[0]: Value(ValueId(0))
DEBUG CALL ARGS:   Arg[0] loaded successfully
DEBUG DIRECT LOOKUP: SymbolId(216) -> WASM index 91 (DIRECT)
```

**The debug shows arguments ARE being loaded**, but WASM validation says NO arguments are passed!

## The Real Issue

There's a discrepancy between:
1. **MIR/Codegen**: Shows 1 argument being loaded (the receiver)
2. **WASM Output**: Validation error says 0 arguments are passed

**Hypothesis**: The WASM call instruction generation might be dropping the arguments between the "load arguments" phase and the actual "call" instruction emission.

## Investigation Needed

### Check WASM Instruction Emission

**File to examine**: `src/codegen/mir_codegen.rs`
**Look for**: Where `Instruction::Call` is emitted

The code loads arguments onto the WASM stack, then should emit a `call` instruction. The validation error suggests that either:

1. **Arguments are being popped before the call** (incorrect DROP instructions)
2. **The call instruction is emitted before arguments are loaded** (instruction ordering issue)
3. **A different code path is being taken** that doesn't load arguments

### Specific Debug Points

1. Add debug output RIGHT BEFORE emitting `Instruction::Call`
2. Check if there are any DROP instructions between argument loading and call
3. Verify the instruction order in the generated WASM function

### Files to Examine

1. `src/codegen/mir_codegen.rs` - Call instruction generation (lines ~1400-1600)
2. Look for `wasm_function.instruction(&Instruction::Call(...))`
3. Check surrounding code for DROP instructions or stack manipulation

## Comparison: Working vs Failing

### Working Test (`test_method_override.cln`)
- **Methods**: 2 (Vehicle.getName, Car.getName)
- **Calls**: Simple, single method call
- **Result**: Validates ✅

### Failing Test (`16_classes_polymorphism_fixed.cln`)
- **Methods**: 10+ (multiple classes, multiple methods)
- **Calls**: Complex, multiple method calls in sequence
- **String concatenation**: Heavy use of `+` operator
- **Result**: Validation fails ❌

**Difference**: Complexity? Multiple string concatenations? Function that ends with expression statements?

## Next Steps for Debugging

1. **Add instrumentation** to see actual WASM instructions being emitted
2. **Compare WASM output** of working vs failing test with `wasm2wat`
3. **Check void function handling** - the `demonstratePolymorphism` function is void but might be leaving values on stack

### Quick Test

Create a simplified version of the failing test:
```clean
class Vehicle
    functions:
        string getInfo()
            return "info"

functions:
    void test(Vehicle v)
        print("A: " + v.getInfo())  # String concat + method call
        print("B: " + v.getInfo())  # Multiple calls

start()
    Vehicle v = Vehicle()
    test(v)
```

If this fails, the issue is the combination of:
- String concatenation
- Method calls on parameters
- Multiple statements in void function

## Error Analysis

### Error 1: `type mismatch in call, expected [i32] but got []`
- **Location**: offset 0x0da5
- **Meaning**: A function expecting 1 i32 parameter is being called with 0 arguments
- **Suspects**: One of the method calls (getInfo, start, stop, getMaxSpeed)

### Error 2: `type mismatch at end of function, expected [] but got [i32]`
- **Location**: offset 0x0da5 (same location!)
- **Meaning**: Function should have empty stack but has an i32 value
- **Cause**: The failed method call left its return value on the stack
- **Function**: `demonstratePolymorphism` is declared as `void`

### Root Cause Hypothesis

The two errors at the same location suggest:
1. A method call fails (missing argument)
2. That method call's return value is left on stack
3. The void function ends with a value on stack (invalid)

The method call that's failing is likely the LAST method call in the function, because:
- Previous method calls (print statements) seem to work
- The error is at the END of the function
- The stack has an i32 (the method's return value) that should have been consumed

### Likely Culprit

Looking at the test file:
```clean
void demonstratePolymorphism(Vehicle vehicle)
    print("=== Vehicle Demonstration ===")
    print("Info: " + vehicle.getInfo())        # Line 72
    print("Starting: " + vehicle.start())      # Line 73
    print("Max Speed: " + vehicle.getMaxSpeed().toString() + " km/h")  # Line 74
    print("Stopping: " + vehicle.stop())       # Line 75
```

The LAST line (75) is `print("Stopping: " + vehicle.stop())`.

**Hypothesis**: The `vehicle.stop()` call is being generated incorrectly, leaving the return value on stack instead of passing it to string concatenation.

## Recommended Fix Approach

### Option A: Fix Void Function Stack Cleanup

If the issue is that void functions aren't properly cleaning up the stack:

**Location**: `src/codegen/mir_codegen.rs` lines 490-518
**Current behavior**: Adds DROP for void functions that don't end with explicit Return
**Issue**: The detection of "explicit return" might be wrong

### Option B: Fix Method Call Argument Passing

If method calls aren't passing the receiver:

**Location**: Where `Instruction::Call` is emitted for method calls
**Fix**: Ensure arguments loaded onto stack are preserved until call

### Option C: Fix Expression Statement Handling

If expression statements in void functions are handled incorrectly:

**Location**: How expression statements are codegen'd in void contexts
**Fix**: Ensure return values are consumed or dropped appropriately

## Success Criteria

The fix is complete when:
1. `/tmp/test_base_minimal.cln` still validates ✅
2. `/tmp/test_method_param.cln` still validates ✅
3. `/tmp/test_method_override.cln` still validates ✅
4. `tests/cln/language/classes/16_classes_polymorphism_fixed.cln` validates ✅
5. `tests/cln/language/classes/16_classes_polymorphism_new.cln` validates ✅
6. No new validation errors introduced ✅

## Remaining Errors (After This Fix)

After fixing the polymorphism issue (2 files):
- **specification_compliance_test.cln** - static method parameter count mismatch
- **calculator_application.cln** - return type mismatch (f64 vs i32)

**Expected final success rate**: 100% WASM validation (233/233 files)
