# KNOWLEDGE.md — Clean Language Compiler

Known fragile areas discovered across sessions. Read before modifying any compiler code.

---

## 1. String Heap Pointer Initialization Order

**What:** The heap start (initial `__heap_ptr`) must be computed AFTER all string constants are transferred into WASM linear memory, aligned to 8 bytes. Layout: string literals from offset 4096 (`CodeGenerator::string_offset_counter`), then the transient arena pool, then the main bump heap. If the heap start is computed too early, string constants get overwritten by heap allocations.

**Where:** `src/codegen/mir_codegen/utilities.rs` (`finalize_module`, reads `string_offset_counter` after `setup_string_pool` and all function bodies have run), `src/codegen/codegen_module_builder.rs` (`get_or_create_string_offset`)

**Watch for:** Any change to string pool transfer order, memory layout initialization, or bump allocator setup.

---

## 2. String Comparison Inversion

**What:** String equality comparison uses `i32.eqz` to invert the result of a byte-by-byte compare loop: `Equal` needs `i32.eqz`, `NotEqual` does NOT. Getting this backward silently produces wrong results without any runtime error.

**Where:** `src/codegen/mir_codegen/instructions.rs` (`generate_binary_operation`)

**Watch for:** Any refactoring of comparison operators, string equality logic, or conditional branching on string values.

---

## 3. Codegen Architecture (CLEANED UP 2026-04-12)

**What:** `MirCodeGenerator` (`src/codegen/mir_codegen/`) is the sole active codegen path. It wraps `CodeGenerator` (mod.rs) as `wasm_generator` field, using its infrastructure (type_manager, memory_utils data section, function_map, register_* methods). The final module is assembled only by `MirCodeGenerator::finalize_module` (`mir_codegen/utilities.rs`).

**Deleted:** expression_generator.rs, statement_generator.rs, binary_operations.rs, type_manager_tests.rs (2026-04-12); the old `CodeGenerator::assemble_module` path with wasm_module_builder.rs, the never-called `generate_basic_block`/`generate_terminator` linear emitters, and `src/mir/control_flow.rs` (2026-10).

**Still present but infrastructure-only:** `CodeGenerator` struct in mod.rs provides field storage and registration methods used by `MirCodeGenerator` (codegen_module_builder.rs, codegen_registration.rs, type_manager.rs, memory.rs).

**Where:** `src/codegen/mir_codegen/` — `blocks.rs` (functions, structured blocks, `emit_return`), `control_flow.rs`, `instructions.rs`, `operands.rs` (loads/stores, `emit_numeric_coercion`), `utilities.rs` (types, module setup, `finalize_module`).

**Watch for:** All new codegen work goes in `src/codegen/mir_codegen/`. The CodeGenerator struct methods in mod.rs should only be modified when changing infrastructure used by MirCodeGenerator. Numeric conversions between `integer`, `integer:64` and `number` go through `emit_numeric_coercion` (stores and returns); do not add another copy of the match.

---

## 4. Recursive Function Pre-registration

**What:** Functions must be registered in the function map BEFORE their bodies are generated, so that recursive calls within the body can resolve the function index. Skipping pre-registration causes "function not found" errors for valid recursive code.

**Where:** `src/codegen/mir_codegen/mod.rs` (pre-registration loop; see `compile_mir`, asserting "All functions pre-registered in function_map") and `src/codegen/mir_codegen/utilities.rs` (lookup site that panics if the invariant is violated). The legacy `function_generator.rs` was removed in the 2026-04-12 codegen cleanup; pre-registration responsibility moved to `MirCodeGenerator`.

**Watch for:** Any refactoring of function compilation order or function map construction.

---

## 5. Stack Balance in Type Conversions

**What:** String-to-integer, integer-to-string, and boolean conversion operations require careful operand stack management. Stack underflow/overflow in generated WASM caused 68/68 test failures at one point before being fully resolved. Each conversion path must leave exactly the right number of values on the WASM operand stack.

**Where:** Throughout `src/codegen/mir_codegen/` (`operands.rs::emit_numeric_coercion`, `instructions.rs` Cast handling) and `src/codegen/native_stdlib/type_conversions.rs`

**Watch for:** Adding new type conversion paths, modifying function call sequences, changing how temporary locals are allocated.

---

## 6. String Pointer Representation

**What:** Strings use a length-prefixed format in WASM memory: 4 bytes of length followed by content bytes. When passing strings as `Ptr(U8)`, the content pointer is at offset +4 from the string base pointer. The compiler and all bridge functions must agree on this layout.

**Where:** `src/codegen/mir_codegen/operands.rs` (`load_string_argument_for_print`), `src/codegen/mir_codegen/instructions.rs` (bridge call argument expansion)

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

## 8. Remaining Design Notes in mir_codegen/ (UPDATED 2026-04-12)

Three workarounds assessed and resolved, two remain as known limitations:

**Resolved:**
1. ~~SymbolId→index collision avoidance~~ — assessed as correct cascading resolution design, not a workaround
2. ~~Hardcoded void function list~~ — **FIXED**: replaced with `function_return_types` registry lookup
3. ~~Stdlib return type fallback~~ — **FIXED**: `get_stdlib_return_type()` now uses `function_return_types` registry with namespace-based heuristic fallback

**Known limitations (kept as-is):**
4. ~~Ptr(Void) type ambiguity~~ — **FIXED (2026-04-12)**: MIR lowering now emits `MirType::Any` instead of `Ptr(Void)` for unresolved generics and complex type fallbacks. Codegen handles `MirType::Any` as i32 pointer to boxed value. Legacy `Ptr(Void)` checks retained for backward compatibility.
5. **Name conversion fallback** (`mir_codegen/instructions.rs`, call lowering): Underscore/dot conversion for function lookups. Defensive code that doesn't cause bugs. Rarely triggered.

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

### 13.3 `break` / `continue` arms are not merges

A Jump to a loop header, to an `iterate` loop's increment block or to a loop exit ends the iteration and is emitted as a `br`; it never names an if's merge block. `ends_loop_iteration` is the single test for this. Walks that follow Jumps (`block_eventually_jumps_to`, `collect_jump_targets`, the nested-Branch continuation in `generate_branch_block`) must stop there: following the back edge reached the whole loop body again, so an if/else whose arm ends in `continue` looked like it flowed into its own else arm and the statements after the if/else were dropped. A false block labelled `"else"` by the MIR builder is always an else clause (`is_continuation_not_else`), even when the then-arm ends in `break`. Regression: `tests/cln/bugfixes/statement_after_if_else_whose_arm_continues.cln`.

### 13.4 A loop that never reaches its back edge is still a loop

A `while` / `iterate` whose body always ends in `break` (or `return`) has no back edge, so the DFS in `is_loop_header` does not find it. A Jump to such a header from a nested branch was then taken for a merge that an outer structure emits, and the whole loop disappeared. `is_loop_header` also accepts the MIR builder's loop header labels (`while_header`, `for_header`, `range_for_header`). Regression: `tests/cln/bugfixes/nested_loop_ending_in_break_runs.cln`.

**Where:** `src/codegen/mir_codegen/control_flow.rs` (`chase_jump_chain`, `collect_jump_targets`, `is_continuation_not_else`, `ends_loop_iteration`, `is_loop_header`), `src/codegen/mir_codegen/blocks.rs` (`generate_branch_block` Branch handler also updated to route through `find_eventual_continuation` when nested branches end in Branch terminators rather than only Jump).

**Watch for:**
- Any new code path that walks MIR blocks looking for "the continuation" of an if/else. Use `find_eventual_continuation`, not a single-step terminator inspection — otherwise this bug class reappears the next time someone nests three levels deep.
- Any new "empty-block + Jump → must be continuation" shortcut. The break/continue exceptions must be preserved; `LoopCodegenContext` is the canonical source for which Jump targets are loop control points.
- New regression fixtures live under `tests/cln/control/conditionals/08_stmt_after_nested_if_else.cln` and `tests/cln/control/loops/else_break_inside_while.cln`; the Rust gate is `tests/test_codegen_nested_control_flow.rs`. Don't delete either side.

**Origin bug:** `COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS` (dashboard fp `f80ee96ce507`). frame.ui's `process_text_node` was the canonical reproducer for §13.1 (the `remaining = remaining.substring(...)` statement following the inner if/else got dropped, so the `while` loop never advanced — the safety counter `if c > 100: break` the plugin author added masked it as a bounded but redundant 100-iteration loop instead of an outright hang). With §13.1 fixed, `find_unescaped_quote` surfaced §13.2 as the next hang. With both fixed, page-project compiles run end-to-end instead of trapping on the plugin-call deadline.

---

## 13a. Per-Iteration Heap Scopes

**What:** A loop body gets a `mem_scope_push` at its start and a `mem_scope_pop` on every exit of the iteration only when `body_is_iter_scope_safe` (`src/mir/mir_builder/helpers.rs`) proves no pointer allocated in the iteration can outlive it: no assignment of a heap value to an outer variable, field or element, no `return`, `later` or `background`, and no call that can store a value (`call_may_store`) in any expression of the body. Methods of strings and numbers and the reading methods of collections are allowed; calls to Clean functions and class methods are not. `break` and `continue` emit their own pop; the end-of-body pop is emitted only on the fall-through path (`current_block_terminated`), otherwise the scope is popped twice and the caller's strings are freed.

**Regression:** `tests/cln/bugfixes/loop_ending_in_break_pops_heap_scope_once.cln`, `tests/cln/bugfixes/loop_string_stored_by_call_survives_iteration.cln`.

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

---

## 15. MIR Constant Folding Must Match the Unoptimized WASM

**What:** `ConstantFoldingPass` (`src/mir/optimization.rs`) runs at the default `-O2`. A folded result must be bit-identical to what the unoptimized WASM computes, or programs print different values depending on the optimization level.

- `MirConstant::Integer` is an `i32` at runtime (`load_constant` narrows the `i64` payload): fold with `i32` wrapping arithmetic and compare as `i32`.
- `MirConstant::Integer64` is an `i64`: fold with `i64` wrapping arithmetic.
- Integer `Div`/`Rem` by zero (which raises "division by zero") and `MIN / -1` (which traps) are left unfolded so the runtime behavior still happens.
- Floats compare with exact IEEE-754 equality (`f64.eq`), never an epsilon.

**Regression test:** `tests/cln/bugfixes/mir_constant_folding_matches_runtime.cln`.

## 16. Stdlib Registration Order — Last Registration Wins

**What:** `register_function` / `add_function_alias` overwrite the name in `function_map`, so when two registrars register the same name the later one silently wins. The active order (`src/codegen/mir_codegen/mod.rs`) is: string class (non-allocating) → conditional ops (`method_style.rs`, `list_behavior.rs`) → HTTP/file → native memory operations (`__malloc`, the literal append `__list_push_i32` and native string functions in `codegen_registration.rs`) → validator → `ListManager` (`list_ops.rs`) → pairs → JSON.

**Where:** `ListManager::register_functions` runs after `__malloc` exists and calls `ListClass::register_functions` and `StringClass::register_heap_operations`. Every list method other than the `list_behavior.rs` ones (`add`, `size`, `isEmpty`, `isNotEmpty`, i32 `peek`) and every allocating string method lives there. Element-typed list variants (`list.<m>.f64`, `list.<m>.i64`, `list.<m>.string`, `__list_{first,last,peek,get,removeAt,...}_{f64,i64}`) are picked by `MirBuilder::list_method_variant` from the receiver's element type; `list.add` goes through `MirBuilder::list_add_symbol` (`list.add_f64`, `list.add_i64`, and `list.add.string`, which compares text on a `.unique` list). `list<number>` and `list<integer:64>` both use 8-byte elements; an `integer` element value is widened by `coerce_list_element_arg`. An `integer:64` boxed into `any` is stored as a number (tag 3), and an integer literal outside the i32 range is typed `integer:64` by the type checker. `.remove(index)` goes to `list.removeAt` / `__list_removeAt_f64`, `.pop()` / `.removeLast()` to `list.removeLast` / `__list_removeLast_f64`, the index-less `.remove()` to the behavior-aware `list.remove` / `__list_remove_f64`, and `.set` to `list.set` / `list.set.f64` (all in `list_class.rs`). Namespace calls `list.<m>(items, ...)` are lowered through the method-call path (`MirBuilder::is_list_receiver_method`), so both forms share dispatch and receiver write-back; `list.fill(size, value)` with a `number` value calls `list.fill.f64`.

**Watch for:**
- Registering an allocating stdlib function before `register_memory_operations` — `__malloc` does not exist yet, which previously left `padStart`/`padEnd` as identity functions.
- Allocating through the host's `memory_runtime.mem_alloc` import. Compiled code allocates only with `__malloc` (MEMORY_MODEL.md "Allocation"; boxes, class instances, object literals, class JSON trees and validators moved in #40). The import stays because EXECUTION_LAYERS.md lists `memory_runtime.*` as always emitted. Regression: `tests/test_compiled_code_allocates_with_malloc.rs`.
- f64-returning stdlib functions named `list.*` / `string.*`: the MIR return-type heuristic treats those prefixes as i32. Use an internal `__` name and `record_wasm_return_type`.
- New lists must set capacity to the allocated slot count (4-byte slots; two per `number`) — `list.add` trusts it when appending in place.

## 17. Compile Entry Points Share Two Front Ends

**What:** Every compile and type-check entry point in `src/lib.rs` runs one of two front ends, so checking and compiling cannot disagree:

- `run_multi_file_frontend` (discovery, `package:` manifests, plugins, imports, HIR merge, validation, resolution, type checking) — used by `compile_multi_file_*` (`cln compile`, `cln build`, release and client builds), `type_check_project` (`cln check`, `cln watch`), and the source-string wrappers `compile_with_external_plugins_and_opt_level` / `type_check_with_external_plugins` (MCP `compile`/`check`/`run`, `cln test`), which write the source to a scratch entry file when needed.
- `run_single_source_frontend` — used by the registry-driven library API (`compile`, `compile_with_plugins*`, `type_check*`) and `compile_with_target` (plugin builds).

**Watch for:** Adding a stage to one entry point only — put it in the front end. A `package:` `target:` sets the thread-local host-class override for the build; the library wrappers and `type_check_project` restore the caller's value afterwards so it does not leak into later compiles on the same thread.

## 18. If/else Merge Blocks Are Recorded, Not Guessed

**What:** The MIR builder records the merge (`continue`) block of every source `if` in `MirFunction::if_merges`, keyed by the block whose Branch tests the condition. `generate_branch_block` and `generate_structured_blocks` use the recorded merge (`recorded_if_merge`, valid while the block still ends in that `if`'s Branch) to decide whether there is an `else` arm and which block follows the `if`; the CFG heuristics of §13 (`find_eventual_continuation`, `resolve_if_merge`, `is_continuation_not_else`) remain only for branches without a recorded merge. A merge with no incoming edge (every arm returns, breaks or continues) is not emitted.

**Watch for:** New MIR builder code that lowers an `if`-shaped construct should insert into `if_merges` too. Code that rewrites a block's terminator must keep the entry consistent. Regression: `tests/cln/bugfixes/if_else_arm_ending_with_if_in_loop.cln`.

## 19. `onError` Regions and Error Unwinding

**What:** `error(msg)` lowers to the native `__err_raise(msg)`: with no `onError` region active (`ERROR_DEPTH_GLOBAL == 0`) it prints the message and traps; inside one it sets `ERROR_PENDING_GLOBAL`. The HIR builder brackets each protected part with `__err_region_begin()` / `__err_region_end()` followed by `if __err_take(): <handler>` (standalone `onError:` covers the statements since the block start or the previous handler; `expr onError:` covers the expression). The MIR builder turns a region into a loop that runs once (`begin_error_region`), and after every call to a Clean function or `__err_raise` emits `MirOperation::ErrorCheck`: while an error is pending it branches to the innermost region's exit (a loop exit label) or returns the zero value, so callers unwind to the region. `expr onError fallback` is the same region plus `if __err_take(): result = fallback`. Programs without any region get no checks (they are stripped after the build).

Runtime operations raise the same way: `.toInteger()` / `.toNumber()` of text (`__string_to_int`, `__string_to_int64`, `__string_to_number`, through `__conversion_error`; also reached by `any` → integer / number conversions) and integer `/` / `%` by zero (`emit_integer_division`, "division by zero"). `add_instruction` puts an `ErrorCheck` after those calls, after `UnboxAnyTo{I32,I64,F64}` / `OptionalConvert::FromAny`, and after a `Div`/`Rem` that is not by a non-zero constant or on numbers; `has_side_effects` keeps such a division when its result is unused.

**Where:** `src/hir/hir_builder.rs` (`build_block_inner`), `src/mir/mir_builder/helpers.rs` (`add_instruction`, `begin_error_region`, `end_error_region`, `leave_error_regions`), `src/codegen/mir_codegen/instructions.rs` (`ErrorCheck`), `src/codegen/codegen_registration.rs` (`register_error_handling_operations`), globals 5-6 in `native_stdlib/mod.rs`.

**Watch for:** `return`, `break` and `continue` out of a region must call `__err_leave` for each region they leave (`leave_error_regions`), or the depth count leaks and later errors are swallowed instead of trapping. A WASM `br` emitted inside nested instructions (short-circuit rhs) must count the enclosing `if` in `current_block_depth`. The `error` variable inside a handler is not implemented yet. Regression: `tests/cln/bugfixes/onerror_catches_error_from_called_function.cln`.

## 20. Class Instance Header and Instances in `any`

**What:** Every class instance starts with a 4-byte object header, `CLASS_HEADER_MARK | class_id` (`0xC1A50000 | id`, `mir_types::class_header_word`), written by the constructor call (`expressions.rs`, constructor allocation). The mark makes the word negative, so it never equals the non-negative `count` that starts a JSON-tree object or array; that is how the runtime tells a boxed instance (tag 6, payload = instance pointer) from a JSON-tree object (tag 6, payload = fragment). Boxing an instance keeps the instance, so `any` → class unboxing returns it; boxing a null instance gives a null box (`emit_box_value`, tag selected at run time).

The JSON hosts only understand JSON-tree fragments, so every value bound for a JSON bridge goes through `MirBuilder::json_value_of`: a class value through `__json_tree_instance` (switch on the header → `__json_tree_ClassName`), anything that can contain an instance through `__json_normalize` (copy-on-change walk that replaces boxed instances with their objects). `any` → class goes through `__any_to_instance`. The three functions are native WASM emitted after the MIR functions (`src/codegen/mir_codegen/class_json.rs`); their indices are reserved before MIR bodies are generated (`preregister_class_json_runtime`) because MIR calls them by name. They exist only when the program declares a class.

**Watch for:**
- Never box an instance as a JSON tree at boxing time: frame.data records travel through `any` and back (`Post? latest()`), and only the instance pointer survives the round trip. Convert at the JSON boundary instead.
- A new place that hands a value to a host JSON bridge (or to any bridge that reads boxed-Any trees) must use `json_value_of`, or a boxed instance reaches the host as raw memory.
- Capability dispatch compares the whole header word, not the bare class-id.
- Any new instance allocation path must write the header.

**Regression:** `tests/cln/bugfixes/class_instance_json_through_any.cln`, `tests/cln/bugfixes/record_rows_through_any_and_back.cln`, `tests/cln/bugfixes/class_instance_round_trips_through_any.cln`.

## 21. Plugin Helpers Are Kept by Name

**What:** Plugin helper functions (`location.file == "<plugin-output>"`) are dead-code eliminated in `mir_codegen/mod.rs::generate()` by NAME: a helper survives when its name is in the reachable set from `collect_all_called_names_from_mir` (`mir_codegen/utilities.rs`). That set also gets the name of every function the BFS visited. A user function can share a helper's name — a class method `notFound` next to frame.server's `notFound` helper — and then the helper survives too, without the BFS ever walking its body.

**Watch for:** every name added to the reachable set must be added before the preamble fixpoint, which walks the body of each kept helper. Adding names after it keeps a helper whose callees (`__json_escape`) were dropped, and codegen fails with "Function '__json_escape' not found in function map".

**Regression:** `tests/cln/bugfixes/method_named_like_plugin_helper.cln` (found in the website app, `FormatHttpResponse.notFound`).

## 22. Exports, Reserved Names and the Final Dead-Function Pass

**What:** The module exports only what hosts look up (`src/codegen/mir_codegen/exports.rs`): `memory`, `__heap_ptr`, `malloc`/`free`/`scope_push`/`scope_pop`, `_start`, `_run_tests`, `handle_event_N`, `__indirect_function_table`, and every program function (plugin-generated ones included) under its own name. Imports and runtime helpers are not exported. Program functions, imports and runtime helpers share `function_map`, so `check_function_names` rejects (SEM003) a source-file function that takes a runtime name and any function that takes a module export name; plugin-generated functions may take runtime language names (frame.server's `redirect`).

After `finalize_module` assembles the module, `codegen::module_dce` drops every defined function not reachable from the exports, the element segments and the start function, and renumbers the rest. Imports are kept (the Import Minimality Rule decides them at registration). Table slots keep their positions (a function value is its slot, `slot N holds function N`), only the function stored in each slot is renumbered.

**Watch for:**
- A host that needs a new export by name: add it to `exports.rs`, or the dead-function pass removes the function as well.
- Runtime code that refers to a function by index outside `call` / `return_call` / `ref.func` / element segments / exports (for example an index baked into an `i32.const` that is not a table slot): the pass cannot see it and renumbers the function. Function values must stay table slots.
- New module shapes (non-function imports, passive element segments, `ref.func` in globals): `module_dce` returns such modules unchanged; extend it rather than working around it.

**Regression:** `tests/test_module_exports.rs`, `tests/cln/bugfixes/function_named_like_runtime_helper_rejected.cln`, `tests/cln/bugfixes/function_named_memory_rejected.cln`, unit tests in `src/codegen/module_dce.rs`.

## 23. Locations of Code Parsed from Plugin Fragments

**What:** `_emit_stmt_from_source` / `_emit_expr_from_source` parse Clean source the plugin assembled from the block body (an endpoint handler, a component event, a query). Plugins pass origin offset 0 and re-indent or rewrite lines, so offsets cannot place that code. `src/parser/grammar.rs` first makes the parsed locations fragment-relative (`FragmentFrame`); then `typed_emission/fragment_origin.rs` matches each fragment line against the block's content lines (exact text, or a rewrite whose words contain the content line's words in order), aligns the matches in order, and gives matched code the user's file, line and column. Unmatched lines are the plugin's own code and get `<plugin:NAME>`. Where the content starts in the file comes from `FrameworkBlock::body_location`, set by the token parser (`block_body_location`); blocks without it (ORM queries the expander builds with a binding header) are never matched. The statements that replace an ORM query statement get that statement's location (`expand_orm_query`). The error reporter takes the snippet from the file the error is in, and shows none for `<…>` locations.

**Watch for:**
- A new way for plugins to hand back source must relocate it the same way, or its errors name `<plugin-fragment>`.
- `body_location` is compiler-internal (`serde(skip_serializing)`): changing what plugins receive changes their output.
- A new AST node with a location must be visited by `Statement`/`Expression::for_each_location_mut`.

**Regression:** `tests/test_plugin_block_diagnostic_locations.rs`, `tests/cln/bugfixes/plugin_*_names_user_line.cln` (#27).

## 24. Optional Primitives Are Boxed

**What:** `integer?`, `number?`, `boolean?` and their sized forms (`integer:8?` … `integer:64u?`, `number:32?`) are a pointer to an immutable 12-byte boxed-any block (tag 1 + i64 for every integer width, tag 3 + f64 for both number widths, tag 2 for booleans), `0` for none (`foundation/spec/type-system.md` §8 "Representation", `MEMORY_MODEL.md` "Optional primitive"). Their MIR type is `MirType::Any`. `erase_optional` keeps their `ConcreteType::Optional` wrapper so the MIR builder can tell them apart; every other `T?` is erased to `T` (none = null pointer).

Design choice (#38): the alternatives were a sentinel bit pattern (i64::MIN for `integer?`, a NaN payload for `number?`, 2 for `boolean?`) and a hidden companion flag. A sentinel steals a value and has no room at all in `integer:8?` (every bit pattern is a value); a flag doubles every slot, parameter, return value, field and element and changes every function signature. The box costs one 12-byte allocation per store of a plain value, but holds every value, is 4 bytes everywhere, and is already the layout of `any`, so `T?` → `any` is the same pointer and a JSON `null` maps to none.

Where it is handled:
- **Into a `T?`:** `TypeInference::require_assignable` wraps every value stored into an optional primitive (declaration, assignment, argument, return, field, list/pairs literal element, both branches of a conditional) in `__optional_box` (`tast::OPTIONAL_BOX`); the subtype check uses the value's own type. `MirBuilder::convert_into_optional` (`mir_builder/optional.rs`) lowers it to `MirOperation::OptionalConvert`: `Box` for a plain value, `FromAny` for an `any` or an optional of another type (null pointer or null box → 0, otherwise the `any` conversion to `T`, then a box), nothing for none or a value of the same type. Codegen: `mir_codegen/optional.rs`.
- **Out of a `T?`:** `!` (`build_optional_required`: tag test → `Required` trap → `Unbox`), `default` (`emit_non_null_test` reads the tag; `coerce_on_error_branch` unboxes the value arm into a plain result and boxes a plain arm into an optional result), `==` / `!=` / `is` (`build_optional_comparison`: none equals only none, values compare as values, two optionals compare presence, then value).
- **Declarations:** an optional variable takes the box pointer with a Copy, never through `emit_unbox_any` (which returns optional targets unchanged) — otherwise `integer? d = c` shared `c`'s local.
- **JSON / any:** `emit_box_any` turns an optional into its box or a fresh null box (`OptionalConversion::ToJson`), so hosts never get a 0 pointer. Class JSON trees treat an optional field like an `any` field (`__json_try_decode_v2`), and a list of optionals like `list<any>`. Descriptor codes `I`, `L`, `N`, `B` (`json_class::any_descriptor`) carry optional elements through `__cln_to_any` (the box is the value) and `__any_convert` (null → none, a value of the right tag → its box, otherwise the conversion error).
- **Lists:** `contains`, `indexOf`, `lastIndexOf`, `sort` and a `.unique` `add` of `list<T?>` use the `list.*.boxed` variants (`list_class.rs`: `__list_box_eq`, `__list_box_cmp`; none orders first), which compare values, not box addresses.
- **Escapes:** `is_arena_alloc_type` counts optional primitives as heap pointers, so a loop that assigns one to an outer variable gets no per-iteration scope pop.

**Watch for:**
- A new place that stores a value into a typed slot without `require_assignable` must call `convert_into_optional`, or a raw integer is stored where a pointer is expected.
- Code that strips `Optional` with `HirType::without_optional` must check `is_optional_primitive` first: the inner type's MIR type (i32/f64) is not the box pointer.
- Never write into a box: copies share it.
- `x!` must stay in the program when its value is unused: `has_side_effects` keeps `Required`.

**Regression:** `tests/cln/bugfixes/optional_primitive_zero_is_a_value.cln`, `optional_primitive_params_returns_fields.cln`, `optional_sized_primitive_holds_every_value.cln`, `optional_primitive_collections_and_any.cln`, `optional_primitive_list_search_sort_onerror.cln`; `tests/test_required_halts_on_none.rs` (`!` halts only on none).

## 25. Overridden Methods Dispatch on the Object Header

**What:** A call `x.m()` on a class-typed receiver runs the method of the object's own class when a subclass of the declared type redefines `m` (15-classes-and-objects.md "Method Overriding"). `MirBuilder::method_overrides` (`mir_builder/helpers.rs`) lists, for the statically bound method, each overriding method with the class-ids of the subclasses that run it (nearest definition by name from each subclass up); when the list is not empty the MethodCall arm emits `MirOperation::CallVirtual` instead of `Call`. Codegen (`generate_call_virtual`, `mir_codegen/instructions.rs`) compares the header word with each class's header and runs one ordinary `generate_call_instruction` per arm, so argument coercion and result storage are those of a direct call; any other class (and the declared class itself) runs the bound method. A leaf type, a method no subclass redefines, a private method, `ClassName.method()` and constructors stay direct calls.

**Watch for:**
- Every method of a class that has subclasses takes `this` (`type_inference.rs` `in_hierarchy`); otherwise a parent method of a class without fields is lowered static and its overrides cannot take the same call.
- An override whose parameters or result lower to other WASM types than the bound method's is not dispatched to (the spec has no rule for such a redefinition yet).
- A new pass over MIR calls (reachability BFS, preamble fixpoint, `onError` checks, liveness) must treat `CallVirtual` as a call of every method it names.
- `obj.toString()` with no `toString` on the declared class is still the declared class's name.

**Regression:** `tests/cln/bugfixes/overridden_method_*.cln`, `overridden_void_and_number_methods_dispatch.cln`.
