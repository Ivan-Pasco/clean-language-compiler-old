# Clean Language Compiler - Implementation Tasks

## 🔴 CRITICAL: frame.data plugin compatibility — 2 tests failing (SYN001)

**Priority**: CRITICAL — ORM/data plugin tests cannot compile
**Discovered**: 2026-05-22
**Verified**: 2026-06-01 — simple_data.cln and plugin_keywords.cln now pass; 2 remain broken
**Files**: `tests/cln/bugfixes/syn001_orm_query_in_function.cln`, `tests/cln/bugfixes/sem003_orm_variable_bound_in_function.cln`
**Errors**:
  - `syn001`: plugin WASM traps at runtime (wasm function 262) on complex join/where queries
  - `sem003`: plugin-generated code declares `list<User>` but type checker infers `Array<string>` — plugin output type mismatch
**Root cause**: frame.data plugin was compiled with older compiler versions with codegen bugs.
  Plugin WASM crashes or generates wrong-typed output during expansion.
**Blocked on**: Cross-component fix in clean-framework. Bug fingerprint: da50fd867b8fe041.
**Action**: clean-framework team rebuilds frame.data plugin WASM with compiler >= 0.30.151.

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

**Action**: Report via `report_error` to coordinate server side, then implement compiler side.

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
**Action**:
  1. Add `test.http_request` to `clean-server` host bridge (cross-component)
  2. Once bridge exists: add HIR/codegen for `TestCaseKind::Endpoint` to emit actual HTTP calls

---

## 🟢 LOW: P10 — COM002/COM006 concurrency violation error codes

**Priority**: LOW — missing structured error codes for concurrency rule violations
**Discovered**: 2026-05-20

The spec (semantic-rules.md COM002/COM006) defines error codes for:
- COM002: accessing shared state from a background task without synchronization
- COM006: using request-context values outside a request handler

**Current state**: No detection sites exist. Requires new static analysis tracking
  whether a function is inside a `background:` block or request handler context.

**Action**: Add detection in the typechecker or HIR validator. COM006 can be partially
  enforced by tracking whether a function is inside `background:` when accessing
  request-context builtins.

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

## 🟢 LOW: Split oversized functions for readability

**Priority**: LOW — functions over 500 lines; difficult to review and test
**Discovered**: 2026-05-20
**Design**: Split is straightforward — extract per-namespace helper functions from `setup_linker`,
  then apply the same pattern to the other candidates below.

| Function | File | Lines | Suggested split |
|----------|------|-------|-----------------|
| `setup_linker` | `src/plugins/wasm_adapter.rs:104` | ~2370 | Extract per-namespace registration fns (console, file, http, db, etc.) |
| `generate_parse_object_instructions` | `src/stdlib/json_class.rs:2606` | ~1258 | Extract key-scan, value-scan, object-assembly helpers |
| `register_method_style_functions` | `src/runtime/host_functions.rs:1463` | ~1175 | Extract per-class registration fns |
| `peek_has_orm_subclauses` | `src/parser/token_parser/blocks.rs:453` | ~1146 | Extract clause-type detectors |
| `resolve_expression_internal` | `src/resolver/resolver_impl.rs:1428` | ~1059 | Extract per-expression-kind helpers |
| `parse_private_state_section` | `src/parser/token_parser/blocks.rs:1599` | ~1026 | Extract field-type parsers |
| `new_with_default` | `src/ast/mod.rs:192` | ~1012 | Extract per-node-type default builders |
| `infer_expression` | `src/typechecker/type_inference.rs:2993` | ~944 | Extract per-expression-kind inference helpers |

**Action**: Split one function at a time. Each split must keep all tests green. Start with `setup_linker`.

---

## 🟢 LOW: padStart / padEnd return original string (semantic placeholder)

**Priority**: LOW — functions compile correctly but return original string unchanged
**Discovered**: 2026-06-01
**File**: `src/stdlib/string_class.rs:342-360` — `generate_pad_start` and `generate_pad_end` emit
  only `Instruction::LocalGet(0)` (return receiver as-is). WASM validation passes, but no padding occurs.
**Action**: Implement proper padding loops:
  - `padStart(str, width, pad)`: prepend `pad` chars until `str.length() >= width`, then return
  - `padEnd(str, width, pad)`: append `pad` chars until `str.length() >= width`, then return
  - Both require memory allocation via `mem_alloc` and string copy instructions in WASM
