# Session 2025-10-26: Function Index Error Investigation - Final Status

## Date
2025-10-26

## Session Goal
Fix function index out of range errors affecting 129 WASM files (217/301 WASM validation success rate).

## What Was Accomplished

### 1. Root Cause Identified
The compiler-debugger agent identified that:
- **Problem**: MIR codegen path was NOT registering stdlib functions that the AST path registers
- **Impact**: Call instructions referenced non-existent function indices
- **Files Affected**: 129 WASM files using stdlib operations (lists, strings, math, etc.)

### 2. Fixes Implemented

#### A. Made stdlib registration idempotent (`src/codegen/mod.rs`)
```rust
pub fn register_import_function(...) -> Result<u32, CompilerError> {
    // Check if function is already registered to prevent duplicates
    if let Some(&existing_index) = self.function_map.get(field) {
        tracing::debug!("Function already registered, returning existing index");
        return Ok(existing_index);
    }
    // ... original registration logic
}
```

#### B. Added stdlib registration to MIR path (`src/codegen/mir_codegen.rs`)
```rust
if self.wasm_generator.include_runtime_imports {
    debug_mir!("DEBUG MIR: Registering ALL stdlib functions (comprehensive)");
    self.wasm_generator
        .register_stdlib_functions()
        .map_err(|e| vec![e])?;
    debug_mir!("DEBUG MIR: ALL stdlib functions registered");
}
```

#### C. Fixed Result handling in `src/codegen/builtin_generator.rs`
```rust
// Lines 353-354 and 358-362
for (name, params, returns) in &file_functions {
    let index = self.register_import_function("env", name, params.clone(), returns.clone())?;  // Added ?
    self.file_import_indices.insert(name.to_string(), index);
}

// File namespace operations
self.register_import_function("env", "file.read", vec![ValType::I32], vec![ValType::I32])?;  // Added ?
self.register_import_function("env", "file.write", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
// ... etc (all have ? now)
```

#### D. Made InstructionGenerator idempotent (`src/codegen/instruction_generator.rs`)
```rust
if let Some(existing_index) = self.function_signatures.get(&signature) {
    // Return existing index instead of erroring
    tracing::debug!("Function signature already registered, using existing index");
    return Ok(*existing_index);
}
```

### 3. Additional Attempted Fixes (From Earlier in Session)
- Pre-registration of MIR functions with base_function_index
- Sorting functions by pre-registered index before generation
- Changed add_function_to_module to use function_count

## Current Status

### Build Status
✅ **Compiler builds successfully** (0.50s incremental build)

### Test Results
❌ **Compilation still failing** with:
```
Error: File import function 'file_read' not found
```

### WASM Validation
❌ **Still has errors** (from stale WASM file):
- function variable out of range: 88 (max 88)
- type mismatch in call
- type mismatch in implicit return

## Remaining Issues

### Issue 1: file_import_indices Lookup Failure
The error "File import function 'file_read' not found" suggests that even though we're now registering file operations with `?`, something is looking them up in `file_import_indices` and not finding them.

**Hypothesis**: The code path that looks up file_import_indices is separate from the registration path. Need to investigate where file_import_indices is being read.

**Files to Check**:
- `src/codegen/mod.rs` - Where file_import_indices is used for lookup
- `src/codegen/builtin_generator.rs` - Where it's populated

### Issue 2: Test File May Not Need File Operations
`34_list_behaviors.cln` is about lists, not file operations. The error suggests the compiler is trying to register/lookup file operations even when not needed.

## Next Steps

### Immediate Action (Current Session Continuation)
1. **Search for where file_import_indices is used for lookup**:
   ```bash
   grep -n "file_import_indices.get" src/codegen/*.rs
   ```

2. **Check if file operations are actually needed**:
   - Does `34_list_behaviors.cln` import any file operations?
   - Is the compiler over-eagerly registering functions?

3. **Test with a simpler file**:
   - Try compiling a file that only uses basic operations
   - See if the file_read error still occurs

### Short-term Fix
1. Make file operations registration optional/conditional
2. Only register file operations if actually used in the program
3. Or ensure file operations are always registered in all paths

### Long-term Solution
**Architectural Refactoring** (as recommended by agent):
1. Create single `register_all_runtime_functions()` method
2. Register ALL functions in one place in correct order
3. Remove individual `register_X_operations()` methods
4. Implement `StdlibRegistry` to track registered functions

## Files Modified This Session

1. `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/codegen/mir_codegen.rs`
   - Added stdlib registration call (lines 151-161)
   - Added function pre-registration (lines 195-214)
   - Added function sorting before generation (lines 216-225)
   - Changed add_function_to_module to use function_count (line 1691)

2. `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/codegen/mod.rs`
   - Made register_import_function idempotent (lines 408-449)

3. `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/codegen/instruction_generator.rs`
   - Made add_function_type idempotent (lines 1485-1496)
   - Added function_index parameter to register_function (earlier in session, by agent)

4. `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/src/codegen/builtin_generator.rs`
   - Fixed Result handling with ? operator (lines 353-354, 358-362)

## Test Commands

### Test Single File
```bash
cargo build --release
./target/release/clean-language-compiler compile \
  -i tests/cln/core/types/34_list_behaviors.cln \
  -o tests/output/34_list_behaviors.wasm
wasm-validate tests/output/34_list_behaviors.wasm
```

### Check WASM Validation Status
```bash
python3 -c "
import subprocess, os
valid, total = 0, 0
for file in os.listdir('tests/output'):
    if file.endswith('.wasm'):
        total += 1
        result = subprocess.run(['wasm-validate', f'tests/output/{file}'], capture_output=True)
        if result.returncode == 0: valid += 1
print(f'WASM Valid: {valid}/{total} ({valid*100//total if total > 0 else 0}%)')
"
```

## Documentation Created This Session
- `session_2025-10-26_FUNCTION_INDEX_INVESTIGATION.md` - Initial analysis
- `session_2025-10-25_function_index_fix.md` - Agent's initial analysis
- `session_2025-10-25_FINAL_ANALYSIS.md` - Agent's detailed recommendations
- `session_2025-10-25_COMPREHENSIVE_SUMMARY.md` - Agent's complete summary
- `session_2025-10-26_FINAL_STATUS.md` - This document

## Key Insights
1. MIR and AST paths must register identical sets of functions
2. Registration must be idempotent to handle multiple calls
3. Result handling is critical - missing `?` causes silent failures
4. Stdlib registration architecture needs comprehensive refactoring
5. file_import_indices appears to be used in a separate code path than expected

## Recommendation
The issue is very close to being resolved. The core fix (idempotent registration + stdlib registration in MIR path) is correct. The remaining "file_read not found" error suggests a lookup path issue that should be quick to identify and fix.

**Estimated time to complete**: 30-60 minutes
**Risk level**: LOW - Core fix is sound, just need to find the lookup issue

## Success Criteria
- ✅ Compiler builds successfully
- ❌ Test file compiles without errors  ← Current blocker
- ❌ WASM validates successfully
- ❌ All 129 previously failing files now validate
- ❌ WASM validation improves from 217/301 (72%) to >240/301 (80%+)
