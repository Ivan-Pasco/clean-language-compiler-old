// Runtime metadata and target configuration.
//
// This module holds compile-time runtime metadata (target capability
// descriptors, runtime selection). Actual WASM execution is the
// responsibility of clean-server (Layer 2) and clean-runner — never the
// compiler crate. See foundation/management/ARCHITECTURE_BOUNDARIES.md
// and foundation/platform-architecture/EXECUTION_LAYERS.md.

pub mod runtime_manager;
pub mod runtime_trait;

// Wasmtime engine construction for the standalone `wasmtime_runner`
// dev binary.
// Not used at compile time on the user-facing code path.
pub mod wasmtime_config;

// Layer 2 JSON bridges (`_json_*_v2`, `_json_get`) for the wasmtime hosts
// inside this crate: the plugin sandbox and the `wasmtime_runner`.
pub mod json_bridge;
