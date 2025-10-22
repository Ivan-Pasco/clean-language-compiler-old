# Next Session Roadmap - Path to 100% WASM Validation

## Current Status
- **Validation Rate:** 94% (257/272 files)
- **Unit Tests:** ✅ 279/279 passing
- **Remaining Files:** 15 (6% of total)

---

## Quick Start Commands

### Check Current Status
```bash
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

# Validate all WASM files
./validate_all.sh

# List failing files
/tmp/find_invalid.sh

# Categorize error patterns
/tmp/categorize_errors.sh
```

### Test Specific File
```bash
# Compile
cargo run --release --bin clean-language-compiler compile -i tests/cln/path/to/file.cln -o /tmp/test.wasm

# Validate
wasm-validate /tmp/test.wasm

# Inspect WASM
wasm-objdump -d /tmp/test.wasm
```

---

## Priority Issues (Ranked by Impact)

### 🔴 PRIORITY 1: String List Methods (6 files)
**Impact:** Highest - affects 40% of remaining failures

**Files Affected:**
- 68_list_behaviors_comprehensive
- test_list_type
- test_exact_68_structure
- (3 more related to string operations)

**Problem:**
```clean
list<string> visitors = []
if visitors.contains("Alice")  // ❌ Leaves [i32, i32] on stack
    print("Found")
```

**Root Cause:**
- Strings are (ptr, len) tuples internally
- `list<integer>.contains()` works ✅
- `list<string>.contains()` leaves tuple on stack ❌

**Investigation Steps:**
1. Compare WASM output between:
   - `list<integer>.contains(42)` (working)
   - `list<string>.contains("test")` (broken)
2. Check `src/stdlib/list_ops.rs` for string handling
3. Trace MIR generation for method calls with string arguments
4. Look for tuple expansion/flattening issues

**Suspected Locations:**
- `src/mir/mir_builder.rs` - MethodCall handling
- `src/codegen/mir_codegen.rs` - String argument loading
- `src/stdlib/list_ops.rs` - List method implementations

---

### 🟡 PRIORITY 2: Error Handling (6 files)
**Impact:** Medium - 40% of remaining failures

**Files Affected:**
- 21_error_handling_try_catch
- 58_error_handling_onerror
- 65_error_handling_onerror_spec
- 71_error_handling_onerror_comprehensive
- test_complex_onerror
- test_onerror_simple

**Problem:**
```
Error: type mismatch in local.set, expected [f64] but got [i32]
Error: type mismatch in if, expected [i32] but got [f64]
Error: type mismatch in return, expected [f64] but got [i32]
```

**Root Cause:**
- Error handling constructs (try/catch, onError) have inconsistent type handling
- Error propagation may be generating wrong types
- Control flow in error paths needs special handling

**Investigation Steps:**
1. Review error handling AST → MIR lowering
2. Check if error values have proper types
3. Ensure error blocks match function return types

**Suspected Locations:**
- `src/mir/mir_builder.rs` - Error statement handling
- `src/typechecker/type_inference.rs` - Error type propagation

---

### 🟢 PRIORITY 3: Edge Cases (3 files)
**Impact:** Low - 20% of remaining failures

**Files Affected:**
- test_equality_only (Note: may have regression from Drop changes)
- test_function_return_only
- test_implicit

**Problem:**
- Various implicit return issues
- Expression statement type mismatches
- Possibly related to Drop instruction changes

**Investigation Steps:**
1. Check if files compile (some may have type errors now)
2. Review Drop instruction changes for side effects
3. May need to revert Drop changes and use different approach

---

## Recommended Approach

### Session Plan

**Phase 1: Quick Wins (30 min)**
1. Run validation to confirm current state
2. Check if test_equality_only regression is real
3. If Drop changes caused issues, consider reverting

**Phase 2: String List Methods (2 hours)**
1. Create minimal test case for string list issue
2. Compare working vs broken WASM disassembly
3. Fix string tuple handling in method calls
4. **Expected result:** +6 files (94% → 96%)

**Phase 3: Error Handling (1.5 hours)**
1. Analyze error handling type mismatches
2. Fix error value type propagation
3. Ensure error blocks properly typed
4. **Expected result:** +6 files (96% → 98%)

**Phase 4: Edge Cases (1 hour)**
1. Address remaining 3 files individually
2. Fix any Drop-related regressions
3. **Expected result:** +3 files (98% → 100%)

**🎯 Goal:** Achieve 100% WASM validation (272/272 files)

---

## Known Working Patterns

### ✅ Power Operations
```rust
// In codegen: Skip auto-conversion for integer power
let needs_conversion = name != "math.pow_i32";
```

### ✅ If/Else-If Returns
```rust
// Recursive check for all branches returning
fn block_always_returns(block: &MirBlock) -> bool {
    match &block.terminator {
        MirTerminator::Return { .. } => true,
        MirTerminator::Branch { if_true, if_false, .. } => {
            block_always_returns(if_true) && block_always_returns(if_false)
        }
        _ => false,
    }
}
```

### ✅ Type Conversion Methods
```rust
// Special handling in MIR builder for .toInteger(), .toNumber(), etc.
match method_name.as_str() {
    "toNumber" if matches!(object_type, ConcreteType::Integer) => {
        MirOperand::Function(SymbolId(10001))
    }
    "toInteger" if matches!(object_type, ConcreteType::Number) => {
        MirOperand::Function(SymbolId(10002))
    }
    // ...
}
```

---

## Debug Helpers

### Trace WASM Generation
```bash
# Compile with verbose output
RUST_LOG=debug cargo run --release --bin clean-language-compiler compile -i file.cln -o output.wasm 2>&1 | less

# Filter for specific operations
RUST_LOG=debug cargo run --release --bin clean-language-compiler compile -i file.cln -o output.wasm 2>&1 | grep "Call\|BinaryOp"
```

### Compare WASM Output
```bash
# Disassemble both versions
wasm-objdump -d tests/output/working.wasm > /tmp/working.wat
wasm-objdump -d tests/output/broken.wasm > /tmp/broken.wat
diff /tmp/working.wat /tmp/broken.wat
```

### Quick Type Check
```bash
# Just run type checking without codegen
cargo run --release --bin clean-language-compiler compile -i file.cln -o /dev/null 2>&1 | grep -i "type error"
```

---

## Important Notes

### Drop Instruction Changes
- Located in `src/codegen/mir_codegen.rs`
- Added Drop for unused values in: Copy, BinaryOp, UnaryOp, Load, Call, GetElementPtr, AsyncAssign
- **Status:** No improvement, possible regression
- **Action:** May need to review or revert if causing type errors

### File Locations
- Test files: `tests/cln/` (organized by category)
- Output files: `tests/output/`
- Helper scripts: `/tmp/find_invalid.sh`, `/tmp/categorize_errors.sh`

### SymbolId Reference
- 0: print
- 5: int_to_string
- 6: float_to_string
- 1000: string_concat
- 1001: math.pow_f64
- 1002: math.pow_i32
- 10001-10006: Type conversion methods

---

## Success Criteria

### Minimum Goal (Acceptable)
- **96% validation** (261/272 files)
- String list methods fixed
- No new regressions

### Target Goal (Expected)
- **98% validation** (267/272 files)
- String list + error handling fixed
- Clean solution for stack management

### Stretch Goal (Ideal)
- **100% validation** (272/272 files)
- All edge cases resolved
- Production-ready compiler

---

## References

- **Session Report:** `system-documents/session_2025-10-21_wasm_validation_improvements.md`
- **Language Spec:** `documentation/Clean_Language_Specification.md`
- **Previous Sessions:** `system-documents/session_2025-10-*.md`

---

**Last Updated:** October 21, 2025
**Next Session Start:** Focus on string list method stack issues
**Validation Status:** 257/272 (94%)
