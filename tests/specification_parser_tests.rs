//! Comprehensive specification compliance tests for the Clean Language parser
//!
//! These tests ensure 100% compliance with the Clean Language Specification
//! by testing all language constructs and syntax forms.

use clean_language_compiler::parser::specification_parser::*;
use clean_language_compiler::ast::Value;

/// Helper function to parse source code
fn parse_program(source: &str) -> Result<ProgramNode, String> {
    SpecificationParser::from_source(source, "test.cln")
        .and_then(|mut parser| parser.parse_program())
        .map_err(|e| format!("{}", e))
}

/// Test basic function parsing
#[test]
fn test_function_parsing() {
    let source = r#"functions:
	integer add(integer a, integer b)
		return a + b
"#;

    let program = parse_program(source).expect("Failed to parse function");
    assert_eq!(program.items.len(), 1);
    
    if let TopLevelItem::FunctionsBlock { functions, .. } = &program.items[0] {
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "add");
        assert_eq!(functions[0].parameters.len(), 2);
        assert_eq!(functions[0].parameters[0].name, "a");
        assert_eq!(functions[0].parameters[1].name, "b");
    } else {
        panic!("Expected functions block");
    }
}

/// Test start function parsing
#[test]
fn test_start_function_parsing() {
    let source = r#"start()
	integer x = 42
	print(x)
"#;

    let program = parse_program(source).expect("Failed to parse start function");
    assert_eq!(program.items.len(), 1);
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        assert_eq!(body.statements.len(), 2);
    } else {
        panic!("Expected start function");
    }
}

/// Test class parsing with methods
#[test]
fn test_class_parsing() {
    let source = r#"class Person
	string name
	integer age
	
	constructor(string name, integer age)
		this.name = name
		this.age = age
	
	string getName()
		return this.name
"#;

    let program = parse_program(source).expect("Failed to parse class");
    assert_eq!(program.items.len(), 1);
    
    if let TopLevelItem::ClassDeclaration { name, members, .. } = &program.items[0] {
        assert_eq!(name, "Person");
        assert_eq!(members.len(), 4); // 2 fields + 1 constructor + 1 method
    } else {
        panic!("Expected class declaration");
    }
}

/// Test inheritance parsing
#[test]
fn test_inheritance_parsing() {
    let source = r#"class Student extends Person
	string school
	
	constructor(string name, integer age, string school)
		base(name, age)
		this.school = school
"#;

    let program = parse_program(source).expect("Failed to parse inheritance");
    assert_eq!(program.items.len(), 1);
    
    if let TopLevelItem::ClassDeclaration { name, parent, .. } = &program.items[0] {
        assert_eq!(name, "Student");
        assert_eq!(parent.as_ref().unwrap(), "Person");
    } else {
        panic!("Expected class declaration with inheritance");
    }
}

/// Test import statements
#[test]
fn test_import_parsing() {
    let test_cases = vec![
        ("import math", "math", None),
        ("import console {print, println}", "console", Some(vec!["print".to_string(), "println".to_string()])),
    ];

    for (source, expected_module, expected_items) in test_cases {
        let program = parse_program(source).expect(&format!("Failed to parse import: {}", source));
        assert_eq!(program.items.len(), 1);
        
        if let TopLevelItem::ImportStatement { module_name, items, .. } = &program.items[0] {
            assert_eq!(module_name, expected_module);
            assert_eq!(*items, expected_items);
        } else {
            panic!("Expected import statement");
        }
    }
}

/// Test variable declarations with different types
#[test]
fn test_variable_declarations() {
    let source = r#"start()
	integer x = 42
	number y = 3.14
	string name = "Alice"
	boolean flag = true
	List<integer> numbers = [1, 2, 3]
"#;

    let program = parse_program(source).expect("Failed to parse variable declarations");
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        assert_eq!(body.statements.len(), 5);
        
        // Check each variable declaration
        for (i, expected_type) in ["integer", "number", "string", "boolean"].iter().enumerate() {
            if let StatementNode::VariableDeclaration { var_type, .. } = &body.statements[i] {
                if let TypeNode::Simple { name, .. } = var_type {
                    assert_eq!(name, expected_type);
                }
            }
        }
        
        // Check generic type (List<integer>)
        if let StatementNode::VariableDeclaration { var_type, .. } = &body.statements[4] {
            if let TypeNode::Generic { name, .. } = var_type {
                assert_eq!(name, "List");
            }
        }
    }
}

/// Test expressions and operators
#[test]
fn test_expressions() {
    let source = r#"start()
	integer result = (5 + 3) * 2 - 1
	boolean flag = x > 10 and y < 20
	number power = x ^ 2
"#;

    let program = parse_program(source).expect("Failed to parse expressions");
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        assert_eq!(body.statements.len(), 3);
        
        // Check that we have variable declarations with complex expressions
        for stmt in &body.statements {
            if let StatementNode::VariableDeclaration { initializer, .. } = stmt {
                assert!(initializer.is_some());
            }
        }
    }
}

/// Test method calls and property access
#[test]
fn test_method_calls() {
    let source = r#"start()
	string text = "hello"
	integer length = text.length()
	string upper = text.toUpper()
	
	List<integer> numbers = [1, 2, 3]
	numbers.push(4)
	integer first = numbers[0]
"#;

    let program = parse_program(source).expect("Failed to parse method calls");
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        assert_eq!(body.statements.len(), 6);
        
        // Verify we have method calls in the initializers
        if let StatementNode::VariableDeclaration { initializer: Some(expr), .. } = &body.statements[1] {
            if let ExpressionNode::MethodCall { method, .. } = expr.as_ref() {
                assert_eq!(method, "length");
            }
        }
        
        if let StatementNode::VariableDeclaration { initializer: Some(expr), .. } = &body.statements[2] {
            if let ExpressionNode::MethodCall { method, .. } = expr.as_ref() {
                assert_eq!(method, "toUpper");
            }
        }
    }
}

/// Test control flow statements
#[test]
fn test_control_flow() {
    let source = r#"start()
	if x > 0
		print("positive")
	else
		print("negative")
	
	while y < 10
		y = y + 1
	
	for i in 1..10
		print(i)
"#;

    let program = parse_program(source).expect("Failed to parse control flow");
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        assert_eq!(body.statements.len(), 3);
        
        // Check if statement
        if let StatementNode::If { then_branch, else_branch, .. } = &body.statements[0] {
            assert!(then_branch.statements.len() > 0);
            assert!(else_branch.is_some());
        }
        
        // Check while statement
        if let StatementNode::While { body: while_body, .. } = &body.statements[1] {
            assert!(while_body.statements.len() > 0);
        }
        
        // Check for statement
        if let StatementNode::For { variable, body: for_body, .. } = &body.statements[2] {
            assert_eq!(variable, "i");
            assert!(for_body.statements.len() > 0);
        }
    }
}

/// Test function calls and arguments
#[test]
fn test_function_calls() {
    let source = r#"start()
	print("Hello, World!")
	integer sum = add(5, 3)
	boolean result = compare(x, y, z)
"#;

    let program = parse_program(source).expect("Failed to parse function calls");
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        assert_eq!(body.statements.len(), 3);
        
        // Check function call statement
        if let StatementNode::Expression { expression, .. } = &body.statements[0] {
            if let ExpressionNode::FunctionCall { arguments, .. } = expression.as_ref() {
                assert_eq!(arguments.len(), 1);
            }
        }
        
        // Check function call in variable declaration
        if let StatementNode::VariableDeclaration { initializer: Some(expr), .. } = &body.statements[1] {
            if let ExpressionNode::FunctionCall { arguments, .. } = expr.as_ref() {
                assert_eq!(arguments.len(), 2);
            }
        }
    }
}

/// Test array literals and indexing
#[test]
fn test_arrays() {
    let source = r#"start()
	List<integer> numbers = [1, 2, 3, 4, 5]
	integer first = numbers[0]
	integer last = numbers[numbers.length() - 1]
"#;

    let program = parse_program(source).expect("Failed to parse arrays");
    
    if let TopLevelItem::StartFunction { body, .. } = &program.items[0] {
        // Check array literal
        if let StatementNode::VariableDeclaration { initializer: Some(expr), .. } = &body.statements[0] {
            if let ExpressionNode::ArrayLiteral { elements, .. } = expr.as_ref() {
                assert_eq!(elements.len(), 5);
            }
        }
        
        // Check array indexing
        if let StatementNode::VariableDeclaration { initializer: Some(expr), .. } = &body.statements[1] {
            if let ExpressionNode::IndexAccess { .. } = expr.as_ref() {
                // Successfully parsed index access
            } else {
                panic!("Expected index access expression");
            }
        }
    }
}

/// Test test blocks
#[test]
fn test_tests_block() {
    let source = r#"tests:
	test "addition works"
		integer result = add(2, 3)
		assert result == 5
	
	test "subtraction works" description "Test basic subtraction"
		integer result = subtract(5, 2)
		assert result == 3
"#;

    let program = parse_program(source).expect("Failed to parse tests block");
    assert_eq!(program.items.len(), 1);
    
    if let TopLevelItem::TestsBlock { tests, .. } = &program.items[0] {
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0].name, "addition works");
        assert_eq!(tests[1].name, "subtraction works");
        assert!(tests[1].description.is_some());
        assert_eq!(tests[1].description.as_ref().unwrap(), "Test basic subtraction");
    } else {
        panic!("Expected tests block");
    }
}

/// Test comprehensive program with multiple top-level items
#[test]
fn test_comprehensive_program() {
    let source = r#"import math
import console {print, println}

functions:
	integer factorial(integer n)
		if n <= 1
			return 1
		else
			return n * factorial(n - 1)

class Calculator
	string model
	
	constructor(string model)
		this.model = model
	
	integer add(integer a, integer b)
		return a + b

start()
	Calculator calc = Calculator("Basic")
	integer result = calc.add(5, 3)
	println("Result: " + result.toString())

tests:
	test "factorial calculation"
		integer result = factorial(5)
		assert result == 120
"#;

    let program = parse_program(source).expect("Failed to parse comprehensive program");
    assert_eq!(program.items.len(), 5); // 2 imports + functions + class + start + tests
    
    // Verify all top-level items are present
    let mut has_import = false;
    let mut has_functions = false;
    let mut has_class = false;
    let mut has_start = false;
    let mut has_tests = false;
    
    for item in &program.items {
        match item {
            TopLevelItem::ImportStatement { .. } => has_import = true,
            TopLevelItem::FunctionsBlock { .. } => has_functions = true,
            TopLevelItem::ClassDeclaration { .. } => has_class = true,
            TopLevelItem::StartFunction { .. } => has_start = true,
            TopLevelItem::TestsBlock { .. } => has_tests = true,
        }
    }
    
    assert!(has_import, "Missing import statements");
    assert!(has_functions, "Missing functions block");
    assert!(has_class, "Missing class declaration");
    assert!(has_start, "Missing start function");
    assert!(has_tests, "Missing tests block");
}

/// Test error recovery and reporting
#[test]
fn test_error_reporting() {
    let invalid_sources = vec![
        "function", // Missing function name
        "class", // Missing class name
        "start() integer x", // Missing newline/indentation
        "if x > 0 print(x)", // Missing indentation for if body
        "integer x = ", // Missing value in assignment
    ];

    for source in invalid_sources {
        let result = parse_program(source);
        assert!(result.is_err(), "Expected error for invalid source: {}", source);
        
        // Verify error contains useful information
        let error = result.unwrap_err();
        assert!(!error.is_empty(), "Error message should not be empty");
    }
}

/// Test performance with large programs
#[test]
fn test_parser_performance() {
    use std::time::Instant;
    
    // Generate large program
    let mut large_program = String::from("functions:\n");
    
    for i in 0..1000 {
        large_program.push_str(&format!("	integer func{}(integer x)\n", i));
        large_program.push_str("		return x * 2\n\n");
    }
    
    large_program.push_str("start()\n");
    large_program.push_str("	integer result = func0(42)\n");
    large_program.push_str("	print(result)\n");
    
    let start = Instant::now();
    let result = parse_program(&large_program);
    let elapsed = start.elapsed();
    
    println!("Parsed large program in {:?}", elapsed);
    assert!(result.is_ok(), "Failed to parse large program");
    
    // Performance should be reasonable (< 100ms for 1000 functions)
    assert!(elapsed.as_millis() < 100, 
        "Parser performance too slow: {:?}", elapsed);
}