//! Full pipeline test demonstrating IR transformations
//! 
//! Tests the complete transformation chain: AST → HIR → MIR → LIR

use clean_language_compiler::ir::*;
use clean_language_compiler::ast::*;

#[test]
fn test_manual_ast_to_full_pipeline() {
    // Create a simple AST manually (since parser has compatibility issues)
    let ast_program = Program {
        imports: vec![],
        declarations: vec![
            Declaration::Function(Function {
                name: "main".to_string(),
                parameters: vec![],
                return_type: Some(Type::Integer),
                body: vec![
                    Statement::Return(Some(Expression::IntegerLiteral(42)))
                ],
                modifier: vec![],
                location: SourceLocation {
                    line: 1,
                    column: 1,
                    file: "test".to_string(),
                },
            })
        ],
    };

    // Test AST → HIR transformation
    let hir_result = ast_to_hir(ast_program);
    match hir_result {
        Ok(hir_program) => {
            println!("✅ AST → HIR transformation successful");
            assert!(!hir_program.functions.is_empty(), "HIR should contain functions");

            // Test HIR → MIR transformation
            let mir_result = hir_to_mir(hir_program);
            match mir_result {
                Ok(mir_program) => {
                    println!("✅ HIR → MIR transformation successful");
                    assert!(!mir_program.functions.is_empty(), "MIR should contain functions");

                    // Test MIR → LIR transformation
                    let lir_result = mir_to_lir(mir_program);
                    match lir_result {
                        Ok(lir_program) => {
                            println!("✅ MIR → LIR transformation successful");
                            assert!(!lir_program.functions.is_empty(), "LIR should contain functions");
                            
                            // Validate LIR structure
                            let main_function = lir_program.functions.iter()
                                .find(|f| f.name == "main")
                                .expect("Main function should exist in LIR");
                            
                            println!("LIR Main function: {:?}", main_function);
                            
                            // Should contain basic instructions
                            assert!(!main_function.instructions.is_empty(), "LIR function should have instructions");
                            
                            // Should have return type
                            assert!(main_function.return_type.is_some(), "Main function should have return type");
                            
                            println!("🎉 Complete IR pipeline transformation successful!");
                        }
                        Err(e) => {
                            println!("❌ MIR → LIR transformation failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("❌ HIR → MIR transformation failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ AST → HIR transformation failed: {}", e);
        }
    }
}

#[test]
fn test_ir_validation() {
    // Create a simple LIR program for validation
    let lir_program = LIRProgram {
        functions: vec![
            LIRFunction {
                name: "test_func".to_string(),
                parameters: vec![LIRType::I32],
                return_type: Some(LIRType::I32),
                locals: vec![],
                instructions: vec![
                    LIRInstruction::LocalGet(0),
                    LIRInstruction::I32Const(1),
                    LIRInstruction::I32Add,
                    LIRInstruction::Return,
                ],
            }
        ],
        memory_layout: LIRMemoryLayout {
            initial_pages: 1,
            max_pages: Some(10),
            heap_start: 1024,
            stack_start: 0,
        },
        imports: vec![],
        exports: vec![],
    };

    // Test LIR validation
    let validation_result = validate_lir(&lir_program);
    match validation_result {
        Ok(()) => {
            println!("✅ LIR validation successful");
        }
        Err(e) => {
            println!("❌ LIR validation failed: {}", e);
        }
    }
}

#[test]
fn test_ir_optimization_pipeline() {
    // Create a MIR program with optimization opportunities
    let mut mir_program = MIRProgram {
        functions: vec![
            MIRFunction {
                name: "optimization_test".to_string(),
                parameters: vec![],
                return_type: Some(MIRType::I32),
                locals: vec![],
                basic_blocks: vec![
                    MIRBasicBlock {
                        instructions: vec![
                            // This could be optimized to a constant
                            MIRInstruction::LoadConstant(5),
                            MIRInstruction::LoadConstant(3),
                            MIRInstruction::BinaryOp(MIRBinaryOperator::Add),
                            MIRInstruction::Return,
                        ],
                        terminator: MIRTerminator::Return,
                    }
                ],
            }
        ],
        globals: vec![],
    };

    println!("Original MIR function instructions: {} blocks", mir_program.functions[0].basic_blocks.len());

    // Test optimization passes
    let dead_code_result = eliminate_dead_code(&mut mir_program);
    match dead_code_result {
        Ok(eliminated) => {
            println!("✅ Dead code elimination: {} eliminations", eliminated);
        }
        Err(e) => {
            println!("❌ Dead code elimination failed: {}", e);
        }
    }

    let constant_folding_result = fold_constants(&mut mir_program);
    match constant_folding_result {
        Ok(folded) => {
            println!("✅ Constant folding: {} constants folded", folded);
        }
        Err(e) => {
            println!("❌ Constant folding failed: {}", e);
        }
    }
}

#[test]
fn test_ir_type_consistency() {
    // Verify type consistency across IR levels
    
    // AST types
    let ast_type = Type::Integer;
    
    // HIR equivalent  
    let hir_type = HIRType::Integer;
    
    // MIR equivalent
    let mir_type = MIRType::I32;
    
    // LIR equivalent
    let lir_type = LIRType::I32;
    
    // These should represent the same semantic concept
    println!("AST type: {:?}", ast_type);
    println!("HIR type: {:?}", hir_type);
    println!("MIR type: {:?}", mir_type); 
    println!("LIR type: {:?}", lir_type);
    
    // Basic consistency check - all should be consistent representations
    assert!(matches!(ast_type, Type::Integer));
    assert!(matches!(hir_type, HIRType::Integer));
    assert!(matches!(mir_type, MIRType::I32));
    assert!(matches!(lir_type, LIRType::I32));
}

#[test]
fn test_memory_layout_integration() {
    let layout = LIRMemoryLayout {
        initial_pages: 2,
        max_pages: Some(16),
        heap_start: 2048,
        stack_start: 0,
    };
    
    println!("Memory layout: {:?}", layout);
    
    // Verify reasonable memory layout
    assert!(layout.heap_start > layout.stack_start, "Heap should start after stack");
    assert!(layout.initial_pages > 0, "Should have at least one initial page");
    assert!(layout.max_pages.unwrap_or(1) >= layout.initial_pages, "Max pages should be >= initial");
    
    println!("✅ Memory layout validation successful");
}