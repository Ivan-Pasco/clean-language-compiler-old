# ValueId(6) Missing Local Registration Fix

**Session Date**: 2025-10-27
**Issue**: Compiler error "ValueId(6) not found in local variable map during store_to_local"
**Status**: ✅ RESOLVED

## Problem Summary

When compiling `tests/cln/language/classes/14_classes_basic.cln`, the compiler threw an error indicating that ValueId(6) was being referenced in WASM codegen but had never been registered in the function's locals map during MIR building.

### Error Details
```
ERROR generating function 'start': ValueId(6) not found in local variable map during store_to_local.
This indicates the MIR builder did not properly allocate this value in function.locals.
```

### Debug Output Analysis
- Function 'start' had 12 locals registered: ValueIds 0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12
- **ValueId(6) was MISSING** from the locals map
- ValueId(6) was being used in MIR instructions but never added to `function.locals`

## Root Cause

The bug was located in `src/mir/mir_builder.rs` in the **Print statement handler** for type conversion functions.

### The Problematic Pattern

In the Integer, Number, and Boolean conversion cases, the code was:
```rust
let conversion_instruction = MirInstruction {
    dest: Some(ValueId(context.function.next_value_id)),  // ❌ Using next_value_id directly
    operation: MirOperation::Call {
        function: MirOperand::Function(SymbolId(5)),
        arguments: vec![MirOperand::Value(value_id)],
    },
    location: location.clone(),
};
let converted_id = ValueId(context.function.next_value_id);  // ❌ Same value!
context.function.next_value_id += 1;
self.add_instruction(context, conversion_instruction);
```

**The bug**: The instruction was created with `ValueId(next_value_id)` as destination, but the ValueId was never registered in `function.locals` before the instruction was added.

## The Fix

The fix ensures ValueIds are:
1. **Allocated** first
2. **Registered** in `function.locals` via `register_temp_local()`
3. **Then used** in instructions

### Fixed Pattern (Applied to Integer, Number, Boolean conversions)

```rust
ConcreteType::Integer => {
    // Convert integer to string using int_to_string (SymbolId(5))
    let converted_id = ValueId(context.function.next_value_id);
    context.function.next_value_id += 1;

    // ✅ Register the converted_id BEFORE creating the instruction
    self.register_temp_local(
        context,
        converted_id,
        MirType::StringTuple,
        location.clone(),
    );

    let conversion_instruction = MirInstruction {
        dest: Some(converted_id),  // ✅ Now using the allocated and registered ID
        operation: MirOperation::Call {
            function: MirOperand::Function(SymbolId(5)),
            arguments: vec![MirOperand::Value(value_id)],
        },
        location: location.clone(),
    };
    self.add_instruction(context, conversion_instruction);
    converted_id
}
```

## Files Modified

### `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/mir/mir_builder.rs`

**Changes**:
1. **Lines 582-605**: Fixed Integer → String conversion to register ValueId before use
2. **Lines 607-630**: Fixed Number → String conversion to register ValueId before use
3. **Lines 632-655**: Fixed Boolean → String conversion to register ValueId before use

**Key Principle**: All ValueIds that will be used as instruction destinations MUST be registered in `function.locals` before the instruction is created. This maintains the MIR SSA (Static Single Assignment) invariant.

## Verification

### Test Case
- **File**: `tests/cln/language/classes/14_classes_basic.cln`
- **Test Content**: Person class with constructor, methods (getName, getAge, setAge, toString), and start function using print statements

### Results
1. ✅ **Compilation**: Successfully compiled to `tests/output/14_classes_basic.wasm`
2. ✅ **WASM Validation**: `wasm-validate` passes without errors
3. ✅ **All Functions Generated**: start, constructor, getName, getAge, setAge, toString

### Debug Output Confirmation
```
DEBUG MIR FUNC: Local ValueIds: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
```
✅ ValueId(6) is now properly included in the locals map.

## Technical Analysis

### Why This Matters

In MIR SSA form, every value must have a unique ValueId and a corresponding entry in `function.locals`. The codegen phase relies on this mapping to:
1. Allocate WASM local indices for each ValueId
2. Generate correct store/load instructions
3. Maintain type safety during code generation

When a ValueId is used without being registered, codegen fails because it cannot map the ValueId to a WASM local index.

### The MIR Invariant

**Invariant**: For every ValueId used in a MirInstruction.dest or MirOperand::Value, there MUST be a corresponding entry in `MirFunction.locals`.

This fix restores that invariant for type conversion calls in Print statements.

## Lessons Learned

1. **Always allocate → register → use**: Never create instructions with unregistered ValueIds
2. **Consistent patterns**: All expression handlers should follow the same registration pattern
3. **Early registration**: Register locals immediately after allocating the ValueId
4. **Type safety**: Register with the correct MirType (e.g., MirType::StringTuple for string conversions)

## Related Code Patterns

This fix follows the same pattern already used correctly in other parts of mir_builder.rs:
- FunctionCall expressions (lines 1479-1484)
- MethodCall expressions (lines 1729-1736)
- BinaryOperation expressions (lines 1316-1321)
- Literal expressions (lines 1105-1106)

## Impact

This fix resolves compilation errors for any Clean Language program that:
- Uses `print()` or `printl()` statements
- Prints integer, number, or boolean values (requires automatic toString conversion)
- Particularly affects class-based programs with print debugging

## Next Steps

✅ **Fix Complete**: The ValueId(6) error is resolved
✅ **Pattern Verified**: Same fix pattern can be applied to any similar issues
✅ **Testing**: Compilation and WASM validation both pass

### Recommendation
Search for any other instances in mir_builder.rs where ValueIds are allocated but might not be registered immediately. The grep pattern to check:
```bash
grep -n "ValueId(context.function.next_value_id)" src/mir/mir_builder.rs
```

All such allocations should be followed by `register_temp_local()` or insertion into `function.locals`.
