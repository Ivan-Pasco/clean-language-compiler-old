/// Plugin Contracts v3 — Typed AST Emission subsystem.
///
/// This module implements the host-side of the typed-emission ABI described in
/// `foundation/spec/plugins/contracts/typed-emission.md`.
///
/// Architecture:
///   - `arena`    — single-call arena with 1-indexed handles and consumption tracking
///   - `bridges`  — ~30 bridge functions registered onto the typed-emission linker
///   - `error`    — `EmitDiagnostic` and `EmitError` types
///   - `json`     — JSON helpers for parameter/statement/class payloads
///   - `ops`      — stable integer op-code tables (typed-emission-ops.toml mirror)
pub(crate) mod arena;
pub mod batch_schema;
pub(crate) mod bridges;
pub(crate) mod error;
pub(crate) mod json;
pub mod ops;

/// Re-export the public types used by `wasm_adapter.rs`.
pub(crate) use arena::EmitArena;
pub(crate) use bridges::register_typed_emission_bridges;
