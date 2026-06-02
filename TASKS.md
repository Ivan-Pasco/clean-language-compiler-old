# Clean Language Compiler - Implementation Tasks

## 🔴 CRITICAL: frame.data plugin compatibility — 2 tests failing (SYN001)

**Priority**: CRITICAL — ORM/data plugin tests cannot compile
**Discovered**: 2026-05-22
**Verified**: 2026-06-01 — simple_data.cln and plugin_keywords.cln now pass; 2 remain broken
**Files**: `tests/cln/bugfixes/syn001_orm_query_in_function.cln`, `tests/cln/bugfixes/sem003_orm_variable_bound_in_function.cln`
**Errors**:
  - `syn001`: plugin WASM traps at runtime (wasm function 262) on complex join/where queries
  - `sem003`: plugin-generated code declares `list<User>` but type checker infers `Array<string>` — plugin output type mismatch
**Root cause**: Bug in frame.data plugin Clean Language source logic (not the compiler version).
  Rebuilding with compiler 0.30.211 does not resolve — plugin source must be fixed in clean-framework.
**Reported**: 2026-06-01 (framework component)
  - SYN001 report ID `7b4c60bd-5bef-4f4f-a885-b80caf496164`
  - SEM003 report ID `54e673ed-fc1f-4293-9b0d-02c6b5d1e087`
**Action**: clean-framework team fixes expand logic in frame.data/src/main.cln for complex where/join queries and ORM result type propagation.

---

## 🟡 MEDIUM-HIGH: P02 — string.matches compile-time pattern ID resolution

**Priority**: MEDIUM-HIGH — runtime host bridge contract must change together with compiler
**Discovered**: 2026-05-20

The spec (stdlib-reference.md) requires `string.matches` to resolve the pattern name to a
compile-time integer ID before emitting the WASM call, reducing the import signature from
4 parameters `(str_ptr, str_len, pattern_ptr, pattern_len)` to 3 parameters `(str_ptr, str_len, pattern_id)`.

**Mapping**: email→1, url→2, uuid→3, slug→4, numeric→5, alpha→6, phone→7, date→8

**Files to change** (compiler side):
- `src/codegen/codegen_registration.rs` lines ~654–672 — change import signature to 3 params
- MIR codegen call site for `string.matches` — emit `I32Const(id)` instead of string ptr+len

**Cross-component prerequisite** (do NOT change before server is updated):
- `clean-server/host-bridge` — `string_matches(str_ptr, str_len, pattern_id)` implementation
- This is a breaking contract change; both sides must ship in the same release.

**Reported**: 2026-06-01 — report ID `24d6ea05-fac4-408f-86bb-38854659fd80`
**Action**: Once server ships updated bridge, implement compiler side (codegen_registration.rs ~654–672).

---

## 🟡 MEDIUM-HIGH: Endpoint test codegen — missing test.http_request host bridge

**Priority**: MEDIUM-HIGH — endpoint tests compile but cannot execute without host bridge
**Discovered**: 2026-05-28
**Verified**: 2026-06-01 — compiler side parses and compiles endpoint test syntax correctly
**Files**: `tests/cln/testing/endpoint_test_syntax.cln`, `src/parser/token_parser/blocks.rs`
**Missing**: Host bridge function `test.http_request` in `clean-server`. Expected signature:
  ```
  test.http_request(method: string, path: string, body_json: string | null, header_key: string | null, header_val: string | null) -> HttpTestResponse
  ```
  where `HttpTestResponse` exposes `.status: integer`, `.body_json: string`, `.ok: boolean`.
**Reported**: 2026-06-01 — report ID `4a51fd68-4d43-4cea-9c58-3d959c35552b`
**Action**:
  1. Server team adds `test.http_request` to `clean-server` host bridge
  2. Once bridge exists: implement HIR/codegen for `TestCaseKind::Endpoint` in `src/codegen/`

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

## 🟢 LOW: Refactor — move page-companion assembly logic out of compiler into framework plugin hook

**Priority**: LOW — architectural hygiene, no user-visible breakage today
**Discovered**: 2026-06-01
**Files**: `src/compilation/multi_file_compiler.rs` (functions: `prefix_companion_functions`,
  `derive_page_route_from_cln`, `derive_page_name_from_cln`, `generate_page_route_source`,
  `route_path_to_params_literal`, and `PageCompanionRecord`)
**Problem**: The compiler encodes frame.ui/frame.server-specific knowledge that violates
  the "No Plugin Logic in the Compiler" rule. These concepts should live in the framework.
**Root cause**: No plugin hook exists for "transform sources before multi-file assembly."
**Ideal fix**: Add a first-class `PluginAssemblyHook` trait to the plugin system:
  1. Define `fn on_assemble(sources: &[SourceFile]) -> AssemblyResult`
  2. frame.ui implements the hook: detects page companions, prefixes load/guard, generates route module
  3. Compiler calls `plugin.on_assemble(...)` and merges results, with no framework knowledge
  4. Remove the six frame-specific functions from `multi_file_compiler.rs`
**Blocked on**: Plugin system needs a compile-time (not runtime) hook surface — design required.

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
