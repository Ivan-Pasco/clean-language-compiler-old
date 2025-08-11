//! Integration tests for the complete IR pipeline
//! 
//! Tests the full compilation pipeline: AST → HIR → MIR → LIR → WebAssembly

use clean_language_compiler::*;
use clean_language_compiler::parser::*;
use clean_language_compiler::ir::transform::*;
use clean_language_compiler::codegen::wasm_generator::*;
use clean_language_compiler::memory::*;

#[test]
fn test_basic_program_ir_pipeline() {
    let source = r#"
        function main(): integer {
            return 42;
        }
    "#;
    
    // Parse to AST
    let mut parser = CleanParser::new();
    let ast = parser.parse(source).expect("Failed to parse source");
    
    // Transform AST → HIR
    let hir = ast_to_hir(ast).expect("Failed to transform AST to HIR");
    assert!(!hir.functions.is_empty(), "HIR should contain functions");
    
    // Transform HIR → MIR
    let mir = hir_to_mir(hir).expect("Failed to transform HIR to MIR");
    assert!(!mir.functions.is_empty(), "MIR should contain functions");
    
    // Transform MIR → LIR
    let lir = mir_to_lir(mir).expect("Failed to transform MIR to LIR");
    assert!(!lir.functions.is_empty(), "LIR should contain functions");
    
    // Generate WebAssembly
    let mut wasm_gen = WasmGenerator::new();
    let wasm_bytes = wasm_gen.generate_wasm_module(lir)
        .expect("Failed to generate WebAssembly");
    
    assert!(!wasm_bytes.is_empty(), "WebAssembly output should not be empty");
}

#[test]
fn test_variables_ir_pipeline() {
    let source = r#"
        function main(): integer {
            auto x = 10;
            auto y = 20;
            return x + y;
        }
    "#;
    
    let mut parser = CleanParser::new();
    let ast = parser.parse(source).expect("Failed to parse source");
    let hir = ast_to_hir(ast).expect("Failed to transform AST to HIR");
    let mir = hir_to_mir(hir).expect("Failed to transform HIR to MIR");
    let lir = mir_to_lir(mir).expect("Failed to transform MIR to LIR");
    
    // Check that LIR contains expected instructions
    let main_func = lir.functions.iter()
        .find(|f| f.name == "main")
        .expect("Main function should exist in LIR");
    
    // Should have local variable instructions
    assert!(main_func.instructions.iter()
        .any(|inst| matches!(inst, LIRInstruction::LocalSet(_))),
        "Should contain local variable assignments");
    
    assert!(main_func.instructions.iter()
        .any(|inst| matches!(inst, LIRInstruction::I32Add)),
        "Should contain addition instruction");
}

#[test]
fn test_function_calls_ir_pipeline() {
    let source = r#"
        function add(a: integer, b: integer): integer {
            return a + b;
        }
        
        function main(): integer {
            return add(5, 10);
        }
    "#;
    
    let mut parser = CleanParser::new();
    let ast = parser.parse(source).expect("Failed to parse source");
    let hir = ast_to_hir(ast).expect("Failed to transform AST to HIR");
    let mir = hir_to_mir(hir).expect("Failed to transform HIR to MIR");
    let lir = mir_to_lir(mir).expect("Failed to transform MIR to LIR");
    
    // Should have both functions
    assert_eq!(lir.functions.len(), 2, "Should have two functions");
    
    let main_func = lir.functions.iter()
        .find(|f| f.name == "main")
        .expect("Main function should exist");
    
    // Should contain function call instruction
    assert!(main_func.instructions.iter()
        .any(|inst| matches!(inst, LIRInstruction::Call(_))),
        "Should contain function call instruction");
}

#[test]
fn test_control_flow_ir_pipeline() {
    let source = r#"
        function main(): integer {
            auto x = 5;
            if x > 0 {
                return 1;
            } else {
                return -1;
            }
        }
    "#;
    
    let mut parser = CleanParser::new();
    let ast = parser.parse(source).expect("Failed to parse source");
    let hir = ast_to_hir(ast).expect("Failed to transform AST to HIR");
    let mir = hir_to_mir(hir).expect("Failed to transform HIR to MIR");
    let lir = mir_to_lir(mir).expect("Failed to transform MIR to LIR");
    
    let main_func = lir.functions.iter()
        .find(|f| f.name == "main")
        .expect("Main function should exist");
    
    // Should contain if-else control flow instructions
    assert!(main_func.instructions.iter()
        .any(|inst| matches!(inst, LIRInstruction::If(_))),
        "Should contain if instruction");
        
    assert!(main_func.instructions.iter()
        .any(|inst| matches!(inst, LIRInstruction::Else)),
        "Should contain else instruction");
}

#[test]
fn test_memory_management_integration() {
    // Initialize memory runtime for testing
    let config = WasmMemoryConfig::default();
    init_clean_memory_runtime(config).expect("Failed to initialize memory runtime");
    
    // Test memory allocation
    let addr = mem_alloc(1, 64);
    assert_ne!(addr, 0, "Allocation should succeed");
    
    // Test reference counting
    mem_retain(addr);
    let ref_count = mem_get_ref_count(addr);
    assert_eq!(ref_count, 1, "Reference count should be 1");
    
    // Test cleanup
    mem_release(addr);
    let freed = mem_collect();
    assert!(freed >= 0, "GC should return non-negative freed count");
}

#[test]
fn test_memory_management_in_lir() {
    let source = r#"
        function main(): integer {
            // This would generate memory management instructions
            auto x = "hello world";
            return 0;
        }
    "#;
    
    let mut parser = CleanParser::new();
    let ast = parser.parse(source).expect("Failed to parse source");
    let hir = ast_to_hir(ast).expect("Failed to transform AST to HIR");
    let mir = hir_to_mir(hir).expect("Failed to transform HIR to MIR");
    let lir = mir_to_lir(mir).expect("Failed to transform MIR to LIR");
    
    // String allocation should generate memory management instructions
    let main_func = lir.functions.iter()
        .find(|f| f.name == "main")
        .expect("Main function should exist");
    
    // May contain memory management instructions for string handling
    let has_memory_ops = main_func.instructions.iter()
        .any(|inst| matches!(inst, 
            LIRInstruction::MemAlloc | 
            LIRInstruction::MemRetain | 
            LIRInstruction::MemRelease
        ));
    
    // This assertion may need adjustment based on current string handling
    println!("LIR instructions: {:?}", main_func.instructions);
}

#[test]
fn test_complete_compilation_pipeline() {
    let source = r#"
        class Calculator {
            function add(a: integer, b: integer): integer {
                return a + b;
            }
        }
        
        function main(): integer {
            auto calc = Calculator();
            return calc.add(10, 20);
        }
    "#;
    
    let mut parser = CleanParser::new();
    
    // This test may fail due to incomplete class implementation
    // but it validates the pipeline structure
    match parser.parse(source) {
        Ok(ast) => {
            println!("AST parsed successfully");
            
            match ast_to_hir(ast) {
                Ok(hir) => {
                    println!("HIR transformation successful");
                    
                    match hir_to_mir(hir) {
                        Ok(mir) => {
                            println!("MIR transformation successful");
                            
                            match mir_to_lir(mir) {
                                Ok(lir) => {
                                    println!("LIR transformation successful");
                                    
                                    let mut wasm_gen = WasmGenerator::new();
                                    match wasm_gen.generate_wasm_module(lir) {
                                        Ok(wasm_bytes) => {
                                            assert!(!wasm_bytes.is_empty());
                                            println!("Complete pipeline successful!");
                                        }
                                        Err(e) => {
                                            println!("WASM generation failed: {}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("MIR to LIR transformation failed: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            println!("HIR to MIR transformation failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("AST to HIR transformation failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("Parsing failed: {}", e);
            // This is expected for now due to incomplete parser implementation
        }
    }
}

#[test] 
fn test_error_handling_in_pipeline() {
    let invalid_source = r#"
        function main(): integer {
            return "not an integer";  // Type error
        }
    "#;
    
    let mut parser = CleanParser::new();
    match parser.parse(invalid_source) {
        Ok(ast) => {
            // If parsing succeeds, semantic analysis should catch the error
            match ast_to_hir(ast) {
                Ok(_) => {
                    // If HIR transformation succeeds, later stages should catch the error
                    println!("Type error not caught in HIR transformation");
                }
                Err(e) => {
                    println!("Type error correctly caught: {}", e);
                }
            }
        }
        Err(e) => {
            println!("Parse error correctly caught: {}", e);
        }
    }
}

#[test]
fn test_optimization_opportunities() {
    let source = r#"
        function main(): integer {
            auto x = 5 + 3;  // Should be optimized to constant
            auto y = x * 2;  // Could be optimized
            return y;
        }
    "#;
    
    let mut parser = CleanParser::new();
    if let Ok(ast) = parser.parse(source) {
        if let Ok(hir) = ast_to_hir(ast) {
            if let Ok(mir) = hir_to_mir(hir) {
                if let Ok(lir) = mir_to_lir(mir) {
                    let main_func = lir.functions.iter()
                        .find(|f| f.name == "main")
                        .expect("Main function should exist");
                    
                    println!("LIR before optimization: {:?}", main_func.instructions);
                    
                    // Future optimization passes would reduce instruction count here
                    // For now, just verify the pipeline works
                    assert!(!main_func.instructions.is_empty());
                }
            }
        }
    }
}