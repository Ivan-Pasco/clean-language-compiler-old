# KNOWLEDGE.md — Clean Language Compiler

Known fragile areas discovered across sessions. Read before modifying any compiler code.

---

## 1. String Heap Pointer Initialization Order

**What:** The heap pointer (`__heap_base`) must be set AFTER all string constants are transferred into WASM linear memory, aligned to 8 bytes. If initialization happens too early, string constants get overwritten by heap allocations.

**Where:** `src/codegen/mir_codegen.rs` (~line 5893), `src/codegen/mod.rs` (~line 7199)

**Watch for:** Any change to string pool transfer order, memory layout initialization, or bump allocator setup.

---

## 2. String Comparison Inversion

**What:** String equality comparison uses `i32.eqz` to invert the result of a byte-by-byte compare loop: `Equal` needs `i32.eqz`, `NotEqual` does NOT. Getting this backward silently produces wrong results without any runtime error.

**Where:** `src/codegen/mir_codegen.rs` (~line 4548)

**Watch for:** Any refactoring of comparison operators, string equality logic, or conditional branching on string values.

---

## 3. Codegen Architecture (CLEANED UP 2026-04-12)

**What:** `MirCodeGenerator` (mir_codegen.rs) is the sole active codegen path. It wraps `CodeGenerator` (mod.rs) as `wasm_generator` field, using its infrastructure (type_manager, memory_utils, function_map, register_* methods) but NOT its direct AST-to-WASM generation methods.

**Deleted:** expression_generator.rs (1,846 lines), statement_generator.rs (843 lines), binary_operations.rs (619 lines), type_manager_tests.rs (96 lines), and 2 dead tests from tests.rs. Total: ~3,400 lines removed.

**Still present but infrastructure-only:** `CodeGenerator` struct in mod.rs provides field storage and registration methods used by `MirCodeGenerator`. instruction_generator.rs provides `LocalVarInfo` and `InstructionGenerator` types. type_manager.rs, type_conversion.rs, wasm_module_builder.rs, binaryen_optimizer.rs are fields of `CodeGenerator`.

**Where:** `src/codegen/mir_codegen.rs` (active), `src/codegen/mod.rs` (infrastructure)

**Watch for:** All new codegen work goes in `mir_codegen.rs`. The CodeGenerator struct methods in mod.rs should only be modified when changing infrastructure used by MirCodeGenerator.

---

## 4. Recursive Function Pre-registration

**What:** Functions must be registered in the function map BEFORE their bodies are generated, so that recursive calls within the body can resolve the function index. Skipping pre-registration causes "function not found" errors for valid recursive code.

**Where:** `src/codegen/function_generator.rs`

**Watch for:** Any refactoring of function compilation order or function map construction.

---

## 5. Stack Balance in Type Conversions

**What:** String-to-integer, integer-to-string, and boolean conversion operations require careful operand stack management. Stack underflow/overflow in generated WASM caused 68/68 test failures at one point before being fully resolved. Each conversion path must leave exactly the right number of values on the WASM operand stack.

**Where:** Throughout `src/codegen/mir_codegen.rs` and `src/codegen/mod.rs`

**Watch for:** Adding new type conversion paths, modifying function call sequences, changing how temporary locals are allocated.

---

## 6. String Pointer Representation

**What:** Strings use a length-prefixed format in WASM memory: 4 bytes of length followed by content bytes. When passing strings as `Ptr(U8)`, the content pointer is at offset +4 from the string base pointer. The compiler and all bridge functions must agree on this layout.

**Where:** `src/codegen/mir_codegen.rs` (~lines 3871, 4116)

**Watch for:** Changes to string memory layout, bridge function string passing conventions, or `expand_strings` behavior in plugin.toml.

---

## 7. Marker Audit (FINAL 2026-04-12)

- **Before:** 231 `CRITICAL FIX` + 1 `WORKAROUND` markers
- **After:** 0 `CRITICAL FIX`, 0 `WORKAROUND` markers remaining
- All renamed to `NOTE:` (design documentation) or removed with dead code
- 39 `CRITICAL` occurrences remain — these are in error messages and priority labels, not code markers

## 8. Remaining Design Notes in mir_codegen.rs (UPDATED 2026-04-12)

Three workarounds assessed and resolved, two remain as known limitations:

**Resolved:**
1. ~~SymbolId→index collision avoidance~~ — assessed as correct cascading resolution design, not a workaround
2. ~~Hardcoded void function list~~ — **FIXED**: replaced with `function_return_types` registry lookup
3. ~~Stdlib return type fallback~~ — **FIXED**: `get_stdlib_return_type()` now uses `function_return_types` registry with namespace-based heuristic fallback

**Known limitations (kept as-is):**
4. ~~Ptr(Void) type ambiguity~~ — **FIXED (2026-04-12)**: MIR lowering now emits `MirType::Any` instead of `Ptr(Void)` for unresolved generics and complex type fallbacks. Codegen handles `MirType::Any` as i32 pointer to boxed value. Legacy `Ptr(Void)` checks retained for backward compatibility.
5. **Name conversion fallback** (~line 2666): Underscore/dot conversion for function lookups. Defensive code that doesn't cause bugs. Rarely triggered.
