/// Plugin Contract 5 — Lint Extension host-side subsystem.
///
/// Implements the host side of the lint ABI described in
/// `foundation/spec/framework/contracts/lint-extension.md`.
///
/// Phase B scope (compiler-side, this module):
///   - `arena`    — single-call `LintArena` holding a snapshot of the fully-resolved
///                  `Program`, the monotonic `handle`, and the 4 accessor methods
///                  that produce the JSON payloads described in §4.
///   - `bridges`  — 4 WASM host functions (`_ast_list_classes`, `_ast_class_fields`,
///                  `_ast_list_functions`, `_ast_list_blocks`) registered onto a
///                  dedicated lint linker in `wasm_adapter.rs`.
///
/// Not yet in this cycle (Cycle 2/3):
///   - Invocation from `cln lint` / `cln check` / `cln compile`
///   - Diagnostic JSON parsing and routing through the compiler renderer
///   - Cross-plugin code uniqueness enforcement (LINT003)
pub(crate) mod arena;
pub(crate) mod bridges;

pub(crate) use arena::LintArena;
pub(crate) use bridges::register_lint_bridges;
