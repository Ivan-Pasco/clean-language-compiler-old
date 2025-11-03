# Session 2025-10-26: Function Index Bug Investigation

## Date
2025-10-26 (Continuation of session from 2025-10-26_CURRENT_STATUS.md)

## Issue Summary

**Problem**: `_start` wrapper function is calling function index 43, but only indices 0-42 exist.

**WASM Structure**:
- 11 imports (indices 0-10)
- 32 user functions (indices 11-42)
- Total: 43 functions with valid indices 0-42
- Error: `_start` (func[42]) calls function 43 (invalid)

## Root Cause Analysis

### What We Know

1. **Pre-registration loop** (mir_codegen.rs line 203-214):
   ```rust
   for (i, (_symbol_id, function)) in mir_program.functions.iter().enumerate() {
       let function_index = self.wasm_generator.function_count + i as u32;
       // Assigns indices: function_count + 0, function_count + 1, ... function_count + N-1
   }
   ```

2. **Expected behavior**:
   - If `function_count = 11` (number of imports)
   - And `mir_program.functions.len() = 32`
   - Then indices should be: 11, 12, 13, ... 42 ✅

3. **Actual behavior**:
   - The "start" function gets assigned index 43
   - Meaning either:
     - `function_count != 11`, OR
     - `mir_program.functions.len() = 33` (too many functions)

### Investigation Attempts

1. ✅ **Fixed build errors** - `CompilerError::codegen_error()` arguments corrected
2. ✅ **Compiler builds successfully**
3. ❌ **Debug logging not working** - `tracing::debug!()` may be compiled out in release mode

### Files Modified

1. `src/codegen/mir_codegen.rs`:
   - Lines 1693-1702: Fixed `CompilerError::codegen_error()` call
   - Lines 1727-1731: Removed reference to `old_value`
   - Lines 198-219: Added debug logging (but doesn't output in release)

## Next Steps

### Option 1: Use Info Logging (Quick)
Change `tracing::debug!()` to `tracing::info!()` in the pre-registration code to see values in release mode.

**Advantage**: Fast, no recompilation needed
**Disadvantage**: Info logging is verbose

### Option 2: Debug Build (Slower)
Compile in debug mode to enable debug logging.

```bash
cargo build
RUST_LOG=debug ./target/debug/clean-language-compiler compile -i tests/cln/language/classes/07_class_definitions.cln -o /tmp/test_class.wasm
```

**Advantage**: Preserves debug logging
**Disadvantage**: Slower build and execution

### Option 3: Direct Fix Attempt (Risky)
Based on the pattern, the issue is likely that `function_count` is 12 instead of 11, causing:
- Indices: 12, 13, 14, ... 43 (33 functions)
- Or there's an extra function in the MIR

**Hypothesis**: The `_start` wrapper itself might be getting added to the MIR functions list, creating a circular reference.

**Fix**: Check if `_start` is being added to `mir_program.functions` when it shouldn't be.

### Option 4: Count Functions Directly
Add a simple print statement with `eprintln!()` which always outputs:

```rust
eprintln!("DEBUG: function_count={}, mir_functions={}",
    self.wasm_generator.function_count,
    mir_program.functions.len());
```

**Advantage**: Always works, even in release mode
**Disadvantage**: Uses stderr instead of proper logging

## Recommended Approach

**Use Option 4** - Add `eprintln!()` statements to see the exact values, then determine the fix.

### Implementation

1. Add `eprintln!()` before pre-registration loop
2. Add `eprintln!()` for each function showing its assigned index
3. Rebuild in release mode
4. Run test file and capture stderr
5. Analyze the output to find the discrepancy

## Files to Check

1. `src/codegen/mir_codegen.rs:195-220` - Pre-registration loop
2. `src/codegen/mir_codegen.rs:1766-1830` - `_start` wrapper generation
3. `src/mir/mir_builder.rs` - Check if `_start` is being added to MIR

## Success Criteria

- Identify exact value of `function_count` during pre-registration
- Identify exact number of functions in `mir_program.functions`
- Find which function is getting index 43
- Fix the index calculation or remove the extra function
- Verify WASM validates successfully
