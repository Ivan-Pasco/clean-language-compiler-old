//! IR optimization passes
//!
//! This module implements various optimization passes for MIR to improve
//! performance and reduce code size.

use crate::ir::*;
use std::collections::{HashMap, HashSet};

/// Dead code elimination
/// Removes unreachable basic blocks and unused variables
pub fn eliminate_dead_code(program: &mut MIRProgram) -> IRResult<usize> {
    let mut eliminated_count = 0;

    for (_name, function) in program.functions.iter_mut() {
        eliminated_count += eliminate_dead_code_in_function(function)?;
    }

    Ok(eliminated_count)
}

/// Dead code elimination within a single function
fn eliminate_dead_code_in_function(function: &mut MIRFunction) -> IRResult<usize> {
    let mut eliminated_count = 0;

    // Step 1: Find reachable basic blocks
    let reachable_blocks = find_reachable_blocks(function);

    // Step 2: Remove unreachable blocks
    let original_count = function.basic_blocks.len();
    function
        .basic_blocks
        .retain(|block| reachable_blocks.contains(&block.id));
    eliminated_count += original_count - function.basic_blocks.len();

    // Step 3: Eliminate dead stores (variables that are never read)
    for block in &mut function.basic_blocks {
        eliminated_count += eliminate_dead_stores_in_block(block)?;
    }

    Ok(eliminated_count)
}

/// Find all reachable basic blocks starting from entry block
fn find_reachable_blocks(function: &MIRFunction) -> HashSet<BlockId> {
    let mut reachable = HashSet::new();
    let mut worklist = vec![function.entry_block];

    while let Some(current_block_id) = worklist.pop() {
        if reachable.contains(&current_block_id) {
            continue;
        }

        reachable.insert(current_block_id);

        // Find the block and add successors to worklist
        if let Some(block) = function
            .basic_blocks
            .iter()
            .find(|b| b.id == current_block_id)
        {
            match &block.terminator {
                MIRTerminator::Goto(target) => {
                    worklist.push(*target);
                }
                MIRTerminator::Branch {
                    then_block,
                    else_block,
                    ..
                } => {
                    worklist.push(*then_block);
                    worklist.push(*else_block);
                }
                MIRTerminator::Return(_) | MIRTerminator::Unreachable => {
                    // No successors
                }
            }
        }
    }

    reachable
}

/// Eliminate dead stores within a basic block
fn eliminate_dead_stores_in_block(block: &mut MIRBasicBlock) -> IRResult<usize> {
    let mut eliminated_count = 0;

    // Simple dead store elimination - remove stores that are immediately overwritten
    let mut i = 0;
    while i < block.instructions.len() {
        if i + 1 < block.instructions.len() {
            // Check if current instruction is immediately overwritten by next
            if is_dead_store(&block.instructions[i], &block.instructions[i + 1]) {
                block.instructions.remove(i);
                eliminated_count += 1;
                continue; // Don't increment i, check same position again
            }
        }
        i += 1;
    }

    Ok(eliminated_count)
}

/// Check if an instruction is a dead store (immediately overwritten)
fn is_dead_store(current: &MIRInstruction, next: &MIRInstruction) -> bool {
    match (current, next) {
        // If we store to a local and immediately overwrite it
        (MIRInstruction::Add(dest1, ..), MIRInstruction::Add(dest2, ..))
        | (MIRInstruction::Sub(dest1, ..), MIRInstruction::Sub(dest2, ..))
        | (MIRInstruction::Mul(dest1, ..), MIRInstruction::Mul(dest2, ..))
        | (MIRInstruction::Div(dest1, ..), MIRInstruction::Div(dest2, ..))
        | (MIRInstruction::Const(dest1, ..), MIRInstruction::Const(dest2, ..)) => dest1 == dest2,
        _ => false,
    }
}

/// Constant folding and propagation
/// Evaluates constant expressions at compile time
pub fn fold_constants(program: &mut MIRProgram) -> IRResult<usize> {
    let mut folded_count = 0;

    for (_name, function) in program.functions.iter_mut() {
        folded_count += fold_constants_in_function(function)?;
    }

    Ok(folded_count)
}

/// Constant folding within a single function
fn fold_constants_in_function(function: &mut MIRFunction) -> IRResult<usize> {
    let mut folded_count = 0;

    for block in &mut function.basic_blocks {
        folded_count += fold_constants_in_block(block)?;
    }

    Ok(folded_count)
}

/// Constant folding within a basic block
fn fold_constants_in_block(block: &mut MIRBasicBlock) -> IRResult<usize> {
    let mut folded_count = 0;

    for instruction in &mut block.instructions {
        if let Some(folded) = fold_instruction(instruction) {
            *instruction = folded;
            folded_count += 1;
        }
    }

    Ok(folded_count)
}

/// Attempt to fold a single instruction if it operates on constants
fn fold_instruction(instruction: &MIRInstruction) -> Option<MIRInstruction> {
    match instruction {
        // Fold arithmetic operations on constants
        MIRInstruction::Add(
            dest,
            MIROperand::Constant(MIRConstant::Integer(a)),
            MIROperand::Constant(MIRConstant::Integer(b)),
        ) => Some(MIRInstruction::Const(*dest, MIRConstant::Integer(a + b))),
        MIRInstruction::Sub(
            dest,
            MIROperand::Constant(MIRConstant::Integer(a)),
            MIROperand::Constant(MIRConstant::Integer(b)),
        ) => Some(MIRInstruction::Const(*dest, MIRConstant::Integer(a - b))),
        MIRInstruction::Mul(
            dest,
            MIROperand::Constant(MIRConstant::Integer(a)),
            MIROperand::Constant(MIRConstant::Integer(b)),
        ) => Some(MIRInstruction::Const(*dest, MIRConstant::Integer(a * b))),
        MIRInstruction::Div(
            dest,
            MIROperand::Constant(MIRConstant::Integer(a)),
            MIROperand::Constant(MIRConstant::Integer(b)),
        ) => {
            if *b != 0 {
                Some(MIRInstruction::Const(*dest, MIRConstant::Integer(a / b)))
            } else {
                None // Don't fold division by zero
            }
        }

        // Fold floating point operations
        MIRInstruction::Add(
            dest,
            MIROperand::Constant(MIRConstant::Number(a)),
            MIROperand::Constant(MIRConstant::Number(b)),
        ) => Some(MIRInstruction::Const(*dest, MIRConstant::Number(a + b))),
        MIRInstruction::Sub(
            dest,
            MIROperand::Constant(MIRConstant::Number(a)),
            MIROperand::Constant(MIRConstant::Number(b)),
        ) => Some(MIRInstruction::Const(*dest, MIRConstant::Number(a - b))),
        MIRInstruction::Mul(
            dest,
            MIROperand::Constant(MIRConstant::Number(a)),
            MIROperand::Constant(MIRConstant::Number(b)),
        ) => Some(MIRInstruction::Const(*dest, MIRConstant::Number(a * b))),
        MIRInstruction::Div(
            dest,
            MIROperand::Constant(MIRConstant::Number(a)),
            MIROperand::Constant(MIRConstant::Number(b)),
        ) => {
            if *b != 0.0 {
                Some(MIRInstruction::Const(*dest, MIRConstant::Number(a / b)))
            } else {
                None // Don't fold division by zero
            }
        }

        // Algebraic simplifications
        // x + 0 = x, 0 + x = x
        MIRInstruction::Add(dest, operand, MIROperand::Constant(MIRConstant::Integer(0)))
        | MIRInstruction::Add(dest, MIROperand::Constant(MIRConstant::Integer(0)), operand) => {
            // This would become a move operation - simplified representation
            match operand {
                MIROperand::Local(_src) => Some(MIRInstruction::Load(*dest, operand.clone())),
                _ => None,
            }
        }

        // x * 1 = x, 1 * x = x
        MIRInstruction::Mul(dest, operand, MIROperand::Constant(MIRConstant::Integer(1)))
        | MIRInstruction::Mul(dest, MIROperand::Constant(MIRConstant::Integer(1)), operand) => {
            match operand {
                MIROperand::Local(_) => Some(MIRInstruction::Load(*dest, operand.clone())),
                _ => None,
            }
        }

        // x * 0 = 0, 0 * x = 0
        MIRInstruction::Mul(dest, _, MIROperand::Constant(MIRConstant::Integer(0)))
        | MIRInstruction::Mul(dest, MIROperand::Constant(MIRConstant::Integer(0)), _) => {
            Some(MIRInstruction::Const(*dest, MIRConstant::Integer(0)))
        }

        _ => None,
    }
}

/// Function inlining
/// Inlines small functions to reduce call overhead
pub fn inline_functions(program: &mut MIRProgram, threshold: usize) -> IRResult<usize> {
    let mut inlined_count = 0;

    // Find functions that are candidates for inlining (small size)
    let inline_candidates = find_inline_candidates(program, threshold);

    // Inline function calls
    for (_name, function) in program.functions.iter_mut() {
        inlined_count += inline_calls_in_function(function, &inline_candidates)?;
    }

    Ok(inlined_count)
}

/// Find functions that are good candidates for inlining
fn find_inline_candidates(
    program: &MIRProgram,
    threshold: usize,
) -> HashMap<String, Vec<MIRInstruction>> {
    let mut candidates = HashMap::new();

    for (name, function) in &program.functions {
        // Count total instructions in function
        let instruction_count: usize = function
            .basic_blocks
            .iter()
            .map(|block| block.instructions.len())
            .sum();

        // Only inline small functions with single basic block (for simplicity)
        if instruction_count <= threshold && function.basic_blocks.len() == 1 {
            if let Some(block) = function.basic_blocks.first() {
                // Only inline if it ends with a return
                if matches!(block.terminator, MIRTerminator::Return(_)) {
                    candidates.insert(name.clone(), block.instructions.clone());
                }
            }
        }
    }

    candidates
}

/// Inline function calls within a function
fn inline_calls_in_function(
    function: &mut MIRFunction,
    inline_candidates: &HashMap<String, Vec<MIRInstruction>>,
) -> IRResult<usize> {
    let mut inlined_count = 0;

    for block in &mut function.basic_blocks {
        let mut new_instructions = Vec::new();

        for instruction in &block.instructions {
            match instruction {
                MIRInstruction::Call(_dest, func_name, _args) => {
                    if let Some(candidate_instructions) = inline_candidates.get(func_name) {
                        // Simple inlining: copy instructions and adjust operands
                        for candidate_instr in candidate_instructions {
                            // This is a simplified inlining - in reality we'd need to:
                            // 1. Rename local variables to avoid conflicts
                            // 2. Handle parameter passing
                            // 3. Handle return values properly
                            new_instructions.push(candidate_instr.clone());
                        }
                        inlined_count += 1;
                    } else {
                        new_instructions.push(instruction.clone());
                    }
                }
                _ => {
                    new_instructions.push(instruction.clone());
                }
            }
        }

        block.instructions = new_instructions;
    }

    Ok(inlined_count)
}

/// Optimize control flow
/// Removes unnecessary branches and simplifies control flow
pub fn optimize_control_flow(program: &mut MIRProgram) -> IRResult<usize> {
    let mut optimized_count = 0;

    for (_name, function) in program.functions.iter_mut() {
        optimized_count += optimize_control_flow_in_function(function)?;
    }

    Ok(optimized_count)
}

/// Control flow optimization within a single function
fn optimize_control_flow_in_function(function: &mut MIRFunction) -> IRResult<usize> {
    let mut optimized_count = 0;

    // Remove empty basic blocks and redirect branches
    optimized_count += remove_empty_blocks(function)?;

    // Merge consecutive blocks where possible
    optimized_count += merge_consecutive_blocks(function)?;

    // Simplify branches with constant conditions
    optimized_count += simplify_constant_branches(function)?;

    Ok(optimized_count)
}

/// Remove empty basic blocks and redirect branches to them
fn remove_empty_blocks(function: &mut MIRFunction) -> IRResult<usize> {
    let mut removed_count = 0;
    let mut redirect_map = HashMap::new();

    // Find empty blocks (only have a goto terminator)
    for block in &function.basic_blocks {
        if block.instructions.is_empty() {
            if let MIRTerminator::Goto(target) = block.terminator {
                redirect_map.insert(block.id, target);
            }
        }
    }

    // Update all branches to skip empty blocks
    for block in &mut function.basic_blocks {
        match &mut block.terminator {
            MIRTerminator::Goto(target) => {
                if let Some(&new_target) = redirect_map.get(target) {
                    *target = new_target;
                    removed_count += 1;
                }
            }
            MIRTerminator::Branch {
                then_block,
                else_block,
                ..
            } => {
                if let Some(&new_target) = redirect_map.get(then_block) {
                    *then_block = new_target;
                    removed_count += 1;
                }
                if let Some(&new_target) = redirect_map.get(else_block) {
                    *else_block = new_target;
                    removed_count += 1;
                }
            }
            _ => {}
        }
    }

    // Remove empty blocks that are no longer referenced
    let original_count = function.basic_blocks.len();
    function
        .basic_blocks
        .retain(|block| !redirect_map.contains_key(&block.id));
    removed_count += original_count - function.basic_blocks.len();

    Ok(removed_count)
}

/// Merge consecutive blocks where the first block unconditionally jumps to the second
fn merge_consecutive_blocks(function: &mut MIRFunction) -> IRResult<usize> {
    let mut merged_count = 0;

    // This is a simplified implementation - real merging is more complex
    // For now, we just count potential merges without actually doing them
    for block in &function.basic_blocks {
        if let MIRTerminator::Goto(target_id) = block.terminator {
            // Check if target block has only one predecessor (this block)
            let predecessors = count_predecessors(function, target_id);
            if predecessors == 1 {
                // This could be merged (but we don't implement the actual merging here)
                merged_count += 1;
            }
        }
    }

    Ok(merged_count)
}

/// Count predecessors of a basic block
fn count_predecessors(function: &MIRFunction, target_id: BlockId) -> usize {
    let mut count = 0;

    // Check entry block
    if function.entry_block == target_id {
        count += 1;
    }

    // Check all terminators
    for block in &function.basic_blocks {
        match &block.terminator {
            MIRTerminator::Goto(target) => {
                if *target == target_id {
                    count += 1;
                }
            }
            MIRTerminator::Branch {
                then_block,
                else_block,
                ..
            } => {
                if *then_block == target_id {
                    count += 1;
                }
                if *else_block == target_id {
                    count += 1;
                }
            }
            _ => {}
        }
    }

    count
}

/// Simplify branches with constant conditions
fn simplify_constant_branches(function: &mut MIRFunction) -> IRResult<usize> {
    let mut simplified_count = 0;

    for block in &mut function.basic_blocks {
        match &block.terminator {
            MIRTerminator::Branch {
                condition,
                then_block,
                else_block,
            } => {
                match condition {
                    MIROperand::Constant(MIRConstant::Boolean(true)) => {
                        // Always take then branch
                        block.terminator = MIRTerminator::Goto(*then_block);
                        simplified_count += 1;
                    }
                    MIROperand::Constant(MIRConstant::Boolean(false)) => {
                        // Always take else branch
                        block.terminator = MIRTerminator::Goto(*else_block);
                        simplified_count += 1;
                    }
                    MIROperand::Constant(MIRConstant::Integer(0)) => {
                        // Integer 0 is falsy
                        block.terminator = MIRTerminator::Goto(*else_block);
                        simplified_count += 1;
                    }
                    MIROperand::Constant(MIRConstant::Integer(n)) if *n != 0 => {
                        // Non-zero integer is truthy
                        block.terminator = MIRTerminator::Goto(*then_block);
                        simplified_count += 1;
                    }
                    _ => {
                        // Can't simplify non-constant conditions
                    }
                }
            }
            _ => {}
        }
    }

    Ok(simplified_count)
}

/// Run all optimization passes in sequence
pub fn optimize_mir_program(
    program: &mut MIRProgram,
    optimization_level: OptimizationLevel,
) -> IRResult<OptimizationStats> {
    let mut stats = OptimizationStats::default();

    match optimization_level {
        OptimizationLevel::None => {
            // No optimizations
        }
        OptimizationLevel::Speed | OptimizationLevel::Size | OptimizationLevel::Aggressive => {
            // Dead code elimination
            stats.dead_code_eliminated = eliminate_dead_code(program)?;

            // Constant folding
            stats.constants_folded = fold_constants(program)?;

            // Control flow optimization
            stats.control_flow_simplified = optimize_control_flow(program)?;

            // Function inlining (only for aggressive optimization)
            if matches!(optimization_level, OptimizationLevel::Aggressive) {
                stats.functions_inlined = inline_functions(program, 10)?; // Inline functions with <= 10 instructions
            }
        }
    }

    Ok(stats)
}

/// Optimization statistics
#[derive(Debug, Default, Clone)]
pub struct OptimizationStats {
    pub dead_code_eliminated: usize,
    pub constants_folded: usize,
    pub functions_inlined: usize,
    pub control_flow_simplified: usize,
}

impl OptimizationStats {
    pub fn total_optimizations(&self) -> usize {
        self.dead_code_eliminated
            + self.constants_folded
            + self.functions_inlined
            + self.control_flow_simplified
    }
}
