use crate::ast::Program;
use crate::error::CompilerError;
use crate::module::ModuleResolver;
use pest_derive::Parser;

// Define the CleanParser with the proper grammar path
#[derive(Parser)]
#[grammar = "src/parser/grammar.pest"]
pub struct CleanParser;

// Define a parser-specific SourceLocation that uses start/end
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLocation {
    pub start: usize,
    pub end: usize,
}

// Define a function to convert parser SourceLocation to AST SourceLocation
pub fn convert_to_ast_location(loc: &SourceLocation) -> crate::ast::SourceLocation {
    // Convert the parser location to AST location format
    // In a real implementation, we would need to calculate the line/column based on the text
    // For now we're using a simplified approach that provides at least some useful information
    crate::ast::SourceLocation {
        line: loc.start,             // Using start position as a fallback for line number
        column: loc.end - loc.start, // Using length as a fallback for column
        file: String::new(),         // We don't have file information in the parser location
    }
}

// Define submodules
mod class_parser;
mod expression_parser;
mod grammar;
mod parser_impl;
mod program_parser;
mod statement_parser;
mod type_parser;
// mod lexical_analyzer;
// mod function_parser;
mod preprocessor;

// Re-export just what's needed
pub use class_parser::parse_class;
pub use expression_parser::{
    parse_expression, parse_function_call, parse_list_literal, parse_matrix_literal, parse_primary,
    parse_string,
};
pub use parser_impl::{
    get_location, parse, parse_function_in_block, parse_functions_block, parse_start_function,
    parse_with_file, ErrorRecoveringParser, ParseContext,
};
pub use program_parser::parse_program_ast;
pub use statement_parser::parse_statement;
pub use type_parser::parse_type;
// pub use lexical_analyzer::{LexicalAnalyzer, FunctionBoundary, FunctionSegment};
// pub use function_parser::FunctionParser;
pub use preprocessor::FunctionPreprocessor;

impl CleanParser {
    pub fn parse_program(source: &str) -> Result<Program, CompilerError> {
        parser_impl::parse(source)
    }

    /// Parse a program with file path information for better error reporting
    pub fn parse_program_with_file(
        source: &str,
        file_path: &str,
    ) -> Result<Program, CompilerError> {
        parser_impl::parse_with_file(source, file_path)
    }

    /// Parse a program with error recovery - returns multiple errors if found
    pub fn parse_program_with_recovery(
        source: &str,
        file_path: &str,
    ) -> Result<Program, Vec<CompilerError>> {
        let mut parser = ErrorRecoveringParser::new(source, file_path);
        parser.parse_with_recovery(source)
    }
}

/// Parse a program with module resolution support
pub fn parse_with_modules(source: &str, file_path: &str) -> Result<Program, CompilerError> {
    // Parse the basic program first
    let program = parse_with_file(source, file_path)?;

    // If there are imports, resolve them
    if !program.imports.is_empty() {
        let mut resolver = ModuleResolver::new();
        resolver.set_current_module(file_path.to_string());

        // Resolve imports and update the program
        let import_resolution = resolver.resolve_imports(&program)?;

        // Store resolved modules in the program (we'll need to extend Program struct)
        // For now, just validate that imports can be resolved
        for import in &program.imports {
            if !import_resolution
                .resolved_imports
                .contains_key(&import.name)
            {
                return Err(CompilerError::import_error(
                    "Failed to resolve import",
                    &import.name,
                    Some(crate::ast::SourceLocation {
                        file: file_path.to_string(),
                        line: 1,
                        column: 1,
                    }),
                ));
            }
        }
    }

    Ok(program)
}

/// Parse a program with error recovery and module resolution
pub fn parse_with_modules_and_recovery(
    source: &str,
    file_path: &str,
) -> Result<Program, Vec<CompilerError>> {
    let mut parser = parser_impl::ErrorRecoveringParser::new(source, file_path);
    let program = parser.parse_with_recovery(source)?;

    // If parsing succeeded, try module resolution
    if !program.imports.is_empty() {
        let mut resolver = ModuleResolver::new();
        resolver.set_current_module(file_path.to_string());

        match resolver.resolve_imports(&program) {
            Ok(_) => Ok(program),
            Err(module_error) => Err(vec![module_error]),
        }
    } else {
        Ok(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_parsing() {
        // Test just the function declaration first
        let source = r#"function start()"#;
        let result = CleanParser::parse_program(source);
        println!("Function only result: {:?}", result);

        // Test function with empty body
        let source2 = r#"
function start()
	return
        "#;
        let result2 = CleanParser::parse_program(source2);
        println!("Function with return result: {:?}", result2);

        // Test simple variable declaration
        let source3 = r#"
function start()
	integer x = 5
        "#;
        let result3 = CleanParser::parse_program(source3);
        println!("Function with variable result: {:?}", result3);
    }

    #[test]
    fn test_basic_parsing() {
        let source = r#"
start()
	integer x = 5
	print(x)
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Basic parsing should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_error_reporting() {
        let source = r#"
start()
	integer x = 5 +
        "#;

        let result = CleanParser::parse_program(source);
        assert!(result.is_err(), "Invalid syntax should produce an error");

        if let Err(error) = result {
            println!("Error: {}", error);
            // The error should contain useful information
            assert!(error.to_string().contains("Syntax error"));
        }
    }

    #[test]
    fn test_nested_expression_parsing() {
        let source = r#"
start()
	integer x = (1 + 2) * (3 - 4)
	print(x)
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Nested expressions should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_apply_blocks() {
        let source = r#"
start()
	println:
		"Hello"
		"World"

	integer:
		count = 0
		maxSize = 100
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Apply blocks should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_function_syntaxes() {
        let source = r#"
functions:
	integer add()
		input
			integer a
			integer b
		return a + b

	integer multiply()
		description "Multiplies two integers"
		input
			integer a
			integer b
		return a * b

	integer square()
		input integer x
		return x * x

start()
	integer result = add()
	print(result)
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "All function syntaxes should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_string_interpolation() {
        let source = r#"
start()
	string name = "World"
	string greeting = "Hello, {name}!"
	println(greeting)
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "String interpolation should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_on_error_syntax() {
        let source = r#"
start()
	integer result = divide(10, 0) onError 0
	print(result)
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "onError syntax should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_inheritance_with_is() {
        let source = r#"
class Shape
	string color

	constructor(color)

class Circle is Shape
	number radius

	constructor(color, radius)
		super(color)

start()
	circle = Circle("red", 5.0)
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Class inheritance with 'is' should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_complex_error_cases() {
        let test_cases = [
            // Invalid syntax: incomplete expression
            r#"start()
    integer x = 5 +"#,
            // Invalid syntax: invalid token sequence
            r#"start()
    integer @ invalid"#,
            // Invalid syntax: incomplete onError clause
            r#"start()
    integer x = divide(10, 0) onError"#,
        ];

        for (i, source) in test_cases.iter().enumerate() {
            let result = CleanParser::parse_program(source);
            assert!(result.is_err(), "Test case {} should fail: {}", i, source);

            if let Err(error) = result {
                println!("Test case {}: {}", i, error);
                // Each error should be informative
                assert!(!error.to_string().is_empty());
            }
        }
    }

    #[test]
    fn test_file_path_in_errors() {
        let source = r#"invalid syntax here"#;
        let result = CleanParser::parse_program(source);

        if let Err(error) = result {
            println!("Error without file path: {}", error);
        }
    }

    #[test]
    fn test_file_path_in_enhanced_errors() {
        let source = r#"
start()
	integer x = 5 +
        "#;
        let file_path = "test_file.cln";
        let result = CleanParser::parse_program_with_file(source, file_path);

        assert!(result.is_err(), "Invalid syntax should produce an error");

        if let Err(error) = result {
            println!("Enhanced error with file path: {}", error);
            assert!(error.to_string().contains(file_path));
        }
    }

    #[test]
    fn test_type_first_declarations() {
        let source = r#"
start()
	integer count = 0
	number temperature = 23.5
	boolean isActive = true
	string name = "Alice"
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Type-first declarations should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_advanced_types() {
        let source = r#"
start()
	integer:8 smallNum = 100
	integer:64 bigNum = 999999999999
	number:32 preciseNumber = 3.14159
	boolean flag = true
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Advanced sized types should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_matrix_operations() {
        let source = r#"
start()
	Matrix<number> m1 = [[1.0, 2.0], [3.0, 4.0]]
	Matrix<number> m2 = [[5.0, 6.0], [7.0, 8.0]]
	Matrix<number> result = m1 + m2
	Matrix<number> transposed = m1.transpose()
        "#;

        let result = CleanParser::parse_program(source);
        assert!(
            result.is_ok(),
            "Matrix operations should parse correctly: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_print_parsing() {
        // Test print with literal
        let source1 = r#"
start()
	print(5)
        "#;
        let result1 = CleanParser::parse_program(source1);
        println!("Print literal result: {:?}", result1);

        // Test print with identifier
        let source2 = r#"
start()
	print(x)
        "#;
        let result2 = CleanParser::parse_program(source2);
        println!("Print identifier result: {:?}", result2);

        // Test function with variable
        let source3 = r#"
start()
	integer x = 5
        "#;
        let result3 = CleanParser::parse_program(source3);
        println!("Function with variable result: {:?}", result3);

        // Test function with return
        let source4 = r#"
start()
	return
        "#;
        let result4 = CleanParser::parse_program(source4);
        println!("Function with return result: {:?}", result4);

        // Test function only
        let source5 = r#"start()"#;
        let result5 = CleanParser::parse_program(source5);
        println!("Function only result: {:?}", result5);

        // Test variable and print together
        let source6 = r#"
start()
	integer x = 5
	print(x)
        "#;
        let result6 = CleanParser::parse_program(source6);
        println!("Variable + print result: {:?}", result6);
    }
}
