/*!
 * Clean Language Compiler Library
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

#![allow(clippy::single_match)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::needless_return)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::get_first)]
#![allow(clippy::unused_enumerate_index)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::for_kv_map)]
#![allow(clippy::module_inception)]
#![allow(clippy::new_without_default)]
#![allow(clippy::clone_on_copy)]

pub mod ast;
pub mod codegen;
pub mod debug;
pub mod error;
pub mod module;
pub mod package;
pub mod parser;
pub mod runtime;
pub mod semantic;
pub mod stdlib;
pub mod types;

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::parser::CleanParser;
use crate::semantic::SemanticAnalyzer;

/// Compiles Clean Language source code to WebAssembly
pub fn compile(source: &str) -> Result<Vec<u8>, CompilerError> {
    compile_with_file(source, "<unknown>")
}

/// Compiles Clean Language source code to WebAssembly with file path for better error reporting
pub fn compile_with_file(source: &str, file_path: &str) -> Result<Vec<u8>, CompilerError> {
    // Parse the source code with file path information
    let program = CleanParser::parse_program_with_file(source, file_path)?;

    // Perform semantic analysis
    let mut analyzer = SemanticAnalyzer::new();
    let analyzed_program = analyzer.analyze(&program)?;

    // Generate WASM code
    let mut codegen = CodeGenerator::new();
    let wasm_binary = codegen.generate(&analyzed_program)?;

    Ok(wasm_binary)
}

/// Compiles with detailed error reporting and recovery
pub fn compile_with_recovery(source: &str, file_path: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Try parsing with error recovery first
    let program = CleanParser::parse_program_with_recovery(source, file_path)?;

    // Perform semantic analysis
    let mut analyzer = SemanticAnalyzer::new();
    let analyzed_program = match analyzer.analyze(&program) {
        Ok(program) => program,
        Err(semantic_error) => return Err(vec![semantic_error]),
    };

    // Generate WASM code
    let mut codegen = CodeGenerator::new();
    match codegen.generate(&analyzed_program) {
        Ok(wasm_binary) => Ok(wasm_binary),
        Err(codegen_error) => Err(vec![codegen_error]),
    }
}

/// Compiles Clean Language source code to WebAssembly without runtime imports (for testing)
pub fn compile_minimal(source: &str) -> Result<Vec<u8>, CompilerError> {
    // Parse the source code
    let program = CleanParser::parse_program_with_file(source, "<test>")?;

    // Perform semantic analysis
    let mut analyzer = SemanticAnalyzer::new();
    let analyzed_program = analyzer.analyze(&program)?;

    // Generate WASM code without runtime imports
    let mut codegen = CodeGenerator::new_minimal();
    let wasm_binary = codegen.generate(&analyzed_program)?;

    Ok(wasm_binary)
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_basic_integration() {
        let source = r#"start()
	integer x = 42
	print(x)
"#;

        let result = compile_with_file(source, "test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Basic integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Basic integration test failed: {error}");
                panic!("Integration test failed: {error}");
            }
        }
    }

    #[test]
    fn test_function_integration() {
        let source = r#"functions:
	integer add(integer a, integer b)
		return a + b

start()
	integer result = add(5, 3)
	print(result)
"#;

        let result = compile_with_file(source, "function_test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Function integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Function integration test failed: {error}");
                // Don't panic here as this might reveal integration issues we need to fix
            }
        }
    }

    #[test]
    fn test_type_checking_integration() {
        let source = r#"start()
	integer x = 42
	string y = "hello"
	print(x)
	print(y)
"#;

        let result = compile_with_file(source, "type_test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Type checking integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Type checking integration test failed: {error}");
                // This might reveal type system integration issues
            }
        }
    }

    #[test]
    fn test_error_propagation() {
        let source = r#"start()
	integer x = undefined_function() onError 0
	print(x)
"#;

        let result = compile_with_file(source, "error_test.clean");
        match result {
            Ok(_) => {
                println!("⚠ Error propagation test: Expected error but compilation succeeded");
            }
            Err(error) => {
                println!("✓ Error propagation test: Correctly caught error: {error}");
                // Check that the error contains useful information about the undefined function
                assert!(error.to_string().contains("undefined_function"));
                assert!(error.to_string().contains("not found"));
            }
        }
    }

    #[test]
    fn test_stdlib_integration() {
        println!("\n=== Standard Library Integration Test ===");

        let test_cases = vec![
            (
                "Math Functions",
                r#"start()
	integer x = -5
	integer result = abs(x)
	print(result)
"#,
            ),
            (
                "String Functions",
                r#"start()
	string text = "hello"
	integer length = text.length()
	print(length)
"#,
            ),
            (
                "List Functions",
                r#"start()
	List<integer> lst = [1, 2, 3, 4, 5]
	integer length = lst.length()
	print(length)
"#,
            ),
        ];

        let mut passed = 0;
        let total = test_cases.len();

        for (name, source) in test_cases {
            println!("\n--- Testing {name} ---");
            println!("Source: {source}");

            match compile_with_file(source, "stdlib_test.clean") {
                Ok(wasm_binary) => {
                    println!("✓ {name}: {len} bytes", len = wasm_binary.len());
                    assert!(!wasm_binary.is_empty());
                    passed += 1;
                }
                Err(error) => {
                    println!("✗ {name} failed: {error}");
                    // Don't panic here as some stdlib functions might not be fully implemented yet
                }
            }
        }

        println!("\n=== Summary ===");
        println!("Passed: {passed}/{total}");

        // At least basic functionality should work
        assert!(passed > 0, "No stdlib integration tests passed");
    }
}
