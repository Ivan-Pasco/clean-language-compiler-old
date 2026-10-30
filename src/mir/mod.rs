//! MIR - Medium-level Intermediate Representation - Stage 6 of the Clean Language compiler pipeline
//!
//! This module implements the MIR (Medium-level Intermediate Representation) which serves as
//! an optimization-friendly intermediate form between TAST and WASM code generation.
//!
//! Features:
//! - SSA (Static Single Assignment) form for optimization
//! - Control flow graph representation
//! - Type-erased but still type-safe operations
//! - Platform-independent instruction set
//! - Dead code elimination and constant folding

use crate::error::CompilerError;
use crate::typechecker::tast::TastProgram;

pub mod mir_builder;
pub mod mir_types;
pub mod optimization;

pub use mir_builder::{MirBuildResult, MirBuilder};
pub use mir_types::{
    BasicBlockId, MirBasicBlock, MirFunction, MirInstruction, MirOperand, MirProgram, MirType,
    ValueId,
};
pub use optimization::{MirOptimizer, OptimizationPass};

/// MIR lowering and optimization pipeline
///
/// Transforms TAST into optimized MIR suitable for efficient code generation
#[derive(Debug)]
pub struct MirPipeline {
    /// The MIR builder for lowering from TAST
    builder: MirBuilder,

    /// Optimization passes to apply
    optimizer: MirOptimizer,

    /// Configuration options
    config: MirConfig,
}

/// Configuration for MIR generation and optimization
#[derive(Debug, Clone)]
pub struct MirConfig {
    /// Enable optimization passes
    pub optimize: bool,

    /// Target optimization level (0-3)
    pub opt_level: u8,

    /// Enable dead code elimination
    pub dead_code_elimination: bool,

    /// Enable constant folding
    pub constant_folding: bool,
}

/// Result of MIR lowering and optimization
#[derive(Debug)]
pub struct MirResult {
    /// The generated MIR program
    pub program: MirProgram,
}

impl MirPipeline {
    /// Create a new MIR pipeline with default configuration
    /// Note: This creates an empty symbol table which will be replaced when lower() is called
    pub fn new() -> Self {
        // Create a temporary empty symbol table for initialization
        // The actual symbol table will be set when lower() is called with a TAST
        let empty_symbol_table = std::sync::Arc::new(crate::resolver::GlobalSymbolTable::new());
        Self {
            builder: MirBuilder::new(empty_symbol_table),
            optimizer: MirOptimizer::new(),
            config: MirConfig::default(),
        }
    }

    /// Create a new MIR pipeline with custom configuration
    pub fn with_config(config: MirConfig) -> Self {
        let empty_symbol_table = std::sync::Arc::new(crate::resolver::GlobalSymbolTable::new());
        Self {
            builder: MirBuilder::new(empty_symbol_table),
            optimizer: MirOptimizer::with_config(&config),
            config,
        }
    }

    /// Lower TAST to optimized MIR
    pub fn lower(&mut self, tast: TastProgram) -> Result<MirResult, Vec<CompilerError>> {
        tracing::debug!(
            function_count = tast.functions.len(),
            "Starting TAST to MIR lowering"
        );
        for function in &tast.functions {
            tracing::trace!(
                function_name = %function.name,
                statement_count = function.body.statements.len(),
                "Processing TAST function"
            );
        }

        // Update builder with the actual symbol table from TAST.
        // Preserve pipeline-level configuration flags (e.g. release_mode) that
        // were set on the old builder before this call.
        let preserved_release_mode = self.builder.release_mode;
        self.builder = MirBuilder::new(tast.symbol_table.clone());
        self.builder.release_mode = preserved_release_mode;

        // Phase 1: Lower TAST to unoptimized MIR
        let build_result = self.builder.build(tast)?;

        tracing::debug!(
            function_count = build_result.program.functions.len(),
            "MIR build completed"
        );

        // Phase 2: Apply optimization passes if enabled
        let program = if self.config.optimize {
            self.optimizer.optimize(build_result.program)?
        } else {
            build_result.program
        };

        Ok(MirResult { program })
    }

    /// Set optimization level
    pub fn set_opt_level(&mut self, level: u8) {
        self.config.opt_level = level.min(3);
        self.config.optimize = level > 0;

        // Configure optimization passes based on level
        match level {
            0 => {
                self.config.dead_code_elimination = false;
                self.config.constant_folding = false;
            }
            _ => {
                self.config.dead_code_elimination = true;
                self.config.constant_folding = true;
            }
        }

        // Update optimizer configuration
        self.optimizer = MirOptimizer::with_config(&self.config);
    }

    /// Enable or disable release mode (strips `always:` invariant checks).
    pub fn set_release_mode(&mut self, release: bool) {
        self.builder.release_mode = release;
    }
}

impl Default for MirConfig {
    fn default() -> Self {
        Self {
            optimize: true,
            opt_level: 2,
            dead_code_elimination: true,
            constant_folding: true,
        }
    }
}

impl Default for MirPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to lower TAST to MIR with custom optimization level
pub fn lower_tast_to_mir_with_opt_level(
    tast: TastProgram,
    opt_level: u8,
) -> Result<MirResult, Vec<CompilerError>> {
    let mut pipeline = MirPipeline::new();
    pipeline.set_opt_level(opt_level);
    pipeline.lower(tast)
}

/// Convenience function to lower TAST to MIR with custom optimization level and release mode.
/// In release mode, `always:` invariant checks are stripped from the compiled output.
pub fn lower_tast_to_mir_release(
    tast: TastProgram,
    opt_level: u8,
    release_mode: bool,
) -> Result<MirResult, Vec<CompilerError>> {
    let mut pipeline = MirPipeline::new();
    pipeline.set_opt_level(opt_level);
    pipeline.set_release_mode(release_mode);
    pipeline.lower(tast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mir_pipeline_creation() {
        let pipeline = MirPipeline::new();
        assert_eq!(pipeline.config.opt_level, 2);
        assert!(pipeline.config.optimize);
    }

    #[test]
    fn test_optimization_levels() {
        let mut pipeline = MirPipeline::new();

        // Test level 0 (no optimization)
        pipeline.set_opt_level(0);
        assert!(!pipeline.config.optimize);
        assert!(!pipeline.config.dead_code_elimination);
        assert!(!pipeline.config.constant_folding);

        // Test level 3 (maximum optimization)
        pipeline.set_opt_level(3);
        assert!(pipeline.config.optimize);
        assert!(pipeline.config.dead_code_elimination);
        assert!(pipeline.config.constant_folding);
    }

    #[test]
    fn test_config_default() {
        let config = MirConfig::default();
        assert!(config.optimize);
        assert_eq!(config.opt_level, 2);
        assert!(config.dead_code_elimination);
        assert!(config.constant_folding);
    }
}
