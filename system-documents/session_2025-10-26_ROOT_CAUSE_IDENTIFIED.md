# Session 2025-10-26: Root Cause Identified

## Date
2025-10-26 (Continuation - Root Cause Found)

## Investigation Summary

### Initial Problem
- Function index out of range: 43 (max 43)
- WASM validation failing on 19 class-related test files

### Debug Process

1. **Added eprintln!() debugging** to see exact function counts and indices
2. **Discovered**:
   - `function_count = 40` (11 imports + 29 stdlib functions)
   - `mir_program.functions.len() = 4` (start, getAge, getName, constructor)
   - Pre-registered at indices: 40, 41, 42, 43

3. **WASM Analysis**:
   - Only 3 functions exist in WASM: func[40], func[41], func[42]
   - func[42] is the `_start` wrapper
   - func[43] doesn't exist (causing "out of range" error)

4. **Generation Debug Output**:
   ```
   DEBUG: Generating function 'getAge'
   DEBUG: ERROR generating function 'getAge': ValueId(1) not found in local variable map
   DEBUG: Generating function 'constructor'
   DEBUG: Successfully generated function 'constructor'
   DEBUG: Generating function 'start'
   DEBUG: Successfully generated function 'start'
   DEBUG: Generating function 'getName'
   DEBUG: ERROR generating function 'getName': ValueId(1) not found in local variable map
   ```

## ROOT CAUSE IDENTIFIED

### The Real Problem

**Two class methods (`getAge` and `getName`) are failing to generate due to MIR builder errors.**

**Error Message**:
```
ValueId(1) not found in local variable map during load_operand.
This indicates the MIR builder did not properly track this value.
```

### Why This Causes Function Index Errors

1. **Pre-registration assigns indices**:
   - start → 40
   - getAge → 41
   - getName → 42
   - constructor → 43

2. **Generation fails for getAge and getName**:
   - Errors are caught and added to warnings
   - No WASM function generated for these
   - But function_map still has their indices!

3. **Actual WASM structure**:
   - func[40]: constructor (first successful generation)
   - func[41]: start (second successful generation)
   - func[42]: _start wrapper
   - func[43]: DOESN'T EXIST

4. **function_map is stale**:
   - Still says constructor → 43
   - But constructor is actually at func[40] or func[41]
   - When start() calls constructor, it uses index 43 from function_map
   - Index 43 doesn't exist → WASM validation error

### The Underlying Bug

**Class methods that access instance fields (`name`, `age`) are failing during MIR→WASM generation.**

The methods are defined as:
```clean
string getName()
    return name

integer getAge()
    return age
```

The MIR builder is creating these methods but **not properly tracking the instance field references** (`name` and `age` become ValueId(1) which isn't in the local variable map).

## Impact

This affects **all class-based tests** where methods access instance fields, which explains why we have:
- 19 files with "Function index out of range" errors
- Most are class-related tests

## Fix Strategy

### Option 1: Fix MIR Builder (Proper Fix)
**Locate and fix the bug where instance field access in methods isn't creating proper local variable mappings.**

**Files to investigate**:
- `src/mir/mir_builder.rs` - Method generation for class methods
- Look for how instance fields (`this.name`, `this.age`) are handled
- Ensure they're added to the local variable map with correct ValueIds

### Option 2: Workaround (Temporary)
**Update function_map after generation to reflect actual indices.**

This would mask the symptom but not fix the root cause. Not recommended.

### Option 3: Defensive Fix
**Fail compilation if any function fails to generate, rather than continuing with warnings.**

This would make the error more visible but wouldn't fix the underlying issue.

## Recommended Next Steps

1. **Fix the MIR builder bug** where instance field access creates unmapped ValueIds
2. **Test with the class definition file** to verify the fix
3. **Recompile all test files** to measure impact
4. **Expected result**: 19 files should go from invalid → valid (73% → 79% validation rate)

## Files to Modify

### Primary Fix
- `src/mir/mir_builder.rs` - Fix instance field access in method generation

### Verification
- `tests/cln/language/classes/07_class_definitions.cln` - Test file to verify

## Success Criteria

1. getAge and getName methods generate successfully
2. No "ValueId not found" errors
3. All 4 functions (start, getAge, getName, constructor) generate into WASM
4. WASM validates successfully
5. 19 class-related test files pass validation

## Technical Details

### Current Flow (BROKEN)
1. Pre-register: start→40, getAge→41, getName→42, constructor→43
2. Generate getAge: FAILS (ValueId(1) not found)
3. Generate constructor: SUCCESS (but function_map still says index 43)
4. Generate start: SUCCESS
5. Generate getName: FAILS (ValueId(1) not found)
6. Result: Only 2 functions in WASM, but function_map has stale indices

### Expected Flow (FIXED)
1. Pre-register: start→40, getAge→41, getName→42, constructor→43
2. Generate ALL 4 functions successfully
3. All functions appear in WASM at correct indices
4. function_map indices match actual WASM indices
5. Function calls use correct indices
6. WASM validation passes
