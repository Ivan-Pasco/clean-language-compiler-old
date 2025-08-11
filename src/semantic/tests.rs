//! Comprehensive tests for the semantic analysis and type system

use crate::semantic::SemanticAnalyzer;
use crate::ast::{Type, Expression, Statement, Program, Parameter, Function, SourceLocation, Value, BinaryOperator};
use crate::error::CompilerError;

fn create_test_location() -> Option<SourceLocation> {
    Some(SourceLocation::new(1, 1, "test.cln"))
}

#[test]
fn test_basic_type_inference() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Test integer literal inference
    let expr = Expression::Value(Value::Integer(42), create_test_location());
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Integer);
    
    // Test number literal inference
    let expr = Expression::Value(Value::Number(3.14), create_test_location());
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Number);
    
    // Test string literal inference
    let expr = Expression::Value(Value::String("hello".to_string()), create_test_location());
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::String);
    
    // Test boolean literal inference
    let expr = Expression::Value(Value::Boolean(true), create_test_location());
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Boolean);
}

#[test]
fn test_variable_declaration_and_lookup() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Declare a variable
    analyzer.current_scope.declare_variable("x", Type::Integer);
    
    // Test lookup
    let var_type = analyzer.current_scope.lookup_variable("x");
    assert_eq!(var_type, Some(Type::Integer));
    
    // Test lookup of non-existent variable
    let var_type = analyzer.current_scope.lookup_variable("y");
    assert_eq!(var_type, None);
}

#[test]
fn test_binary_expression_type_inference() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Test integer + integer = integer
    let left = Expression::Value(Value::Integer(10), create_test_location());
    let right = Expression::Value(Value::Integer(20), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::Add,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Integer);
    
    // Test number + number = number
    let left = Expression::Value(Value::Number(3.14), create_test_location());
    let right = Expression::Value(Value::Number(2.86), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::Add,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Number);
    
    // Test mixed numeric types: integer + number = number
    let left = Expression::Value(Value::Integer(10), create_test_location());
    let right = Expression::Value(Value::Number(3.14), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::Add,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Number);
}

#[test]
fn test_comparison_operators() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Test integer comparison
    let left = Expression::Value(Value::Integer(10), create_test_location());
    let right = Expression::Value(Value::Integer(20), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::Less,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Boolean);
    
    // Test equality comparison
    let left = Expression::Value(Value::String("hello".to_string()), create_test_location());
    let right = Expression::Value(Value::String("world".to_string()), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::Equal,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Boolean);
}

#[test]
fn test_logical_operators() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Test boolean AND
    let left = Expression::Value(Value::Boolean(true), create_test_location());
    let right = Expression::Value(Value::Boolean(false), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::And,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    let inferred_type = analyzer.infer_expression_type(&expr).unwrap();
    assert_eq!(inferred_type, Type::Boolean);
}

#[test]
fn test_function_type_checking() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Register a test function
    let func = Function {
        name: "add".to_string(),
        description: None,
        type_parameters: vec![],
        type_constraints: vec![],
        parameters: vec![
            Parameter {
                name: "a".to_string(),
                type_: Type::Integer,
                default_value: None,
                location: create_test_location(),
            },
            Parameter {
                name: "b".to_string(), 
                type_: Type::Integer,
                default_value: None,
                location: create_test_location(),
            },
        ],
        return_type: Type::Integer,
        body: vec![],
        location: create_test_location(),
        syntax: crate::ast::FunctionSyntax::Traditional,
        visibility: crate::ast::Visibility::Public,
        modifiers: vec![],
        is_constructor: false,
        is_async: false,
    };
    
    // Test function registration
    let result = analyzer.analyze_function(&func);
    assert!(result.is_ok());
    
    // Test function lookup
    let function_info = analyzer.function_table.get("add");
    assert!(function_info.is_some());
    
    let overloads = function_info.unwrap();
    assert_eq!(overloads.len(), 1);
    assert_eq!(overloads[0].1, Type::Integer); // return type
    assert_eq!(overloads[0].0.len(), 2); // parameter count
}

#[test]
fn test_scope_management() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Declare variable in outer scope
    analyzer.current_scope.declare_variable("x", Type::Integer);
    assert_eq!(analyzer.current_scope.lookup_variable("x"), Some(Type::Integer));
    
    // Enter new scope
    analyzer.current_scope.enter();
    
    // Variable should still be visible
    assert_eq!(analyzer.current_scope.lookup_variable("x"), Some(Type::Integer));
    
    // Declare variable with same name in inner scope
    analyzer.current_scope.declare_variable("x", Type::String);
    assert_eq!(analyzer.current_scope.lookup_variable("x"), Some(Type::String));
    
    // Exit scope
    analyzer.current_scope.exit();
    
    // Should see outer scope variable again
    assert_eq!(analyzer.current_scope.lookup_variable("x"), Some(Type::Integer));
}

#[test]
fn test_type_constraint_checking() {
    use crate::semantic::type_constraint::{NumericTypeConstraint, TypeConstraint};
    
    let numeric_constraint = NumericTypeConstraint;
    
    // Test numeric types
    assert!(numeric_constraint.check(&Type::Integer));
    assert!(numeric_constraint.check(&Type::Number));
    
    // Test non-numeric types
    assert!(!numeric_constraint.check(&Type::String));
    assert!(!numeric_constraint.check(&Type::Boolean));
    assert!(!numeric_constraint.check(&Type::Void));
}

#[test]
fn test_generic_type_resolution() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Test that 'any' type can be resolved to specific types
    let any_type = Type::Any;
    
    // In context of integer assignment
    let resolved = analyzer.resolve_generic_type(&any_type, &Type::Integer);
    assert_eq!(resolved, Type::Integer);
    
    // In context of string assignment
    let resolved = analyzer.resolve_generic_type(&any_type, &Type::String);
    assert_eq!(resolved, Type::String);
}

#[test]
fn test_builtin_function_registration() {
    let analyzer = SemanticAnalyzer::new();
    
    // Check that built-in functions are registered
    assert!(analyzer.function_table.contains_key("print"));
    assert!(analyzer.function_table.contains_key("println"));
    
    // Check print function signature
    let print_overloads = &analyzer.function_table["print"];
    assert_eq!(print_overloads.len(), 1);
    let (params, return_type, _) = &print_overloads[0];
    assert_eq!(params.len(), 1);
    assert_eq!(params[0], Type::String);
    assert_eq!(*return_type, Type::Void);
}

#[test]
fn test_type_error_detection() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Test type mismatch in binary operation
    let left = Expression::Value(Value::String("hello".to_string()), create_test_location());
    let right = Expression::Value(Value::Integer(42), create_test_location());
    let expr = Expression::Binary {
        left: Box::new(left),
        operator: BinaryOperator::Add,
        right: Box::new(right),
        location: create_test_location(),
    };
    
    // This should result in a type error
    let result = analyzer.infer_expression_type(&expr);
    assert!(result.is_err());
}

#[test]
fn test_return_type_checking() {
    let mut analyzer = SemanticAnalyzer::new();
    
    // Set function return type
    analyzer.current_function_return_type = Some(Type::Integer);
    
    // Test compatible return value
    let return_expr = Expression::Value(Value::Integer(42), create_test_location());
    let result = analyzer.check_return_type(&return_expr);
    assert!(result.is_ok());
    
    // Test incompatible return value
    let return_expr = Expression::Value(Value::String("hello".to_string()), create_test_location());
    let result = analyzer.check_return_type(&return_expr);
    assert!(result.is_err());
}

#[test]
fn test_program_analysis() {
    let mut analyzer = SemanticAnalyzer::new();
    
    let program = Program {
        imports: vec![],
        statements: vec![],
        location: create_test_location(),
        functions: vec![
            Function {
                name: "test".to_string(),
                description: None,
                type_parameters: vec![],
                type_constraints: vec![],
                parameters: vec![],
                return_type: Type::Void,
                body: vec![],
                location: create_test_location(),
                syntax: crate::ast::FunctionSyntax::Traditional,
                visibility: crate::ast::Visibility::Public,
                modifiers: vec![],
                is_constructor: false,
                is_async: false,
            }
        ],
        classes: vec![],
        start_function: None,
        tests: vec![],
    };
    
    let result = analyzer.analyze_program(&program);
    assert!(result.is_ok());
}