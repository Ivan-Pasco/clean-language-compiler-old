//! MIR Optimization Passes
//!
//! This module implements optimization passes for MIR code, including:
//! - Dead code elimination
//! - Constant folding and propagation
//! - Function inlining
//! - Control flow simplification
//! - Peephole optimizations

#![allow(dead_code)]

use crate::error::CompilerError;
use crate::mir::mir_types::{
    MirBinaryOp, MirConstant, MirFunction, MirOperand, MirOperation, MirProgram, MirTerminator,
    MirUnaryOp, ValueId,
};
use crate::mir::MirConfig;
use std::collections::{HashMap, HashSet};

/// Optimization statistics
#[derive(Debug, Default, Clone)]
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

/// MIR optimizer that applies optimization passes
#[derive(Debug)]
pub struct MirOptimizer {
    /// Configuration for optimization passes
    config: MirConfig,

    /// Available optimization passes
    passes: Vec<Box<dyn OptimizationPass>>,
}

/// Trait for optimization passes
pub trait OptimizationPass: std::fmt::Debug {
    /// Get the name of this optimization pass
    fn name(&self) -> &str;

    /// Apply this optimization pass to a function
    fn optimize_function(&mut self, function: &mut MirFunction)
        -> Result<PassStats, CompilerError>;

    /// Apply this optimization pass to the entire program
    fn optimize_program(&mut self, program: &mut MirProgram) -> Result<PassStats, CompilerError> {
        let mut total_stats = PassStats::default();

        for function in program.functions.values_mut() {
            let stats = self.optimize_function(function)?;
            total_stats.merge(stats);
        }

        Ok(total_stats)
    }

    /// Check if this pass is enabled in the given configuration
    fn is_enabled(&self, config: &MirConfig) -> bool;
}

/// Statistics for a single optimization pass
#[derive(Debug, Default, Clone)]
pub struct PassStats {
    pub instructions_eliminated: usize,
    pub blocks_eliminated: usize,
    pub constants_folded: usize,
    pub functions_inlined: usize,
}

impl PassStats {
    /// Merge another PassStats into this one
    pub fn merge(&mut self, other: PassStats) {
        self.instructions_eliminated += other.instructions_eliminated;
        self.blocks_eliminated += other.blocks_eliminated;
        self.constants_folded += other.constants_folded;
        self.functions_inlined += other.functions_inlined;
    }
}

impl MirOptimizer {
    /// Create a new MIR optimizer with default passes
    pub fn new() -> Self {
        let mut passes: Vec<Box<dyn OptimizationPass>> = Vec::new();

        // Add optimization passes in order
        passes.push(Box::new(ConstantFoldingPass::new()));
        passes.push(Box::new(DeadCodeEliminationPass::new()));
        passes.push(Box::new(ControlFlowSimplificationPass::new()));
        passes.push(Box::new(PeepholeOptimizationPass::new()));
        passes.push(Box::new(FunctionInliningPass::new()));

        Self {
            config: MirConfig::default(),
            passes,
        }
    }

    /// Create a new MIR optimizer with custom configuration
    pub fn with_config(config: &MirConfig) -> Self {
        let mut optimizer = Self::new();
        optimizer.config = config.clone();
        optimizer
    }

    /// Optimize a MIR program
    pub fn optimize(
        &mut self,
        mut program: MirProgram,
    ) -> Result<(MirProgram, OptimizationStats), Vec<CompilerError>> {
        let start_time = std::time::Instant::now();
        let mut total_stats = OptimizationStats::default();

        // Apply optimization passes multiple times until convergence
        let max_iterations = match self.config.opt_level {
            0 => 0,
            1 => 1,
            2 => 3,
            3 => 5,
            _ => 3,
        };

        for _iteration in 0..max_iterations {
            let mut changed = false;

            for pass in &mut self.passes {
                if !pass.is_enabled(&self.config) {
                    continue;
                }

                match pass.optimize_program(&mut program) {
                    Ok(stats) => {
                        if stats.instructions_eliminated > 0
                            || stats.blocks_eliminated > 0
                            || stats.constants_folded > 0
                        {
                            changed = true;
                        }

                        total_stats.instructions_eliminated += stats.instructions_eliminated;
                        total_stats.blocks_eliminated += stats.blocks_eliminated;
                        total_stats.constants_folded += stats.constants_folded;
                        total_stats.functions_inlined += stats.functions_inlined;
                    }
                    Err(error) => {
                        return Err(vec![error]);
                    }
                }
            }

            // If no changes were made, we've reached convergence
            if !changed {
                break;
            }
        }

        total_stats.optimization_time_us = start_time.elapsed().as_micros() as u64;

        Ok((program, total_stats))
    }
}

/// Constant folding optimization pass
#[derive(Debug)]
pub struct ConstantFoldingPass {
    /// Constants discovered during analysis
    constant_values: HashMap<ValueId, MirConstant>,
}

impl ConstantFoldingPass {
    pub fn new() -> Self {
        Self {
            constant_values: HashMap::new(),
        }
    }

    /// Try to fold a binary operation with constant operands
    fn try_fold_binary_op(
        &self,
        op: &MirBinaryOp,
        left: &MirConstant,
        right: &MirConstant,
    ) -> Option<MirConstant> {
        match (left, right) {
            (MirConstant::Integer(a), MirConstant::Integer(b)) => match op {
                MirBinaryOp::Add => Some(MirConstant::Integer(a + b)),
                MirBinaryOp::Sub => Some(MirConstant::Integer(a - b)),
                MirBinaryOp::Mul => Some(MirConstant::Integer(a * b)),
                MirBinaryOp::Div if *b != 0 => Some(MirConstant::Integer(a / b)),
                MirBinaryOp::Rem if *b != 0 => Some(MirConstant::Integer(a % b)),
                MirBinaryOp::Eq => Some(MirConstant::Boolean(a == b)),
                MirBinaryOp::Ne => Some(MirConstant::Boolean(a != b)),
                MirBinaryOp::Lt => Some(MirConstant::Boolean(a < b)),
                MirBinaryOp::Le => Some(MirConstant::Boolean(a <= b)),
                MirBinaryOp::Gt => Some(MirConstant::Boolean(a > b)),
                MirBinaryOp::Ge => Some(MirConstant::Boolean(a >= b)),
                _ => None,
            },
            (MirConstant::Float(a), MirConstant::Float(b)) => match op {
                MirBinaryOp::Add => Some(MirConstant::Float(a + b)),
                MirBinaryOp::Sub => Some(MirConstant::Float(a - b)),
                MirBinaryOp::Mul => Some(MirConstant::Float(a * b)),
                MirBinaryOp::Div if *b != 0.0 => Some(MirConstant::Float(a / b)),
                MirBinaryOp::Eq => Some(MirConstant::Boolean((a - b).abs() < f64::EPSILON)),
                MirBinaryOp::Ne => Some(MirConstant::Boolean((a - b).abs() >= f64::EPSILON)),
                MirBinaryOp::Lt => Some(MirConstant::Boolean(a < b)),
                MirBinaryOp::Le => Some(MirConstant::Boolean(a <= b)),
                MirBinaryOp::Gt => Some(MirConstant::Boolean(a > b)),
                MirBinaryOp::Ge => Some(MirConstant::Boolean(a >= b)),
                _ => None,
            },
            (MirConstant::Boolean(a), MirConstant::Boolean(b)) => match op {
                MirBinaryOp::And => Some(MirConstant::Boolean(*a && *b)),
                MirBinaryOp::Or => Some(MirConstant::Boolean(*a || *b)),
                MirBinaryOp::Eq => Some(MirConstant::Boolean(a == b)),
                MirBinaryOp::Ne => Some(MirConstant::Boolean(a != b)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Try to fold a unary operation with constant operand
    fn try_fold_unary_op(&self, op: &MirUnaryOp, operand: &MirConstant) -> Option<MirConstant> {
        match (op, operand) {
            (MirUnaryOp::Neg, MirConstant::Integer(i)) => Some(MirConstant::Integer(-i)),
            (MirUnaryOp::Neg, MirConstant::Float(f)) => Some(MirConstant::Float(-f)),
            (MirUnaryOp::Not, MirConstant::Boolean(b)) => Some(MirConstant::Boolean(!b)),
            _ => None,
        }
    }
}

impl OptimizationPass for ConstantFoldingPass {
    fn name(&self) -> &str {
        "constant-folding"
    }

    fn optimize_function(
        &mut self,
        function: &mut MirFunction,
    ) -> Result<PassStats, CompilerError> {
        let mut stats = PassStats::default();
        self.constant_values.clear();

        // Track constants through the function
        for block in function.blocks.values_mut() {
            let _instructions_to_remove: Vec<usize> = Vec::new();

            for (_i, instruction) in block.instructions.iter_mut().enumerate() {
                match &mut instruction.operation {
                    MirOperation::Copy {
                        source: MirOperand::Constant(constant),
                    } => {
                        // Track this constant value
                        if let Some(dest) = instruction.dest {
                            self.constant_values.insert(dest, constant.clone());
                        }
                    }

                    MirOperation::BinaryOp { op, left, right } => {
                        // Try to resolve operands to constants
                        let left_const = match left {
                            MirOperand::Constant(c) => Some(c.clone()),
                            MirOperand::Value(id) => self.constant_values.get(id).cloned(),
                            _ => None,
                        };

                        let right_const = match right {
                            MirOperand::Constant(c) => Some(c.clone()),
                            MirOperand::Value(id) => self.constant_values.get(id).cloned(),
                            _ => None,
                        };

                        // Try to fold if both operands are constants
                        if let (Some(left_val), Some(right_val)) = (left_const, right_const) {
                            if let Some(result) = self.try_fold_binary_op(op, &left_val, &right_val)
                            {
                                // Replace with constant assignment
                                instruction.operation = MirOperation::Copy {
                                    source: MirOperand::Constant(result.clone()),
                                };

                                // Track the result constant
                                if let Some(dest) = instruction.dest {
                                    self.constant_values.insert(dest, result);
                                }

                                stats.constants_folded += 1;
                            }
                        }
                    }

                    MirOperation::UnaryOp { op, operand } => {
                        // Try to resolve operand to constant
                        let operand_const = match operand {
                            MirOperand::Constant(c) => Some(c.clone()),
                            MirOperand::Value(id) => self.constant_values.get(id).cloned(),
                            _ => None,
                        };

                        // Try to fold if operand is constant
                        if let Some(operand_val) = operand_const {
                            if let Some(result) = self.try_fold_unary_op(op, &operand_val) {
                                // Replace with constant assignment
                                instruction.operation = MirOperation::Copy {
                                    source: MirOperand::Constant(result.clone()),
                                };

                                // Track the result constant
                                if let Some(dest) = instruction.dest {
                                    self.constant_values.insert(dest, result);
                                }

                                stats.constants_folded += 1;
                            }
                        }
                    }

                    _ => {}
                }
            }
        }

        Ok(stats)
    }

    fn is_enabled(&self, config: &MirConfig) -> bool {
        config.constant_folding
    }
}

/// Dead code elimination optimization pass
#[derive(Debug)]
pub struct DeadCodeEliminationPass {
    /// Values that are used (live)
    live_values: HashSet<ValueId>,
}

impl DeadCodeEliminationPass {
    pub fn new() -> Self {
        Self {
            live_values: HashSet::new(),
        }
    }

    /// Mark a value as live and recursively mark its dependencies
    fn mark_live(&mut self, value_id: ValueId, function: &MirFunction) {
        if self.live_values.contains(&value_id) {
            return; // Already marked
        }

        self.live_values.insert(value_id);

        // Find the instruction that defines this value and mark its operands as live
        for block in function.blocks.values() {
            for instruction in &block.instructions {
                if instruction.dest == Some(value_id) {
                    // Mark operands as live
                    self.mark_operands_live(&instruction.operation, function);
                    break;
                }
            }
        }
    }

    /// Mark operands of an operation as live
    fn mark_operands_live(&mut self, operation: &MirOperation, function: &MirFunction) {
        match operation {
            MirOperation::Copy { source } => {
                self.mark_operand_live(source, function);
            }
            MirOperation::Move { source } => {
                self.mark_operand_live(source, function);
            }
            MirOperation::BinaryOp { left, right, .. } => {
                self.mark_operand_live(left, function);
                self.mark_operand_live(right, function);
            }
            MirOperation::UnaryOp { operand, .. } => {
                self.mark_operand_live(operand, function);
            }
            MirOperation::Cast { value, .. } => {
                self.mark_operand_live(value, function);
            }
            MirOperation::Call {
                function: func,
                arguments,
            } => {
                self.mark_operand_live(func, function);
                for arg in arguments {
                    self.mark_operand_live(arg, function);
                }
            }
            MirOperation::Load { source } => {
                self.mark_operand_live(source, function);
            }
            MirOperation::GetElementPtr { base, indices, .. } => {
                self.mark_operand_live(base, function);
                for index in indices {
                    self.mark_operand_live(index, function);
                }
            }
            MirOperation::Alloca { size, .. } => {
                self.mark_operand_live(size, function);
            }
            MirOperation::Phi { incoming } => {
                for (_block, operand) in incoming {
                    self.mark_operand_live(operand, function);
                }
            }
            MirOperation::AsyncAssign { source } => {
                self.mark_operand_live(source, function);
            }
            MirOperation::BoxAny { value, .. } => {
                self.mark_operand_live(value, function);
            }
            MirOperation::AnyToString { value } => {
                self.mark_operand_live(value, function);
            }
            MirOperation::UnboxAnyToI32 { value } => {
                self.mark_operand_live(value, function);
            }
            MirOperation::UnboxAnyToF64 { value } => {
                self.mark_operand_live(value, function);
            }
            MirOperation::Store { .. } => {
                // Store is handled separately in optimize_function
            }
        }
    }

    /// Mark a single operand as live
    fn mark_operand_live(&mut self, operand: &MirOperand, function: &MirFunction) {
        if let MirOperand::Value(value_id) = operand {
            self.mark_live(*value_id, function);
        }
    }
}

impl OptimizationPass for DeadCodeEliminationPass {
    fn name(&self) -> &str {
        "dead-code-elimination"
    }

    fn optimize_function(
        &mut self,
        function: &mut MirFunction,
    ) -> Result<PassStats, CompilerError> {
        let mut stats = PassStats::default();
        self.live_values.clear();

        // Step 1: Mark all side-effect instructions and their dependencies as live
        for block in function.blocks.values() {
            // Terminators are always live
            match &block.terminator {
                MirTerminator::Return {
                    value: Some(operand),
                } => {
                    self.mark_operand_live(operand, function);
                }
                MirTerminator::Branch { condition, .. } => {
                    self.mark_operand_live(condition, function);
                }
                _ => {}
            }

            // Instructions with side effects are live
            for instruction in &block.instructions {
                match &instruction.operation {
                    MirOperation::Store { destination, value } => {
                        // Store instructions are always live (side effect)
                        if let Some(dest_id) = instruction.dest {
                            self.mark_live(dest_id, function);
                        }
                        self.mark_operand_live(destination, function);
                        self.mark_operand_live(value, function);
                    }
                    MirOperation::Call { .. } => {
                        // Function calls are always live (potential side effects)
                        if let Some(dest_id) = instruction.dest {
                            self.mark_live(dest_id, function);
                        }
                        // CRITICAL FIX: Mark all operands (function and arguments) as live
                        self.mark_operands_live(&instruction.operation, function);
                    }
                    _ => {}
                }
            }
        }

        // Step 2: Remove dead instructions
        for block in function.blocks.values_mut() {
            let original_len = block.instructions.len();

            block.instructions.retain(|instruction| {
                match instruction.dest {
                    Some(dest_id) => self.live_values.contains(&dest_id),
                    None => true, // Instructions without destinations are kept (side effects)
                }
            });

            stats.instructions_eliminated += original_len - block.instructions.len();
        }

        Ok(stats)
    }

    fn is_enabled(&self, config: &MirConfig) -> bool {
        config.dead_code_elimination
    }
}

/// Control flow simplification pass
#[derive(Debug)]
pub struct ControlFlowSimplificationPass;

impl ControlFlowSimplificationPass {
    pub fn new() -> Self {
        Self
    }
}

impl OptimizationPass for ControlFlowSimplificationPass {
    fn name(&self) -> &str {
        "control-flow-simplification"
    }

    fn optimize_function(
        &mut self,
        _function: &mut MirFunction,
    ) -> Result<PassStats, CompilerError> {
        let stats = PassStats::default();

        // Control flow simplification (placeholder for future optimization)
        // Potential improvements: unreachable block removal, block merging, branch simplification

        Ok(stats)
    }

    fn is_enabled(&self, config: &MirConfig) -> bool {
        config.optimize && config.opt_level >= 1
    }
}

/// Peephole optimization pass
#[derive(Debug)]
pub struct PeepholeOptimizationPass;

impl PeepholeOptimizationPass {
    pub fn new() -> Self {
        Self
    }
}

impl OptimizationPass for PeepholeOptimizationPass {
    fn name(&self) -> &str {
        "peephole-optimization"
    }

    fn optimize_function(
        &mut self,
        _function: &mut MirFunction,
    ) -> Result<PassStats, CompilerError> {
        let stats = PassStats::default();

        // Peephole optimizations (placeholder for future optimization)
        // Potential improvements: strength reduction, identity operations, algebraic simplification

        Ok(stats)
    }

    fn is_enabled(&self, config: &MirConfig) -> bool {
        config.optimize && config.opt_level >= 2
    }
}

/// Function inlining optimization pass
#[derive(Debug)]
pub struct FunctionInliningPass {
    /// Functions eligible for inlining
    inline_candidates: HashMap<crate::resolver::SymbolId, MirFunction>,
}

impl FunctionInliningPass {
    pub fn new() -> Self {
        Self {
            inline_candidates: HashMap::new(),
        }
    }
}

impl OptimizationPass for FunctionInliningPass {
    fn name(&self) -> &str {
        "function-inlining"
    }

    fn optimize_function(
        &mut self,
        _function: &mut MirFunction,
    ) -> Result<PassStats, CompilerError> {
        let stats = PassStats::default();

        // Function inlining (placeholder for future optimization)
        // Potential improvements: small function inlining, size-based thresholds

        Ok(stats)
    }

    fn optimize_program(&mut self, program: &mut MirProgram) -> Result<PassStats, CompilerError> {
        // Collect inlining candidates and apply inlining
        let mut total_stats = PassStats::default();

        for function in program.functions.values_mut() {
            let stats = self.optimize_function(function)?;
            total_stats.merge(stats);
        }

        Ok(total_stats)
    }

    fn is_enabled(&self, config: &MirConfig) -> bool {
        config.function_inlining
    }
}

impl Default for MirOptimizer {
    fn default() -> Self {
        Self::new()
    }
}
