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
//! - Register allocation preparation

use crate::error::CompilerError;
use crate::typechecker::tast::TastProgram;

pub mod control_flow;
pub mod mir_builder;
pub mod mir_types;
pub mod optimization;

pub use control_flow::{BasicBlock, ControlFlowGraph};
pub use mir_builder::{MirBuildResult, MirBuilder};
pub use mir_types::{
    BasicBlockId, MirBasicBlock, MirFunction, MirInstruction, MirOperand, MirProgram, MirType,
    RegisterId, ValueId,
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

    /// Enable debug information preservation
    pub debug_info: bool,

    /// Enable dead code elimination
    pub dead_code_elimination: bool,

    /// Enable constant folding
    pub constant_folding: bool,

    /// Enable function inlining
    pub function_inlining: bool,

    /// Maximum function size for inlining
    pub inline_threshold: usize,
}

/// Result of MIR lowering and optimization
#[derive(Debug)]
pub struct MirResult {
    /// The generated MIR program
    pub program: MirProgram,

    /// Optimization statistics
    pub stats: OptimizationStats,

    /// Any warnings generated during lowering
    pub warnings: Vec<CompilerError>,
}

/// Statistics about optimizations performed
#[derive(Debug, Default)]
pub struct OptimizationStats {
    /// Number of instructions eliminated
    pub instructions_eliminated: usize,

    /// Number of basic blocks eliminated
    pub blocks_eliminated: usize,

    /// Number of functions inlined
    pub functions_inlined: usize,

    /// Number of constants folded
    pub constants_folded: usize,

    /// Total optimization time in microseconds
    pub optimization_time_us: u64,
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
        let (optimized_program, stats) = if self.config.optimize {
            let start_time = std::time::Instant::now();
            let (program, mut stats) = self.optimizer.optimize(build_result.program)?;
            stats.optimization_time_us = start_time.elapsed().as_micros() as u64;
            (program, stats)
        } else {
            (
                build_result.program,
                optimization::OptimizationStats::default(),
            )
        };

        // Convert optimization stats to local OptimizationStats type
        let local_stats = OptimizationStats {
            instructions_eliminated: stats.instructions_eliminated,
            blocks_eliminated: stats.blocks_eliminated,
            functions_inlined: stats.functions_inlined,
            constants_folded: stats.constants_folded,
            optimization_time_us: stats.optimization_time_us,
        };

        Ok(MirResult {
            program: optimized_program,
            stats: local_stats,
            warnings: build_result.warnings,
        })
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
                self.config.function_inlining = false;
            }
            1 => {
                self.config.dead_code_elimination = true;
                self.config.constant_folding = true;
                self.config.function_inlining = false;
            }
            2 => {
                self.config.dead_code_elimination = true;
                self.config.constant_folding = true;
                self.config.function_inlining = true;
                self.config.inline_threshold = 50;
            }
            3 => {
                self.config.dead_code_elimination = true;
                self.config.constant_folding = true;
                self.config.function_inlining = true;
                self.config.inline_threshold = 100;
            }
            _ => unreachable!(),
        }

        // Update optimizer configuration
        self.optimizer = MirOptimizer::with_config(&self.config);
    }

    /// Enable or disable debug information
    pub fn set_debug_info(&mut self, enabled: bool) {
        self.config.debug_info = enabled;
    }

    /// Enable or disable release mode (strips `always:` invariant checks).
    pub fn set_release_mode(&mut self, release: bool) {
        self.builder.release_mode = release;
    }

    /// Get current configuration
    pub fn config(&self) -> &MirConfig {
        &self.config
    }
}

impl Default for MirConfig {
    fn default() -> Self {
        Self {
            optimize: true,
            opt_level: 2,
            debug_info: false,
            dead_code_elimination: true,
            constant_folding: true,
            function_inlining: true,
            inline_threshold: 50,
        }
    }
}

impl Default for MirPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to lower TAST to MIR with default configuration
pub fn lower_tast_to_mir(tast: TastProgram) -> Result<MirResult, Vec<CompilerError>> {
    let mut pipeline = MirPipeline::new();
    pipeline.lower(tast)
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
        assert!(!pipeline.config.function_inlining);

        // Test level 3 (maximum optimization)
        pipeline.set_opt_level(3);
        assert!(pipeline.config.optimize);
        assert!(pipeline.config.dead_code_elimination);
        assert!(pipeline.config.constant_folding);
        assert!(pipeline.config.function_inlining);
        assert_eq!(pipeline.config.inline_threshold, 100);
    }

    #[test]
    fn test_config_default() {
        let config = MirConfig::default();
        assert!(config.optimize);
        assert_eq!(config.opt_level, 2);
        assert!(config.dead_code_elimination);
        assert!(config.constant_folding);
        assert!(config.function_inlining);
    }
}
