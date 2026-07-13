# Clean Language Compiler - Implementation Tasks

## 🔎 INVESTIGATION NOTES (2026-07-13) — receiver_type inference for `.toNumber()` / `.toBoolean()` on chained `json.get(...)`

Discovered while fixing CODEGEN-UNBOX-TO-I32-MISSING-STRING-TAG-CASE (#0ccc47714523).

`Any.toInteger()` correctly routes through `UnboxAnyToI32` when the receiver
type is inferred as `Any`, e.g.:
- `json.get(blob, key).toInteger()` inside a wrapper function → OK
- `any x = json.get(...); x.toInteger()` → OK

But `.toNumber()` and `.toBoolean()` on the SAME chained shape route
through a different lowering path — the emitted WASM shows an
`emit_any_to_string`-style multi-tag dispatch that stringifies the Any
per-tag and then feeds the resulting string to `string_to_float`. That
dispatch does not call `UnboxAnyToF64` / `UnboxAnyToBoolean` at all, so
the tag=String branches I just added never fire on the chained shape.

Reproduction:
- `json.get(blob, key).toNumber()` (chained, no intermediate) → returns 0
- `any r = json.get(blob, key); r.toNumber()` → correct
- `json.get(blob, key).toBoolean()` (chained) → returns false
- `any r = json.get(...); r.toBoolean()` → correct

Suspected cause: the receiver_type at
`src/mir/mir_builder/expressions.rs:1997` is not `ConcreteType::Any` when
the receiver is a chained `Call` expression whose declared return type is
`Any`. Something in type inference between HIR and MIR is narrowing the
type prematurely. Since the `(ConcreteType::Any, "toNumber")` branch at
line 2157 is skipped, the fallback lowering emits stringify-then-parse
which is buggy.

Not fixing in this session — reporter's actual repro (integer path) is
closed, and the chained-toNumber path is not a regression from this
change. The pre-existing multi-tag stringify code path was already
producing wrong results for tag=String, this fix just doesn't reach it.

Workaround for callers: assign `json.get(...)` to `any r` before calling
`.toNumber()` / `.toBoolean()`.

Fix location once someone picks this up:
`src/mir/mir_builder/expressions.rs:1997` — trace why receiver_type is not
`ConcreteType::Any` for a chained call whose function returns Any.

## 🔎 INVESTIGATION NOTES (2026-07-13) — /fix run findings, not yet actionable

Recorded so the next /fix run does not repeat this work.

### CODEGEN-STRING-ACCUM-LOOP-TRUNCATION-V2 (fp `eed00ffee567`) — minimal repro from bug does NOT reproduce on 0.33.58 standalone

- The bug's `minimal_repro` (`build_rows` function with 9-fragment `+` chain inside `while + if/else`, iterating a single-row JSON fixture) was reduced to a standalone `.cln` file, compiled with 0.33.58, executed under `wasmtime_runner`. Output was **correct** (466 chars, starts with `<tr><td class='task-id-col'>#1</td>`, ends with `</tr>`, no truncation).
- Reporter's own note admits their existing regression pin `codegen_string_accum_in_if_else_truncation.cln` also passes locally. Truncation only fires in the **full production shape** (`clean-errors app/server/pages/tasks.cln`, ~900 lines, many concat sites across many functions in the same module).
- Suggests a scale-sensitive heap-layout or bump-alloc-interaction bug: minimal file compiles a small linear-memory footprint, does not trigger the aliasing condition.
- **Do not chase the minimal repro further.** Next fixer needs the reporter's real `pages/tasks.cln` shape (with surrounding module) and to bisect by shrinking IT, not by expanding a synthetic repro.
- Test artefact at `/tmp/accum-v2/test.cln` if needed to see the "passing" shape.

### WASM-HANDLER-TRAP-JSON-ITERATION (fp `5986e77a214f`) — compiler-side minimal repro passes; bug lives in endpoint/plugin path

- Extracted the essence of the bug (while-loop iterating `json.get(arr, i.toString())` with per-iteration class construction + html-like string return) into standalone `.cln`. Runs cleanly on 0.33.58 in `wasmtime_runner`, outputs `<div class="card"><h3>A</h3></div>...` for 3 iterations, exits 0, no trap.
- Reporter's trap only fires when the same pattern runs inside `endpoints server: GET "/probe"` under `clean-server 1.9.76` with `frame.server` plugin.
- Rules out reporter's hypothesis (a): `json.get(arr, out-of-range)` correctly returns `""` and the loop exits — no infinite loop in the compiler-side code path.
- Bug likely lives in one of:
  1. Plugin-generated route wrapper (compiled by cln, but source is from frame.server plugin.wasm)
  2. `clean-server`'s handler dispatch or response bridge
  3. Interaction between the plugin's assemble hook and per-iteration class-constructor allocation under a route context (heap owned by the request, not the module)
- **Not fixable inside the compiler alone.** Needs a session that boots clean-server + frame.server together and reproduces the trap end-to-end.
- Test artefact at `/tmp/json-iter-trap/test2.cln` (passes).

## 🔍 POST-PLAN CLEANUP: Dev-queue investigations (2026-06-29)

### SEM007 (fingerprint 2426d9b5f7f0cd4d) — `Function 'expand_block' not found`
- **Source**: `clean-framework/plugins/frame.data/tests/test_expand.cln` (mtime 2026-06-26)
- **Occurrences**: 3 (last 2026-06-29 23:08:46Z, compiler 0.30.401)
- **Ownership**: frame.data (cross-component) — file lives under another component's tree per `Dev reason: source inside component tree`
- **Status**: NOT a compiler bug. The `.cln` test file references `expand_block` which is a frame.data plugin DSL helper. Investigation should determine if (a) test file is stale from C4 testing and frame.data needs to clean it up, or (b) the v2 plugin migration removed `expand_block` and this test wasn't updated. Either way, fix lives in frame.data, not the compiler.
- **Action**: Report to frame.data owners; do not clear from dev queue (still reproducible).

### COM001 (ba21b72c / f05aa5dc) — Not found in current dev queue
- The prefixes referenced in the prompt do not exist in the current dev-queue state. Either already cleared in a prior session or the prefixes were misremembered. No action needed; if the stack-mismatch trap recurs it will re-fingerprint.

## ✅ FIXED (pending ship): CMP-SSR-MALLOC-OOM-PAGE-RENDER (helper-leak path) — transient arena + body-local routing

**Status as of this commit:** All three steps of the Cyclone-style nested-region fix have shipped. The per-iter conditional-helper leak (`headHtml = "<h2>" + head + "</h2>"`) is now routed through a separate transient pool that resets at every iteration boundary. The main heap continues to hold cross-iteration live values, so the rolled-back dangling-pointer regression cannot recur.

### Why this approach

The previous `string_builder_reclaim` mechanism (0.30.373/0.30.374) tried to "punch a hole" mid-region in the main bump heap by resetting `HEAP_PTR` to `max(init_mark, builder + 8 + capacity)`. That free'd whatever transient bytes lived above the builder's tail — and corrupted any cross-iteration live pointer (`head = json.get(...)`) that happened to sit in the same range. Surfaced as **CMP-SSR-RECLAIM-FREES-LIVE-POINTER** (#7fc4f890aab9) within hours of 0.30.374 and was rolled back in 0.30.375.

Bumpalo's own docs are explicit: "if you save a checkpoint and allocate additional data, then reset to that checkpoint while live allocations exist above it, those allocations become dangling references. The system cannot prevent this because it lacks per-allocation tracking." Every authoritative source (Cyclone region-inference, bump-scope `scoped()`, frame-allocator practice) uses the same answer: **two physically separate regions, sorted by lifetime instead of time.** The outer region holds cross-iteration values; the inner region holds per-iteration transients. Resetting the inner region cannot dangle outer pointers because they were never in it.

### What landed in this commit (Steps 1 + 2)

1. **`src/codegen/native_stdlib/transient_arena.rs`** (new) — `__transient_scope_enter`, `__transient_scope_exit`, `__transient_alloc`. Lazily-allocated 64KB pool, separate `TRANSIENT_BASE_GLOBAL` (idx 4) + `TRANSIENT_PTR_GLOBAL` (idx 5). 5 unit tests lock in the bookkeeping invariants (lazy init, base==0 fallback to `__malloc`, 8-byte alignment, mark==0 no-op guard).
2. **`src/codegen/native_stdlib/mod.rs`** — `RESERVED_GLOBAL_COUNT = 6` with documented layout.
3. **`src/codegen/mod.rs` + `src/codegen/mir_codegen/utilities.rs`** — both global-section emitters now reserve 5 slots after `__heap_ptr`. **State-variable globals shift base from 4 → `RESERVED_GLOBAL_COUNT` (6)** in `mir_codegen/mod.rs`.
4. **`src/codegen/codegen_registration.rs`** — registers the three helpers next to `__heap_ptr_snapshot`, sharing `malloc_idx`.
5. **`src/resolver/resolver_impl.rs`** — declares `transient_scope_enter`/`_exit`/`_alloc` as builtin functions for the type checker.
6. **`src/codegen/mir_codegen/instructions.rs`** — adds `transient_scope_exit` + `__transient_scope_exit` to `is_known_void_builtin`.
7. **`src/hir/hir_builder.rs::try_match_accumulator_pair`** — wraps every accumulator-rewritten loop body with:
   ```
   __sb_N_tmark = __transient_scope_enter()
   <body>
   __transient_scope_exit(__sb_N_tmark)
   ```
   This is currently a **no-op at runtime** because nothing routes through `__transient_alloc` yet. But it establishes the scope, so Step 3 can flip routing on incrementally without re-introducing the dangling-pointer failure mode.

### Verification

- `cargo test --lib`: 448 pass (including 5 new `transient_arena` tests, 7 existing `string_builder` tests).
- All 5 SSR regression tests still compile and run:
  - `ssr_concat_chain_no_oom` — OK
  - `ssr_concat_in_loop_no_oom` — OK
  - `ssr_concat_in_loop_no_oom_nested_call` — OK
  - `ssr_concat_conditional_helper_no_oom` — OK
  - `ssr_reclaim_no_live_pointer_corruption` — OK (the regression test from the rollback commit)
- `strings tests/output/bugfixes/ssr_concat_in_loop_no_oom.wasm | grep transient` confirms all three helpers are present in generated WASM.
- Integration tests touching globals layout still pass: `test_memory_exports`, `test_host_registration_conformance`, `test_compiler_emitted_imports_conformance`, `test_dual_naming_imports`, `test_reg001_function_index_ordering`, `test_codegen_nested_control_flow`.

### Step 3 — landed: HIR-level body-local routing

The complication noted in the earlier draft of this entry (that `BinaryOp::Add` on strings lowers at MIR-level, not HIR) turned out to be tractable without touching MIR at all. The chosen design:

- **`SYM_BUILTIN_STRING_CONCAT_TRANSIENT`** (new MIR symbol, ID `SYNTHETIC_BUILTIN_BASE + 12`) maps to `string_concat_transient`, which the codegen registration aliases to `__string_concat_transient`. The new native function reuses `gen_concat` parameterized over `transient_alloc_idx` — identical body to `__string_concat` except the result-alloc call site.
- **HIR rewriter** (`try_match_accumulator_pair` in `src/hir/hir_builder.rs`) gains a two-phase post-pass after `rewrite_self_append_in_block`:
  1. `collect_body_local_string_names` — walks the rewritten body, returns the set of string-typed names declared inside it (descending into nested `If`/`While`/`For` blocks).
  2. `rewrite_body_local_helpers_to_transient` — walks the body again, finds every `Assignment`/`VariableDeclaration` whose LHS variable is in the body-local set AND whose RHS is a `BinaryOp::Add`/`StringConcat` chain, and rewrites the chain into nested `Call { function: "string_concat_transient", ... }` expressions via `fold_concat_chain_to_transient_calls`.
- The body is then wrapped in the transient `enter`/`exit` pair from Step 2. Routing was added in the same site so the invariant "routing only happens inside a transient scope" is preserved by construction.

Why this is safe where the rolled-back reclaim wasn't: a name like `head` from the canonical CMP-SSR-RECLAIM-FREES-LIVE-POINTER repro is declared OUTSIDE the rewritten loop body, so it is never in `body_local_string_names`, so its `head = json.get(...)` reassignment continues to allocate via `__malloc` and lives on the main heap across iterations. The transient pool reset at end-of-iter cannot dangle it because it was never in the pool. This is the Cyclone region invariant in action — lifetime-by-construction, not lifetime-by-runtime-heuristic.

### Verification — Step 3

- `cargo build`: clean.
- `cargo clippy --lib --no-deps`: 0 warnings.
- `cargo test --lib`: 448 pass.
- All 5 SSR regression tests pass (compile + run):
  - `ssr_concat_chain_no_oom` — OK
  - `ssr_concat_in_loop_no_oom` — OK
  - `ssr_concat_in_loop_no_oom_nested_call` — OK
  - `ssr_concat_conditional_helper_no_oom` — OK at full **5000 iterations**, output `265000`. This is the repro that drove CMP-SSR-MALLOC-OOM-PAGE-RENDER-CACHE-INEFFECTIVE (#ac6112d9beb6) and CMP-SSR-MALLOC-OOM-CONDITIONAL-HELPER (#e4c682d19d00).
  - `ssr_reclaim_no_live_pointer_corruption` — OK. This is the regression test from the 0.30.375 rollback; it stresses the exact `head = json.get(...)` outer-scope-reassignment shape that broke the previous attempt. Confirms the new routing doesn't reintroduce the dangling-pointer failure.
- `strings tests/output/bugfixes/ssr_concat_conditional_helper_no_oom.wasm` confirms `string_concat_transient`, `transient_scope_enter/exit`, and `transient_alloc` are all present in the generated module.
- Integration tests passing: `test_memory_exports`, `test_host_registration_conformance`, `test_compiler_emitted_imports_conformance`, `test_dual_naming_imports`, `test_reg001_function_index_ordering`, `test_codegen_nested_control_flow`, `test_iterate_iter_scope`, `test_while_iter_scope`.

### Files changed in Step 3

- `src/codegen/codegen_registration.rs` — registers `__string_concat_transient` after `__transient_alloc`, sharing the `transient_alloc_idx`.
- `src/resolver/symbol_table.rs` — adds `SYM_BUILTIN_STRING_CONCAT_TRANSIENT` at `SYNTHETIC_BUILTIN_BASE + 12`.
- `src/mir/mir_builder/mod.rs` — adds the symbol-name-map entry pointing the transient symbol at `"string_concat_transient"`.
- `src/codegen/mir_codegen/instructions.rs` — extends the `string.concat` call-site dispatch to also match `string_concat_transient` / `__string_concat_transient` (same calling convention).
- `src/resolver/resolver_impl.rs` — declares `string_concat_transient` as a typechecker builtin with the same signature as `string.concat`.
- `src/hir/hir_builder.rs` — adds `collect_body_local_string_names`, `rewrite_body_local_helpers_to_transient`, `is_string_concat_chain`, `fold_concat_chain_to_transient_calls`, `placeholder_void_expression` helpers; wires them into `try_match_accumulator_pair` between `rewrite_self_append_in_block` and the transient `enter`/`exit` wrap.

### What this does NOT fix

- The original `head = json.get(...)` parse-tree leak path was already addressed by 0.30.369's `json.get` cache (commit `8d523b13`). That's still doing its work.
- Stranded old builder regions from intra-iter grow events (the O(n)-total geometric stranding accepted by the doubling-growth design) are not reclaimed and continue to leak inside the main heap. Per-render this is bounded (the builder doubles a logarithmic number of times); cross-render reclaim is the host's responsibility via `scope_push`/`scope_pop`.
- Outer-scope string assignments whose RHS is a concat chain (`outerStr = outerStr + "x"`) are intentionally NOT routed through transient — the LHS is outer-scope. If this shape appears in production hot loops it must be handled by the existing accumulator-matcher (which only catches single-accumulator-per-loop), or by a future extension that handles multiple accumulators.

### Sources informing the design

- bump-scope (`scoped()` mechanism) — https://github.com/bluurryy/bump-scope
- Bumpalo `Bump` (explicit warning against per-allocation reclaim) — https://docs.rs/bumpalo
- Cyclone region-based memory management (Grossman & Morrisett) — https://www.cs.umd.edu/projects/cyclone/papers/cyclone-regions.pdf
- Fast Escape Analysis for Region-Based Memory Management — https://www.sciencedirect.com/science/article/pii/S1571066105002616

---

## ✅ FIXED (pending ship): CMP-SSR-MALLOC-OOM-PAGE-RENDER — root cause was `c = json.get(...)` assignment never unboxing Any→string + DCE breaking after first defining-instruction match

**Fix landed locally 2026-06-26** (commit pending). Two surgical changes:

1. **`src/mir/mir_builder/statements.rs::TastStatement::Assignment` → `TastExpressionKind::Variable`**: Added `needs_unboxing` check mirroring the existing `VariableDeclaration` logic. When the RHS type is `Any` (or its actual MIR type is `Any`) and the target type is not `Any`, emit `emit_unbox_any` to convert the boxed-Any wrapper into the underlying scalar/pointer. Without this, `c = json.get(...)` (where `c: string`) silently wrote the wrapper-struct pointer (`[tag=4, inner_ptr, 0]`) into `c`'s WASM local. Downstream `string_compare`/`string.length` then read the wrapper's tag byte (`04 00 00 00`) as a length-4 string. This is the actual SSR symptom: the loop's `while c != ""` condition either truncated iteration count (when the 4 content bytes encoded as `""`) or kept reading garbage strings.

2. **`src/mir/optimization.rs::DeadCodeEliminationPass::mark_live`**: Removed the `break` after finding the first defining instruction. The MIR is NOT in SSA form — a single ValueId can be the destination of multiple instructions (the canonical case is an outer-scope variable re-assigned inside a loop). Stopping at the first match silently dropped operand-liveness from every subsequent re-definition; the codegen would emit `local.get N` on a WASM local that was never written to, producing garbage values. The fix walks all defining instructions and marks all operands live. Strictly more values live → strictly fewer dead instructions eliminated → no correctness regressions possible (only potential minor WASM size increase).

The first change alone is insufficient: my UnboxAnyToI32 MIR instruction was being correctly added for every re-assignment, but DCE was killing all but the first one's dispatch. The two bugs compound: the missing unbox at the MIR-builder level + the DCE bug that wouldn't have manifested before because the unbox was missing.

### Verification

| Test | Before fix | After fix | Expected |
|---|---|---|---|
| `/tmp/minimal_bug.cln` (rewrite path) | 2 iters, len=6 | 3 iters, len=9 | 3 iters, len=9 ✓ |
| `/tmp/minimal_no_rewrite.cln` (matcher bypassed via `acc = "0"`) | 3 iters, len=10 | 3 iters, len=10 | 3 iters, len=10 ✓ |
| `/tmp/loop_no_rewrite_probe.cln` (`c.length()` per iter) | 1, 4, 4 → exit at iter 3 | 1, 1, 1 → exit at iter 3 | 1, 1, 1 ✓ |
| `/tmp/loop_indep.cln` (counter exit, `c` inside loop scope) | 1, 1, 1 | 1, 1, 1 | 1, 1, 1 ✓ |
| `tests/cln/bugfixes/assignment_unbox_any_for_non_any_target.cln` (new regression test) | `1 _ _ _\n3\n2` (3 lengths corrupted) | `1 1 1 1\n3\n2` | `1 1 1 1\n3\n2` ✓ |
| `tests/cln/bugfixes/ssr_concat_in_loop_no_oom.cln` (existing SSR test) | 190 500 2950 110 | 190 500 2950 110 | 190 500 2950 110 ✓ |
| `cargo test --lib --release` | 436/436 | 436/436 | 436/436 ✓ |

### Files changed

- `src/mir/mir_builder/statements.rs` — Added `needs_unboxing` check + `emit_unbox_any` call in the `TastStatement::Assignment` → `Variable` branch (parallel to the existing logic in `VariableDeclaration` at line ~67-90).
- `src/mir/optimization.rs` — Removed `break` in `DeadCodeEliminationPass::mark_live`; added doc comment explaining why MIR is not SSA-form and why all defining instructions must be visited.
- `tests/cln/bugfixes/assignment_unbox_any_for_non_any_target.cln` — New regression test covering the four shapes that exposed the bug: sequential re-assignment with `c.length()` (catches the wrapper-tag-as-length symptom), `while c != ""` driven by `c = json.get(...)` (the SSR pattern), and the rewrite-enabled accumulator (the original failing case).

### Cross-component impact

This is a pure compiler-side fix. No framework, server, or plugin changes required. The user's `/syntax` and `/tutorials` 500 errors should resolve once a release is cut with this fix and the SSR rendering path re-runs against the new compiler.

The four prior false-resolved dashboard entries (`0e3d5cd7`, `fbc8793a`, `80e48890`, `fe33c4bf` from the user's report, plus `27aa9c637433`, `ffae539222ff`, `ff0c62d05604`, and `031d534fb4bd` reopened 2026-06-26) all share this root cause. Future `/resolve-fix CMP-SSR-MALLOC-OOM-PAGE-RENDER <VERSION>` should cite this commit so the resolution loop is auditable.

### Side benefit: the 0.30.365/0.30.366 SSR matcher

The single-RHS string accumulator matcher (`hir_builder.rs::rewrite_string_accumulator_loops`) shipped in 0.30.365/0.30.366 stays in. It IS a real O(n)-from-O(n²) win on the trivial single-RHS pattern, and the new fix makes its iteration semantics correct for the json-driven-condition case (which was the trap that blocked extending it to chained-RHS). The next step is to extend the matcher to chained-RHS (left-spine walking) — that work is no longer blocked by the iteration-count regression.

---

## 🔴 SUPERSEDED: original "CMP-SSR-MALLOC-OOM-PAGE-RENDER 0.30.366 only covers single-RHS" entry (kept for history below)

The diagnosis section below was the WORKING THEORY before the actual root cause was identified. It correctly noted that the SSR rewrite changed iteration count under json-driven conditions, but pursued three wrong hypotheses (heap-sync, JSON-parser inconsistency, scope_pop reclamation) before converging on the right one (missing unbox in Assignment + DCE break-after-first). Kept here for archaeology — the actual fix is documented above.

## 🔴 RE-OPEN: CMP-SSR-MALLOC-OOM-PAGE-RENDER — 0.30.366 fix only covers `acc = acc + x` (single RHS); production uses chained `acc = acc + x + y + …` and is unchanged

**Dashboard fingerprints (all REOPENED 2026-06-26 after user-reported regression on 0.30.366)**: `27aa9c637433`, `ffae539222ff`, `ff0c62d05604`, plus the new report `031d534fb4bd` filed against 0.30.366 with live evidence of unchanged byte counts on `/syntax` and `/tutorials`.

**Last shipped attempt**: 0.30.365 / 0.30.366 (commits `3902422b` and `36636dc5`). Both verified locally against a synthetic SSR Builder repro that uses the trivial `acc = acc + b.render()` form; both passed. Closed on the dashboard 2026-06-26. **Dashboard close was premature** — the synthetic repro did not exercise the actual production pattern. The user observed that `/syntax` and `/tutorials` still 500 with `WASM malloc returned null in string.concat: need 16314 bytes, buffer is 128.0 MB` under 0.30.366, with byte counts essentially identical to 0.30.361.

### Why 0.30.366 didn't reach production

Real-world SSR loops concatenate multiple expressions per iteration. The canonical shape — present verbatim in `app/ui/web/pages/syntax.cln::buildFilterButtons` in Web Site Clean — is:

```clean
string acc = ""
integer i = 0
string catJson = json.get(categoriesJson, "0")
while catJson != ""
    RenderSyntaxCategorySection sec = RenderSyntaxCategorySection(catJson)
    acc = acc + sec.filterButton()
    i = i + 1
    catJson = json.get(categoriesJson, i.toString())
```

The `acc + sec.filterButton()` arm DOES match the 0.30.366 matcher (single RHS = `sec.filterButton()`), but `buildCategorySections` below it has `acc = acc + sec.serve()` where `serve()` internally builds a chained string. And inside frame.ui-expanded components, the body almost always ends up `acc = acc + "<tag>" + value.toString() + "</tag>"` after macro expansion.

A chained `acc = acc + e1 + e2 + e3` parses as the left-fold `(((acc + e1) + e2) + e3)`. The matcher in `hir_builder.rs::analyze_stmt_for_accumulator` checks whether the assignment's `value.left` is exactly `Variable(acc)` — but the outermost `value.left` is itself a `BinaryOp`, not `Variable(acc)`. So the matcher rejects, no rewrite fires, the loop falls back to repeated `string.concat` calls, and the original O(n²) blowup persists. The 0.30.366 SSR test (`tests/cln/bugfixes/ssr_concat_in_loop_no_oom.cln`) only covers the single-RHS form, so the gap wasn't caught locally.

### Why naive chain-fix attempt (today, unshipped) made it worse

A left-spine walker was prototyped on 2026-06-26 to flatten `acc = acc + e1 + e2 + … + eN` into N sequential `__sb = string_builder_append(__sb, eN)` statements. The flatten works correctly in isolation (multiple tests with `while i < N` style conditions produce correct length output).

**But the rewrite interacts catastrophically with loops whose condition is `c != ""` where `c` is updated mid-body by a `json.get` call** — exactly the production SSR pattern. The loop exits 1 iteration early under the rewrite, even though the same source compiles and runs correctly without the rewrite (i.e., on 0.30.364). The mechanism is not yet diagnosed:

- The minimal repro is in `/tmp/minimal_bug.cln` (12 lines, no plugins required): a `while c != ""` loop where `c` is reassigned by `json.get(j, i.toString())` at the end of each iter. With rewrite enabled (single-RHS body `acc = acc + "[X]"`), the loop runs 2 iters and final length is 6. Without rewrite (init `acc = "0"` to bypass the matcher), the loop runs 3 iters and final length is 10. Both versions exhibit a *separate, pre-existing* `json.get` length-corruption symptom: `c.length()` returns 4 instead of 1 (verifiable by adding `print(c.length().toString())` inside the loop on either 0.30.364 or 0.30.366) — but only the rewrite-enabled version truncates the iteration count.
- The pre-existing `c.length()` corruption hits both compiled outputs; only the rewrite changes when the comparison `c != ""` decides to exit. Suspect: a heap-layout interaction between the host-side `allocate_string_in_memory` / `mem_alloc` path used by `string.concat` and `json.get`, vs. the WASM-native `__malloc` path used by `__string_builder_append`. The host path and native path both update `__heap_ptr` correctly in isolation; the timing of when each takes the high-water mark may produce different `__heap_ptr` values during loop-condition evaluation. Not yet confirmed.

The naive chain matcher has been discarded from the working tree pending a real diagnosis. Shipping it would silently change the iteration count of any user loop with a json-driven condition — strictly worse than the OOM, since failures would be silent.

### What's true going forward

- The three dashboard fingerprints `27aa9c637433`, `ffae539222ff`, `ff0c62d05604` are REOPENED (`POST /api/v1/fingerprints/<fp>/reopen`).
- The new report `031d534fb4bd` (filed against 0.30.366 with live `/syntax` and `/tutorials` evidence) stays open under the same error code.
- The 0.30.366 fix DOES help on the trivial single-RHS pattern. Synthetic SSR Builder tests that emit one render-call per iteration produce 134690 chars correctly and no longer OOM. That part of the fix is sound and stays shipped.
- The 0.30.366 fix does NOT help on chained-RHS patterns (the production case). A real fix needs to (a) detect chained RHS via left-spine walking AND (b) not change loop semantics under json-driven conditions.

### Investigation prerequisites before the next attempt

1. **Diagnose the iteration-count delta** in `/tmp/minimal_bug.cln` (or an equivalent self-contained test). Specifically: what is `__heap_ptr` immediately before each `string_compare(c, "")` call in (a) the rewrite-enabled WAT vs (b) the no-rewrite WAT? What is `c` (its pointer value, the 4 bytes at `c+0`, and the bytes at `c+4..c+4+len`)? The host-side `string_compare` reads `c+0..c+3` as length — if those bytes differ between the two compilations, the comparison sees different strings.
2. **Confirm the pre-existing `c.length()` bug** is independent of the rewrite (it appears in 0.30.364 too) and file it as a separate report once the SSR fix is unblocked.
3. **Extend the test fixture** to cover the chained-RHS pattern AND the json-driven-condition pattern BEFORE re-shipping. The 0.30.366 test was insufficient because it only used the trivial form. The new fixture must compile and pass when run end-to-end with `cln` invoked through `wasmtime_runner` AND when run through `clean-server` (the host that originally hit the 128 MB cap) so we catch both the WASM-side bug and any host-bridge interaction.
4. **Cite the four prior false-resolved entries** (`0e3d5cd7`, `fbc8793a`, `80e48890`, `fe33c4bf` per the user's report) in the eventual fix commit so the resolution loop is visible across cycles.

### Investigation update 2026-06-26 — iteration delta is a SYMPTOM of a JSON return-type bug, not a heap-sync issue

Followed step 1 with a temporary trace patch to `src/bin/wasmtime_runner.rs` (revert after — DO NOT ship). Patched `string_compare`, `mem_alloc`, and `allocate_string_in_memory` to dump `__heap_ptr`, the input pointers, the bytes at `[ptr1-16 .. ptr1+32]`, and (for mem_alloc) the pre/post host offsets. Then ran `/tmp/minimal_bug.cln` (rewrite path) and `/tmp/minimal_no_rewrite.cln` (same source with `acc = "0"` to bypass the matcher) both built with 0.30.366.

**The heap-sync hypothesis is RULED OUT.** The trace shows the host-side `NEXT_ALLOCATION_OFFSET` syncs correctly with `__heap_ptr` on every host call. Both versions consume heap monotonically. The rewrite version's extra `string_builder_new` (24 bytes via the WASM-native `__malloc` in `func 159`) shifts subsequent allocations by 24 bytes but does NOT introduce overlap.

**The real bug is in the WASM-internal JSON path-lookup.** `json.get(arr, "N")` returns a pointer that is sometimes a Clean-string pointer and sometimes a JSON-node-wrapper pointer, depending on the index. Concretely, with `j = "[\"a\",\"b\",\"c\"]"`:

| call | pointer returned | bytes at ptr+0..7 | host reads length as |
|---|---|---|---|
| `json.get(j, "0")` | 1048616 | `01 00 00 00 61 00 00 00` | **1 (correct — points at Clean string "a")** |
| `json.get(j, "1")` | 1048864 | `04 00 00 00 08 01 10 00` | **4 (WRONG — pointing at JSON node `{tag=4, ptr=1048840}`)** |
| `json.get(j, "2")` | 1049088 | `04 00 00 00 e8 01 10 00` | **4 (WRONG — pointing at JSON node `{tag=4, ptr=1049064}`)** |
| `json.get(j, "3")` | 0 | n/a | 0 (correct null) |

The bytes `04 00 00 00 <ptr>` are the wrapper struct `[tag=4, inner_ptr]` — the JSON-internal representation of a string. Index 0 is special-cased to return the inner string pointer; indices 1+ return the wrapper. **This is a bug in the JSON array-index extractor.** It is fully reproducible without the SSR rewrite (`/tmp/loop_no_rewrite_probe.cln` is a 14-line repro). `c.length()` returns 4 instead of 1 for indices ≥ 1, on both 0.30.364 and 0.30.366.

**How this manifests as the iter-count delta:**
- Both versions exit the loop only when `string_compare(c, "")` returns 0 (equal).
- In the no-rewrite path, the wrapper bytes at iter 1 are `04 00 00 00 08 01 10 00` → reads as "length 4, content `\x08\x01\x10\x00`" → NOT equal to "" → loop continues.
- In the rewrite path, the heap layout is shifted by 24 bytes (the StringBuilder header), so the wrapper at iter 2 happens to land at a region where the 4 content bytes are `00 00 00 00` → reads as "length 4, content `\0\0\0\0`" → which `from_utf8` decodes as "" → EQUAL to "" → loop EXITS one iter early.
- The pre-existing JSON bug + the rewrite's heap shift combine to produce the user-visible iteration-count delta.

**Where the JSON bug lives** (compiler-side, WASM-internal — `tests/output/minimal_366.wat` decoded):
- `func 239` (json.get entry): `if wrapper.tag == 4 { call 220(wrapper.ptr) } else { wrapper }` → calls `call 238` path navigator.
- `func 238` (path navigator): splits path on `.`, dispatches to `call 237` (array index) or `call 236` (object key) per segment.
- `func 237` (array index): loads `array[i]` from `array+4+i*4` (32-bit slot). Returns the slot value directly unless it equals sentinels 1 or 2 (null/false), in which case it boxes.
- **The bug**: the array slots store inconsistent pointer types. Slot 0 contains a Clean-string pointer; slots 1+ contain wrapper pointers. The parser that writes the slots is in `func 217` and its array-handling helper (probably `func 218` or `func 219`). The fix is to make the parser store one consistent form (likely the inner Clean-string pointer, matching slot 0).

The bug should be filed as a separate `report_error` once the SSR fix can ship — but it is **on the critical path for the SSR fix**, because the rewrite without addressing this bug will silently change iteration counts for any user loop that walks JSON arrays via `json.get(arr, i.toString())`. The chained-RHS extension cannot ship while this trap is live.

### Path forward (revised)

The original "chain matcher" approach is the wrong starting point. Order of work:

1. **First fix the JSON array slot inconsistency in the parser.** Most likely in the array-parsing helper called by `func 217`. The fix should make every array slot store the same pointer type (Clean-string ptr for strings, wrapper ptr otherwise — pick one consistently). This is a pure-WASM compiler-side bug; no host changes needed.
2. **Add a non-loop regression test** that asserts `json.get(j, "1").length() == 1` for a JSON string array. Should fail today, pass after fix #1.
3. **Re-test the rewrite-enabled SSR loop** (`/tmp/minimal_bug.cln`). After fix #1, both rewrite and no-rewrite versions should produce the same iteration count.
4. **THEN extend the matcher to chained-RHS** (the original plan). Safe to do once the JSON bug is closed because the rewrite no longer changes loop semantics.
5. **Add tests for both patterns** to `tests/cln/bugfixes/`:
   - `ssr_concat_chain_no_oom.cln` — chained RHS form.
   - `json_array_get_index_returns_string.cln` — regression for the JSON parser bug.
6. **Ship as a single coordinated release** so dashboard resolution covers the actual user-visible failure (the chained-RHS production form), not just the trivial single-RHS form.

### Minimal repros (kept for the next session)

- `/tmp/minimal_bug.cln` — 12 lines, single-RHS, rewrite fires, loops 2× (wrong).
- `/tmp/minimal_no_rewrite.cln` — same source with `acc = "0"`, rewrite skipped, loops 3× (correct).
- `/tmp/loop_no_rewrite_probe.cln` — adds `print(c.length().toString())` per iter, proves `c.length() = 4` for indices 1+ even without the rewrite.
- `/tmp/prove_json_bug.cln` — sequential `json.get` (no loop) — shows lengths are all correct (1).
- `/tmp/seq_with_alloc.cln` — sequential with allocation between — still correct (1, 1, 1).
- `/tmp/seq_dynamic_path.cln` — sequential with dynamic `i.toString()` paths — still correct (1, 1, 1).
- `/tmp/loop_indep.cln` — **CRUCIAL**: loop with counter exit (`while i < 3`) where `c` is declared *inside* the loop body — produces correct lengths (1, 1, 1).

### Final diagnosis 2026-06-26 — the bug is `mem_scope_pop` reclaiming the heap region a re-assigned outer-scope string points at

The JSON-extractor-returns-wrapper hypothesis is also RULED OUT. `prove_json_bug.cln`, `seq_with_alloc.cln`, `seq_dynamic_path.cln`, and `loop_indep.cln` all return the correct lengths (1) for every index. The JSON parser is fine.

**The bug is specifically about variable scoping inside a loop.** It fires only when:
- A string variable (`c`) is declared in the OUTER scope (before the loop), AND
- That variable is re-assigned inside the loop body to a freshly-allocated heap string (e.g. `c = json.get(...)`), AND
- The loop body otherwise advances the heap (other allocations), AND
- The loop generates a `mem_scope_push` / `mem_scope_pop` pair per iteration.

The codegen pushes a scope mark at the top of each loop iter and pops at the bottom. The pop resets `__heap_ptr` back to the mark, reclaiming everything allocated during the iter. **But `c` still points into that reclaimed region.** The next iter's allocations write over the bytes that `c` points at. By the time `string_compare(c, "")` runs at the top of the next iter, the bytes at `c` have been overwritten — first by the wrapper's `mem_alloc(0,12)` (writing `04 00 00 00 <ptr>`), then by the JSON parser's internal allocations.

This is the SAME class of bug that `body_is_iter_scope_safe` exists to prevent (per the comment in `src/hir/hir_builder.rs:2806`). For the `__sb` builder, the rewrite already declares the outer variable as `String` so the predicate suppresses scope_pop. **For `c = json.get(...)`, no such suppression exists** — the loop body unconditionally runs scope_pop, reclaiming the heap region `c` points into.

**Why the rewrite makes it worse than no-rewrite:**
- In no-rewrite, the `acc = acc + "[X]"` body emits `string.concat` (host alloc), which uses `allocate_string_in_memory` — that DOES sync `__heap_ptr` after the alloc but does NOT prevent the loop's scope_pop from reclaiming everything afterwards. So `c`'s bytes are also reclaimed, but happen to be overwritten with non-zero JSON data → string_compare reads len=4 + non-zero bytes → still != "" → loop continues to iter 3 (where `c=0` returns from json.get).
- In rewrite, the body does `string_builder_append` (pure WASM) which writes into the pre-loop `__sb` buffer (which lives OUTSIDE the per-iter scope region, so survives scope_pop). After scope_pop, `__heap_ptr` resets to the iter-mark. Next iter's `mem_alloc` then lands AT THE SAME OFFSET as the previous iter's wrapper, writing `04 00 00 00 00 00 00 00` (12 bytes, last 4 are still zero because `int_to_string(2)` writes "2" to a different offset). `c` ends up pointing at `04 00 00 00 00 00 00 00` → string_compare reads len=4, content all-zero → from_utf8 returns "" → equal to "" → loop EXITS.
- The 24-byte StringBuilder allocation before the loop ALSO shifts the per-iter mark by 24 bytes, which deterministically lines up the wrapper bytes to produce the all-zero content. Without the rewrite, the offset is different and the bytes that happen to land at `c` are non-zero.

**The real fix lives in the loop scope-management logic, not in the SSR rewrite matcher.** The matcher is correct. The codegen for the loop body must not pop the scope mark for an outer-scope variable that has been re-assigned to a heap pointer during the iter. Either:
1. **Skip scope_pop entirely** if any outer-scope variable was written-to during the iter (conservative; loses arena benefit). OR
2. **Copy the outer-scope variable's heap data to a survivor region** before scope_pop (preserves arena benefit, costs one copy per assignment). OR
3. **Promote any heap value assigned to an outer-scope variable to allocate from a region above the iter mark** (e.g. via `mem_alloc` with the host-side `NEXT_ALLOCATION_OFFSET`, which is not affected by scope_pop). This is what `allocate_string_in_memory` already does, but the host-side advancement is undone by `mem_scope_pop`'s `*NEXT_ALLOCATION_OFFSET = mark`.

Option 3 needs a finer scope semantic: scope_pop should reclaim ONLY allocations made for values that did NOT escape the scope. Tracking escapes precisely is hard, but a simple sound approximation: scope_pop reclaims back to the higher of (a) the iter mark and (b) `__heap_ptr` AT THE LAST WRITE to any outer-scope variable during the iter. This preserves anything that an outer-scope write could reach.

### Path forward (corrected)

1. **First fix the loop scope-pop reclamation bug.** Most likely in:
   - The loop codegen that emits `mem_scope_push`/`mem_scope_pop` — `src/codegen/mir_codegen/` (search for `mem_scope_pop` emission).
   - The `body_is_iter_scope_safe` predicate logic in MIR — extend to detect outer-scope string assignments and either suppress the per-iter scope_pop, or generate a copy.
2. **Add a regression test** matching `/tmp/minimal_no_rewrite.cln` semantics: outer-scope string variable reassigned via `json.get` in a while-loop body, with the loop condition reading the same variable. Should produce 3 iters end-to-end.
3. **After fix #1**, re-test `/tmp/minimal_bug.cln` (rewrite path). Should also produce 3 iters.
4. **THEN extend the matcher to chained-RHS** (the original plan). Safe to do once the scope bug is closed.
5. **Add tests for both patterns** to `tests/cln/bugfixes/`:
   - `ssr_concat_chain_no_oom.cln` — chained-RHS coverage.
   - `loop_outer_string_assignment_survives_scope_pop.cln` — regression for the scope-pop bug.
6. **Ship as a single coordinated release** so dashboard resolution covers the actual production failure.

The scope-pop bug is the gating issue. It almost certainly affects more than just SSR rendering — any user code with `while x != ""` where `x` is updated by a heap-allocating function inside the loop body would hit it. The reason it hasn't surfaced widely is that most such loops are also subject to the SSR rewrite (the same pattern), and the rewrite's heap shift is what makes the corruption deterministic. Without the rewrite, the corruption is heap-layout-dependent and easier to miss.

### Files (unchanged, still shipped, still correct on trivial-form pattern)

- `src/codegen/native_stdlib/string_builder.rs`
- `src/codegen/native_stdlib/mod.rs`
- `src/codegen/codegen_registration.rs`
- `src/resolver/resolver_impl.rs`
- `src/hir/hir_builder.rs` (single-RHS matcher only — left-spine extension reverted)
- `tests/cln/bugfixes/ssr_concat_in_loop_no_oom.cln` (single-RHS coverage only — chained-RHS coverage pending)

### Cross-component impact

None added by 0.30.366. The user's `/syntax` and `/tutorials` server failures continue to surface in the website's render path until the chained-RHS gap is closed. Until then, the website team should not rely on the dashboard's `fixed_in_version` field as a signal of production resolution.

---

## ✅ RESOLVED: BUILTIN-NAMESPACE-OVERREACH — closed on dashboard in 0.30.301

**Dashboard fingerprint**: `cf5bbd0c6c55bd307278e32b9e61f85cb25ff23e970e572e31661c9adf55c192` — resolved 2026-06-16, commit `224ae553`.
**Cross-component plan (now resolved)**: `foundation/management/cross-component-prompts/resolved/framework-builtin-namespace-overreach-sub-a-finalize.md`

All umbrella sub-findings are closed for the dashboard ticket. Architectural residue is documented below as separate items that no longer block this ticket.

| Sub-finding | Status | Owner |
|---|---|---|
| Sub-B (register_http_server_wrappers) | ✅ Done in 0.30.288 — dead code deleted (~180 LOC) | compiler |
| Sub-D-MCP-1 (BuiltinRegistry) | ✅ Done in 0.30.289 — deleted, MCP rewired to SymbolTable (~1670 LOC) | compiler |
| Layer-3 prefix classifier (utilities.rs:1665-1669) | ✅ Done in 0.30.289 — bridge_functions lookup replaces hardcoded prefixes | compiler |
| Sub-A (resolver req.*/auth.*/db.*/crypto.*/env/time/now/http.setCache/noCache) | ✅ Done in 0.30.296 + 0.30.301 — framework v2.12.29 supplies declarations, compiler deletes the duplicates | compiler |
| Sub-B-rest (register_http_imports server bridges) | ✅ Subsumed by Sub-A — http.setCache/noCache moved to frame.server plugin.toml in 0.30.301 | compiler |
| Sub-C (register_db_builtin_wrappers) | ✅ Done in 0.30.296 + 0.30.301 — db.* entries now come from frame.data plugin.toml | compiler |

## 🟡 OPEN: Architectural residue from BUILTIN-NAMESPACE-OVERREACH (not blocking the umbrella dashboard ticket)

| Residue | Why kept | Next move |
|---|---|---|
| `crypto.sha256` / `crypto.sha512` in resolver | Registry aliases are `crypto.hash_sha256`/`crypto.hash_sha512`; resolver entries reference nonexistent bridge names but `utilities.rs:1755` `explicit_reachable` redirects to `_crypto_hash_*`. Mismatch papered over, not fixed. | Needs spec decision — rename registry aliases to `crypto.sha256`/`crypto.sha512` (breaks any caller using `crypto.hash_sha256`), or update resolver entries to match the existing aliases (breaks any caller using `crypto.sha256`). |
| `_req_*` and `_server_sleep` raw bridges in resolver | Framework-generated code calls bridges directly, bypassing the language→bridge alias map | Either rewrite how the framework emits these, or accept the impurity |
| `http.redirect`, `http.setHeader`, `http.head`, `http.options`, `http.encodeUrl`, `http.decodeUrl`, `http.buildQuery`, `file.*` | No plugin declarations exist anywhere; resolver entries reference bridges that may not exist | Either bridge implementations land in clean-server + clean-node-server + function-registry.toml (then add plugin declarations), or remove the resolver entries (and accept the user-facing break) |
| Sub-D-MCP-2 (MCP plugin lists in src/mcp/server.rs) | Compiler-only; biggest remaining LOC win | Needs runtime plugin loading design — at MCP startup, iterate loaded plugins and synthesize the per-plugin example lists instead of hardcoding them |
| Sub-E (runtime/host_functions.rs stubs) | Compiler-only | Needs WASM-import-section inspection (extract bridge signatures from loaded .wasm at runner startup, register zero-value stubs for each missing import). Bigger lift than expected; deferred. |

**Directions to continue** (when next picking this up):

### Step 1 — Decide whether to do compiler-only pieces ahead of cross-component

Two compiler-only pieces are ready to ship without waiting on framework:
- **Layer-3 prefix classifier** (`src/codegen/mir_codegen/utilities.rs:1665-1669`): hardcodes `"_req_"`, `"_res_"`, `"_session_"`, `"_auth_"`, `"_http_"` prefix strings to filter Layer 3 calls from import-name collection. Replace with `self.bridge_functions.iter().filter(|f| f.hosts.as_deref().map_or(false, |h| h == ["server"]))` — or move the layer-classification into the plugin manifest (the cleaner long-term answer).
- **Sub-E runtime stubs** (`src/runtime/host_functions.rs:2599-2666`): test-only wasmtime stubs. If a `PluginRegistry` handle is available at runner setup, iterate `bridge_functions` to register zero-valued stubs from manifest signatures. Falls back to current hardcoded list when registry is absent.

### Step 2 — Cross-component coordination (when ready)

The biggest reductions are blocked on plugin.toml updates in `clean-framework`:
- For Sub-A: every `[[functions]]` entry with `maps_to` needs explicit `params` and `returns` Clean Language types (see Path B proposal §Step 2). Without these, removing the resolver's hardcoded entries degrades `req.body()` from `String` to `Any`.
- For Sub-C: frame.data needs `{ name = "db.begin", maps_to = "_db_begin" }` (and commit/rollback) so `language_to_bridge_map` carries the alias, making `register_db_builtin_wrappers` skippable.

**Framework prompt filed 2026-06-16**: `foundation/management/cross-component-prompts/framework-builtin-namespace-overreach-sub-a-finalize.md`. Enumerates the exact remaining plugin.toml additions after the v2.12.26 slice landed (`db.query`/`db.execute`, `crypto.*`, `env.get`, `time.now`/`now`, `http.setCache`/`http.noCache`). When the framework lands these and a new framework version ships, the compiler-side cleanup is: delete the matching `add_builtin_namespace_functions` entries in `src/resolver/symbol_table.rs:1222-1293`, switch `http.setCache`/`http.noCache` return type from `Integer` to `Boolean` to match the bridge, and rerun the architecture_boundaries lint (the `src/resolver/symbol_table.rs` EXEMPT_FILES entry can be removed when nothing in Sub-A still hardcodes).

### Step 3 — Sub-D-MCP-1: delete BuiltinRegistry (compiler-only, biggest LOC win)

`src/builtins/registry.rs` (~1744 LOC) duplicates language-builtin signatures already present in `src/resolver/symbol_table.rs`. Only consumer is `tool_list_builtins` in `src/mcp/server.rs:2587`. Refactor:
1. Replace the MCP tool's iteration over `BuiltinRegistry::new()` with iteration over `SymbolTable::new().all_symbols()` filtered by `is_builtin`.
2. Accepted feature regression: MCP loses `BuiltinCategory` filter (Math/String/etc.) and per-class static/instance distinction.
3. Delete `BuiltinRegistry` struct + methods. Keep `BuiltinType` (used in 7 files outside `registry.rs`).
4. Delete dead `BridgeFunction::to_builtin_function()` (only used by `BuiltinRegistry`'s own tests).

### Step 4 — Lint cleanup after each landing

After each sub-finding lands, remove the corresponding `EXEMPT_FILES` entry from `tests/architecture_boundaries.rs`. Each entry has a `KNOWN VIOLATION (BUILTIN-NAMESPACE-OVERREACH, sub-finding X)` comment matching the sub-letter above.

### Step 5 — Resolve on dashboard

`/resolve-fix BUILTIN-NAMESPACE-OVERREACH <VERSION>` only when **all** EXEMPT entries tagged with this fingerprint have been removed. Partial work doesn't close the dashboard ticket.

**Risk notes**:
- Sub-A and Sub-C are tempting to do compiler-only with no plugin.toml updates, but doing so causes silent type degradation (`Any` instead of `String`/`Boolean`). The hardcoded entries exist specifically because plugin manifests don't declare language-level types today. Don't delete them in isolation.
- The Layer-3 prefix classifier is the only piece where compiler "owns" the architectural decision (which prefixes are Layer 3). Even the cleanest refactor still encodes that knowledge somewhere — moving it to plugin.toml's `hosts` field is the right answer.

---

## 🟡 IN PROGRESS: VISIBILITY-FLIP — Private-by-default visibility model (compiler landed, framework migration pending)

**Priority**: CRITICAL — Principle 24 violation resolved for compiler-internal scope; remains open for the framework/plugin ecosystem.
**Discovered**: 2026-06-25
**Compiler implementation landed**: 2026-06-26
**Reported error code**: VISIBILITY-FLIP
**Cross-component**: spec is shared; compiler change is local; .cln migrations span every component.

### Status snapshot (2026-06-26)

| Layer | Status |
|---|---|
| Spec files (Clean_Language_Specification.md, EBNF, semantic-rules, type-system, ast.md) | ✅ Landed 2026-06-25 |
| Compiler lexer (`TokenKind::Public`) | ✅ Landed 2026-06-26 |
| Compiler AST (`Function`/`Field` defaults flipped to `Visibility::Private`) | ✅ Landed 2026-06-26 |
| Compiler parser (`public:` sections, default-Private for plain decls) | ✅ Landed 2026-06-26 |
| Compiler resolver/typechecker (SEM005 enforcement) | ✅ Already structurally correct; works under flipped flags |
| HIR builder (lowers `Visibility::Private` → `is_private: true`) | ✅ Already correct; no edit needed |
| Compiler test suite (`cargo test --lib`) | ✅ 431/431 passing |
| Compiler test suite (8 .cln fixtures migrated to `public:` syntax) | ✅ Migrated 2026-06-26 |
| Compiler test suite (`test_sym_collision_synthetic_builtins`) | ✅ Test source migrated, all tests in file passing |
| MCP `get_quick_reference` class examples | ✅ Updated 2026-06-26 |
| **Cross-component: frame.ui plugin emits user-callable classes (HYDRATE_AUTO)** | ❌ Blocker — see §"Framework blocker" below |
| Cross-component: clean-framework `.cln` examples migration | ⏳ Pending (separate session) |
| Cross-component: Studio, clean-errors, Web Site Clean `.cln` migration | ⏳ Pending (separate session) |
| Books 1–5 class examples | ⏳ Pending (separate session) |
| VS Code extension (`public` add, `private` keep-or-deprecate) | ⏳ Pending (separate session) |
| Lexer cleanup: remove `TokenKind::Private` once no consumers remain | ⏳ Pending (after all migrations) |
| Parser cleanup: remove `parse_private` top-level block once no consumers remain | ⏳ Pending (after all migrations) |

### Framework blocker — frame.ui emits classes whose methods the framework calls externally (HYDRATE_AUTO)

After the compiler-side flip, two integration tests fail:
- `tests/test_hydrate_auto_e2e.rs::client_init_splice_reaches_start_body`
- `tests/test_hydrate_auto_e2e.rs::event_handlers_exported_by_bare_name`

**Symptom**: both compile a `component:` block via the frame.ui plugin and then attempt to invoke the synthesized class methods (e.g. `MyToolbar.render()`) from the framework's spliced code. Under the new spec these calls are SEM005 violations because the synthesized class has no `public:` block.

**Diagnosis**: this is NOT a compiler bug. The plugin output is now spec-non-conforming. The framework's `expand_component` (and presumably `expand_page`, `expand_screen`) macros emit user-shaped classes without wrapping the framework-callable methods in a `public:` section.

**Resolution path**: the frame.ui (and any other framework plugin that synthesizes classes which the framework itself calls) must be updated to emit `public:` sections around the methods that participate in the framework dispatch contract. Specifically:
1. `expand_component` must wrap the synthesized `render` method, lifecycle methods (`onMount`, etc.), and any event handler whose name is referenced from `events:` in a `public:` method section.
2. `expand_page` / `expand_screen` need the same treatment for their respective lifecycle methods.
3. Component instance fields that the framework reads from outside the class (e.g. `instance_my_toolbar.handler`) must live in a `public:` field section, or the framework must reach them via a `public:` getter.

**Compiler-side mitigation**: NONE. Per `.claude/rules/compiler-work.md` "No Plugin Logic in the Compiler" and ARCHITECTURE_BOUNDARIES.md "Workaround Trap", the compiler will not bypass SEM005 for plugin-synthesized classes. The fix lives in `clean-framework/plugins/frame.ui/`.

**Cross-component prompt filed**: see `foundation/management/cross-component-prompts/framework-visibility-flip-public-sections-in-component-expansion.md` (to be authored alongside this commit).

**Test status**: the two failing `test_hydrate_auto_e2e.rs` tests gate on `frame_ui_available()` and skip silently when the plugin is not installed. Locally they fail when a frame.ui is present (any version up to and including 2.12.68). When the framework lands public-section emission in `expand_component`, these tests should turn green automatically. Until then, expect a red signal on dev machines with frame.ui installed.

### Compiler implementation details (landed 2026-06-26)

Files touched:

| File | Change |
|---|---|
| `src/lexer/specification_token.rs` | Added `TokenKind::Public` variant + Display/keyword/source-string mappings. `TokenKind::Private` retained as reserved-but-unmatched. |
| `src/ast/mod.rs` | `Function::new()` and `Field::new()` / `Field::new_with_default()` now default `visibility` to `Visibility::Private`. |
| `src/parser/token_parser/declarations.rs` | `parse_private_functions_section` → `parse_public_functions_section`, matches `TokenKind::Public`, sets `Visibility::Public` on contents. Class-body inline section (was `TokenKind::Private`) now matches `TokenKind::Public` and sets `Visibility::Public`. `parse_function`, `parse_function_in_block`, `parse_field`, `parse_field_name_colon_type` now produce `Visibility::Private` by default (overridden inside `public:` sections). `parse_start_function` and `parse_event_handler_in_block` keep `Visibility::Public` (entry-points / plugin contract). |
| `src/parser/token_parser/blocks.rs` | `parse_private_state_section` → `parse_public_state_section`, matches `TokenKind::Public`, marks contents `is_private: false`. Default state declaration now `is_private: true`. |
| `tests/cln/spec_compliance/classes/private_class_members_spec.cln` | Rewritten using `public:` blocks; kept original filename for spec-test discovery. |
| `tests/cln/spec_compliance/functions/private_functions_spec.cln` | Rewritten using `public:` block inside `functions:`. |
| `tests/cln/spec_compliance/statements/state_private_section_spec.cln` | Rewritten using `public:` block inside `state:`. |
| `tests/cln/language/classes/91_inline_private_visibility.cln` | Rewritten using `public:` blocks. |
| `tests/cln/future/91_inline_private_visibility.cln` | Same migration (duplicate of above). |
| `tests/cln/advanced/modules/53_import_export_blocks.cln` | Rewritten; removed top-level `private:` block; moved exported functions into a `public:` sub-section of `functions:`. |
| `tests/cln/advanced/modules/67_import_export_comprehensive.cln` | Same as above. |
| `tests/cln/examples/54_integration_test.cln` | Same as above. |
| `tests/test_sym_collision_synthetic_builtins.rs` | Migrated inline `class Item` source to use `public:` blocks. |
| `src/mcp/server.rs` | `get_quick_reference()` class examples rewritten to use `public:` sections. Added a short "Visibility model" section before the inheritance example. |

Intentional non-edits (to be cleaned up in a follow-up after no remaining consumers):
- `src/parser/token_parser/declarations.rs::parse_private` (top-level `private:` block) — kept as dead code path so any unmigrated `.cln` files in the wild still parse. Remove once cross-component migration is done.
- `src/parser/grammar.pest` `private_block` rule — same rationale.
- `TokenKind::Private` enum variant — same rationale.
- `src/parser/token_parser/mod.rs::TokenKind::Private =>` dispatch arm — same rationale.

### Background

The Clean Language specification was updated on 2026-06-25 to flip the visibility default for class fields, class methods, module functions, and module state. The new rule is:

> Members are **private by default**. The `public:` sub-section header (replacing the old `private:` sub-section) is the sole mechanism for making a member visible outside its declaring scope. There is no per-member visibility modifier.

**Spec changes landed in this commit** (single source of truth — implementation must follow):

- `Clean_Language_Specification.md` — Classes and Objects section (§"Visibility: Private by Default"), Visibility Model section, all class examples migrated to `public:` blocks. `private` removed from the reserved-keyword list; `public` added.
- `foundation/spec/grammar.ebnf` — Five productions renamed and inverted:
  - `private_block` → REMOVED (top-level visibility now lives inside `functions:` only).
  - `private_functions_section` → `public_functions_section`.
  - `private_class_fields_section` → `public_class_fields_section`.
  - `private_class_methods_section` → `public_class_methods_section`.
  - `private_state_section` → `public_state_section`.
  - Keyword `"private"` removed from the reserved set; `"public"` added.
- `foundation/spec/semantic-rules.md` — SEM005 rewritten (private-by-default model), CLASS007 added (at most one `public:` block per scope), CLASS008 added (override must match parent visibility), CLASS001–CLASS008 range update.
- `foundation/spec/type-system.md` — new "Member Visibility and Resolution" subsection in §4 documents the resolution algorithm.
- `foundation/spec/ast.md` — `Visibility` enum semantics flipped (default is `Private`), `Function.visibility` and `Field.visibility` descriptions updated.

### Compiler work required (this component)

1. **Lexer (`src/lexer/specification_token.rs`)**: register `public` as a reserved keyword token. Optionally remove `private` if no other syntax depends on it.
2. **Parser (`src/parser/token_parser/declarations.rs` and `src/parser/grammar.rs`)**:
   - Rename and invert all five `private_*` productions to match the EBNF: `public_functions_section`, `public_class_fields_section`, `public_class_methods_section`, `public_state_section`.
   - Remove the top-level `private_block` parser path (§6.7).
   - Default-emit `Visibility::Private` for every declaration; set `Visibility::Public` only for declarations that appear inside a `public:` sub-section.
   - Enforce CLASS007 at parse time (at most one `public:` block per enclosing scope).
3. **AST (`src/ast/mod.rs`)**: confirm `Visibility::Private` is the default for `Function`, `Field`, and module/state declarations. No structural enum change; only default flip.
4. **HIR validation (`src/hir/validation.rs`)**: enforce CLASS007 (single public block) and CLASS008 (override matches parent visibility). Carry visibility through HIR lowering unchanged.
5. **Resolver (`src/resolver/resolver_impl.rs`)**: SEM005 enforcement against the new default. The check at member-access time becomes: "if `member.visibility == Private` and the access site is outside the declaring scope, emit SEM005." Subclass-resolution must skip private parent members rather than inheriting them.
6. **Typechecker (`src/typechecker/type_inference.rs`)**: only relevant if visibility affects type resolution (it should not — visibility is a SCOPE/SEM concern, not a TYPE concern).
7. **Codegen (`src/codegen/mod.rs`)**: no runtime impact — visibility is fully checked at compile time. No WASM changes expected.
8. **Existing tests that assume the old `private:` section model** must be migrated:
   - `tests/cln/spec_compliance/classes/private_class_members_spec.cln` → rename to `public_class_members_spec.cln`, rewrite to declare private-default fields with `public:` exposing the methods. Update grammar-rule citation in header comment.
   - `tests/cln/spec_compliance/functions/private_functions_spec.cln` → migrate similarly for module-level functions.
   - `tests/cln/spec_compliance/statements/state_private_section_spec.cln` → migrate for `state:`.
   - `tests/cln/language/classes/91_inline_private_visibility.cln` → migrate.

### Ecosystem `.cln` migration required (separate sessions)

Every `.cln` file in the ecosystem that defines a class with externally-called methods (or a module with externally-called functions) needs review. Without a `public:` block, the file will fail to compile under the new model — every existing class becomes opaque to its callers.

Estimated scope (from the surface-area survey):
- `clean-language-compiler/tests/cln/` — ~50+ files with class declarations.
- `clean-framework/examples/` — all example projects.
- `clean-framework/plugins/frame.*/src/` — plugin source files with classes.
- `clean-framework/tests/framework/unit/plugins/` — framework tests.
- `Clean Studio/app/logic/` — Studio codebase.
- `clean-errors/app/` — error tracking app.
- `Web Site Clean/app/logic/` — website backend.
- Books 1–5 — every class example.
- `clean-language-compiler/src/mcp/server.rs` `get_quick_reference` — embedded examples.
- `clean-extension/syntaxes/clean.tmLanguage.json` — keyword highlighting (`public` add, `private` remove).

Total `.cln` impact: ~150 files (rough estimate from the surface-area report, 2026-06-25).

### Migration mechanics for `.cln` files

The mechanical rewrite is:

1. Find every `class` declaration.
2. Wrap fields that are read from outside the class (or that the example clearly intends to expose) in a `public:` field sub-section at the position they currently occupy in the class body.
3. Inside each `functions:` block, wrap methods that are called from outside the class in a `public:` method sub-section.
4. Delete every `private:` sub-section header — its members are already private by default, so the section becomes redundant. Indentation of its contents drops one level.
5. For module-level `functions:` blocks in files that are imported elsewhere, wrap the exported functions in a `public:` block. Helper functions stay outside (private by default).

Judgment calls remain (deciding which method is "really" part of the API vs. an implementation accident), so this is not a pure search-and-replace.

### Acceptance criteria

- `cargo test --lib` and `cargo test --test integration` pass.
- All `.cln` files in `tests/cln/` compile.
- The renamed `public_class_members_spec.cln` test verifies that:
  - private fields/methods inside a class are NOT accessible from `start:`.
  - public fields/methods (inside `public:` blocks) ARE accessible from `start:`.
- `tests/test_dual_naming_imports.rs` continues to pass (this change is orthogonal to bridge-import dual-emission).
- A new compiler test verifies CLASS007 (single `public:` block per scope) and CLASS008 (override matches parent visibility).
- MCP `get_quick_reference` returns class examples using the new `public:` syntax.

### Risks / open questions

- **`private` keyword removal**: the EBNF still has scattered references in identifier-NOT predicates and example comments. The grammar's reserved keyword list dropped `private` and added `public`. The compiler's lexer must match. If any currently-released `.cln` file in the wild uses `private` as a regular identifier (unlikely — it was reserved), this would silently start parsing differently. Mitigate by surveying all known `.cln` files before flipping the lexer.
- **Inheritance interaction with CLASS008**: the spec says an override of a public parent method must itself be public. This is enforceable at HIR validation. There is no implicit-override; the developer must literally place the overriding method inside the child's `public:` block.
- **`state:` migration**: the spec flipped `state:` too. Any framework or app code that relies on importing a state variable from another module must add the variable to the `public:` state sub-section. Survey before flipping.

### Suggested order of execution (multiple sessions)

1. **Session A (compiler core)**: lexer + parser + AST default + HIR validation + resolver SEM005. Run `cargo test --lib`. Expect ~10–20 existing tests to fail; migrate those `.cln` test fixtures alongside the code change.
2. **Session B (compiler tests + MCP)**: migrate remaining `tests/cln/` files. Update `src/mcp/server.rs::get_quick_reference()` and `get_ecosystem_catalog()`. Run full `cargo test`.
3. **Session C (extension + framework)**: VS Code extension keyword update. Cross-component `.cln` example survey for clean-framework — coordinate with framework owner via `report_error` or cross-component prompt.
4. **Session D (other components)**: Studio, clean-errors, Web Site Clean. Each owns its own .cln files; treat as cross-component (file `report_error` or cross-component prompt for each).
5. **Session E (books)**: Books 1–5 chapter-by-chapter rewrite. This is documentation, not code — can lag the implementation.

### Reference / authority

- Prose spec section: `Clean_Language_Specification.md` §"Classes and Objects" → §"Visibility: Private by Default" and §"Visibility Model".
- Grammar: `foundation/spec/grammar.ebnf` §6.2a, §6.4a, §6.4b, §6.8a.
- Semantic rules: `foundation/spec/semantic-rules.md` SEM005, CLASS007, CLASS008.
- Type resolution: `foundation/spec/type-system.md` §4 "Member Visibility and Resolution".
- AST: `foundation/spec/ast.md` `Visibility` enum.

---

## ✅ COMPLETED: BUILD_FRONTEND — `cln build` does not generate `frontend.wasm` for client-side components

**Priority**: HIGH (priority 56)
**Discovered**: 2026-06-09
**Completed**: 2026-06-09
**Reported error code**: `BUILD_FRONTEND`
**Files**:
  - `src/lib.rs` — added `client_mode` parameter to `compile_multi_file_with_memory_tier`,
    new `compile_multi_file_client_mode` wrapper, `is_server_only_module` helper
  - `src/main.rs` — `handle_build` now scans the project for `events:` blocks and
    emits `frontend.wasm` as a sibling of the main output when present; also
    threads the new `client_mode: false` argument to existing call sites
  - `tests/test_client_mode_build.rs` — regression test that asserts the client mode
    pipeline succeeds for a minimal component-bearing project

**Issue**: `cln build` only produced `dist/app.wasm` for projects with `target: web` + a
component declaring `events:`. The browser loader (`loader.js`, served by clean-server at
`/loader.js`) expects a sibling `frontend.wasm` that contains only the client-side WASM,
per `foundation/spec/plugins/frame-ui-semantics.md` §UI-B009. Without it the loader returns
404 and hydration never runs.

**Fix**: After the main compile, the build CLI scans for any `events:` block in the project
tree (the spec signal for client hydration). When present, it runs a second compilation pass
in client mode that:
  - Drops server-only modules from the HIR merge (paths under `/server/`, `/backend/`,
    `/api/`, `/pages/`, the `routes` module, and the synthetic `__page_routes_generated`)
  - Replaces the `start:` body with an empty no-op so the browser's `_start()` does not
    attempt to call `_http_listen` or other server bridge imports
  - Guarantees a `_start` export exists even when the entry module is server-side
Component classes and their `events:` handler functions remain as exports for the
`_ui_on_event` runtime registrations to reach.

**Limitation**: this is a path-heuristic split, not a full client/server reachability
analysis. Server functions called from shared (non-server) modules will still appear in
`frontend.wasm`. A follow-up task tracks proper function-level dead-code elimination keyed
on bridge namespace.

---

## ✅ COMPLETED: CODEGEN-TIME-NOW-DEREF — time.now() causes unreachable trap in integer arithmetic

**Priority**: HIGH (priority 106)
**Discovered**: 2026-06-05
**Completed**: 2026-06-05
**Files**: `src/resolver/resolver_impl.rs`

**Issue**: `time.now()` triggered an WASM unreachable trap when its result was used in arithmetic.
The frame.auth plugin declared `time.now` with `Any` return type, shadowing the compiler's
built-in `Integer` return type. The resolver registered the `Any`-typed alias, causing the
code generator to emit a spurious `UnboxAnyToI32` sequence that trapped.

**Fix**: Added a guard in `register_language_function_aliases` in `resolver_impl.rs` to skip
plugin aliases that degrade a known builtin's return type from a concrete type to `Any`.

---

## ✅ COMPLETED: SYN001/compiler — ORM insert: block as statement fails with "Unsupported statement type: Assign"

**Priority**: HIGH (priority 56)
**Discovered**: 2026-06-05
**Completed**: 2026-06-05
**Files**: `src/plugins/wasm_adapter.rs` (`call_expand` function), `tests/cln/plugins/orm_insert_no_binding.cln` (new test)

**Issue**: `User.insert:` blocks used as statements (without a variable binding) caused a
"Unsupported statement type: Assign" error. The `call_expand` function always assumed the
first line of block content was a variable binding header of the form `type name =`.
For `insert:` blocks, all lines are field assignments (`field = value`) — there is no binding header.

**Fix**: Detect binding header by counting whitespace-separated tokens before the first `=`
on the first line. Two tokens (e.g. `list<User> rows =`) → binding header present.
One token (e.g. `name =`) → field assignment, no binding. When no binding, the entire block
content is passed as the body and the query expression is emitted directly as a statement.

---

## ✅ COMPLETED: FRAME-UI-SHARED-LOGIC-BREAKS-SSR — frame.ui in shared file breaks SSR template variables

**Priority**: HIGH (priority 56)
**Discovered**: 2026-06-05
**Completed**: 2026-06-06
**Files**:
  - `src/compilation/multi_file_compiler.rs` (Pass 2 assemble hook logic)
  - `src/plugins/registry.rs` (`has_wasm_assemble_hook()` method + regression test)

**Issue**: Declaring `plugins: frame.ui` in a shared logic file (e.g. `email.cln`) broke SSR
template variable substitution for all pages. The `{greeting}` placeholder was emitted
literally instead of being substituted at runtime.

**Root cause**: Two conflicting assemble hooks both transformed page companion files:
1. The builtin Rust `PageCompanionAssembler` shim in `builtin_assemblers.rs`
2. The frame.ui WASM plugin's own `assemble` export (present since frame.ui 2.6.11)

Both produced `TransformedSource` entries for the same page companion file paths.
The WASM plugin's output was merged on top of the shim's output via `.extend()`,
resulting in function bodies shifted by one index slot in the final WASM binary.
The exported `pages_home_render` function ended up pointing to the wrong function body
(`render_email` from email.cln instead of the frame.ui-generated SSR wrapper).

**Fix**: Added `has_wasm_assemble_hook()` to `PluginRegistry` that checks whether any
loaded plugin manifest declares an `assemble` export. In `multi_file_compiler.rs` Pass 2,
the builtin `PageCompanionAssembler` shim is now skipped entirely when a WASM plugin
already handles assembly. The shim is retained as a fallback for older frame.ui versions
that predate the `assemble` export.

**Regression test**: `test_has_wasm_assemble_hook_detects_plugin_assemble_export` in `registry.rs`

---

## ✅ RESOLVED: COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS — landed 0.30.334

**Dashboard fp**: `f80ee96ce507` — close after release.
**Mitigation**: 0.30.333 (plugin-call epoch deadline → actionable trap + backtrace instead of silent hang).
**Root cause fix**: 0.30.334 (two MIR → WASM control-flow lowering bugs in `src/codegen/mir_codegen/`).

**What the root cause turned out to be**: not the plugin loop the dashboard reporter suspected, not the `mem_alloc` grow path the frame.ui in-source comment (`plugins/frame.ui/src/main.cln:4033-4040`) accused, and not the recently-removed per-iteration `mem_scope_pop`. It was two independent codegen bugs that silently dropped statements from the emitted WASM for two specific MIR shapes:

1. **`collect_jump_targets` stopped at the innermost merge.** For an if/else whose both branches contained another if/else, the function reported the inner merge blocks instead of chasing through them to the eventual common continuation. The trailing statement (e.g. `remaining = remaining.substring(brace_end + 1, remaining.length())` in `process_text_node`) was therefore not inlined — `generate_branch_block`'s `generated.contains(...)` check short-circuited and dropped it.
2. **`is_continuation_not_else` mistook `else: break` for "no else clause".** An `else: break` body is an empty `false_block` with a Jump terminator targeting the loop's exit. The check fired the no-else shortcut, the entire else arm was skipped, and the loop ran forever because nothing ever broke it. Surfaced as the second hang inside `find_unescaped_quote` after fix 1 landed.

Both fixes live in `src/codegen/mir_codegen/control_flow.rs` (`chase_jump_chain`, `collect_jump_targets`, `is_continuation_not_else`) and `src/codegen/mir_codegen/blocks.rs` (`generate_branch_block`). Regression fixtures live at `tests/cln/control/conditionals/08_stmt_after_nested_if_else.cln` and `tests/cln/control/loops/else_break_inside_while.cln`; the Rust gate is `tests/test_codegen_nested_control_flow.rs`.

**Bonus bug found during the investigation (still open)**: a string literal containing `{identifier}` (e.g. `"hello {x}"`) is silently parsed as a string interpolation and collapses to an empty string when the identifier is undefined. The plugin source doesn't trigger it (no such literals), so it did not contribute to the page-project hang — but it is a real lexer/parser bug worth filing if you stumble on it again.

---

## 🟢 OPEN: MCP-DOCS-HARDCODED-PATHS — `get_app_structure` / `get_quick_reference` documentation strings still cite `app/web/...`

**Priority**: LOW
**Discovered**: 2026-06-15 (while implementing manifest-driven folder discovery)
**Files**: `src/mcp/server.rs` lines ~3476, ~3478, ~3487, ~3495, ~3604, ~3606, ~3608, ~3642, ~3671, ~3672, ~3914-3916, ~3934, ~3939

**Issue**: After the `[paths].owns`-driven discovery landed, the compiler no longer
hardcodes folder names — but the long-form documentation strings returned by MCP tools
`get_app_structure` and `get_quick_reference` still describe `app/web/pages/`,
`app/web/components/`, `app/web/layouts/` etc. as canonical locations. AI assistants
reading these strings will continue to recommend the legacy layout even after
frame.ui ships its `app/ui/web/...` migration.

**Fix options** (per cross-component prompt §"Files to change" / `src/mcp/server.rs`):
1. Remove the path examples and describe the layout conceptually (`pages folder`,
   `components folder`) without nailing down a path.
2. Fetch the path examples from `PluginRegistry::loaded_manifests()[name].paths.owns`
   at MCP tool invocation time. Requires plumbing a registry handle into the tool
   handlers (currently they're stateless).

**Why deferred**: option 2 needs a registry handle in MCP tool context that doesn't
exist today; option 1 is a documentation rewrite that's best done in coordination
with the framework's `app/ui/web/...` rollout (so the docs flip from old to new
without a transient "everything stripped" state).

**Cross-component anchor**: `foundation/management/cross-component-prompts/compiler-app-ui-render-target-nesting.md` §"Files to change" — this entry tracks the deferred half of that prompt.

---

## 🟢 OPEN: SYNC-LIST-PUSH — Redundant `list.push` host import shadowed by native local

**Priority**: LOW
**Discovered**: 2026-06-04 (during `/audit sync`)
**Files**: `src/stdlib/list_ops.rs:28-33`, `src/codegen/codegen_registration.rs:362-371`

**Issue**: `src/stdlib/list_ops.rs::register_functions()` registers `list.push` as a host
import via `register_import_function("env", "list.push", ...)`. Separately,
`codegen_registration.rs` registers a native local WASM function `__list_push_i32` and
aliases `list.push` to its index. The native path resolves first at call-site, so the
host import is never reachable.

Import Minimality currently tree-shakes the redundant import — no shipping fixture in
`tests/output/` imports `list.push` (only `list.push_f64` ships, which has a distinct
f64-typed signature and no native equivalent). So this is not a runtime bug; it is dead
declaration code that confuses readers and inflates the static-emittable set.

**Fix**: remove the `list.push` host import registration in `list_ops.rs:28-33`. Keep
`list.push_f64` (that one has no native counterpart). Verify no test regression after
removal — Import Minimality should produce identical .wasm output.

**Why not done in the audit run**: removing it touches stdlib registration ordering and
deserves its own focused change with a dedicated test, not a bulk audit-time edit.

---

## 🟡 OPEN: SYNC-PLUGIN-DRIFT — Plugin.toml signatures disagree with function-registry.toml (cross-component)

**Priority**: MEDIUM-HIGH (CRITICAL severity at the framework component; this entry is the compiler-side bookkeeping)
**Discovered**: 2026-06-04 (during `/audit sync`)
**Reported**: framework component — fingerprint 150b6140d1b8777aa3baa4969b05dc5128797bbb9d2e0ffc04b2221aeaef02d4
**Tracking**: https://errors.cleanlanguage.dev/errors/detail?fp=150b6140d1b8777aa3baa4969b05dc5128797bbb9d2e0ffc04b2221aeaef02d4

107 `[bridge]` declarations in `clean-framework/plugins/*/plugin.toml` disagree with
`foundation/platform-architecture/function-registry.toml` (51 ptr/string convention drift +
56 typed mismatch). Per-plugin breakdown saved at `/tmp/plugin_drift_grouped.txt`
during audit run.

**Compiler-side action**: none required — the compiler emits WASM imports based on the
registry, not plugin.toml. This entry exists so the compiler component knows the drift
is reported and to re-run `/audit sync` after the framework fix lands to verify drift
drops to 0.

---

## 🟡 OPEN: SYNC-NODE-HOST-MISSING — 16 bridge registrations missing from clean-node-server

**Priority**: MEDIUM-HIGH (CRITICAL at node-server; compiler-side bookkeeping)
**Discovered**: 2026-06-04 (during `/audit sync`)
**Reported**: node-server component — fingerprint d68fd0e1a8650967581a9034db6d7955d3b9e14923a43fd887464c560405ffc3
**Tracking**: https://errors.cleanlanguage.dev/errors/detail?fp=d68fd0e1a8650967581a9034db6d7955d3b9e14923a43fd887464c560405ffc3

clean-node-server is missing 16 bridges that clean-server registers and shipping fixtures
import: `_async_fire`, `_auth_can`, `_auth_get_session`, `_auth_has_any_role`,
`_auth_require_auth`, `_auth_require_role`, `_db_execute`, `_db_query`, `_http_set_cookie`,
`_server_sleep`, `_session_delete`, `_session_get`, `_session_store`, `_state_reset_all`,
`_state_reset_named`, `list.push_f64`.

**Compiler-side action**: none. Re-run `/audit sync` after the node-server fix to verify.

---

## 🟡 OPEN: SYNC-CANVAS-STUBS-MISSING — Canvas host stubs missing from both server hosts

**Priority**: MEDIUM-HIGH (HIGH at server + node-server; compiler-side bookkeeping)
**Discovered**: 2026-06-04 (during `/audit sync`)
**Reported**:
  - clean-server — fingerprint 4362655f2a6a496757c7458dfead904ce27ad8997309e3a0bcb2844d780e1404
  - clean-node-server — fingerprint e046452c1286be26beddf7fe92280dc759a54ce07cc5c021dc9fad6f5841cd8c

5 `_canvas_*` imports currently appear in shipping fixtures with no host-side stubs.
frame.canvas declares 238 bridges in plugin.toml; the complete fix stubs all 238 to
prevent future regressions as more canvas tests/examples are added.

**Compiler-side action**: none.

---

## 🟢 OPEN: SPEC-EBNF-GENERIC — Clarify generic_type / type_parameters scope in grammar.ebnf

**Priority**: LOW
**Discovered**: 2026-06-03 (during `/audit compiler grammar`)
**Files**: `foundation/spec/grammar.ebnf` lines 200-203, 689

**Issue**: The EBNF defines `generic_type`, `type_arguments`, `type_parameters`, and `type_parameter`
as if user-level generics with `<T>` are valid syntax (e.g. `function_in_block` references
`function_return_type` which could include them). The prose specification explicitly states
"no angle brackets in user code (`<>`) — these are internal representations" and instructs
users to use `any` for generic behavior. The compiler parser correctly does NOT accept
user-written `<T>` parameters (sets `type_parameters: vec![]` unconditionally in
`src/parser/token_parser/declarations.rs:57,478`).

The ambiguity: `list<integer>`, `matrix<number>`, `pairs<K,V>` are built-in parametric types
that DO use `<>` at the source level. The EBNF's `generic_type` rule (`identifier "<" type_arguments ">"`)
was likely intended only for these built-ins, but reads as general user syntax.

**Resolution needed (developer judgment)**:
  - Option A: Remove `generic_type`, `type_parameters`, `type_parameter` productions entirely,
    keep only the special-cased `list_type`, `matrix_type`, `pairs_type` rules (which is the
    current implementation reality).
  - Option B: Add a comment to each of those productions clarifying they describe internal
    representation only and are not valid user syntax outside the three built-in container types.
  - Option C: If user generics are planned future work, leave the productions and move the EBNF
    to a "future syntax" section.

Requires developer approval before editing the spec (Principle 25). No test impact — no
existing tests use user-level generic type parameters.

---

## 🟡 OPEN: SPEC-OPS-SAFE-NAV — `?.` (safe navigation) and `??` (none-coalescing alt) not implemented

**Priority**: MEDIUM-HIGH
**Discovered**: 2026-06-04 (during compiler-wide spec audit)
**Spec ref**: `foundation/spec/type-system.md` §8 — Operator 2 (`??`, lines 220–226) and Operator 4 (`?.`, lines 248–256)

**Issue**: The type-system spec defines two none-handling operators that have no tokenizer/parser/AST support:

- `?.` — Safe navigation: `patron.address?.city?.toUpperCase()`. Propagates `none` through a chain of field accesses and method calls without trapping. Spec text: "Regular `.` on a `none` value is a runtime trap (RUN004). `?.` is the safe alternative whenever an intermediate value may be absent."
- `??` — None-coalescing alternate syntax: `string name = a ?? b ?? "Anonymous"`. Spec text: "Syntactically equivalent to `default`. Use `??` when both sides are expressions of similar length and `default` would read awkwardly."

**Implementation gap**: No tokens, no AST nodes, no parser rules, no type-inference handling, no codegen.

**Workaround currently available**:
  - `??` → use `default` keyword (implemented and spec-compliant)
  - `?.` → no workaround; users must restructure code or accept the runtime trap risk

**Suggested ordering when implemented**:
  1. Add `?.` and `??` tokens to the Pest grammar (`src/parser/grammar.pest`)
  2. AST nodes: `SafeFieldAccess`, `SafeMethodCall` (or extend existing access nodes with an `is_safe` flag); `??` desugars to `default`
  3. Type inference: `?.` chain wraps the result type as none-able and short-circuits on the first `none` intermediate
  4. Codegen: `?.` chain emits a guard check before each access; `??` is a straight desugar to `default`
  5. Tests: `tests/cln/operators/safe_nav_*.cln`, `tests/cln/operators/none_coalesce_alt.cln`

No spec change needed — implementation must catch up to the spec.

---

## ✅ DONE: SPEC-ERRCODE-SYN100-101 — Validate-block error codes not in error-codes.md

**Priority**: MEDIUM
**Discovered**: 2026-06-04 (during compiler-wide spec audit)
**Resolved**: 2026-06-04

SYN100 and SYN101 formally added to `foundation/spec/error-codes.md` and `foundation/spec/semantic-rules.md` with precise meanings.

---

## ✅ DONE: SPEC-LIST-BEHAVIOUR-RUNTIME — `.unique` / `.line` / `.pile` fully runtime-enforced

**Priority**: LOW
**Discovered**: 2026-06-04 (during compiler-wide spec audit)
**Resolved**: 2026-06-04

Full runtime enforcement implemented:
- `ListBehavior` enum extended with `to_flags()` method (bit flags: LINE=0x01, PILE=0x02, UNIQUE=0x04)
- Parser produces `Type::List(T, ListBehavior)` — behavior propagates through AST
- HIR builder injects `name.setFlags(flags)` call after every list declaration with non-Default behavior
- New `list.setFlags(ptr, flags_i32)` function registered in all pipeline layers (builtins, GlobalSymbolTable, WASM codegen)
- `list.add` WASM rewritten with real O(n) duplicate scan for UNIQUE flag
- `list.pop`/`removeLast`/no-arg `remove` rerouted to behavior-aware `list.remove` (FIFO/LIFO dispatch)
- Verified by `tests/cln/spec_compliance/types/list_behavior_enforcement.cln`: pile LIFO=3, unique size=2, line FIFO=10

---

## 🟢 OPEN: SPEC-VALIDATOR-NS — `validator` namespace not exposed as a callable namespace

**Priority**: LOW
**Discovered**: 2026-06-04 (during compiler-wide spec audit)
**Spec ref**: `foundation/spec/stdlib-validator.md` (referenced in audit; not yet read in full for this entry)

**Issue**: The `validate:` block DSL works (parser → HIR → codegen complete). What's not exposed is a programmatic `validator.run(...)` / `validator.create(...)` namespace API documented in `stdlib-validator.md`. Users can validate via the DSL but cannot invoke validators dynamically.

**Resolution paths**:
  - Option A: Implement the validator namespace as a builtin (parallel to `Math.*`, `String.*`). Useful for libraries that need to validate input at runtime against user-supplied schemas.
  - Option B: Mark the namespace as deferred in the spec and document that `validate:` blocks are the supported entry point today.

No DSL-side regression — only the programmatic API is missing.

---

## ✅ COMPLETED: CODEGEN001 — Object literals `{ key: value }` compiled to `return 0`

**Priority**: CRITICAL
**Discovered**: 2026-06-02
**Completed**: 2026-06-02 — fixed in v0.30.217
**Files**: `src/typechecker/type_inference.rs`, `src/mir/mir_builder/expressions.rs`, `src/codegen/mir_codegen/operands.rs`
**Root cause**: `infer_literal_expression` forwarded `Value::Pairs` to `infer_literal` which returned
  `(TastLiteral::Null, ConcreteType::Unknown)` — the object literal was erased to `i32.const 0`.
  `TastExpressionKind::ObjectLiteral` also had no handler in the MIR builder (fell to error).
  `load_string_argument_for_print` had no `MirType::Any` case, treating boxed values as raw strings.
**Fix**: (1) `infer_literal_expression` intercepts `Value::Pairs` → `ObjectLiteral` w/ `ConcreteType::Any`;
  (2) MIR builder `ObjectLiteral` handler allocates raw JSON-format object and boxes as `AnyTypeTag::Object`;
  (3) `load_string_argument_for_print` adds `MirType::Any` branch via `emit_any_to_string` dispatch.

---

## ✅ COMPLETED: frame.data plugin compatibility — SYN001 + SEM003

**Priority**: CRITICAL — ORM/data plugin tests cannot compile
**Discovered**: 2026-05-22
**Completed**: 2026-06-02 — fixed in frame 2.10.90; both tests compile and produce valid WASM
**Files**: `tests/cln/bugfixes/syn001_orm_query_in_function.cln`, `tests/cln/bugfixes/sem003_orm_variable_bound_in_function.cln`
**Fix**: clean-framework team fixed expand_block logic in frame.data for complex join/where queries
  and ORM result type propagation in frame 2.10.90.
**Resolved**: 2026-06-02 — SYN001 fp `812a478e`, SEM003 fp `f3f43fcd`

---

## ✅ COMPLETED: P02 — string.matches compile-time pattern ID resolution

**Priority**: MEDIUM-HIGH
**Discovered**: 2026-05-20
**Completed**: 2026-06-02 — compiler emits 3-param call (str_ptr, str_len, pattern_id); server bridge updated
**Fix**: Compiler side implemented in `src/mir/mir_builder/expressions.rs` (pattern→ID at compile time)
  and `src/codegen/codegen_registration.rs` (3-param import + wrapper). Server bridge updated to match.
**Resolved**: 2026-06-02 — BRIDGE001 fp `8d790e29`

---

## ✅ COMPLETED: Endpoint test codegen — test.http_request bridge

**Priority**: MEDIUM-HIGH
**Discovered**: 2026-05-28
**Completed**: 2026-06-02 — compiler side fixed in 0.30.213; server bridge added by server team
**Fix**: Registered `_test_http_request`, `_test_response_status`, `_test_response_body` as builtins
  in resolver (`src/resolver/resolver_impl.rs`) so endpoint test blocks pass name resolution.
  Codegen bridge registration already existed in `src/codegen/codegen_registration.rs`.
**Resolved**: 2026-06-02 — BRIDGE002 fp `c8a02821`

---

## ✅ COMPLETED: P10 — Concurrency violation static analysis (CONC001/CONC002)

**Priority**: LOW — was missing structured error codes for concurrency rule violations
**Discovered**: 2026-05-20
**Completed**: 2026-06-01

Implemented static analysis in `src/hir/validation.rs`:

- **CONC001**: `background:` expression that directly reads or writes a module-level state
  variable is now flagged at HIR validation time. The `ValidationContext` tracks all
  state variable names from the top-level `state:` block and sets `inside_background = true`
  while recursing into `HirStatement::Background`. Any `Variable` reference or
  `Assignment` targeting a state var inside that context emits CONC001.

- **CONC002**: Any `NamespaceCall` or `MethodCall` on a request-context namespace
  (`req`, `res`, `session`, `auth`) that occurs outside a recognised request handler is
  flagged as CONC002. Request handlers are identified by: function name starting with
  `__route_handler_`, a parameter named `req` or `request`, or a parameter of type `Request`.
  The `inside_request_handler` flag is set when entering such functions.

**Note on error codes**: The existing `foundation/spec/semantic-rules.md` defines COM002 as
  "Optimization Error" and COM006 as "Function Not Found During Compilation" — both already
  have different meanings. New codes `CONC001` and `CONC002` were used to avoid conflicting
  with the spec (Principle 25: no spec changes without developer approval). The developer
  should add a CONC range to `foundation/spec/semantic-rules.md` to formalise these codes.

**Files changed**: `src/hir/validation.rs`
**Tests added**: 6 new unit tests in `hir::validation::concurrency_tests`
  — all pass (`cargo test --lib`: 344 passed)

---

## ✅ COMPLETED: Refactor — move page-companion assembly logic out of compiler into framework plugin hook

**Completed**: 2026-06-02 — shipped in v0.30.214.
**What was done**:
  - Added `assemble()` hook to `FrameworkPlugin` trait (default no-op)
  - Added `AssembleInput`, `AssembleOutput`, `AssembleSourceFile`, `InjectedSource`, `TransformedSource` to `plugin_abi.rs`
  - Added `PluginRegistry::run_assemble_hooks()` to call all registered plugin assemblers
  - Created `src/plugins/builtin_assemblers.rs` with `PageCompanionAssembler` Rust shim containing the 6 migrated functions
  - Removed 6 hardcoded functions and `PageCompanionRecord` from `multi_file_compiler.rs`
  - `build_from_file()` now routes through the hook; WASM plugins that export `assemble` will also participate
  - Cross-component report PLUGIN001 filed for frame.ui to implement WASM `assemble` export
  - All 346 tests pass

---

## ✅ COMPLETED: Split oversized functions for readability

**Completed**: 2026-06-01 — all 8 functions split, all tests green (346 passing).

| Function | File | Before | After |
|----------|------|--------|-------|
| `setup_linker` | `src/plugins/wasm_adapter.rs` | ~2370 | 15 lines, 11 helpers |
| `generate_parse_object_instructions` | `src/stdlib/json_class.rs` | ~1258 | 13 lines, 5 helpers |
| `register_method_style_functions` | `src/runtime/host_functions.rs` | ~1175 | 11 lines, 7 helpers |
| `peek_has_orm_subclauses` | `src/parser/token_parser/blocks.rs` | ~1146 | 17 lines, 3 helpers |
| `resolve_expression_internal` | `src/resolver/resolver_impl.rs` | ~1059 | 102 lines, 18 helpers |
| `parse_private_state_section` | `src/parser/token_parser/blocks.rs` | ~1026 | 108 lines, 2 helpers |
| `new_with_default` | `src/ast/mod.rs` | ~1012 | Added 11 named builders |
| `infer_expression` | `src/typechecker/type_inference.rs` | ~944 | 145 lines, 18 helpers |

---

## ✅ COMPLETED: padStart / padEnd proper WASM implementation

**Completed**: 2026-06-01
**File**: `src/stdlib/string_class.rs` — `generate_pad_start` and `generate_pad_end`
Implemented full WASM instruction sequences: early-out if `str_len >= width`, allocate `4 + width`
bytes via `__malloc`, cyclically fill pad bytes, copy original string, return new pointer.
Falls back to returning original string if malloc is unavailable or width already satisfied.

---

## ✅ RESOLVED: COVERAGE-INT64-LITERAL-LEXER — Lexer now accepts the full i64 range

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-25 in 0.30.358

The lexer at `src/lexer/specification_lexer.rs:read_number_literal` already calls
`i64::from_str_radix` / `i64::from_str` (lines 1018, 1043, 1068, 1248-1257, 1326), so the
underlying capacity was always there. The earlier rejection observed in the `/coverage` pass
was a stale-build artifact — re-running on a clean compile of `tests/cln/future/integer_64_edges.cln`
shows `9223372036854775807` parses successfully.

The companion bug COVERAGE-INT64-LITERAL-TRUNCATION (codegen-side truncation) remains open
below — fixing the lexer alone does not produce correct runtime values.

---

## ✅ RESOLVED: COVERAGE-INT64-LITERAL-TRUNCATION — Sized integers collapse to base Integer in HIR→Concrete conversion

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-26 in 0.30.362
**Repro test**: `tests/cln/bugfixes/integer_64_literal_roundtrip.cln`

`integer:64 maxv = 9223372036854775807; print(maxv.toString())` now correctly outputs
`9223372036854775807` instead of `-1`. Values in the u32 range (e.g. 3000000000) and
beyond (e.g. 4294967296) also round-trip correctly.

**Fix landed**:
1. `hir_type_to_concrete` (instance method) now maps `HirType::Integer64` →
   `ConcreteType::IntegerSized{bits:64, unsigned:false}` (and Integer64u → unsigned: true).
   8/16/32-bit integers continue to collapse to `Integer` since they fit in i32.
2. New native `gen_int64_to_string` (src/codegen/native_stdlib/type_conversions.rs)
   emits a parallel of `gen_int_to_string` using i64 arithmetic. Registered as
   `__int64_to_string` with alias `int64_to_string` in `codegen_registration.rs`.
3. `int64_to_string` registered in `function-registry.toml`, builtin resolver
   (`register_builtin_fn`), and wasmtime_runner (linker stub).
4. `convert_value_to_string` (mir_builder/types.rs) and a new EARLY-handler in
   `expressions.rs` route `IntegerSized{64}.toString()` to `int64_to_string` instead
   of narrowing through `int_to_string(i32)`.
5. Cross-component bridge gap reported against clean-server and clean-node-server —
   they must register `int64_to_string` for production WASM to load.

**Original analysis (kept for reference)**:

```clean
integer:64 over32 = 4294967296    // lexer + parser preserve i64 value correctly
print(over32.toString())          // expected: "4294967296"; actual: "0"
```

**Root cause** (deeper than originally traced): the typechecker's `hir_type_to_concrete`
at `src/typechecker/type_inference.rs:5778-5797` collapses **every** sized integer
(`HirType::Integer8/16/32/64`) and sized number (`HirType::Number32/64`) to the base type
`ConcreteType::Integer` / `ConcreteType::Number`. Width information is lost at the variable
declaration step — by the time the MIR builder runs, `integer:64 x = ...` looks identical to
`integer x = ...`. As a result every codegen path downstream sees i32 and emits i32.

Note the static helper `Self::hir_type_to_concrete_type` at line 980 of the same file
already preserves `IntegerSized`/`NumberSized` correctly — the instance method at 5778 is a
divergent copy that drops the precision modifier.

**Why a one-line fix in `hir_type_to_concrete` is insufficient** (validated in 0.30.358):
fixing line 5778 alone surfaces a cascade of missing arms throughout the pipeline:
- `src/mir/mir_builder/types.rs::convert_value_to_string` (line 299) has arms for `Integer`,
  `Number`, `Boolean` but `IntegerSized` falls through to "use as-is", breaking `.toString()`
  in `print(x.toString())`.
- `src/mir/mir_builder/types.rs::mir_type_to_concrete` (line 410) has no `I64` arm — it
  maps every pointer-ish thing through `Ptr(I8)→String` etc., but `MirType::I64` falls
  through to a generic case.
- `int_to_string` host signature is declared in `src/codegen/mir_codegen/utilities.rs:446`
  as parameterless `create_builtin_signature("int_to_string", 5, Ptr(I32))` — the real WASM
  import is `(i64) → i32` but the MIR signature table doesn't reflect that.
- `MirConstant` arithmetic (Add, Sub, Mul, etc.) emits i32 ops; mixing i64 operands triggers
  WASM validation errors.

**Infrastructure landed in 0.30.358** (ready for the broader refactor):
- `MirConstant::Integer64(i64)` variant added at `src/mir/mir_types.rs:524-530`.
- `load_constant` handles `Integer64` → `Instruction::I64Const` at
  `src/codegen/mir_codegen/operands.rs:86-89`.
- `get_operand_mir_type` returns `MirType::I64` for `Integer64` at
  `src/codegen/mir_codegen/utilities.rs:59`.
- `convert_literal` and `convert_literal_type` accept `expr_type: &ConcreteType` and route
  to the appropriate width at `src/mir/mir_builder/types.rs:209-265`.
- `widen_literal_to_declared_type` helper at `src/typechecker/type_inference.rs:4232-4256`
  retags literal expressions to match a 64-bit declared destination, ready to fire once
  the HIR→Concrete conversion preserves the precision modifier.

**Full fix plan** (when ready to take this on):
1. Change `hir_type_to_concrete` at line 5778 to mirror `hir_type_to_concrete_type` at
   line 980 — emit `IntegerSized`/`NumberSized` for sized variants. (Or unify the two
   functions to a single source.)
2. Extend `convert_value_to_string` to handle `IntegerSized { bits: 64 }` (emit
   `int_to_string` call with i64 arg, signature already matches).
3. Add an `I64 → Integer` arm in `mir_type_to_concrete`.
4. Update `convert_value_to_string` and other type-bridging helpers to handle the wider
   variants. Likely also need updates in:
   - `print` codegen (load_string_argument_for_print)
   - Binary-op codegen (i64 add/sub/mul/div instead of i32)
   - Type-cast instructions for narrowing (`Cast` with `i32.wrap_i64` when storing i64 into
     a smaller slot).
5. Update the host signature table to reflect `(i64) → i32` for `int_to_string`,
   `print_integer`, etc.

Each step is mechanical but the cascade is wide. Tests for each step should live alongside
`tests/cln/future/integer_64_edges.cln`.

---

## ✅ RESOLVED: COVERAGE-BASE-CONSTRUCTOR-NOT-PROPAGATED — `base(arg)` now propagates correctly

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-25 in 0.30.358

**Root cause**: NOT in the BaseCall lowering itself — args were always being passed to the
parent constructor correctly. The bug was in `build_function_body`'s auto-return path:
constructors have non-void return type (the class type), so when the body's last statement
was an `Expression { expression: BaseCall }`, the auto-return logic at
`src/mir/mir_builder/functions.rs:164` overwrote the implicit "return this" with the void
result of the base call. The subsequent `ensure_function_termination` saw the block already
had a `Return` terminator and skipped the constructor-specific "return this" injection in
`src/mir/mir_builder/helpers.rs:132-141`.

**Fix**: detect constructor functions (`class_context.is_some() && name == "constructor"`)
and skip the auto-return path. The implicit "return this" from `ensure_function_termination`
then handles the return value correctly.

**Diff**: `src/mir/mir_builder/functions.rs:162-172` — added `is_constructor` check before
deriving `has_non_void_return`.

**Test**: `tests/cln/spec_compliance/classes/class001_parent_must_exist.cln` (promoted out
of `future/`) — `Dog("Rex").speak()` now prints "Rex barks" as expected. CI tier-3
`t3_class_inheritance` and the rest of the class test suite remain green.

---

## ✅ RESOLVED: COVERAGE-RESET-STATEMENT-NOOP — `reset <name>` now lowers to inline assignment

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-26 in 0.30.362
**Repro test**: `tests/cln/bugfixes/reset_state_named_restores_initial.cln`

The earlier classification ("not a compiler bug") was wrong. Every host stubbed
`_state_reset_named` / `_state_reset_all` as a no-op, so the feature was completely broken
across the ecosystem — there was no working implementation anywhere. The fix lives in the
compiler because it's the only component that knows the initializer expression at compile
time.

**Fix landed**:
- `HirBuilder` gained a `state_initializers: HashMap<String, Expression>` map populated
  in `build_hir` before functions are lowered (so the map is ready by the time any
  `ResetStmt` in `start:` is visited).
- The `Statement::ResetStmt` arm for `ResetTarget::Variable(name)` now rewrites to
  `HirStatement::Assignment { target: name, value: <initializer-expr> }` directly,
  bypassing the host bridge entirely. The fallback to `_state_reset_named` is kept for
  the case where the name doesn't match a known state declaration (so semantic analysis
  still surfaces the right diagnostic).
- `reset state` (the all-state form) still falls back to `_state_reset_all` because
  `build_statement` can only return a single `HirStatement` and unrolling the multi-
  assignment requires a richer return shape. Tracked as a follow-up.

The `_state_reset_*` host stubs remain (compiled WASM still imports them via Layer-2
boilerplate) but are no longer load-bearing for `reset <name>`.

---

## ✅ RESOLVED: COVERAGE-STRING-INTERP-METHODCALL — Interpolation now accepts any expression start

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-25 in 0.30.358

**Root cause**: The lexer's "is this an interpolation?" heuristic at
`src/lexer/specification_lexer.rs:570` required the first non-whitespace character inside
`{...}` to be `is_alphabetic() || == '_'`. Strings like `"{1.toString()}"`, `"{(x + 1)}"`,
and even `"{1}"` started with a digit/paren and fell through to literal-brace handling —
the braces and their contents were emitted verbatim.

**Fix**: extend the heuristic to accept any character that can begin a `logical_expression`:
digit, `(`, `-`, `+`, `!`, `[`, `"`, in addition to the existing alphabetic/underscore.
The downstream inner-scan rules (rejecting `:`, `;`, `#`, `@`, `$`, `?`, and top-level `,`)
still veto DSL/glob strings like `"{a,b,c}"` or `"color: { family: Inter, weight: 700 }"`,
so the fix doesn't introduce new false positives.

**Diff**: `src/lexer/specification_lexer.rs:570-582` — replaced single-char check with
named boolean `is_expression_start`.

**Test**: `tests/cln/spec_compliance/expressions/string_interpolation_edges.cln` updated
to use the inline method-call form `"{1.toString()}-{2.toString()}-{3.toString()}"` (was
working around the bug with pre-computed bindings).

---

## ✅ RESOLVED: COVERAGE-SPEC-DRIFT — Test files cited semantic codes not in `semantic-rules.md`

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-25

Developer decision: keep the `(tracking: …)` parenthesized form as the long-term convention.
The 14 affected test files were renormalized to cite existing codes from
`foundation/spec/semantic-rules.md` with the original tracking ID preserved in parens. The
convention is now documented in `tests/UNIFIED_TESTING_STRATEGY.md` §4 "Test Header Citation
Convention". Future recurring tracking IDs that justify formal codes should be raised here
for developer approval before being added to the spec.

Tracking IDs encountered and normalized: `SEM-COMPARE-01` → `SEM001`, `SEM-CTRL-01` →
`FUNC004`, `FUNC-BUILTIN` → `FUNC001`, `SEM-JSON-ENCODE-TRAP` → `RUN002`, `CODEGEN001` →
`COM001`, `RUNTIME002` → `RUN002`.

---

## ✅ RESOLVED: COVERAGE-PARITY-ITERATE — false positive

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-25

`iterate_statement` and `range_iterate_statement` ARE present in
`foundation/spec/grammar.ebnf` at lines 542–550 and match the Pest grammar exactly. The
original `/coverage` report flagged this as a parity gap based on an extraction false
positive. No spec change required.

---

## ✅ RESOLVED: COVERAGE-STDLIB-LAYERING — `stdlib-reference.md` restructured into Part A / Part B

**Discovered**: 2026-06-25 (via `/coverage` spec gap pass)
**Resolved**: 2026-06-25

Developer decision: move categories 9–12 + 14 under a "Part B — Plugin- and Server-Provided
Functions" section header in `foundation/spec/stdlib-reference.md`. Categories 1–8 + 13
remain in Part A as Layer-1 language built-ins. The spec now explicitly maps each Part B
category to its provider component and test location, removing the ambiguity that made the
coverage picture confusing.

---

## /audit residue — 2026-06-25

A full spec audit (compiler section) found 45 candidate gaps; most turned out to be
audit false positives where the grep-based search couldn't locate the emission site.
Truly-actionable items are listed below.

### ✅ Fixed in this session

| Item | Fix | File |
|------|-----|------|
| Audit #1 (SEM003) | function/class redefinition now tagged with code | `hir/validation.rs:215-258`, `resolver/resolver_impl.rs:219` |
| Audit #2 (FUNC008–11) | new `semantic_error_with_code()` helper; all 10 emission sites updated | `error/mod.rs:472`, `hir/hir_builder.rs` (10 sites) |
| Audit #5 (Array.get instance method) | added to `inferred_method_return_type` | `typechecker/type_inference.rs:4975` |
| Audit #6 / #12 / #13 (Integer/Number conversion methods) | toInteger/toNumber/toBoolean added on both Integer and Number; Number.toBoolean + identity conversions added to MIR codegen | `typechecker/type_inference.rs:4908-4925`, `mir/mir_builder/expressions.rs:2180-2240` |
| Audit #8 (SEM008) | inheritance cycle now tagged with code | `hir/validation.rs:464` |
| Audit #13 (Array.isNotEmpty) | added to `inferred_method_return_type` | `typechecker/type_inference.rs:4972` |

### 🟢 OPEN — truly-missing items

#### AUDIT-26 — `list.push` returns `Void`, spec says `list`

Stdlib reference `foundation/spec/stdlib-reference.md:235` declares `.push(item) → list`
(chainable), but the resolver signature in `src/resolver/symbol_table.rs:910` returns Void.
Codegen treats it as a void statement throughout. Changing it to return the receiver list
would enable `xs.push(a).push(b)` chaining but risks breaking every site that uses
`xs.push(x)` as a statement.

**Decision needed (developer)**: keep Void and amend the spec, or change to chainable and
audit every existing call site for breakage.

#### AUDIT-3 — grammar.pest `list_behavior` only handles single modifiers

`grammar.pest:65` defines `list_behavior = { "." ~ ("line" | "pile" | "unique") }`. The
spec EBNF (`grammar.ebnf:181-187`) allows canonical ordering combinations
`.line.unique.pile`. The token parser (`token_parser/declarations.rs:1611-1622`) correctly
handles all 8 ListBehavior variants, so user-facing parsing already works. Only the pest
grammar file is out of sync.

**Decision needed (developer)**: update grammar.pest to express the combinations, or
formally mark grammar.pest as non-authoritative (the token parser is the real parser).
Same root cause covers AUDIT-4, AUDIT-23, AUDIT-24, AUDIT-29, AUDIT-31, AUDIT-32 — all of
which are pest-only gaps that the token parser already handles.

#### AUDIT-25 — generic parser errors not tagged with SYN002 / SYN004 / SYN005

Parser errors for unexpected-token, unterminated-construct, and malformed-construct are
emitted but they all carry the default `SYN001` code. To fix, classify each error site in
`src/parser/token_parser/` and `src/lexer/` and pick the right code.

**Effort**: ~1 day. Each call site to `CompilerError::syntax_error` needs review.

#### AUDIT-9, 10, 11 — `math.random`, `math.sign`, `file.lines`

Listed in `stdlib-reference.md` as built-ins but absent from the compiler registry. These
are Layer-2 (I/O) functions. Per the `BUILTIN-NAMESPACE-OVERREACH` refactor (≥0.30.289),
all Layer-2 functions belong in plugin `[bridge]` declarations, not the compiler. Adding
them to the compiler would reverse that decision.

**Decision needed (developer)**: confirm these should move to a plugin (probably
`frame.server` for `math.random`/`math.sign`, `frame.server` or a new `frame.fs` for
`file.lines`). The spec then needs a note marking them plugin-provided.

#### AUDIT-38–45 — EXTRA items in code, absent from spec

All of these need a "remove from code OR add to spec" decision and explicit approval per
Principle 25:

- **#38** `validator.*` namespace (~15 functions in `resolver/symbol_table.rs:1138-1222`)
- **#39** `StringUtils_*` legacy aliases (length/concat/substring/indexOf/replace)
- **#40** `list.fill`, `list.setFlags`, `list.removeLast`, `list.peek` — `setFlags` is the
  runtime carrier for ListBehavior and likely an INTENTIONAL hidden function; others may
  also be intentional
- **#41** Input function underscore variants coexist with dot form
- **#42** `FUNC012` (method-call on standalone function) — implemented in
  `error/mod.rs:571`, no entry in `semantic-rules.md`
- **#43** `FUNC008–FUNC011` named-arg codes — in code and in `error-codes.md`, but not in
  `semantic-rules.md` (which still cuts off at FUNC007)
- **#44** Compiler hardcodes `crypto.sha256`/`crypto.sha512` while other `crypto.*` are
  delegated to plugin — inconsistent boundary
- **#45** Cosmetic EBNF naming differences (`if_stmt` vs `if_statement` etc.); not
  worth fixing unless we standardise the whole grammar.pest naming convention

### 🔵 Audit corrections (false negatives — already implemented)

These items appeared in the audit's MISSING list but a code search confirms they are
implemented. No action needed; recorded for future audit-tool calibration.

| Audit item | Spec code | Actual location |
|---|---|---|
| #7 | SEM010 | `mir/mir_builder/expressions.rs:1672-1714` (string.matches compile-time check) |
| #14 | SYN007 | `parser/token_parser/mod.rs:188-217` |
| #15 | SYN008 | `parser/token_parser/blocks.rs:427-437` |
| #16 | SCOPE004 | `typechecker/type_inference.rs:1217-1256` |
| #17 | SCOPE005 | `hir/mod.rs:34-80` + resolver enforcement |
| #18 | FUNC006 | `hir/validation.rs:425` |
| #19 | CLASS004 | `hir/validation.rs:505` (warning, not error) |
| #20 | CLASS005 | `hir/validation.rs:643` + tests at 1601-1646 |
| #21 | CLASS006 | (see validation.rs; verified during audit) |
| #22 | STATE004 | `typechecker/type_inference.rs:2802` |
| #27 | IMPORT004 | `hir/validation.rs:366` |
| #28 | COM002 / COM006 | `error/enhanced_hierarchy.rs:294,298` |
| #30 | FUNC005 | `hir/validation.rs:709` |
| #33 | BLD-LAYOUT | `compilation/multi_file_compiler.rs:820` (the format is in the spec at line 685, just non-standard) |
| #35 | STATE005 | `typechecker/type_inference.rs:2194` |
| #36 | FUNC007 | `hir/validation.rs:435` |

### 🟢 Approved-but-deferred — work items with decisions captured 2026-06-25

The developer reviewed each of these and gave a directional answer; the work itself is
scheduled for dedicated future sessions.

#### AUDIT-9, AUDIT-10, AUDIT-11 — math.random / math.sign / file.lines as bridge declarations

**Decision (2026-06-25)**: Add back to compiler as Layer-2 bridge declarations only.

**Scope**: Add three rows to `src/resolver/symbol_table.rs` registering the dot-form names
with their bridge targets. Bridges referenced:

| Clean Language name | Bridge name | Notes |
|---|---|---|
| `math.random` | `_math_random` | takes nothing, returns `number` |
| `math.sign` | `_math_sign` | takes `number`, returns `number` (−1.0/0.0/1.0) |
| `file.lines` | `_file_lines` | takes `string` (path), returns `list<string>` |

Host implementations must land in clean-server, clean-node-server and (where applicable)
the browser bridge before these names will work at runtime. Filing a separate
`server-` / `node-server-` prompt is recommended.

**Risk**: Adding declarations without host implementations means code compiles but traps
at runtime. Mitigation: add a `_TODO: missing host` note in the symbol_table entry until
the bridges land.

#### AUDIT-25 — tag parser/lexer errors as SYN002 / SYN004 / SYN005

**Decision (2026-06-25)**: Fix now (this session). **Status**: deferred to a dedicated
session — the session-scope check at end of `/audit` flagged this as ~2-3 hours of
mechanical work touching every `CompilerError::syntax_error` call site.

**Scope**:
- Search every call to `CompilerError::syntax_error(` in `src/parser/token_parser/` and
  `src/lexer/`. For each, classify:
  - Lexer unterminated string / comment / block → switch to a new helper
    `syntax_error_with_code(_, _, _, "SYN004")`
  - Parser unexpected-token (saw X expected Y) → `"SYN002"`
  - Parser malformed-construct (saw partial valid structure) → `"SYN005"`
- Add the helper to `src/error/mod.rs` next to `semantic_error_with_code`.
- No new behavior; only error codes change. Run the full lib test suite and inspect any
  test that pattern-matches against the old `SYN001` code.

**Effort estimate**: ~2-3 hours. ~30-50 call sites.

#### AUDIT-3 family — sync grammar.pest with spec EBNF

**Decision (2026-06-25)**: Sync grammar.pest to spec.

**Scope** (each is independent):

1. **Compound `list_behavior`** — grammar.pest:65 currently
   `{ "." ~ ("line" | "pile" | "unique") }`. Expand to all 7 combinations (Default,
   Line, Pile, Unique, LinePile, LineUnique, PileUnique, LineUniquePile) per
   grammar.ebnf:181-187. Canonical order: line → unique → pile.
2. **`required_op` (`!`)** — add to grammar.pest and reference from a new
   `postfix_primary = { primary ~ "!"? }`.
3. **`print:` block** — add `print_block_stmt` production matching
   `token_parser/blocks.rs:368`.
4. **`default_op`** — add `default` operator rule + `default_expression` production at
   the right precedence level (between logical-or and not).
5. **`handler_type`** — add as a `variable_type` alternative.
6. **`argument_expression`** precedence — rework grammar.pest:202-211 to delegate to
   `expression` instead of building a parallel chain.

**Risk**: Each addition may interact with PEG-style ordering rules and break the parse of
unrelated constructs. Test the full `tests/cln/` suite after each addition individually
(do not bundle).

**Effort estimate**: ~3-4 hours; ~7 separate landings.

#### AUDIT-#45 — standardise EBNF naming in grammar.pest

**Decision (2026-06-25)**: Standardise now. **Status**: deferred — paired with the
AUDIT-3 work above since both modify grammar.pest.

**Scope**: ~19 mechanical renames in `src/parser/grammar.pest` and any downstream
`Rule::xxx` enum references in `src/parser/*.rs`. Examples:

- `if_stmt` → `if_statement`
- `while_stmt` → `while_statement`
- `break_stmt` → `break_statement`
- `boolean` (literal) → `boolean_literal`
- `integer` (literal) → `integer_literal`
- `float` → `float_literal`
- `null` → `none_literal` (already partly done)
- `type_` → consider keeping the trailing underscore for Pest keyword-collision safety,
  document the exception

After the rename, `cargo check` and the full test suite must pass with no test changes
beyond pattern updates.

**Effort estimate**: ~1 hour. Tight enough to bundle with AUDIT-3.

---

## /audit done — outstanding work

Net changes this session:

- 7 code edits across 5 files (SEM003 + SEM008 tagged, FUNC008-12 routed through
  code-aware helper, Number/Integer conversion methods + Array.get/.isNotEmpty added,
  StringUtils_* aliases removed).
- 4 spec changes applied with developer approval (Principle 25):
  - stdlib-reference.md: list.push returns void, list.fill/removeLast/peek documented,
    validator.* §15 added.
  - semantic-rules.md: FUNC012 entry added.
  - error-codes.md: reserved range narrowed to FUNC013+.
- 2 new `.cln` tests landed (numeric conversions, list method dispatch).
- 1 cross-component prompt filed (crypto.sha256/512 plugin migration).
- Audit's coverage methodology was found to under-count by ~16 items due to grep
  false-negatives. Audit corrections table preserved above so future audits don't re-flag.

**Approved-but-deferred** items above keep the open work tracked with each decision
captured, so the next session can pick them up without re-asking.

---

## /audit residue follow-up — 2026-06-25 (same day continuation)

After the initial /audit residue landed, the developer chose `proceed` and we worked
through the remaining approved-but-deferred items.

### ✅ Closed in follow-up

**AUDIT-25 — SYN002 / SYN004 / SYN005 tagged.**
- `src/error/mod.rs:455-471` — new `syntax_error_with_code()` helper.
- `src/lexer/specification_lexer.rs:1768-1788` — `From<LexError>` now lifts
  `UnterminatedString` / `UnterminatedComment` into a `Syntax` variant tagged
  `SYN004`.
- `src/parser/preprocessor.rs:321`, `src/parser/function_parser.rs:87,117` —
  three "no function found" sites now tagged `SYN005`.
- `src/parser/token_parser/blocks.rs` — eleven "expected X" / "unknown X" sites
  in the endpoint-test parser now tagged `SYN002`.
- Three remaining "Failed to parse" sites in preprocessor / function_parser /
  parser_impl kept as the generic `SYN001` — they are not "unexpected token" or
  "malformed construct" specifically.

**AUDIT-9 — `math.random` declared.**
**AUDIT-10 — `math.sign` declared.**
- `src/resolver/symbol_table.rs:768-775` — both added with bridge mapping comments.
- `src/resolver/symbol_table.rs:1300-1302` — both added to math namespace.
- Bridges `math_random` and `math_sign` already exist in
  `foundation/platform-architecture/function-registry.toml:858-897` with the
  `math.random` / `math.sign` aliases. No registry or host changes needed.
- New smoke test: `tests/cln/stdlib/math_random_sign.cln` (compiles clean).

**AUDIT-11 — `file.lines` deferred until host bridge exists.**
- No bridge in `function-registry.toml`. Adding the declaration without a bridge
  would compile but trap at runtime — worse UX than the current "function not found"
  compile error.
- Cross-component prompt filed:
  `foundation/management/cross-component-prompts/all-file-lines-bridge.md`.

**AUDIT-3 family + AUDIT-#45 — `grammar.pest` documented as non-authoritative.**
- Per developer decision (`Document reality`), the cheap path was chosen over the
  ~3-4 hour Pest sync. The canonical parser is `token_parser/`; Pest is only
  invoked from legacy test paths and a small handful of `parser_impl.rs` consumers.
- `src/parser/grammar.pest:1-39` — added a header comment block enumerating the
  six known drift points (compound list_behavior, postfix `!`, print: block,
  default operator, handler type, argument_expression precedence) and the
  ~19 cosmetic naming differences, each pointing at the canonical
  `token_parser/` file. New contributors will see the warning and the pointer.
- This closes AUDIT-3, AUDIT-4, AUDIT-23, AUDIT-24, AUDIT-29, AUDIT-31, AUDIT-32,
  and AUDIT-#45 in the residue list — all "documented intentional drift" now.

### Outstanding (none from /audit)

All audit-derived items are now either fixed, filed as cross-component prompts,
or documented as intentional drift with the developer's explicit decision.

The audit is closed.

---

## COM001 follow-up — stale stack-mismatch from pre-Phase-C session (2026-06-29)

**Status:** TRACKED. Dev-queue entries deferred from Phase C `--skip-dev-queue` landing.

Two dev-queue entries fingerprint `ba21b72c94101332` and `f05aa5dc4856c97c` were
captured during the Phase C artifact-emitter landing pre-flight:

- Both: COM001 — `generated WebAssembly is invalid: type mismatch: values
  remaining on stack at end of block; section=code, function=func[233]`.
- Offsets: `0x4bbb` (19387) and `0x4ba7` (19367). Adjacent — same function,
  same instruction class, one byte apart in the emitted code section.
- Compiler that captured them: **0.30.400** (we are now on 0.30.401+).
- Source field: `main.cln` (no path), occurrences=1 each.
- First/last seen: identical instant 2026-06-29T19:42:10Z — single session.
- Dev reason: `dev build: /Users/earcandy/.../clean-language-compiler/target/debug/cln`.

### Investigation

Could not reproduce post-landing. The path-less `main.cln` source field offers
no usable repro, and the dev-build that produced the traps was the 0.30.400
debug binary which has been superseded. The byte pattern near the offset
(`20 01 28 02 00 10 e8 01 0b 07 00 20 00 ac 10 0f`) is consistent with an
integer-to-string conversion call followed by a stale `i32` left on the stack
before block end — `0x10 0xe8 0x01` is `call func 232`, `0x0b` is `end`, then
the next block starts with `0x07 0x00` (a `try` body?) and `0x20 0x00`
(`local.get 0`) before another `call`. The two adjacent offsets suggest the
same codegen path emitted two slightly-different functions back-to-back.

Hypothesis: the trap was hit by an in-session test compile of a fixture that
exercised an integer-to-string conversion (`toString`/`int_to_string` host
import) inside a control-flow block — likely from cross-compiles of frame.*
plugin sources during the C3-C5 + C7 work. The bug is plausibly the same
class as the historic "stack balance in type conversions" fragility called
out in [KNOWLEDGE.md](./KNOWLEDGE.md).

### Action

- Leave the dev-queue entries **in place** so a fresh reproduction (which will
  carry a real source path) can attach to the same fingerprint and increment
  the occurrence count. A stale single-occurrence entry that never recurs is
  cheap; a cleared entry that resurfaces under a different fingerprint is
  expensive to triage.
- Add to the post-plan cleanup batch: a sweep of integer-to-string codegen
  paths in `src/codegen/mod.rs` and `src/codegen/mir_codegen/instructions.rs`
  for stack-balance invariants under nested control flow.
- If the entries persist across the next two compiler releases without a new
  occurrence, mark them stale and clear.

---

## SEM007 follow-up — frame.data test_expand.cln calls plugin-only function (2026-06-29)

**Status:** CROSS-COMPONENT. NOT a compiler bug. Tracked here so the dev-queue
sweep can re-attribute it.

Dev-queue entry `2426d9b5f7f0cd4d` records SEM007 (`Function 'expand_block'
not found`) at
`/Users/earcandy/Documents/Dev/Clean Language/clean-framework/plugins/frame.data/tests/test_expand.cln`.
Occurrences: 2 — first 2026-06-29T22:06:36Z, last 2026-06-29T22:06:53Z.

### Investigation

The file exists, predates this session (mtime 2026-06-26), and calls
`expand_block(...)` as a top-level function. `expand_block` is a plugin export
(WASM function the plugin manifest declares via `[handles].blocks`), not a
global Clean Language function. The compiler **correctly** rejects the
reference — there is no global symbol of that name to resolve against. The
dev-queue auto-capture fires because the cwd matches `clean-framework/`, but
the bug is in the test file's setup, not the compiler.

Re-tested on 0.30.401 just before this commit: still reproduces. The compiler
behaviour is correct; the test file needs to either (a) import the plugin and
call through it, or (b) move into the plugin's source tree as a unit test
that targets the plugin's internal `expand_block`. This is an issue for
frame.data, not the compiler.

### Action

- Component re-attribution: this is frame.data, not compiler. The dev-queue
  recorder's "source inside component tree" heuristic mis-assigned it because
  the source path is inside `clean-framework/`, but the `clean-framework/`
  workflow owns it.
- Filed via `report_error` against `component = "frame.data"` from the
  framework agent's next session — recorded here for traceability.
- Leave the entry in the dev-queue with a note in the post-plan cleanup batch
  to re-tag it when component-tagging is supported in the recorder.

