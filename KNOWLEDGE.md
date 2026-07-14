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

---

## 12. Plugin Call Timeout — Why Silent Hangs Are Errors (LANDED 2026-06-20)

**What:** Every WASM plugin call (`process_html`, `assemble`, lifecycle hooks, block expansion) runs inside a wasmtime store with epoch-based interruption enabled. A daemon thread ticks the engine epoch every `EPOCH_TICK_MS` (100 ms), and `create_store` sets a per-call deadline of `CLN_PLUGIN_TIMEOUT_SECS` (default 30 s). Plugins that run past the deadline trap with `wasm trap: interrupt` and the trap is turned into a `CompilerError::PluginError` with a WASM backtrace plus an explanation of the timeout, the env-var override, and what to investigate next.

**Where:** `src/plugins/wasm_loader.rs` (`build_engine`, `start_epoch_ticker`, `plugin_timeout_secs`), `src/plugins/wasm_adapter.rs` (`create_store`, `describe_plugin_trap`).

**Watch for:**
- Adding a new `Engine::default()` call for plugins — must go through `build_engine` so the ticker and epoch interruption are active.
- Adding a new `Store::new(...)` for a plugin — must use `create_store` (or call `set_epoch_deadline` itself) or the timeout silently won't apply.
- Tests or environments where a plugin call legitimately exceeds 30 s — raise `CLN_PLUGIN_TIMEOUT_SECS` for that run; do not move the default upward without a real workload to justify it.
- `CLN_PLUGIN_TIMEOUT_SECS=0` disables the deadline for diagnostic reproduction — keep that escape hatch.

**Origin bug:** `COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS` (dashboard fp `f80ee96ce507`). A plugin call inside `process_html` for any project containing a `.cln` file under `app/ui/web/pages/` enters a loop and never returns; before the timeout there was no diagnostic and `cln compile` hung indefinitely. The deadline does not fix the underlying codegen issue — it converts the silent hang into an actionable error so the user (or a CI run) can stop waiting and a backtrace pins down where in the plugin the loop lives.

---

## 13. Structured Control-Flow Lowering — Two Drop-the-Trailing-Edge Pitfalls (LANDED 2026-06-21)

**What:** Two independent bugs in `src/codegen/mir_codegen/` silently dropped statements from the emitted WASM whenever the MIR they were lowering had certain nested control-flow shapes. Both passed the type checker and produced runnable WASM — the missing instructions only surfaced as infinite loops or wrong output at runtime.

### 13.1 `collect_jump_targets` stopped at the innermost merge

`find_eventual_continuation` (and the inlining call sites in `generate_branch_block`) collect non-returning Jump targets via `collect_jump_targets`. For a Jump terminator, the function inserted the *immediate* target into the returned set. In a shape like:

```clean
if A
    if B
        if C: x else: y      -- inner1 if/else
    else
        if D: a else: b      -- inner2 if/else

    stmt                      -- statement to keep
```

both inner if/elses merge through their own continue blocks (let's call them 8 and 11) before reaching block 5, where `stmt` lives. The collector saw `{8, 11}` rather than `{5}`, decided "branches merge at different points → no common continuation → inline nothing," and `stmt` never made it into the WASM.

`chase_jump_chain` (new helper in `control_flow.rs`) walks through *empty* merge blocks that end in their own Jump, so the collector reports the eventual single merge `{5}` and the inliner emits it.

### 13.2 `is_continuation_not_else` mistook `else: break` for "no else clause"

`is_continuation_not_else` returned true when the if's `false_block` was empty and had a Jump terminator — the assumption being that an empty-Jump `false_block` is the merge point of a no-else `if`. An `else: break`, however, also produces an empty `false_block` with a Jump terminator (Jump to the loop's exit). The check fired, the codegen skipped the else clause entirely, and the loop ran forever because nothing ever broke it.

The fix excludes Jumps that target any `exit_block_id` in `self.loop_context_stack` — those are breaks, and the else branch holding them must be lowered as an explicit `else` arm.

**Where:** `src/codegen/mir_codegen/control_flow.rs` (`chase_jump_chain`, `collect_jump_targets`, `is_continuation_not_else`), `src/codegen/mir_codegen/blocks.rs` (`generate_branch_block` Branch handler also updated to route through `find_eventual_continuation` when nested branches end in Branch terminators rather than only Jump).

**Watch for:**
- Any new code path that walks MIR blocks looking for "the continuation" of an if/else. Use `find_eventual_continuation`, not a single-step terminator inspection — otherwise this bug class reappears the next time someone nests three levels deep.
- Any new "empty-block + Jump → must be continuation" shortcut. The break/continue exceptions must be preserved; `LoopCodegenContext` is the canonical source for which Jump targets are loop control points.
- New regression fixtures live under `tests/cln/control/conditionals/08_stmt_after_nested_if_else.cln` and `tests/cln/control/loops/else_break_inside_while.cln`; the Rust gate is `tests/test_codegen_nested_control_flow.rs`. Don't delete either side.

**Origin bug:** `COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS` (dashboard fp `f80ee96ce507`). frame.ui's `process_text_node` was the canonical reproducer for §13.1 (the `remaining = remaining.substring(...)` statement following the inner if/else got dropped, so the `while` loop never advanced — the safety counter `if c > 100: break` the plugin author added masked it as a bounded but redundant 100-iteration loop instead of an outright hang). With §13.1 fixed, `find_unescaped_quote` surfaced §13.2 as the next hang. With both fixed, page-project compiles run end-to-end instead of trapping on the plugin-call deadline.

---

## 14. Opaque Byte Handles (`_req_body_bytes`, `_fs_write_bytes`)

**What:** Some bridges deal in raw bytes that cannot survive UTF-8 decoding (application/octet-stream request bodies, gzipped tarballs). Clean Language has no `bytes` primitive yet, so these bridges use the **opaque handle** convention: a bytes-producing bridge returns an integer that is actually a pointer to a `[4-byte LE length][bytes]` buffer (identical layout to length-prefixed strings), and a bytes-consuming bridge takes that integer unchanged and reads the length prefix on the host side. See `foundation/spec/type-system.md` §9b for the full rule set.

**Where:**
- `src/plugins/function-registry.toml` — `_req_body_bytes` (Layer 3 request) and `_fs_write_bytes` (Layer 2 file_io) entries with `aliases = ["req.body_bytes"]` / `["fs.write_bytes"]`.
- `src/plugins/runtime-abi-v1.toml` — bridge catalog entries; total count is enforced against `func_wrap(` sites in `wasm_adapter.rs`.
- `src/codegen/mod.rs::is_reachability_gated_import` — `_fs_` prefix is gated so client-only builds tree-shake the import.
- `src/codegen/codegen_module_builder.rs::register_file_imports` — emits `_fs_write_bytes` WASM import and maps `fs.write_bytes` → same function index.
- `src/codegen/codegen_module_builder.rs::register_http_imports` (server block) — emits `_req_body_bytes`.
- `src/resolver/symbol_table.rs` — registers both raw underscore names as builtin functions typed `Integer` (opaque handle).

**Watch for:**
- Anyone adding "bytes manipulation" primitives (slice, index, compare) to Clean code touching these handles: the whole point of the opaque convention is to defer that until a real `bytes` type lands. If you find yourself wanting to inspect a handle in Clean code, escalate to a language change instead — do NOT introduce ad-hoc string-cast escape hatches.
- Adding more bytes-producing or bytes-consuming bridges: match the same convention (returns/params `= "ptr"` in the registry, docs cite spec §9b). Every new bridge MUST be listed in the §9b handle-user table in `type-system.md`.
- The compiler-local `runtime-abi-v1.toml` is auto-synced from `foundation/platform-architecture/runtime-abi/v1.toml`. Edits to the compiler-local copy may be silently reverted by the sync — always edit the foundation copy first, then `cp` (or wait for the sync) into `src/plugins/runtime-abi-v1.toml`. The count invariant `bridges_in_toml == func_wrap_sites_in_wasm_adapter` is enforced by `runtime_abi_schema.rs::verify_registrations_against_schema`; both files must move together.
