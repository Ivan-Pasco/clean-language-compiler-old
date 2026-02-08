//! Comprehensive tests for HIR (High-level Intermediate Representation)
//!
//! These tests verify that AST to HIR transformation works correctly,
//! including validation, desugaring, and error handling.

use super::*;
use crate::ast::{BinaryOperator as AstBinaryOperator, *};
use crate::hir::hir_builder::HirBuilder;

/// Helper to create test source location
fn test_location() -> SourceLocation {
    SourceLocation {
        file: "test.cln".to_string(),
        line: 1,
        column: 1,
        byte_start: None,
        byte_end: None,
    }
}

/// Helper to create test program with given functions
fn test_program_with_functions(functions: Vec<Function>) -> Program {
    Program {
        imports: vec![],
        plugins: vec![],
        statements: vec![],
        functions,
        classes: vec![],
        start_function: None,
        tests: vec![],
        screens: vec![],
        state: None,
        watch_blocks: Vec::new(),
        screen_blocks: Vec::new(),
        externals: Vec::new(),
        source_block: None,
        location: Some(test_location()),
    }
}

/// Helper to create test program with a start function
fn test_program_with_start(start_fn: Function) -> Program {
    Program {
        imports: vec![],
        plugins: vec![],
        statements: vec![],
        functions: vec![],
        classes: vec![],
        start_function: Some(start_fn),
        tests: vec![],
        screens: vec![],
        state: None,
        watch_blocks: Vec::new(),
        screen_blocks: Vec::new(),
        externals: Vec::new(),
        source_block: None,
        location: Some(test_location()),
    }
}

/// Helper to create test program with classes
fn test_program_with_classes(classes: Vec<Class>) -> Program {
    Program {
        imports: vec![],
        plugins: vec![],
        statements: vec![],
        functions: vec![],
        classes,
        start_function: None,
        tests: vec![],
        screens: vec![],
        state: None,
        watch_blocks: Vec::new(),
        screen_blocks: Vec::new(),
        externals: Vec::new(),
        source_block: None,
        location: Some(test_location()),
    }
}

/// Helper to create a simple function for testing
fn simple_add_function() -> Function {
    Function {
        name: "add".to_string(),
        type_parameters: vec![],
        type_constraints: vec![],
        parameters: vec![
            Parameter {
                name: "a".to_string(),
                type_: Type::Integer,
                default_value: None,
            },
            Parameter {
                name: "b".to_string(),
                type_: Type::Integer,
                default_value: None,
            },
        ],
        return_type: Type::Integer,
        body: vec![Statement::Return {
            value: Some(Expression::Binary(
                Box::new(Expression::Variable("a".to_string())),
                AstBinaryOperator::Add,
                Box::new(Expression::Variable("b".to_string())),
            )),
            location: Some(test_location()),
        }],
        description: None,
        syntax: FunctionSyntax::Simple,
        visibility: Visibility::Public,
        modifier: FunctionModifier::None,
        location: Some(test_location()),
    }
}

#[cfg(test)]
mod hir_builder_tests {
    use super::*;

    #[test]
    fn test_simple_function_to_hir() {
        let ast = test_program_with_functions(vec![simple_add_function()]);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(
            result.is_ok(),
            "Simple function should convert to HIR successfully"
        );

        let hir = result.unwrap().hir;
        assert_eq!(hir.functions.len(), 1);
        assert_eq!(hir.functions[0].name, "add");
        assert_eq!(hir.functions[0].parameters.len(), 2);
        assert_eq!(hir.functions[0].parameters[0].name, "a");
        assert_eq!(hir.functions[0].parameters[1].name, "b");
    }

    #[test]
    fn test_start_function_to_hir() {
        let start_fn = Function {
            name: "start".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body: vec![
                Statement::VariableDecl {
                    name: "x".to_string(),
                    type_: Type::Integer,
                    initializer: Some(Expression::Literal(Value::Integer(42))),
                    location: Some(test_location()),
                },
                Statement::Expression {
                    expr: Expression::Call(
                        "print".to_string(),
                        vec![Expression::Variable("x".to_string())],
                    ),
                    location: Some(test_location()),
                },
            ],
            description: None,
            syntax: FunctionSyntax::Standalone,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(test_location()),
        };

        let ast = test_program_with_start(start_fn);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(
            result.is_ok(),
            "Start function should convert to HIR successfully: {:?}",
            result.err()
        );

        let hir = result.unwrap().hir;
        assert!(hir.start_function.is_some());
        let start = hir.start_function.unwrap();
        assert_eq!(start.name, "start");
        assert_eq!(start.body.statements.len(), 2);
    }

    #[test]
    fn test_class_to_hir() {
        let person_class = Class {
            name: "Person".to_string(),
            type_parameters: vec![],
            description: None,
            base_class: None,
            base_class_type_args: vec![],
            fields: vec![Field {
                name: "name".to_string(),
                type_: Type::String,
                visibility: Visibility::Public,
                is_static: false,
                default_value: None,
            }],
            methods: vec![],
            constructor: Some(Constructor {
                parameters: vec![Parameter {
                    name: "name".to_string(),
                    type_: Type::String,
                    default_value: None,
                }],
                body: vec![Statement::Assignment {
                    target: "name".to_string(),
                    value: Expression::Variable("name".to_string()),
                    location: Some(test_location()),
                }],
                location: Some(test_location()),
            }),
            location: Some(test_location()),
        };

        let ast = test_program_with_classes(vec![person_class]);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(
            result.is_ok(),
            "Class should convert to HIR successfully: {:?}",
            result.err()
        );

        let hir = result.unwrap().hir;
        assert_eq!(hir.classes.len(), 1);
        assert_eq!(hir.classes[0].name, "Person");
        assert_eq!(hir.classes[0].fields.len(), 1);
        assert_eq!(hir.classes[0].fields[0].name, "name");
        assert!(hir.classes[0].constructor.is_some());
    }

    #[test]
    #[ignore] // OBSOLETE: Duplicate detection is correctly implemented in Resolver (resolver_impl.rs:80-88), not HIR builder
    fn test_duplicate_function_error() {
        let duplicate1 = Function {
            name: "duplicate".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body: vec![],
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(test_location()),
        };

        let duplicate2 = Function {
            name: "duplicate".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Integer,
            body: vec![],
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(test_location()),
        };

        let ast = test_program_with_functions(vec![duplicate1, duplicate2]);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(result.is_err(), "Duplicate functions should cause an error");
    }

    #[test]
    #[ignore] // OBSOLETE: Duplicate detection is correctly implemented in Resolver (resolver_impl.rs:137-141), not HIR builder
    fn test_duplicate_class_error() {
        let duplicate1 = Class {
            name: "Duplicate".to_string(),
            type_parameters: vec![],
            description: None,
            base_class: None,
            base_class_type_args: vec![],
            fields: vec![],
            methods: vec![],
            constructor: None,
            location: Some(test_location()),
        };

        let duplicate2 = Class {
            name: "Duplicate".to_string(),
            type_parameters: vec![],
            description: None,
            base_class: None,
            base_class_type_args: vec![],
            fields: vec![],
            methods: vec![],
            constructor: None,
            location: Some(test_location()),
        };

        let ast = test_program_with_classes(vec![duplicate1, duplicate2]);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(result.is_err(), "Duplicate classes should cause an error");
    }

    #[test]
    fn test_expression_desugaring() {
        let start_fn = Function {
            name: "start".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body: vec![Statement::VariableDecl {
                name: "result".to_string(),
                type_: Type::Integer,
                initializer: Some(Expression::Binary(
                    Box::new(Expression::Binary(
                        Box::new(Expression::Literal(Value::Integer(5))),
                        AstBinaryOperator::Add,
                        Box::new(Expression::Literal(Value::Integer(3))),
                    )),
                    AstBinaryOperator::Multiply,
                    Box::new(Expression::Literal(Value::Integer(2))),
                )),
                location: Some(test_location()),
            }],
            description: None,
            syntax: FunctionSyntax::Standalone,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(test_location()),
        };

        let ast = test_program_with_start(start_fn);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(
            result.is_ok(),
            "Complex expressions should desugar correctly: {:?}",
            result.err()
        );

        let hir = result.unwrap().hir;
        let start = hir.start_function.unwrap();
        assert_eq!(start.body.statements.len(), 1);

        if let HirStatement::VariableDeclaration {
            initializer: Some(expr),
            ..
        } = &start.body.statements[0]
        {
            // Verify nested binary operations are preserved
            assert!(matches!(expr, HirExpression::BinaryOp { .. }));
        } else {
            panic!("Expected variable declaration with initializer");
        }
    }

    #[test]
    fn test_method_call_desugaring() {
        let start_fn = Function {
            name: "start".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body: vec![
                Statement::VariableDecl {
                    name: "text".to_string(),
                    type_: Type::String,
                    initializer: Some(Expression::Literal(Value::String("hello".to_string()))),
                    location: Some(test_location()),
                },
                Statement::VariableDecl {
                    name: "length".to_string(),
                    type_: Type::Integer,
                    initializer: Some(Expression::MethodCall {
                        object: Box::new(Expression::Variable("text".to_string())),
                        method: "length".to_string(),
                        arguments: vec![],
                        location: test_location(),
                    }),
                    location: Some(test_location()),
                },
            ],
            description: None,
            syntax: FunctionSyntax::Standalone,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(test_location()),
        };

        let ast = test_program_with_start(start_fn);

        let mut hir_builder = HirBuilder::new();
        let result = hir_builder.build_hir(ast);
        assert!(
            result.is_ok(),
            "Method calls should desugar correctly: {:?}",
            result.err()
        );

        let hir = result.unwrap().hir;
        let start = hir.start_function.unwrap();
        assert_eq!(start.body.statements.len(), 2);

        if let HirStatement::VariableDeclaration {
            initializer: Some(expr),
            ..
        } = &start.body.statements[1]
        {
            if let HirExpression::MethodCall { method, .. } = expr {
                assert_eq!(method, "length");
            } else {
                panic!("Expected method call expression, got: {:?}", expr);
            }
        } else {
            panic!("Expected variable declaration with initializer");
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_end_to_end_simple_program() {
        let add_fn = simple_add_function();

        let start_fn = Function {
            name: "start".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body: vec![Statement::VariableDecl {
                name: "result".to_string(),
                type_: Type::Integer,
                initializer: Some(Expression::Call(
                    "add".to_string(),
                    vec![
                        Expression::Literal(Value::Integer(5)),
                        Expression::Literal(Value::Integer(3)),
                    ],
                )),
                location: Some(test_location()),
            }],
            description: None,
            syntax: FunctionSyntax::Standalone,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(test_location()),
        };

        let ast = Program {
            imports: vec![],
            plugins: vec![],
            statements: vec![],
            functions: vec![add_fn],
            classes: vec![],
            start_function: Some(start_fn),
            tests: vec![],
            screens: vec![],
            state: None,
            watch_blocks: Vec::new(),
            screen_blocks: Vec::new(),
            externals: Vec::new(),
            source_block: None,
            location: Some(test_location()),
        };

        // Build HIR
        let mut hir_builder = HirBuilder::new();
        let hir_result = hir_builder.build_hir(ast);
        assert!(
            hir_result.is_ok(),
            "Simple program should build HIR successfully: {:?}",
            hir_result.err()
        );

        let hir = hir_result.unwrap().hir;

        // Verify structure
        assert_eq!(hir.functions.len(), 1);
        assert!(hir.start_function.is_some());
        assert_eq!(hir.classes.len(), 0);

        // Verify function
        let add_fn = &hir.functions[0];
        assert_eq!(add_fn.name, "add");
        assert_eq!(add_fn.parameters.len(), 2);

        // Verify start function
        let start = hir.start_function.unwrap();
        assert_eq!(start.name, "start");
        assert_eq!(start.body.statements.len(), 1);
    }

    #[test]
    fn test_end_to_end_class_with_inheritance() {
        let animal_class = Class {
            name: "Animal".to_string(),
            type_parameters: vec![],
            description: None,
            base_class: None,
            base_class_type_args: vec![],
            fields: vec![Field {
                name: "name".to_string(),
                type_: Type::String,
                visibility: Visibility::Public,
                is_static: false,
                default_value: None,
            }],
            methods: vec![Function {
                name: "getName".to_string(),
                type_parameters: vec![],
                type_constraints: vec![],
                parameters: vec![],
                return_type: Type::String,
                body: vec![Statement::Return {
                    value: Some(Expression::PropertyAccess {
                        object: Box::new(Expression::Variable("this".to_string())),
                        property: "name".to_string(),
                        location: test_location(),
                    }),
                    location: Some(test_location()),
                }],
                description: None,
                syntax: FunctionSyntax::Simple,
                visibility: Visibility::Public,
                modifier: FunctionModifier::None,
                location: Some(test_location()),
            }],
            constructor: None,
            location: Some(test_location()),
        };

        let dog_class = Class {
            name: "Dog".to_string(),
            type_parameters: vec![],
            description: None,
            base_class: Some("Animal".to_string()),
            base_class_type_args: vec![],
            fields: vec![Field {
                name: "breed".to_string(),
                type_: Type::String,
                visibility: Visibility::Public,
                is_static: false,
                default_value: None,
            }],
            methods: vec![],
            constructor: None,
            location: Some(test_location()),
        };

        let ast = test_program_with_classes(vec![animal_class, dog_class]);

        // Build HIR
        let mut hir_builder = HirBuilder::new();
        let hir_result = hir_builder.build_hir(ast);
        assert!(
            hir_result.is_ok(),
            "Class inheritance should build HIR successfully: {:?}",
            hir_result.err()
        );

        let hir = hir_result.unwrap().hir;

        // Verify structure
        assert_eq!(hir.classes.len(), 2);

        // Verify Animal class
        let animal = &hir.classes[0];
        assert_eq!(animal.name, "Animal");
        assert!(animal.parent.is_none());
        assert_eq!(animal.fields.len(), 1);
        assert_eq!(animal.methods.len(), 1);

        // Verify Dog class
        let dog = &hir.classes[1];
        assert_eq!(dog.name, "Dog");
        assert_eq!(dog.parent, Some("Animal".to_string()));
        assert_eq!(dog.fields.len(), 1);
    }
}
