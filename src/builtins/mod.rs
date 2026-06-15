//! Builtin Type Descriptors
//!
//! Hosts the `BuiltinType` enum used by the bridge function ABI. Historically
//! this module also re-exported `BuiltinRegistry` and its companion types
//! (`BuiltinFunction`, `BuiltinClass`, etc.); those were removed in 0.30.289
//! as part of BUILTIN-NAMESPACE-OVERREACH sub-finding D-MCP-1. Builtin
//! signatures now live in exactly one place: `src/resolver/symbol_table.rs`.

pub mod registry;

pub use registry::BuiltinType;
