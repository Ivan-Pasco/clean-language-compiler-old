//! Clean Language Intermediate Representation Module
//!
//! This module implements the multi-layer IR architecture:
//! AST → HIR → MIR → LIR → WebAssembly
//!
//! # IR Levels
//! - **HIR** (High-level IR): Desugared AST with name resolution
//! - **MIR** (Mid-level IR): Control flow graphs with basic blocks  
//! - **LIR** (Low-level IR): WebAssembly-ready instruction sequences

use crate::ast::*;
use crate::error::CompilerError;

pub mod hir;
pub mod lir;
pub mod mir;
pub mod optimization;
pub mod transform;
pub mod validation;

pub use hir::*;
pub use lir::*;
pub use mir::*;
pub use optimization::*;
pub use transform::*;
pub use validation::*;

/// IR transformation result
pub type IRResult<T> = Result<T, CompilerError>;

/// Unique identifier for IR nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IRId(pub usize);

impl IRId {
    pub fn new(id: usize) -> Self {
        IRId(id)
    }
}

/// Debug information preserved through IR transformations
#[derive(Debug, Clone)]
pub struct DebugInfo {
    pub source_span: Option<crate::lexer::Span>,
    pub original_name: Option<String>,
    pub ir_level: IRLevel,
}

/// IR transformation level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IRLevel {
    AST,
    HIR,
    MIR,
    LIR,
}

/// Main IR transformation pipeline
pub struct IRPipeline {
    debug_mode: bool,
    optimization_level: OptimizationLevel,
}

impl IRPipeline {
    pub fn new(debug_mode: bool, optimization_level: OptimizationLevel) -> Self {
        Self {
            debug_mode,
            optimization_level,
        }
    }

    /// Transform AST through all IR levels to LIR
    pub fn transform_program(&self, program: Program) -> IRResult<LIRProgram> {
        // AST → HIR
        let hir = self.ast_to_hir(program)?;

        // HIR → MIR
        let mut mir = self.hir_to_mir(hir)?;

        // Run optimizations on MIR
        let _optimization_stats = optimize_mir_program(&mut mir, self.optimization_level)?;

        if self.debug_mode {
            println!("Optimization stats: {:?}", _optimization_stats);
        }

        // MIR → LIR
        let lir = self.mir_to_lir(mir)?;

        Ok(lir)
    }

    fn ast_to_hir(&self, program: Program) -> IRResult<HIRProgram> {
        ast_to_hir(program)
    }

    fn hir_to_mir(&self, hir: HIRProgram) -> IRResult<MIRProgram> {
        hir_to_mir(hir)
    }

    fn mir_to_lir(&self, mir: MIRProgram) -> IRResult<LIRProgram> {
        mir_to_lir(mir)
    }
}

/// Optimization level configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationLevel {
    None,       // O0 - No optimizations
    Speed,      // O1/O2 - Optimize for speed
    Size,       // Os - Optimize for size
    Aggressive, // O3 - Aggressive optimizations
}

impl Default for OptimizationLevel {
    fn default() -> Self {
        OptimizationLevel::Speed
    }
}
