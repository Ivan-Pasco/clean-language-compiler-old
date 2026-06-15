# Clean Language Compiler - Implementation Tasks

## 🟡 IN PROGRESS: BUILTIN-NAMESPACE-OVERREACH — consolidate hardcoded plugin-domain knowledge

**Priority**: MEDIUM (priority 56 on dashboard)
**Dashboard fingerprint**: `cf5bbd0c6c55bd307278e32b9e61f85cb25ff23e970e572e31661c9adf55c192`
**Cross-component plan**: `foundation/management/cross-component-prompts/all-builtin-namespace-overreach-consolidation.md`

**Status summary**:

| Sub-finding | Status | Owner |
|---|---|---|
| Sub-B (register_http_server_wrappers) | ✅ Done in 0.30.288 — dead code deleted (~180 LOC) | compiler |
| Sub-D-MCP-1 (BuiltinRegistry) | ✅ Done in 0.30.289 — deleted, MCP rewired to SymbolTable (~1670 LOC) | compiler |
| Layer-3 prefix classifier (utilities.rs:1665-1669) | ✅ Done in 0.30.289 — bridge_functions lookup replaces hardcoded prefixes | compiler |
| Sub-A (resolver/symbol_table.rs req.*/auth.*) | ⏸ Blocked on framework plugin.toml updates | cross-component |
| Sub-B-rest (register_http_imports server bridges) | ⏸ Blocked on framework plugin.toml updates (or test-helper redesign) | cross-component |
| Sub-C (register_db_builtin_wrappers) | ⏸ Blocked on frame.data plugin.toml adding `maps_to` entries for db.begin/commit/rollback | cross-component |
| Sub-D-MCP-2 (MCP plugin lists) | 🟡 Needs runtime plugin loading design | compiler |
| Sub-E (runtime/host_functions.rs stubs) | 🟡 Needs WASM-import-section inspection (extract bridge signatures from loaded .wasm at runner startup, register zero-value stubs for each missing import). Bigger lift than expected; deferred. | compiler |

**Directions to continue** (when next picking this up):

### Step 1 — Decide whether to do compiler-only pieces ahead of cross-component

Two compiler-only pieces are ready to ship without waiting on framework:
- **Layer-3 prefix classifier** (`src/codegen/mir_codegen/utilities.rs:1665-1669`): hardcodes `"_req_"`, `"_res_"`, `"_session_"`, `"_auth_"`, `"_http_"` prefix strings to filter Layer 3 calls from import-name collection. Replace with `self.bridge_functions.iter().filter(|f| f.hosts.as_deref().map_or(false, |h| h == ["server"]))` — or move the layer-classification into the plugin manifest (the cleaner long-term answer).
- **Sub-E runtime stubs** (`src/runtime/host_functions.rs:2599-2666`): test-only wasmtime stubs. If a `PluginRegistry` handle is available at runner setup, iterate `bridge_functions` to register zero-valued stubs from manifest signatures. Falls back to current hardcoded list when registry is absent.

### Step 2 — Cross-component coordination (when ready)

The biggest reductions are blocked on plugin.toml updates in `clean-framework`:
- For Sub-A: every `[[functions]]` entry with `maps_to` needs explicit `params` and `returns` Clean Language types (see Path B proposal §Step 2). Without these, removing the resolver's hardcoded entries degrades `req.body()` from `String` to `Any`.
- For Sub-C: frame.data needs `{ name = "db.begin", maps_to = "_db_begin" }` (and commit/rollback) so `language_to_bridge_map` carries the alias, making `register_db_builtin_wrappers` skippable.

The framework-side prompt to file when ready: a `framework-` prefixed entry in `foundation/management/cross-component-prompts/` citing the Path B proposal §Step 2.

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
