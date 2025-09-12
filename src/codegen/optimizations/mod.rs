// Advanced WASM code generation optimizations
pub mod dead_code_elimination;
// Temporarily disabled due to AST restructuring
// pub mod constant_folding;
// pub mod loop_optimization;
// pub mod function_inlining;
// pub mod peephole;
// pub mod optimization_pipeline;

// Re-export optimization components
pub use dead_code_elimination::DeadCodeEliminator;
// Temporarily disabled exports
// pub use constant_folding::ConstantFolder;
// pub use loop_optimization::LoopOptimizer;
// pub use function_inlining::FunctionInliner;
// pub use peephole::PeepholeOptimizer;
// pub use optimization_pipeline::{OptimizationPipeline, OptimizationLevel, OptimizationConfig, OptimizationMetrics};
