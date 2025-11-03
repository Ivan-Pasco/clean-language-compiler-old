# Session 2025-10-27: Void Method Debugging

## Current Status

Still working on fixing the MIR builder bug where void function calls create ValueIds that aren't properly registered in locals, causing WASM validation errors.

## Problem

- Test file: `tests/cln/language/classes/14_classes_basic.cln`
- Error: `ValueId(6) not found in local variable map during store_to_local`
- WASM validation error: `type mismatch in call, expected [i32, i32] but got []`

## Fix Attempted

Modified `src/mir/mir_builder.rs` at lines 1457-1498 (FunctionCall case):
- Always allocate a ValueId for SSA consistency
- Check if `expression.expr_type` matches `ConcreteType::Null`
- If void: set `dest = None`, don't register in locals
- If non-void: set `dest = Some(result_id)`, register in locals

## Problem with Fix

The fix didn't work. The error persists after rebuild and test.

## Investigation Needed

1. **Verify type inference** - Is `expression.expr_type` actually `ConcreteType::Null` for void methods?
2. **Check all call sites** - The test file has multiple function calls. Are they all being handled correctly?
3. **Examine MIR output** - Need to see actual MIR instructions generated to verify `dest` values
4. **Check constructor calls** - `Person("Alice", 25)` might also be going through FunctionCall

## Next Steps

1. Add more targeted debug logging to verify:
   - What is `expression.expr_type` for the `setAge(26)` call?
   - What is `dest_opt` being set to?
   - Are there other calls in the file generating ValueId(6)?

2. Consider whether the issue is in:
   - Type inference (not setting void type correctly)
   - MIR builder (fix not being applied)
   - Code generation (mishandling `dest = None` case)

3. Create minimal test case with just a void method call to isolate the issue

## Hypothesis

The fix might not be working because:
- Type inference isn't setting `ConcreteType::Null` for void methods
- There are multiple code paths (FunctionCall, MethodCall, Constructor) that need fixing
- The actual problem ValueId(6) is coming from a different operation entirely

## Files Modified

- `src/mir/mir_builder.rs:1457-1498` - Added void checking logic to FunctionCall handler
- `src/mir/mir_builder.rs:1488-1495` - Added debug logging (not producing output)
