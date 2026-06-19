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

**Where:** `src/codegen/mir_codegen/mod.rs` (pre-registration loop; see `compile_mir`, asserting "All functions pre-registered in function_map") and `src/codegen/mir_codegen/utilities.rs` (lookup site that panics if the invariant is violated). The legacy `function_generator.rs` was removed in the 2026-04-12 codegen cleanup; pre-registration responsibility moved to `MirCodeGenerator`.

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

## 9. Codegen Bug Verification Protocol

**What:** When a bug report describes a wrong WASM type, wrong instruction, or missing wrap/extend, source code inspection is not sufficient proof of fix. The emitted binary must be inspected directly.

**Rule:** Before calling `/resolve-fix` on any codegen/type bug:
1. Compile the minimal repro: `cln compile repro.cln -o repro.wasm`
2. Inspect the actual emitted type: `wasm-objdump -x repro.wasm | grep "type\[N\]"`
3. Confirm the type matches the spec (e.g. `() -> i64` not `() -> i32`)

**Why this matters:** `WasmType::I64` in source does not guarantee `() -> i64` in the binary — the code path exercised at runtime may differ from the code you read. A successful `cln compile` only proves the file compiles, not that the type table is correct.

**Also verify:** that the tag actually points to the fix commit before running comita. Use `gh release list` to see which commit a published release was built from — a local tag may point to a different commit than what GitHub published.

---

## 10. GEN003 — Preamble Bridge Registration Invariant (FRAGILE)

**What:** `collect_used_function_names_from_mir()` (`src/codegen/mir_codegen/utilities.rs`) must skip only DEAD preamble functions, not all preamble functions. The condition is: skip if `location.file == "<plugin-output>"` AND function name is NOT in `reachable_imports`.

**Why:** Reachable preamble functions (exported or single-underscore callbacks) ARE compiled and their bridge imports MUST be registered. If skipped, codegen crashes with "bridge alias not found in function map" for any bridge call inside a compiled preamble body.

**The invariant:** Register bridge imports for exactly the functions that survive DCE. The BFS pass (`collect_all_called_names_from_mir`) is the authority — if a preamble function's name is in `reachable_imports`, it survived DCE and its body must be scanned.

**Pattern that causes regression:** Treating all preamble functions uniformly — either all skipped (breaks reachable preamble helpers) or all included (breaks GEN003, causes import leakage).

**Where:** `src/codegen/mir_codegen/utilities.rs` ~line 1736, `tests/test_import_tree_shaking.rs`

**History:** First fixed in 0.30.213 (BFS seeding). Regressed in 0.30.226 when the db.query transitive-BFS fix tightened seeding — the body scanner was not updated to match. Fixed again with the reachability-gated skip condition.

---

## 8. Remaining Design Notes in mir_codegen.rs (UPDATED 2026-04-12)

Three workarounds assessed and resolved, two remain as known limitations:

**Resolved:**
1. ~~SymbolId→index collision avoidance~~ — assessed as correct cascading resolution design, not a workaround
2. ~~Hardcoded void function list~~ — **FIXED**: replaced with `function_return_types` registry lookup
3. ~~Stdlib return type fallback~~ — **FIXED**: `get_stdlib_return_type()` now uses `function_return_types` registry with namespace-based heuristic fallback

**Known limitations (kept as-is):**
4. ~~Ptr(Void) type ambiguity~~ — **FIXED (2026-04-12)**: MIR lowering now emits `MirType::Any` instead of `Ptr(Void)` for unresolved generics and complex type fallbacks. Codegen handles `MirType::Any` as i32 pointer to boxed value. Legacy `Ptr(Void)` checks retained for backward compatibility.
5. **Name conversion fallback** (~line 2666): Underscore/dot conversion for function lookups. Defensive code that doesn't cause bugs. Rarely triggered.

---

## 11. Bridge Contract Conformance — How It Stays Bulletproof (LANDED 2026-06-18)

**What:** Four-layer enforcement chain that makes bridge drift a build failure, not a dashboard report. The chain is the architectural answer to the recurring framework-regression pattern in 2026-04 to 2026-06.

| Layer | What it catches | Where it runs |
|---|---|---|
| Registry (`foundation/platform-architecture/function-registry.toml`) | Single source of truth | spec change control |
| Plugin-manifest conformance (`framework_plugins_match_registry`) | Plugin `[bridge]` drift from registry | `cargo test` |
| **Host-registration conformance (`test_host_registration_conformance`)** | **In-repo host signature + param-name drift from registry** | `cargo test` |
| **Compiler-emission conformance (`test_compiler_emitted_imports_conformance`)** | **Compiler emits imports not in registry, or with wrong signature** | `cargo test` |

**Where:** `src/plugins/host_conformance.rs` (parser + checker), `src/plugins/registry_loader.rs` (registry with `param_names`), `tests/test_host_registration_conformance.rs`, `tests/test_compiler_emitted_imports_conformance.rs`.

**Watch for:**
- Adding a `linker.func_wrap(...)` registration in `src/plugins/wasm_adapter.rs` or `src/bin/wasmtime_runner.rs` — host_conformance will fail if the registration disagrees with the registry. Either add the registry entry (with developer approval) or align the closure to the registry.
- Argument-order drift like `mem_alloc(size, _align)` vs registry `(type_id, size)` — only caught when the registry entry has `param_names = [...]`. Currently annotated for the `memory_runtime` namespace; expand coverage as new entry classes need protection.
- Cross-component hosts (clean-server, clean-node-server) — not covered here; cross-component prompts in `foundation/management/cross-component-prompts/` track the work to extend the chain to them.

**History:** Built in response to the diagnostic that identified plugin-contract drift across four locations as the dominant source of recurring framework bugs. The infrastructure pre-existed for plugin-manifest conformance (2026-04 to 2026-06); the host and compiler-emission layers complete the contract chain.

**Origin bug:** `COMPILER-MEM-ALLOC-NO-GROW`. The fix surfaced two distinct drifts (`(size, _align)` vs `(type_id, size)` AND missing `memory.grow`), both invisible to type-only checking. The semantic `param_names` layer was added specifically to catch the first.
