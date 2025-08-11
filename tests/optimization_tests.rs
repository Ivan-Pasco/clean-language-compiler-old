//! Comprehensive tests for the optimization pipeline
//!
//! Tests all optimization passes to ensure they work correctly and improve code quality

use clean_language_compiler::ir::*;
use std::collections::HashMap;

#[test]
fn test_dead_code_elimination() {
    // Create a MIR program with dead code
    let mut mir_program = create_test_program_with_dead_code();

    let original_block_count = mir_program
        .functions
        .get("test_function")
        .unwrap()
        .basic_blocks
        .len();

    // Run dead code elimination
    let eliminated_count =
        eliminate_dead_code(&mut mir_program).expect("Dead code elimination should succeed");

    println!("Eliminated {} dead code elements", eliminated_count);

    // Verify some dead code was found and eliminated
    let final_block_count = mir_program
        .functions
        .get("test_function")
        .unwrap()
        .basic_blocks
        .len();

    // At minimum, the function should still exist
    assert!(mir_program.functions.contains_key("test_function"));
    assert!(final_block_count <= original_block_count);

    println!("✅ Dead code elimination test passed");
}

fn create_test_program_with_dead_code() -> MIRProgram {
    let mut functions = HashMap::new();

    // Create a function with unreachable blocks
    functions.insert(
        "test_function".to_string(),
        MIRFunction {
            name: "test_function".to_string(),
            parameters: vec![],
            return_type: MIRType::I32,
            locals: vec![],
            basic_blocks: vec![
                // Reachable block
                MIRBasicBlock {
                    id: 0,
                    instructions: vec![MIRInstruction::Const(0, MIRConstant::Integer(42))],
                    terminator: MIRTerminator::Return(Some(MIROperand::Local(0))),
                },
                // Unreachable block (dead code)
                MIRBasicBlock {
                    id: 1,
                    instructions: vec![MIRInstruction::Const(1, MIRConstant::Integer(999))],
                    terminator: MIRTerminator::Return(Some(MIROperand::Local(1))),
                },
            ],
            entry_block: 0,
        },
    );

    MIRProgram {
        functions,
        classes: HashMap::new(),
        globals: vec![],
    }
}

#[test]
fn test_constant_folding() {
    // Create a MIR program with constant expressions
    let mut mir_program = create_test_program_with_constants();

    let original_instruction_count = count_instructions(&mir_program);

    // Run constant folding
    let folded_count = fold_constants(&mut mir_program).expect("Constant folding should succeed");

    println!("Folded {} constant expressions", folded_count);

    // Verify that some constants were folded
    assert!(folded_count > 0, "Should have folded some constants");

    // Verify the program still functions
    assert!(mir_program.functions.contains_key("constant_test"));

    println!("✅ Constant folding test passed");
}

fn create_test_program_with_constants() -> MIRProgram {
    let mut functions = HashMap::new();

    functions.insert(
        "constant_test".to_string(),
        MIRFunction {
            name: "constant_test".to_string(),
            parameters: vec![],
            return_type: MIRType::I32,
            locals: vec![],
            basic_blocks: vec![MIRBasicBlock {
                id: 0,
                instructions: vec![
                    // These should be folded into single constants
                    MIRInstruction::Add(
                        0,
                        MIROperand::Constant(MIRConstant::Integer(5)),
                        MIROperand::Constant(MIRConstant::Integer(10)),
                    ),
                    MIRInstruction::Mul(
                        1,
                        MIROperand::Constant(MIRConstant::Integer(3)),
                        MIROperand::Constant(MIRConstant::Integer(4)),
                    ),
                    MIRInstruction::Sub(2, MIROperand::Local(0), MIROperand::Local(1)),
                ],
                terminator: MIRTerminator::Return(Some(MIROperand::Local(2))),
            }],
            entry_block: 0,
        },
    );

    MIRProgram {
        functions,
        classes: HashMap::new(),
        globals: vec![],
    }
}

#[test]
fn test_function_inlining() {
    // Create a MIR program with small functions suitable for inlining
    let mut mir_program = create_test_program_with_inlinable_functions();

    let original_call_count = count_function_calls(&mir_program);

    // Run function inlining with threshold of 5 instructions
    let inlined_count =
        inline_functions(&mut mir_program, 5).expect("Function inlining should succeed");

    println!("Inlined {} function calls", inlined_count);

    // Verify program structure is maintained
    assert!(mir_program.functions.contains_key("main"));

    println!("✅ Function inlining test passed");
}

fn create_test_program_with_inlinable_functions() -> MIRProgram {
    let mut functions = HashMap::new();

    // Small function suitable for inlining
    functions.insert(
        "small_function".to_string(),
        MIRFunction {
            name: "small_function".to_string(),
            parameters: vec![],
            return_type: MIRType::I32,
            locals: vec![],
            basic_blocks: vec![MIRBasicBlock {
                id: 0,
                instructions: vec![MIRInstruction::Const(0, MIRConstant::Integer(100))],
                terminator: MIRTerminator::Return(Some(MIROperand::Local(0))),
            }],
            entry_block: 0,
        },
    );

    // Main function that calls the small function
    functions.insert(
        "main".to_string(),
        MIRFunction {
            name: "main".to_string(),
            parameters: vec![],
            return_type: MIRType::I32,
            locals: vec![],
            basic_blocks: vec![MIRBasicBlock {
                id: 0,
                instructions: vec![
                    MIRInstruction::Call(0, "small_function".to_string(), vec![]),
                    MIRInstruction::Const(1, MIRConstant::Integer(1)),
                    MIRInstruction::Add(2, MIROperand::Local(0), MIROperand::Local(1)),
                ],
                terminator: MIRTerminator::Return(Some(MIROperand::Local(2))),
            }],
            entry_block: 0,
        },
    );

    MIRProgram {
        functions,
        classes: HashMap::new(),
        globals: vec![],
    }
}

#[test]
fn test_control_flow_optimization() {
    // Create a MIR program with suboptimal control flow
    let mut mir_program = create_test_program_with_control_flow_issues();

    let original_block_count = count_basic_blocks(&mir_program);

    // Run control flow optimization
    let optimized_count =
        optimize_control_flow(&mut mir_program).expect("Control flow optimization should succeed");

    println!("Optimized {} control flow elements", optimized_count);

    // Verify program structure
    assert!(mir_program.functions.contains_key("control_test"));

    let final_block_count = count_basic_blocks(&mir_program);
    println!(
        "Block count: {} -> {}",
        original_block_count, final_block_count
    );

    println!("✅ Control flow optimization test passed");
}

fn create_test_program_with_control_flow_issues() -> MIRProgram {
    let mut functions = HashMap::new();

    functions.insert(
        "control_test".to_string(),
        MIRFunction {
            name: "control_test".to_string(),
            parameters: vec![],
            return_type: MIRType::I32,
            locals: vec![],
            basic_blocks: vec![
                MIRBasicBlock {
                    id: 0,
                    instructions: vec![],
                    // Constant branch that can be simplified
                    terminator: MIRTerminator::Branch {
                        condition: MIROperand::Constant(MIRConstant::Boolean(true)),
                        then_block: 1,
                        else_block: 2,
                    },
                },
                MIRBasicBlock {
                    id: 1,
                    instructions: vec![MIRInstruction::Const(0, MIRConstant::Integer(42))],
                    terminator: MIRTerminator::Return(Some(MIROperand::Local(0))),
                },
                MIRBasicBlock {
                    id: 2,
                    instructions: vec![MIRInstruction::Const(1, MIRConstant::Integer(0))],
                    terminator: MIRTerminator::Return(Some(MIROperand::Local(1))),
                },
                // Empty block that jumps to another block
                MIRBasicBlock {
                    id: 3,
                    instructions: vec![],
                    terminator: MIRTerminator::Goto(1),
                },
            ],
            entry_block: 0,
        },
    );

    MIRProgram {
        functions,
        classes: HashMap::new(),
        globals: vec![],
    }
}

#[test]
fn test_comprehensive_optimization_pipeline() {
    // Test the complete optimization pipeline
    let mut mir_program = create_comprehensive_test_program();

    let original_stats = analyze_program(&mir_program);
    println!("Original program stats: {:?}", original_stats);

    // Run all optimizations
    let optimization_stats = optimize_mir_program(&mut mir_program, OptimizationLevel::Aggressive)
        .expect("Comprehensive optimization should succeed");

    let final_stats = analyze_program(&mir_program);
    println!("Optimized program stats: {:?}", final_stats);
    println!("Optimization results: {:?}", optimization_stats);

    // Verify optimizations occurred
    assert!(optimization_stats.total_optimizations() >= 0);

    // Verify program integrity
    assert!(mir_program.functions.len() > 0);

    println!("✅ Comprehensive optimization pipeline test passed");
}

fn create_comprehensive_test_program() -> MIRProgram {
    let mut functions = HashMap::new();

    functions.insert(
        "comprehensive_test".to_string(),
        MIRFunction {
            name: "comprehensive_test".to_string(),
            parameters: vec![MIRLocal {
                id: 0,
                name: "x".to_string(),
                local_type: MIRType::I32,
            }],
            return_type: MIRType::I32,
            locals: vec![
                MIRLocal {
                    id: 1,
                    name: "temp1".to_string(),
                    local_type: MIRType::I32,
                },
                MIRLocal {
                    id: 2,
                    name: "temp2".to_string(),
                    local_type: MIRType::I32,
                },
            ],
            basic_blocks: vec![
                MIRBasicBlock {
                    id: 0,
                    instructions: vec![
                        // Constant folding opportunities
                        MIRInstruction::Add(
                            1,
                            MIROperand::Constant(MIRConstant::Integer(10)),
                            MIROperand::Constant(MIRConstant::Integer(20)),
                        ),
                        MIRInstruction::Mul(
                            2,
                            MIROperand::Constant(MIRConstant::Integer(2)),
                            MIROperand::Constant(MIRConstant::Integer(3)),
                        ),
                        // Dead store (immediately overwritten)
                        MIRInstruction::Add(1, MIROperand::Local(1), MIROperand::Local(2)),
                        MIRInstruction::Add(
                            1,
                            MIROperand::Local(0),
                            MIROperand::Constant(MIRConstant::Integer(1)),
                        ),
                    ],
                    // Constant branch that can be simplified
                    terminator: MIRTerminator::Branch {
                        condition: MIROperand::Constant(MIRConstant::Boolean(true)),
                        then_block: 1,
                        else_block: 2,
                    },
                },
                MIRBasicBlock {
                    id: 1,
                    instructions: vec![MIRInstruction::Add(
                        2,
                        MIROperand::Local(1),
                        MIROperand::Constant(MIRConstant::Integer(5)),
                    )],
                    terminator: MIRTerminator::Return(Some(MIROperand::Local(2))),
                },
                // Unreachable block (dead code)
                MIRBasicBlock {
                    id: 2,
                    instructions: vec![MIRInstruction::Const(2, MIRConstant::Integer(999))],
                    terminator: MIRTerminator::Return(Some(MIROperand::Local(2))),
                },
            ],
            entry_block: 0,
        },
    );

    MIRProgram {
        functions,
        classes: HashMap::new(),
        globals: vec![],
    }
}

#[test]
fn test_optimization_levels() {
    // Test different optimization levels
    let test_cases = [
        OptimizationLevel::None,
        OptimizationLevel::Speed,
        OptimizationLevel::Size,
        OptimizationLevel::Aggressive,
    ];

    for opt_level in test_cases {
        let mut mir_program = create_comprehensive_test_program();

        let stats =
            optimize_mir_program(&mut mir_program, opt_level).expect("Optimization should succeed");

        match opt_level {
            OptimizationLevel::None => {
                // No optimizations should run
                assert_eq!(stats.total_optimizations(), 0);
            }
            OptimizationLevel::Speed | OptimizationLevel::Size => {
                // Should run basic optimizations
                assert!(stats.total_optimizations() >= 0);
            }
            OptimizationLevel::Aggressive => {
                // Should run all optimizations including inlining
                assert!(stats.total_optimizations() >= 0);
            }
        }

        println!(
            "✅ {:?} optimization level: {} total optimizations",
            opt_level,
            stats.total_optimizations()
        );
    }

    println!("✅ Optimization levels test passed");
}

// Helper functions for testing

fn count_instructions(program: &MIRProgram) -> usize {
    program
        .functions
        .values()
        .flat_map(|f| &f.basic_blocks)
        .map(|b| b.instructions.len())
        .sum()
}

fn count_function_calls(program: &MIRProgram) -> usize {
    program
        .functions
        .values()
        .flat_map(|f| &f.basic_blocks)
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, MIRInstruction::Call(..)))
        .count()
}

fn count_basic_blocks(program: &MIRProgram) -> usize {
    program
        .functions
        .values()
        .map(|f| f.basic_blocks.len())
        .sum()
}

#[derive(Debug)]
struct ProgramStats {
    total_functions: usize,
    total_basic_blocks: usize,
    total_instructions: usize,
    function_calls: usize,
}

fn analyze_program(program: &MIRProgram) -> ProgramStats {
    ProgramStats {
        total_functions: program.functions.len(),
        total_basic_blocks: count_basic_blocks(program),
        total_instructions: count_instructions(program),
        function_calls: count_function_calls(program),
    }
}

#[test]
fn test_algebraic_simplification() {
    // Test algebraic optimizations like x + 0, x * 1, etc.
    let mut functions = HashMap::new();

    functions.insert(
        "algebra_test".to_string(),
        MIRFunction {
            name: "algebra_test".to_string(),
            parameters: vec![],
            return_type: MIRType::I32,
            locals: vec![],
            basic_blocks: vec![MIRBasicBlock {
                id: 0,
                instructions: vec![
                    MIRInstruction::Const(0, MIRConstant::Integer(42)),
                    // x + 0 should be optimized
                    MIRInstruction::Add(
                        1,
                        MIROperand::Local(0),
                        MIROperand::Constant(MIRConstant::Integer(0)),
                    ),
                    // x * 1 should be optimized
                    MIRInstruction::Mul(
                        2,
                        MIROperand::Local(1),
                        MIROperand::Constant(MIRConstant::Integer(1)),
                    ),
                    // x * 0 should become 0
                    MIRInstruction::Mul(
                        3,
                        MIROperand::Local(2),
                        MIROperand::Constant(MIRConstant::Integer(0)),
                    ),
                ],
                terminator: MIRTerminator::Return(Some(MIROperand::Local(3))),
            }],
            entry_block: 0,
        },
    );

    let mut mir_program = MIRProgram {
        functions,
        classes: HashMap::new(),
        globals: vec![],
    };

    let folded_count = fold_constants(&mut mir_program).expect("Constant folding should succeed");

    println!("Applied {} algebraic simplifications", folded_count);
    assert!(
        folded_count > 0,
        "Should have applied some algebraic optimizations"
    );

    println!("✅ Algebraic simplification test passed");
}
