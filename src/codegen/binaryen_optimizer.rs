//! Binaryen WebAssembly optimization integration for the Clean Language compiler.
//!
//! This module provides a comprehensive interface to Binaryen's optimization passes,
//! specifically tailored for the Clean Language compiler's needs.

use crate::error::CompilerError;
use crate::runtime::runtime_trait::OptimizationLevel;
// Remove unused path imports
use std::process::Command;
use tempfile::TempDir;

/// Binaryen optimization configuration
#[derive(Debug, Clone)]
pub struct BinaryenOptimizer {
    /// Optimization level to use
    optimization_level: OptimizationLevel,
    /// Custom optimization passes to apply
    custom_passes: Vec<BinaryenPass>,
    /// Whether to shrink the WebAssembly module
    shrink_level: ShrinkLevel,
    /// Whether to converge optimization passes
    converge: bool,
    /// Whether to enable debug information preservation
    debug_info: bool,
}

/// Available Binaryen optimization passes
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryenPass {
    /// Dead code elimination - removes unreachable code and unused functions
    DeadCodeElimination,
    /// Function inlining - inlines small functions to reduce call overhead
    InliningOptimizing,
    /// Constant propagation and precomputation
    Precompute,
    /// Instruction-level optimizations
    OptimizeInstructions,
    /// Local variable coalescing
    CoalesceLocals,
    /// Memory access optimizations
    MemoryPacking,
    /// Control flow graph optimization
    ReReloop,
    /// Remove unused imports and exports
    RemoveUnusedNames,
    /// Merge similar functions
    MergeSimilarFunctions,
    /// Global use-def analysis for better constant propagation
    GUFA,
    /// Optimize for size specifically
    OptimizeForSize,
    /// Optimize stack operations
    OptimizeStackIR,
    /// Simplify control flow
    SimplifyLocals,
    /// Custom pass with specific name
    Custom(String),
}

/// WebAssembly module shrinking levels
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShrinkLevel {
    /// No shrinking
    None,
    /// Basic shrinking
    Basic,
    /// Aggressive shrinking for size optimization
    Aggressive,
}

/// Optimization statistics from Binaryen
#[derive(Debug, Clone)]
pub struct OptimizationStats {
    /// Original module size in bytes
    pub original_size: usize,
    /// Optimized module size in bytes
    pub optimized_size: usize,
    /// Size reduction percentage
    pub size_reduction_percent: f64,
    /// Applied optimization passes
    pub applied_passes: Vec<BinaryenPass>,
    /// Optimization time in milliseconds
    pub optimization_time_ms: u128,
}

impl BinaryenOptimizer {
    /// Create a new Binaryen optimizer with default settings
    pub fn new(optimization_level: OptimizationLevel) -> Self {
        Self {
            optimization_level,
            custom_passes: Vec::new(),
            shrink_level: ShrinkLevel::Basic,
            converge: true,
            debug_info: false,
        }
    }

    /// Create an optimizer for development builds (fast compilation, debugging enabled)
    pub fn for_development() -> Self {
        Self {
            optimization_level: OptimizationLevel::None,
            custom_passes: Vec::new(),
            shrink_level: ShrinkLevel::None,
            converge: false,
            debug_info: true,
        }
    }

    /// Create an optimizer for production builds (balanced optimization)
    pub fn for_production() -> Self {
        Self {
            optimization_level: OptimizationLevel::Speed,
            custom_passes: Self::get_production_passes(),
            shrink_level: ShrinkLevel::Basic,
            converge: true,
            debug_info: false,
        }
    }

    /// Create an optimizer for size-critical applications (web deployment)
    pub fn for_size_optimization() -> Self {
        Self {
            optimization_level: OptimizationLevel::SpeedAndSize,
            custom_passes: Self::get_size_optimization_passes(),
            shrink_level: ShrinkLevel::Aggressive,
            converge: true,
            debug_info: false,
        }
    }

    /// Create an optimizer for maximum performance
    pub fn for_speed_optimization() -> Self {
        Self {
            optimization_level: OptimizationLevel::Speed,
            custom_passes: Self::get_speed_optimization_passes(),
            shrink_level: ShrinkLevel::Basic,
            converge: true,
            debug_info: false,
        }
    }

    /// Add a custom optimization pass
    pub fn with_pass(mut self, pass: BinaryenPass) -> Self {
        self.custom_passes.push(pass);
        self
    }

    /// Set shrink level
    pub fn with_shrink_level(mut self, shrink_level: ShrinkLevel) -> Self {
        self.shrink_level = shrink_level;
        self
    }

    /// Enable or disable convergence (running passes until no more changes)
    pub fn with_converge(mut self, converge: bool) -> Self {
        self.converge = converge;
        self
    }

    /// Enable or disable debug information preservation
    pub fn with_debug_info(mut self, debug_info: bool) -> Self {
        self.debug_info = debug_info;
        self
    }

    /// Optimize a WebAssembly binary using Binaryen
    pub fn optimize(
        &self,
        wasm_bytes: &[u8],
    ) -> Result<(Vec<u8>, OptimizationStats), CompilerError> {
        let start_time = std::time::Instant::now();
        let original_size = wasm_bytes.len();

        // Create temporary directory for input/output files
        let temp_dir = TempDir::new().map_err(|e| {
            CompilerError::codegen_error(
                format!("Failed to create temp directory: {}", e),
                None,
                None,
            )
        })?;

        let input_path = temp_dir.path().join("input.wasm");
        let output_path = temp_dir.path().join("output.wasm");

        // Write input WASM to temporary file
        std::fs::write(&input_path, wasm_bytes).map_err(|e| {
            CompilerError::codegen_error(format!("Failed to write input WASM: {}", e), None, None)
        })?;

        // Build wasm-opt command
        let mut cmd = Command::new("wasm-opt");

        // Add optimization level
        match self.optimization_level {
            OptimizationLevel::None => {
                cmd.arg("-O0");
            }
            OptimizationLevel::Speed => {
                cmd.arg("-O2");
            }
            OptimizationLevel::SpeedAndSize => {
                cmd.arg("-O3");
            }
        }

        // Add shrink level
        match self.shrink_level {
            ShrinkLevel::None => {}
            ShrinkLevel::Basic => {
                cmd.arg("--shrink-level=1");
            }
            ShrinkLevel::Aggressive => {
                cmd.arg("--shrink-level=2");
            }
        }

        // Add convergence
        if self.converge {
            cmd.arg("--converge");
        }

        // Add debug info preservation
        if self.debug_info {
            cmd.arg("--debuginfo");
        }

        // Add custom passes
        let applied_passes = if self.custom_passes.is_empty() {
            self.get_default_passes_for_level()
        } else {
            self.custom_passes.clone()
        };

        for pass in &applied_passes {
            cmd.arg("--pass").arg(pass.to_string());
        }

        // Add input and output files
        cmd.arg(&input_path);
        cmd.arg("-o").arg(&output_path);

        // Execute wasm-opt
        let output = cmd.output().map_err(|e| {
            CompilerError::codegen_error(
                format!(
                    "Failed to execute wasm-opt: {}. Make sure Binaryen is installed.",
                    e
                ),
                None,
                None,
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CompilerError::codegen_error(
                format!("wasm-opt failed: {}", stderr),
                None,
                None,
            ));
        }

        // Read optimized WASM
        let optimized_bytes = std::fs::read(&output_path).map_err(|e| {
            CompilerError::codegen_error(
                format!("Failed to read optimized WASM: {}", e),
                None,
                None,
            )
        })?;

        let optimized_size = optimized_bytes.len();
        let optimization_time_ms = start_time.elapsed().as_millis();

        let size_reduction_percent = if original_size > 0 {
            ((original_size - optimized_size) as f64 / original_size as f64) * 100.0
        } else {
            0.0
        };

        let stats = OptimizationStats {
            original_size,
            optimized_size,
            size_reduction_percent,
            applied_passes,
            optimization_time_ms,
        };

        Ok((optimized_bytes, stats))
    }

    /// Get recommended passes for Clean Language compiler patterns
    pub fn get_clean_language_passes() -> Vec<BinaryenPass> {
        vec![
            BinaryenPass::DeadCodeElimination, // Remove unused stdlib functions
            BinaryenPass::RemoveUnusedNames,   // Remove unused imports/exports
            BinaryenPass::Precompute,          // Optimize mathematical constants
            BinaryenPass::InliningOptimizing,  // Inline property accessors and small functions
            BinaryenPass::CoalesceLocals,      // Optimize local variables
            BinaryenPass::OptimizeInstructions, // Peephole optimizations
            BinaryenPass::SimplifyLocals,      // Simplify local variable usage
            BinaryenPass::MemoryPacking,       // Optimize memory access patterns
        ]
    }

    /// Get production optimization passes (balanced performance/size)
    fn get_production_passes() -> Vec<BinaryenPass> {
        let mut passes = Self::get_clean_language_passes();
        passes.push(BinaryenPass::MergeSimilarFunctions);
        passes
    }

    /// Get size optimization passes (for web deployment)
    fn get_size_optimization_passes() -> Vec<BinaryenPass> {
        let mut passes = Self::get_production_passes();
        passes.extend(vec![
            BinaryenPass::OptimizeForSize,
            BinaryenPass::OptimizeStackIR,
            BinaryenPass::GUFA,
        ]);
        passes
    }

    /// Get speed optimization passes (maximum performance)
    fn get_speed_optimization_passes() -> Vec<BinaryenPass> {
        let mut passes = Self::get_clean_language_passes();
        passes.extend(vec![
            BinaryenPass::ReReloop, // Optimize control flow
            BinaryenPass::GUFA,     // Global analysis for better optimization
        ]);
        passes
    }

    /// Get default passes for the current optimization level
    fn get_default_passes_for_level(&self) -> Vec<BinaryenPass> {
        match self.optimization_level {
            OptimizationLevel::None => Vec::new(),
            OptimizationLevel::Speed => Self::get_speed_optimization_passes(),
            OptimizationLevel::SpeedAndSize => Self::get_size_optimization_passes(),
        }
    }

    /// Check if wasm-opt is available in the system
    pub fn is_available() -> bool {
        Command::new("wasm-opt")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Get the version of wasm-opt
    pub fn get_version() -> Result<String, CompilerError> {
        let output = Command::new("wasm-opt")
            .arg("--version")
            .output()
            .map_err(|e| {
                CompilerError::codegen_error(
                    format!("Failed to get wasm-opt version: {}", e),
                    None,
                    None,
                )
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(CompilerError::codegen_error(
                "Failed to get wasm-opt version",
                None,
                None,
            ))
        }
    }
}

impl BinaryenPass {
    /// Convert pass to wasm-opt command line argument
    fn to_string(&self) -> &str {
        match self {
            BinaryenPass::DeadCodeElimination => "dce",
            BinaryenPass::InliningOptimizing => "inlining-optimizing",
            BinaryenPass::Precompute => "precompute",
            BinaryenPass::OptimizeInstructions => "optimize-instructions",
            BinaryenPass::CoalesceLocals => "coalesce-locals",
            BinaryenPass::MemoryPacking => "memory-packing",
            BinaryenPass::ReReloop => "rereloop",
            BinaryenPass::RemoveUnusedNames => "remove-unused-names",
            BinaryenPass::MergeSimilarFunctions => "merge-similar-functions",
            BinaryenPass::GUFA => "gufa",
            BinaryenPass::OptimizeForSize => "optimize-for-size",
            BinaryenPass::OptimizeStackIR => "optimize-stack-ir",
            BinaryenPass::SimplifyLocals => "simplify-locals",
            BinaryenPass::Custom(name) => name,
        }
    }

    /// Get a description of what the pass does
    pub fn description(&self) -> &str {
        match self {
            BinaryenPass::DeadCodeElimination => "Removes unreachable code and unused functions",
            BinaryenPass::InliningOptimizing => "Inlines small functions to reduce call overhead",
            BinaryenPass::Precompute => "Evaluates constant expressions at compile time",
            BinaryenPass::OptimizeInstructions => "Applies peephole optimizations to instructions",
            BinaryenPass::CoalesceLocals => "Merges local variables to reduce stack usage",
            BinaryenPass::MemoryPacking => {
                "Optimizes memory access patterns for better cache usage"
            }
            BinaryenPass::ReReloop => "Reconstructs control flow for better performance",
            BinaryenPass::RemoveUnusedNames => "Removes unused import/export declarations",
            BinaryenPass::MergeSimilarFunctions => "Merges functions with similar implementations",
            BinaryenPass::GUFA => "Global use-def analysis for advanced constant propagation",
            BinaryenPass::OptimizeForSize => "Applies size-specific optimizations",
            BinaryenPass::OptimizeStackIR => "Optimizes stack-based intermediate representation",
            BinaryenPass::SimplifyLocals => "Simplifies local variable declarations and usage",
            BinaryenPass::Custom(_name) => "Custom optimization pass",
        }
    }
}

impl std::fmt::Display for BinaryenPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl std::fmt::Display for OptimizationStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Binaryen Optimization Results:")?;
        writeln!(f, "  Original size: {} bytes", self.original_size)?;
        writeln!(f, "  Optimized size: {} bytes", self.optimized_size)?;
        writeln!(f, "  Size reduction: {:.2}%", self.size_reduction_percent)?;
        writeln!(f, "  Optimization time: {}ms", self.optimization_time_ms)?;
        write!(
            f,
            "  Applied passes: {}",
            self.applied_passes
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binaryen_pass_to_string() {
        assert_eq!(BinaryenPass::DeadCodeElimination.to_string(), "dce");
        assert_eq!(
            BinaryenPass::InliningOptimizing.to_string(),
            "inlining-optimizing"
        );
        assert_eq!(
            BinaryenPass::Custom("custom-pass".to_string()).to_string(),
            "custom-pass"
        );
    }

    #[test]
    fn test_optimizer_creation() {
        let optimizer = BinaryenOptimizer::new(OptimizationLevel::Speed);
        assert_eq!(optimizer.optimization_level, OptimizationLevel::Speed);
        assert!(optimizer.converge);
        assert!(!optimizer.debug_info);
    }

    #[test]
    fn test_production_optimizer() {
        let optimizer = BinaryenOptimizer::for_production();
        assert_eq!(optimizer.optimization_level, OptimizationLevel::Speed);
        assert_eq!(optimizer.shrink_level, ShrinkLevel::Basic);
        assert!(!optimizer.custom_passes.is_empty());
    }

    #[test]
    fn test_size_optimizer() {
        let optimizer = BinaryenOptimizer::for_size_optimization();
        assert_eq!(
            optimizer.optimization_level,
            OptimizationLevel::SpeedAndSize
        );
        assert_eq!(optimizer.shrink_level, ShrinkLevel::Aggressive);
        assert!(!optimizer.custom_passes.is_empty());
    }

    #[test]
    fn test_clean_language_passes() {
        let passes = BinaryenOptimizer::get_clean_language_passes();
        assert!(passes.contains(&BinaryenPass::DeadCodeElimination));
        assert!(passes.contains(&BinaryenPass::Precompute));
        assert!(passes.contains(&BinaryenPass::InliningOptimizing));
    }

    #[test]
    fn test_pass_descriptions() {
        let pass = BinaryenPass::DeadCodeElimination;
        assert!(!pass.description().is_empty());

        let pass = BinaryenPass::Custom("test".to_string());
        assert!(!pass.description().is_empty());
    }
}
