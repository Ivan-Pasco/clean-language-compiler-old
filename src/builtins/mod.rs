//! Centralized Builtin Registry
//!
//! This module provides a single source of truth for all builtin functions
//! in the Clean Language compiler. Previously, builtins were registered in
//! three separate places with potentially inconsistent signatures:
//! - `src/resolver/symbol_table.rs` (add_builtins)
//! - `src/typechecker/type_inference.rs` (initialize_builtins)
//! - `src/stdlib/mod.rs` (register_functions)
//!
//! This registry unifies all definitions and provides conversion methods
//! for each compiler stage.

pub mod registry;

pub use registry::{
    BuiltinCategory, BuiltinClass, BuiltinFunction, BuiltinMethod, BuiltinNamespace,
    BuiltinRegistry, BuiltinType,
};
