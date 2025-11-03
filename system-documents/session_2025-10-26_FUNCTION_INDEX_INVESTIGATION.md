# Session 2025-10-26: Function Index Out of Range Investigation

## Problem Statement

129 WASM files fail validation with errors like:
```
function variable out of range: 44 (max 42)
function variable out of range: 45 (max 42)
```

This means code is trying to call function indices that don't exist in the WASM module.

## Investigation Performed

### 1. Initial Hypothesis: Off-by-One in InstructionGenerator

**Found:** `InstructionGenerator::register_function` was calculating function indices incorrectly.

**Location:** `src/codegen/instruction_generator.rs:1477`

**Problem:**
```rust
let index = self.function_signatures.len() as u32;
```

This calculated index based on signature map size, NOT accounting for imported functions.

**Fix Applied:**
- Modified `register_function` to accept `function_index` as a parameter
- CodeGenerator now passes its `function_count` value to InstructionGenerator
- This ensures both use the same index

**Files Modified:**
- `src/codegen/instruction_generator.rs` - Added `function_index` parameter
- `src/codegen/mod.rs` - Updated all calls to pass `function_index`

### 2. Testing the Fix

**Result:** The fix compiled successfully, but WASM validation still fails!

**Test File:** `tests/cln/core/types/34_list_behaviors.cln`

**WASM Analysis:**
- Total functions: 42 (indices 0-41)
- Imports: 11 (func[0] - func[10])
- User functions: 31 (func[11] - func[41])
- Invalid calls: func[42], func[43], func[44]

**Debug Output:** No functions with index >= 42 were registered or looked up through InstructionGenerator!

This means the problematic indices are coming from a different code path.

### 3. Discovered: get_or_create_function_index Issue

**Location:** `src/codegen/mod.rs:5224`

**Problem:** This function creates placeholder function indices without actually adding functions to the WASM module:

```rust
pub fn get_or_create_function_index(&mut self, name: &str) -> u32 {
    if let Some(index) = self.function_map.get(name) {
        *index
    } else {
        let index = self.function_count;  // Uses current count
        self.function_count += 1;          // Increments count
        self.function_map.insert(name.to_string(), index);
        // But never adds to function_section or code_section!
        ...
    }
}
```

**Impact:** If called for async runtime functions, it increments `function_count` without registering actual functions.

**Test Result:** Debug output showed this is NOT being called for the failing test file.

### 4. Critical Finding: Multiple CodeGenerator Instances

From previous investigation (`session_2025-10-25_investigation_complete.md`):

**Problem:** Test framework uses OLD CodeGenerator path:
1. MirCodeGenerator wraps a CodeGenerator instance
2. User functions compiled via MIR path
3. Test blocks processed separately through OLD CodeGenerator
4. Creates index mismatch between two CodeGenerator instances

**However:** `34_list_behaviors.cln` doesn't have test blocks - it uses `start()` function.

### 5. Mystery: Where Are Invalid Indices Coming From?

**Evidence:**
- No functions >= 42 registered (debug confirmed)
- No functions >= 42 looked up via InstructionGenerator (debug confirmed)
- get_or_create_function_index not called (debug confirmed)
- But WASM has Call instructions to indices 42, 43, 44

**Possible Sources:**
1. Direct use of CodeGenerator.function_map with wrong indices
2. Cached/stale data somewhere
3. Function indices calculated via arithmetic
4. Different code path not going through InstructionGenerator

## Current Status

### Fixed
- ✅ InstructionGenerator now uses correct indices from CodeGenerator
- ✅ Compiler builds successfully

### Not Fixed
- ❌ 129 WASM files still fail validation
- ❌ Invalid function indices still being generated
- ❌ Root cause not yet identified

## Next Steps Required

### Immediate Investigation Needed

1. **Add comprehensive debug logging:**
   - Log ALL Instruction::Call creations with source location
   - Log ALL function_map inserts and retrievals
   - Track function_count changes

2. **Search for alternative Call generation paths:**
   - Check if MIR codegen has its own function index tracking
   - Look for direct Instruction::Call() creations
   - Check for function index calculations

3. **Verify MirCodeGenerator integration:**
   - Check if MirCodeGenerator.wasm_generator shares function_count with main CodeGenerator
   - Verify all function registrations go through same CodeGenerator instance

4. **Check for caching issues:**
   - Verify InstructionGenerator is fresh for each compilation
   - Check if function_signatures map is being reused

### Testing Strategy

1. Create minimal test case with single method call
2. Add extensive debug logging at every function index usage point
3. Trace complete execution path for one failing case
4. Identify exact location where wrong index is introduced

### Potential Root Causes to Investigate

**Hypothesis 1: MIR has separate function index system**
- Check MirCodeGenerator function handling
- Verify it uses CodeGenerator.function_map

**Hypothesis 2: Method call resolution**
- Methods like `.size()`, `.toString()` may use different lookup
- Check type-qualified function name resolution

**Hypothesis 3: stdlib registration order**
- stdlib functions might be registered after user code
- Indices assigned don't match actual function positions

**Hypothesis 4: Export/Import mismatch**
- Function_count might not account for imports correctly
- Check import registration in CodeGenerator initialization

## Code Changes Made This Session

### instruction_generator.rs
```rust
// Added function_index parameter
pub(crate) fn register_function(
    &mut self,
    name: &str,
    params: &[WasmType],
    return_type: Option<WasmType>,
    _instructions: &[Instruction],
    function_index: u32,  // NEW PARAMETER
) -> Result<u32, CompilerError>
```

### mod.rs
```rust
// Updated all calls to pass function_index
self.instruction_generator
    .register_function(name, params, return_type, instructions, function_index)?;
```

### Debug Output Added
- `InstructionGenerator::register_function` - logs functions >= 42
- `InstructionGenerator::get_function_index_by_signature` - logs lookups >= 42
- `CodeGenerator::get_or_create_function_index` - logs all creations

## Files Affected

- `src/codegen/instruction_generator.rs` - Modified function signature
- `src/codegen/mod.rs` - Updated all register_function calls (3 locations)
- `src/codegen/mod.rs` - Fixed register_basic_array_get_fallback to use CodeGenerator.register_function

## Build Status

- ✅ Compiler compiles successfully
- ✅ No compilation errors
- ❌ WASM validation still fails

## Recommendation

This requires deeper investigation with comprehensive logging. The invalid indices are being generated through a code path we haven't identified yet. A systematic approach with detailed tracing is needed to locate the source.
