# Clean Language Compiler - Implementation Tasks

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
