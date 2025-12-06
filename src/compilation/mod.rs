//! Multi-file Compilation Support for Clean Language
//!
//! This module provides the infrastructure for compiling Clean Language programs
//! that span multiple source files. It handles:
//!
//! - Module discovery and loading from import statements
//! - Dependency graph construction and cycle detection
//! - Topological ordering of modules for compilation
//! - Cross-module symbol resolution
//! - Merging of all modules into a single WASM output
//!
//! # Architecture
//!
//! The multi-file compilation pipeline extends the existing 7-stage pipeline:
//!
//! 1. **Entry Point**: Start with a single entry `.cln` file
//! 2. **Discovery**: Parse imports and recursively discover all modules
//! 3. **Graph Building**: Build a module dependency graph
//! 4. **Ordering**: Topologically sort modules (dependencies first)
//! 5. **Parallel Stages 1-3**: Parse each module to HIR
//! 6. **Unified Stages 4-5**: Resolve and type-check across all modules
//! 7. **Unified Stage 6-7**: Generate MIR and emit single WASM

pub mod compilation_unit;
pub mod module_graph;
pub mod multi_file_compiler;

pub use compilation_unit::*;
pub use module_graph::*;
pub use multi_file_compiler::*;
