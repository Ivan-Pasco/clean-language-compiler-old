# Clean Language Specification Compliance Testing Framework

Status: Technical specification for validation
Authority: Based on AST_Specification.md and pipeline_architecture.md
Version: 1.0
Updated: December 2024

## 1. Overview

This document defines the comprehensive testing framework to ensure 100% compliance with the Clean Language Specification. The testing strategy covers every language construct, syntax element, and semantic rule defined in the specification.

## 2. Testing Philosophy

### Core Principles

1. **Specification Authority**: Tests validate exact compliance with Clean Language Specification
2. **Complete Coverage**: Every language construct must have corresponding tests
3. **Golden Standard**: AST output must match specification exactly
4. **Stage Isolation**: Each pipeline stage tested independently
5. **End-to-End Validation**: Full compilation pipeline tested holistically

### Test Categories

1. **Unit Tests**: Individual components (lexer, parser, type checker)
2. **Golden Tests**: Expected AST output comparisons
3. **Integration Tests**: Multi-stage pipeline interactions
4. **Specification Tests**: Comprehensive language feature coverage
5. **Regression Tests**: Prevent specification compliance breakage
6. **Performance Tests**: Ensure reasonable compilation speeds

## 3. Specification Test Matrix

### Core Language Features (Clean Language Specification)

| Feature Category | Specification Section | Test Coverage | Status |
|------------------|----------------------|---------------|---------|
| **Lexical Elements** | §2 | Keywords, Operators, Literals, Identifiers | Required |
| **Type System** | §3 | Core types, Precision modifiers, Composite types | Required |
| **Apply Blocks** | §4 | Type, Function, Method, Constant apply blocks | Required |
| **Expressions** | §5 | All expression types, Operator precedence | Required |
| **Statements** | §6 | Declarations, Assignments, Control flow | Required |
| **Functions** | §7 | Function blocks, Parameters, Generics | Required |
| **Testing Framework** | §8 | Named/anonymous tests, Test blocks | Required |
| **Control Flow** | §9 | If/else, Loops, Pattern matching | Required |
| **Error Handling** | §10 | OnError, Error blocks, Console input | Required |
| **Classes** | §11 | Definitions, Inheritance, Constructors | Required |
| **Modules** | §12 | Imports, Private blocks, Visibility | Required |
| **Standard Library** | §14 | Namespace calls, Method-style syntax | Required |
| **Method-Style Syntax** | §16 | Preferred patterns, Call disambiguation | Required |
| **Async Programming** | §17 | Start expressions, Later assignments | Required |

## 4. Test Suite Structure

### Directory Organization

```
tests/
├── specification_compliance/
│   ├── lexical/                 # §2: Lexical elements
│   │   ├── keywords.rs
│   │   ├── operators.rs
│   │   ├── literals.rs
│   │   └── identifiers.rs
│   ├── type_system/             # §3: Type system
│   │   ├── core_types.rs
│   │   ├── precision_modifiers.rs
│   │   ├── composite_types.rs
│   │   └── list_behaviors.rs
│   ├── apply_blocks/            # §4: Apply blocks
│   │   ├── type_apply.rs
│   │   ├── function_apply.rs
│   │   ├── method_apply.rs
│   │   └── constant_apply.rs
│   ├── expressions/             # §5: Expressions
│   │   ├── operator_precedence.rs
│   │   ├── method_calls.rs
│   │   ├── property_access.rs
│   │   └── conditionals.rs
│   ├── statements/              # §6: Statements
│   ├── functions/               # §7: Functions
│   ├── testing_framework/       # §8: Testing
│   ├── control_flow/            # §9: Control flow
│   ├── error_handling/          # §10: Error handling
│   ├── classes/                 # §11: Classes
│   ├── modules/                 # §12: Modules
│   ├── standard_library/        # §14: Standard library
│   ├── method_style/            # §16: Method-style syntax
│   └── async_programming/       # §17: Async programming
├── golden_tests/                # Expected AST outputs
├── integration/                 # Multi-stage tests
├── regression/                  # Prevent breakage
└── performance/                 # Compilation speed
```

## 5. Golden Test Framework

### Golden Test Structure

```rust
/// Golden test case comparing actual AST output to expected
#[derive(Debug, Clone)]
pub struct GoldenTestCase {
    pub name: String,
    pub source_code: String,
    pub expected_ast: String,          // Serialized AST representation
    pub expected_tokens: Vec<Token>,   // Expected token stream
    pub specification_reference: String, // §X.Y reference
}

/// Golden test runner for AST compliance
pub struct GoldenTestRunner {
    pub lexer: Box<dyn CompilerStage<SourceCode, TokenStream, Error = LexError>>,
    pub parser: Box<dyn CompilerStage<TokenStream, ParsedProgram, Error = ParseError>>,
}

impl GoldenTestRunner {
    pub fn run_test(&self, test_case: &GoldenTestCase) -> Result<(), CompilerError> {
        // Stage 1: Lexical analysis
        let source = SourceCode {
            content: test_case.source_code.clone(),
            file_path: format!("test_{}.cln", test_case.name),
            encoding: SourceEncoding::Utf8,
        };
        
        let tokens = self.lexer.process(source)?;
        
        // Validate token stream matches expected
        assert_eq!(
            tokens.tokens, test_case.expected_tokens,
            "Token stream mismatch for test: {}", test_case.name
        );
        
        // Stage 2: Parsing
        let parsed = self.parser.process(tokens)?;
        
        // Serialize AST and compare with expected
        let actual_ast = serialize_ast(&parsed.program);
        assert_eq!(
            actual_ast, test_case.expected_ast,
            "AST mismatch for test: {} (spec: {})",
            test_case.name, test_case.specification_reference
        );
        
        Ok(())
    }
}

/// Serialize AST to stable, comparable format
fn serialize_ast(program: &Program) -> String {
    // Deterministic serialization of AST nodes
    // Format: Human-readable with stable ordering
    format!("{:#?}", program)
}
```

## 6. Specification Test Cases

### 6.1 Lexical Elements (§2)

```rust
#[cfg(test)]
mod lexical_tests {
    use super::*;
    
    #[test]
    fn test_all_keywords() {
        let test_cases = vec![
            // All keywords from specification
            ("and", TokenKind::And),
            ("class", TokenKind::Class),
            ("constructor", TokenKind::Constructor),
            ("else", TokenKind::Else),
            ("error", TokenKind::Error),
            ("false", TokenKind::False),
            ("for", TokenKind::For),
            ("from", TokenKind::From),
            ("function", TokenKind::Function),
            ("if", TokenKind::If),
            ("import", TokenKind::Import),
            ("in", TokenKind::In),
            ("iterate", TokenKind::Iterate),
            ("not", TokenKind::Not),
            ("onError", TokenKind::OnError),
            ("or", TokenKind::Or),
            ("print", TokenKind::Print),
            ("println", TokenKind::Println),
            ("return", TokenKind::Return),
            ("start", TokenKind::Start),
            ("step", TokenKind::Step),
            ("test", TokenKind::Test),
            ("tests", TokenKind::Tests),
            ("this", TokenKind::This),
            ("to", TokenKind::To),
            ("true", TokenKind::True),
            ("while", TokenKind::While),
            ("is", TokenKind::Is),
            ("returns", TokenKind::Returns),
            ("description", TokenKind::Description),
            ("input", TokenKind::Input),
            ("unit", TokenKind::Unit),
            ("private", TokenKind::Private),
            ("constant", TokenKind::Constant),
            ("functions", TokenKind::Functions),
        ];
        
        for (source, expected_token) in test_cases {
            let lexer = create_test_lexer();
            let result = lexer.tokenize(source, "test.cln").unwrap();
            assert_eq!(result[0].kind, expected_token);
        }
    }
    
    #[test]
    fn test_precision_modifiers() {
        let test_cases = vec![
            ("42:8", TokenKind::Integer8Literal(42)),
            ("42:8u", TokenKind::Integer8uLiteral(42)),
            ("42:16", TokenKind::Integer16Literal(42)),
            ("42:16u", TokenKind::Integer16uLiteral(42)),
            ("42:32", TokenKind::Integer32Literal(42)),
            ("42:64", TokenKind::Integer64Literal(42)),
            ("3.14:32", TokenKind::Number32Literal(3.14)),
            ("3.14:64", TokenKind::Number64Literal(3.14)),
        ];
        
        for (source, expected_token) in test_cases {
            let lexer = create_test_lexer();
            let result = lexer.tokenize(source, "test.cln").unwrap();
            assert_eq!(result[0].kind, expected_token);
        }
    }
    
    #[test]
    fn test_string_interpolation() {
        let source = r#""Hello {name}!""#;
        let lexer = create_test_lexer();
        let result = lexer.tokenize(source, "test.cln").unwrap();
        
        let expected = vec![
            TokenKind::InterpolationStart,
            TokenKind::Identifier("name".to_string()),
            TokenKind::InterpolationEnd,
        ];
        
        // Verify string interpolation tokenization
        assert!(matches_interpolation_pattern(&result, &expected));
    }
}
```

### 6.2 Type System (§3)

```rust
#[cfg(test)]
mod type_system_tests {
    use super::*;
    
    #[test]
    fn test_core_types() {
        let test_cases = vec![
            ("boolean x = true", Type::Boolean),
            ("integer y = 42", Type::Integer),
            ("number z = 3.14", Type::Number),
            ("string s = \"hello\"", Type::String),
            ("void", Type::Void),
        ];
        
        for (source, expected_type) in test_cases {
            let ast = parse_test_source(source);
            // Verify type annotation matches expected
            assert_type_matches(&ast, expected_type);
        }
    }
    
    #[test]
    fn test_precision_modifiers() {
        let test_cases = vec![
            ("integer:8 x", Type::IntegerSized { bits: 8, unsigned: false }),
            ("integer:8u y", Type::IntegerSized { bits: 8, unsigned: true }),
            ("integer:16 z", Type::IntegerSized { bits: 16, unsigned: false }),
            ("integer:16u w", Type::IntegerSized { bits: 16, unsigned: true }),
            ("integer:32 a", Type::IntegerSized { bits: 32, unsigned: false }),
            ("integer:64 b", Type::IntegerSized { bits: 64, unsigned: false }),
            ("number:32 c", Type::NumberSized { bits: 32 }),
            ("number:64 d", Type::NumberSized { bits: 64 }),
        ];
        
        for (source, expected_type) in test_cases {
            let ast = parse_test_source(source);
            assert_type_matches(&ast, expected_type);
        }
    }
    
    #[test]
    fn test_composite_types() {
        let test_cases = vec![
            ("list<integer> items", Type::List(Box::new(Type::Integer))),
            ("matrix<number> data", Type::Matrix(Box::new(Type::Number))),
            ("pairs<string,integer> map", Type::Pairs(Box::new(Type::String), Box::new(Type::Integer))),
        ];
        
        for (source, expected_type) in test_cases {
            let ast = parse_test_source(source);
            assert_type_matches(&ast, expected_type);
        }
    }
    
    #[test]
    fn test_list_behaviors() {
        let test_cases = vec![
            ("list.type = \"line\"", "FIFO queue behavior"),
            ("list.type = \"pile\"", "LIFO stack behavior"),
            ("list.type = \"unique\"", "Set behavior (no duplicates)"),
            ("list.type = \"line-unique\"", "FIFO with uniqueness"),
            ("list.type = \"pile-unique\"", "LIFO with uniqueness"),
        ];
        
        for (source, description) in test_cases {
            let ast = parse_test_source(source);
            // Verify property assignment is correctly parsed
            assert_property_assignment(&ast, "list", "type");
        }
    }
}
```

### 6.3 Apply Blocks (§4)

```rust
#[cfg(test)]
mod apply_blocks_tests {
    use super::*;
    
    #[test]
    fn test_type_apply_blocks() {
        let source = r#"
integer:
    count = 0
    maxSize = 100
"#;
        let ast = parse_test_source(source);
        
        // Verify TypeApplyBlock structure
        assert_matches!(
            ast.statements[0],
            Statement::TypeApplyBlock {
                type_: Type::Integer,
                assignments: ref assignments,
                ..
            } if assignments.len() == 2
        );
    }
    
    #[test]
    fn test_function_apply_blocks() {
        let source = r#"
println:
    "Hello"
    "World"
"#;
        let ast = parse_test_source(source);
        
        // Verify FunctionApplyBlock structure
        assert_matches!(
            ast.statements[0],
            Statement::FunctionApplyBlock {
                function_name: ref name,
                expressions: ref exprs,
                ..
            } if name == "println" && exprs.len() == 2
        );
    }
    
    #[test]
    fn test_method_apply_blocks() {
        let source = r#"
list.push:
    item1
    item2
    item3
"#;
        let ast = parse_test_source(source);
        
        // Verify MethodApplyBlock structure
        assert_matches!(
            ast.statements[0],
            Statement::MethodApplyBlock {
                object_name: ref obj,
                method_chain: ref methods,
                expressions: ref exprs,
                ..
            } if obj == "list" && methods == &["push"] && exprs.len() == 3
        );
    }
    
    #[test]
    fn test_constant_apply_blocks() {
        let source = r#"
constant:
    integer MAX_SIZE = 100
    number PI = 3.14159
    string VERSION = "1.0.0"
"#;
        let ast = parse_test_source(source);
        
        // Verify ConstantApplyBlock structure
        assert_matches!(
            ast.statements[0],
            Statement::ConstantApplyBlock {
                constants: ref consts,
                ..
            } if consts.len() == 3
        );
    }
}
```

### 6.4 Expression Precedence (§5.1)

```rust
#[cfg(test)]
mod expression_precedence_tests {
    use super::*;
    
    #[test]
    fn test_operator_precedence() {
        let test_cases = vec![
            // Test precedence: Primary → Unary → Power → Multiplicative → Additive → Comparison → Equality → Logical AND → Logical OR
            ("a + b * c", "a + (b * c)"),
            ("a * b ^ c", "a * (b ^ c)"),
            ("-a ^ b", "-(a ^ b)"),
            ("a < b and c > d", "(a < b) and (c > d)"),
            ("a == b or c != d", "(a == b) or (c != d)"),
            ("not a and b", "(not a) and b"),
        ];
        
        for (source, expected_structure) in test_cases {
            let ast = parse_expression(source);
            let actual_structure = serialize_expression_structure(&ast);
            assert_eq!(
                actual_structure, expected_structure,
                "Precedence mismatch for: {}", source
            );
        }
    }
    
    #[test]
    fn test_associativity() {
        let test_cases = vec![
            ("a - b - c", "(a - b) - c"), // Left associative
            ("a ^ b ^ c", "a ^ (b ^ c)"), // Right associative (power)
            ("a and b and c", "(a and b) and c"), // Left associative
        ];
        
        for (source, expected_structure) in test_cases {
            let ast = parse_expression(source);
            let actual_structure = serialize_expression_structure(&ast);
            assert_eq!(
                actual_structure, expected_structure,
                "Associativity mismatch for: {}", source
            );
        }
    }
}
```

### 6.5 Method Call Disambiguation (§16)

```rust
#[cfg(test)]
mod method_disambiguation_tests {
    use super::*;
    
    #[test]
    fn test_call_disambiguation() {
        let test_cases = vec![
            // Namespace calls (lowercase namespace)
            ("math.sqrt(9)", ExpressionType::NamespaceCall),
            ("string.concat(\"a\", \"b\")", ExpressionType::NamespaceCall),
            
            // Method calls (object method)
            ("obj.method(9)", ExpressionType::MethodCall),
            ("name.length()", ExpressionType::MethodCall),
            
            // Static method calls (class method)
            ("String.from(123)", ExpressionType::StaticMethodCall),
            ("Math.abs(-5)", ExpressionType::StaticMethodCall),
        ];
        
        for (source, expected_type) in test_cases {
            let ast = parse_expression(source);
            assert_expression_type(&ast, expected_type);
        }
    }
}
```

## 7. Integration Test Framework

### Pipeline Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    
    #[test]
    fn test_complete_compilation_pipeline() {
        let source_code = r#"
functions:
    integer add(integer a, integer b)
        return a + b

start()
    integer result = add(2, 3)
    print result ln
"#;
        
        // Run complete pipeline
        let pipeline = create_test_pipeline();
        let compiled = pipeline.compile(SourceCode {
            content: source_code.to_string(),
            file_path: "test.cln".to_string(),
            encoding: SourceEncoding::Utf8,
        }).unwrap();
        
        // Verify successful compilation
        assert!(!compiled.wasm_binary.is_empty());
        
        // Verify exports match expectations
        assert!(compiled.export_metadata.exported_functions.contains(&"start".to_string()));
    }
    
    #[test]
    fn test_error_propagation() {
        let invalid_source = r#"
functions:
    integer add(integer a, integer b
        return a + b  // Missing closing parenthesis
"#;
        
        let pipeline = create_test_pipeline();
        let result = pipeline.compile(SourceCode {
            content: invalid_source.to_string(),
            file_path: "test.cln".to_string(),
            encoding: SourceEncoding::Utf8,
        });
        
        // Verify error is caught and reported correctly
        assert!(result.is_err());
        match result.unwrap_err() {
            CompilerError::ParseError(ParseError::UnexpectedToken { location, .. }) => {
                assert_eq!(location.line, 3); // Error on line 3
            }
            _ => panic!("Expected ParseError"),
        }
    }
}
```

## 8. Performance Testing

### Compilation Speed Benchmarks

```rust
#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Duration;
    
    #[test]
    fn test_compilation_speed() {
        let test_cases = vec![
            ("small_program.cln", Duration::from_millis(10)),
            ("medium_program.cln", Duration::from_millis(100)),
            ("large_program.cln", Duration::from_millis(1000)),
        ];
        
        for (file, max_duration) in test_cases {
            let source = load_test_file(file);
            let pipeline = create_test_pipeline();
            
            let start = std::time::Instant::now();
            let _compiled = pipeline.compile(source).unwrap();
            let duration = start.elapsed();
            
            assert!(
                duration <= max_duration,
                "Compilation of {} took {:?}, expected <= {:?}",
                file, duration, max_duration
            );
        }
    }
}
```

## 9. Test Execution Infrastructure

### Test Runner

```rust
/// Main test runner for specification compliance
pub struct SpecificationTestRunner {
    pub pipeline: CompilerPipeline,
    pub golden_tests: Vec<GoldenTestCase>,
    pub integration_tests: Vec<IntegrationTestCase>,
}

impl SpecificationTestRunner {
    pub fn run_all_tests(&self) -> TestResults {
        let mut results = TestResults::new();
        
        // Run golden tests
        for test in &self.golden_tests {
            match self.run_golden_test(test) {
                Ok(_) => results.passed += 1,
                Err(e) => {
                    results.failed += 1;
                    results.failures.push(TestFailure {
                        test_name: test.name.clone(),
                        error: e.to_string(),
                        specification_reference: test.specification_reference.clone(),
                    });
                }
            }
        }
        
        // Run integration tests
        for test in &self.integration_tests {
            match self.run_integration_test(test) {
                Ok(_) => results.passed += 1,
                Err(e) => {
                    results.failed += 1;
                    results.failures.push(TestFailure {
                        test_name: test.name.clone(),
                        error: e.to_string(),
                        specification_reference: test.specification_reference.clone(),
                    });
                }
            }
        }
        
        results
    }
}

#[derive(Debug)]
pub struct TestResults {
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<TestFailure>,
}

#[derive(Debug)]
pub struct TestFailure {
    pub test_name: String,
    pub error: String,
    pub specification_reference: String,
}
```

## 10. Continuous Integration

### CI Pipeline Configuration

```yaml
# .github/workflows/specification-compliance.yml
name: Specification Compliance Tests

on: [push, pull_request]

jobs:
  specification_tests:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v2
    
    - name: Setup Rust
      uses: actions-rs/toolchain@v1
      with:
        toolchain: stable
        
    - name: Run specification compliance tests
      run: |
        cargo test --test specification_compliance --verbose
        
    - name: Run golden tests
      run: |
        cargo test --test golden_tests --verbose
        
    - name: Run integration tests
      run: |
        cargo test --test integration --verbose
        
    - name: Generate compliance report
      run: |
        cargo run --bin compliance-report > compliance_report.md
        
    - name: Upload compliance report
      uses: actions/upload-artifact@v2
      with:
        name: compliance-report
        path: compliance_report.md
```

---

**Authority Note**: This testing framework ensures 100% compliance with the Clean Language Specification. All tests must pass for the compiler to be considered specification-compliant. Any test failures indicate deviations from the specification that must be corrected.