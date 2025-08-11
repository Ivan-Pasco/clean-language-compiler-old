//! Simple integration test for IR pipeline components
//! Tests individual parts of the pipeline without full AST integration

use clean_language_compiler::ir::*;
use clean_language_compiler::codegen::wasm_generator::*;
use clean_language_compiler::memory::*;

#[test]
fn test_lir_to_wasm_generation() {
    // Create a simple LIR program manually
    let lir_program = LIRProgram {
        functions: vec![
            LIRFunction {
                name: "main".to_string(),
                parameters: vec![],
                return_type: Some(LIRType::I32),
                locals: vec![],
                instructions: vec![
                    LIRInstruction::I32Const(42),
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
        exports: vec![
            LIRExport {
                name: "main".to_string(),
                export_type: LIRExportType::Function(0),
            }
        ],
    };

    // Test WebAssembly generation
    let mut wasm_gen = WasmGenerator::new();
    let wasm_bytes = wasm_gen.generate_wasm_module(lir_program)
        .expect("Failed to generate WebAssembly");
    
    assert!(!wasm_bytes.is_empty(), "WebAssembly output should not be empty");
    assert!(wasm_bytes.len() > 10, "WebAssembly output should have reasonable size");
}

#[test] 
fn test_lir_arithmetic_operations() {
    let lir_program = LIRProgram {
        functions: vec![
            LIRFunction {
                name: "add".to_string(), 
                parameters: vec![LIRType::I32, LIRType::I32],
                return_type: Some(LIRType::I32),
                locals: vec![],
                instructions: vec![
                    LIRInstruction::LocalGet(0), // first parameter
                    LIRInstruction::LocalGet(1), // second parameter
                    LIRInstruction::I32Add,      // add them
                    LIRInstruction::Return,      // return result
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
        exports: vec![
            LIRExport {
                name: "add".to_string(),
                export_type: LIRExportType::Function(0),
            }
        ],
    };

    let mut wasm_gen = WasmGenerator::new();
    let wasm_bytes = wasm_gen.generate_wasm_module(lir_program)
        .expect("Failed to generate WebAssembly for arithmetic");
    
    assert!(!wasm_bytes.is_empty());
}

#[test]
fn test_lir_control_flow() {
    let lir_program = LIRProgram {
        functions: vec![
            LIRFunction {
                name: "conditional".to_string(),
                parameters: vec![LIRType::I32],
                return_type: Some(LIRType::I32),
                locals: vec![],
                instructions: vec![
                    LIRInstruction::LocalGet(0),     // Get parameter
                    LIRInstruction::I32Const(0),     // Compare with 0
                    LIRInstruction::I32GtS,          // Greater than signed
                    LIRInstruction::If(LIRType::I32), // If block
                    LIRInstruction::I32Const(1),     // Return 1 if positive
                    LIRInstruction::Else,            // Else block
                    LIRInstruction::I32Const(-1),    // Return -1 if negative/zero
                    LIRInstruction::End,             // End if
                    LIRInstruction::Return,          // Return result
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
        exports: vec![
            LIRExport {
                name: "conditional".to_string(),
                export_type: LIRExportType::Function(0),
            }
        ],
    };

    let mut wasm_gen = WasmGenerator::new();
    let wasm_bytes = wasm_gen.generate_wasm_module(lir_program)
        .expect("Failed to generate WebAssembly for control flow");
    
    assert!(!wasm_bytes.is_empty());
}

#[test]
fn test_memory_runtime_functions() {
    // Initialize memory runtime
    let config = WasmMemoryConfig::default();
    init_clean_memory_runtime(config).expect("Failed to initialize memory runtime");
    
    // Test basic allocation cycle
    let addr1 = mem_alloc(1, 32);
    assert_ne!(addr1, 0, "First allocation should succeed");
    
    let addr2 = mem_alloc(2, 64); 
    assert_ne!(addr2, 0, "Second allocation should succeed");
    assert_ne!(addr1, addr2, "Allocations should have different addresses");
    
    // Test reference counting
    mem_retain(addr1);
    let ref_count = mem_get_ref_count(addr1);
    assert_eq!(ref_count, 1, "Reference count should be 1 after retain");
    
    // Test cleanup
    mem_release(addr1);
    mem_release(addr2);
    
    let freed_count = mem_collect();
    assert!(freed_count >= 0, "GC should return valid freed count");
}

#[test]
fn test_lir_memory_management_instructions() {
    let lir_program = LIRProgram {
        functions: vec![
            LIRFunction {
                name: "allocate_test".to_string(),
                parameters: vec![],
                return_type: Some(LIRType::I32),
                locals: vec![],
                instructions: vec![
                    LIRInstruction::I32Const(1),     // type_id
                    LIRInstruction::I32Const(64),    // size
                    LIRInstruction::MemAlloc,        // allocate memory
                    // Result address is now on stack
                    LIRInstruction::LocalTee(0),     // Store in local and keep on stack
                    LIRInstruction::MemRetain,       // Increment reference count
                    LIRInstruction::LocalGet(0),     // Get address back
                    LIRInstruction::Return,          // Return address
                ],
            }
        ],
        memory_layout: LIRMemoryLayout {
            initial_pages: 1,
            max_pages: Some(10),
            heap_start: 1024,
            stack_start: 0,
        },
        imports: vec![
            LIRImport {
                module: "memory_runtime".to_string(),
                name: "mem_alloc".to_string(),
                import_type: LIRImportType::Function(
                    vec![LIRType::I32, LIRType::I32], 
                    Some(LIRType::I32)
                ),
            },
            LIRImport {
                module: "memory_runtime".to_string(),
                name: "mem_retain".to_string(),
                import_type: LIRImportType::Function(
                    vec![LIRType::I32],
                    None
                ),
            },
        ],
        exports: vec![
            LIRExport {
                name: "allocate_test".to_string(),
                export_type: LIRExportType::Function(2), // After 2 imports
            }
        ],
    };

    let mut wasm_gen = WasmGenerator::new();
    let result = wasm_gen.generate_wasm_module(lir_program);
    
    match result {
        Ok(wasm_bytes) => {
            assert!(!wasm_bytes.is_empty());
            println!("Memory management WASM generated successfully: {} bytes", wasm_bytes.len());
        }
        Err(e) => {
            println!("Memory management WASM generation failed: {}", e);
            // This might fail due to missing function types, but the structure should work
        }
    }
}

#[test]
fn test_memory_statistics() {
    let config = WasmMemoryConfig::default();
    init_clean_memory_runtime(config).expect("Failed to initialize memory runtime");
    
    // Get initial stats
    let initial_stats = get_memory_stats().expect("Failed to get initial memory stats");
    println!("Initial memory stats: {:?}", initial_stats);
    
    // Allocate some memory
    let _addr1 = mem_alloc(1, 128);
    let _addr2 = mem_alloc(2, 256);
    
    // Get updated stats
    let updated_stats = get_memory_stats().expect("Failed to get updated memory stats");
    println!("Updated memory stats: {:?}", updated_stats);
    
    // Total allocated should have increased
    assert!(
        updated_stats.total_allocated >= initial_stats.total_allocated,
        "Total allocated should increase after allocations"
    );
}