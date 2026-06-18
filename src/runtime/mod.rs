// Runtime metadata and target configuration.
//
// This module holds compile-time runtime metadata (target capability
// descriptors, optimization profiles). Actual WASM execution is the
// responsibility of clean-server (Layer 2) and clean-runner — never the
// compiler crate. See foundation/management/ARCHITECTURE_BOUNDARIES.md
// and foundation/platform-architecture/EXECUTION_LAYERS.md.

pub mod runtime_manager;
pub mod runtime_trait;

// Centralized wasmtime config — used by the standalone `wasmtime_runner`
// dev binary and stdlib unit tests that need to validate generated WASM.
// Not used at compile time on the user-facing code path.
pub mod wasmtime_config;
