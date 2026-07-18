# Clean Language Testing Strategy Documentation

This document provides comprehensive guidance for Claude on testing strategies, frameworks, and best practices for the Clean Language compiler. This knowledge is essential for maintaining code quality, preventing regressions, and ensuring reliable compiler behavior across all compilation phases.

> 🔗 **Related Documentation**: [AST Reference](./ast-reference.md) • [Error Handling Guide](./error-handling-guide.md) • [Memory Management](./memory-management.md) • [Development Guide](./development-guide.md)

## Overview

The Clean Language compiler employs a multi-layered testing strategy that covers unit testing, integration testing, performance testing, and end-to-end validation. The testing infrastructure is designed to catch issues early, provide fast feedback during development, and ensure robust behavior across different scenarios and edge cases.

## Testing Architecture

### 1. Test Organization Structure

```
tests/
├── unit/                    # Unit tests for individual components
│   ├── lexer/
│   ├── parser/
│   ├── semantic/
│   ├── codegen/
│   └── stdlib/
├── integration/             # Integration tests
│   ├── compilation/
│   ├── execution/
│   └── interop/
├── clean_files/            # Clean Language test files
│   ├── basic/
│   ├── advanced/
│   ├── error_cases/
│   └── performance/
├── wasm/                   # Generated WebAssembly output
├── fixtures/               # Test data and fixtures
├── golden/                 # Golden master test outputs
└── benchmarks/             # Performance benchmarks
```

### 2. Test Framework Architecture (`tests/framework/mod.rs`)

```rust
/// Core testing framework for Clean Language compiler
pub struct TestFramework {
    test_runner: TestRunner,
    result_collector: ResultCollector,
    config: TestConfig,
    harness: TestHarness,
}

#[derive(Debug, Clone)]
pub struct TestConfig {
    pub parallel_execution: bool,
    pub timeout_seconds: u64,
    pub verbose_output: bool,
    pub fail_fast: bool,
    pub filter_pattern: Option<regex::Regex>,
    pub optimization_levels: Vec<OptimizationLevel>,
    pub target_backends: Vec<TargetBackend>,
}

#[derive(Debug, Clone)]
pub enum TargetBackend {
    WebAssembly,
    Debug,
    Interpreter,
}

impl TestFramework {
    pub fn new(config: TestConfig) -> Self {
        Self {
            test_runner: TestRunner::new(config.clone()),
            result_collector: ResultCollector::new(),
            config,
            harness: TestHarness::new(),
        }
    }
    
    pub fn run_all_tests(&mut self) -> TestResults {
        let mut results = TestResults::new();
        
        // Run different test suites
        results.merge(self.run_unit_tests());
        results.merge(self.run_integration_tests());
        results.merge(self.run_regression_tests());
        results.merge(self.run_performance_tests());
        results.merge(self.run_error_handling_tests());
        
        results
    }
    
    pub fn run_unit_tests(&mut self) -> TestResults {
        let test_cases = self.discover_unit_tests();
        self.test_runner.run_test_suite("Unit Tests", test_cases)
    }
    
    pub fn run_integration_tests(&mut self) -> TestResults {
        let test_cases = self.discover_integration_tests();
        self.test_runner.run_test_suite("Integration Tests", test_cases)
    }
    
    fn discover_unit_tests(&self) -> Vec<TestCase> {
        let mut test_cases = Vec::new();
        
        // Discover tests using reflection-like mechanisms
        test_cases.extend(self.discover_lexer_tests());
        test_cases.extend(self.discover_parser_tests());
        test_cases.extend(self.discover_semantic_tests());
        test_cases.extend(self.discover_codegen_tests());
        test_cases.extend(self.discover_stdlib_tests());
        
        test_cases
    }
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub category: TestCategory,
    pub test_function: TestFunction,
    pub setup: Option<SetupFunction>,
    pub teardown: Option<TeardownFunction>,
    pub timeout: Option<std::time::Duration>,
    pub expected_outcome: ExpectedOutcome,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TestCategory {
    Unit,
    Integration,
    Regression,
    Performance,
    ErrorHandling,
    Memory,
}

#[derive(Debug, Clone)]
pub enum ExpectedOutcome {
    Success,
    CompilationError(String),    // Expected error pattern
    RuntimeError(String),        // Expected runtime error
    Timeout,
    Performance(PerformanceCriteria),
}

#[derive(Debug, Clone)]
pub struct PerformanceCriteria {
    pub max_compilation_time: std::time::Duration,
    pub max_memory_usage: usize,
    pub max_output_size: usize,
}

type TestFunction = fn(&TestContext) -> Result<TestResult, TestError>;
type SetupFunction = fn() -> Result<TestContext, TestError>;
type TeardownFunction = fn(&TestContext) -> Result<(), TestError>;
```

## Unit Testing Framework

### 1. Lexer Unit Tests (`tests/unit/lexer_tests.rs`)

```rust
use crate::lexer::*;
use crate::tests::framework::*;

pub struct LexerTests;

impl LexerTests {
    pub fn all_tests() -> Vec<TestCase> {
        vec![
            test_case("basic_tokens", Self::test_basic_tokens),
            test_case("numbers", Self::test_number_literals),
            test_case("strings", Self::test_string_literals),
            test_case("identifiers", Self::test_identifiers),
            test_case("keywords", Self::test_keywords),
            test_case("operators", Self::test_operators),
            test_case("comments", Self::test_comments),
            test_case("error_recovery", Self::test_error_recovery),
            test_case("unicode", Self::test_unicode_support),
            test_case("performance", Self::test_lexer_performance),
        ]
    }
    
    fn test_basic_tokens(ctx: &TestContext) -> Result<TestResult, TestError> {
        let input = "let x = 42;";
        let expected_tokens = vec![
            Token::new(TokenKind::Let, Span::new(0, 3)),
            Token::new(TokenKind::Identifier("x".to_string()), Span::new(4, 5)),
            Token::new(TokenKind::Assign, Span::new(6, 7)),
            Token::new(TokenKind::Integer(42), Span::new(8, 10)),
            Token::new(TokenKind::Semicolon, Span::new(10, 11)),
            Token::new(TokenKind::Eof, Span::new(11, 11)),
        ];
        
        let mut lexer = Lexer::new(input);
        let mut actual_tokens = Vec::new();
        
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    let is_eof = token.kind == TokenKind::Eof;
                    actual_tokens.push(token);
                    if is_eof { break; }
                }
                Err(e) => return Ok(TestResult::Failed(format!("Lexing error: {}", e))),
            }
        }
        
        if actual_tokens == expected_tokens {
            Ok(TestResult::Passed)
        } else {
            Ok(TestResult::Failed(format!(
                "Token mismatch:\nExpected: {:?}\nActual: {:?}",
                expected_tokens, actual_tokens
            )))
        }
    }
    
    fn test_number_literals(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            ("42", TokenKind::Integer(42)),
            ("0", TokenKind::Integer(0)),
            ("123", TokenKind::Integer(123)),
            ("3.14", TokenKind::Number(3.14)),
            ("0.5", TokenKind::Number(0.5)),
            ("42.0", TokenKind::Number(42.0)),
            ("1e5", TokenKind::Number(1e5)),
            ("1.5e-3", TokenKind::Number(1.5e-3)),
        ];
        
        for (input, expected_kind) in test_cases {
            let mut lexer = Lexer::new(input);
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind != expected_kind {
                        return Ok(TestResult::Failed(format!(
                            "For input '{}': expected {:?}, got {:?}",
                            input, expected_kind, token.kind
                        )));
                    }
                }
                Err(e) => {
                    return Ok(TestResult::Failed(format!(
                        "Lexing error for '{}': {}", input, e
                    )));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_string_literals(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            ("\"hello\"", "hello"),
            ("\"world with spaces\"", "world with spaces"),
            ("\"escape\\nsequence\"", "escape\nsequence"),
            ("\"unicode: 🦀\"", "unicode: 🦀"),
            ("r\"raw string\\n\"", "raw string\\n"),
        ];
        
        for (input, expected_content) in test_cases {
            let mut lexer = Lexer::new(input);
            match lexer.next_token() {
                Ok(token) => {
                    match token.kind {
                        TokenKind::String(content) => {
                            if content != expected_content {
                                return Ok(TestResult::Failed(format!(
                                    "String content mismatch for '{}': expected '{}', got '{}'",
                                    input, expected_content, content
                                )));
                            }
                        }
                        _ => {
                            return Ok(TestResult::Failed(format!(
                                "Expected string token for '{}', got {:?}",
                                input, token.kind
                            )));
                        }
                    }
                }
                Err(e) => {
                    return Ok(TestResult::Failed(format!(
                        "Lexing error for '{}': {}", input, e
                    )));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_error_recovery(ctx: &TestContext) -> Result<TestResult, TestError> {
        let input = "let x = 42; invalid@symbol; let y = 10;";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        let mut errors = Vec::new();
        
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    let is_eof = token.kind == TokenKind::Eof;
                    tokens.push(token);
                    if is_eof { break; }
                }
                Err(e) => {
                    errors.push(e);
                    // Continue lexing after error
                    continue;
                }
            }
        }
        
        // Should have recovered and found the second let statement
        let has_second_let = tokens.iter().any(|t| {
            matches!(t.kind, TokenKind::Identifier(ref name) if name == "y")
        });
        
        if !has_second_let {
            return Ok(TestResult::Failed(
                "Lexer failed to recover and parse second statement".to_string()
            ));
        }
        
        if errors.is_empty() {
            return Ok(TestResult::Failed(
                "Expected lexing errors for invalid symbol".to_string()
            ));
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_lexer_performance(ctx: &TestContext) -> Result<TestResult, TestError> {
        // Generate large input for performance testing
        let mut large_input = String::new();
        for i in 0..10000 {
            large_input.push_str(&format!("let var{} = {};\n", i, i));
        }
        
        let start_time = std::time::Instant::now();
        let mut lexer = Lexer::new(&large_input);
        let mut token_count = 0;
        
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    token_count += 1;
                    if token.kind == TokenKind::Eof { break; }
                }
                Err(e) => {
                    return Ok(TestResult::Failed(format!("Lexing error: {}", e)));
                }
            }
        }
        
        let elapsed = start_time.elapsed();
        let tokens_per_second = token_count as f64 / elapsed.as_secs_f64();
        
        // Performance criterion: should process at least 100,000 tokens per second
        if tokens_per_second < 100_000.0 {
            Ok(TestResult::Failed(format!(
                "Lexer performance too slow: {:.0} tokens/sec (expected >= 100,000)",
                tokens_per_second
            )))
        } else {
            Ok(TestResult::Passed)
        }
    }
}

fn test_case(name: &str, test_fn: TestFunction) -> TestCase {
    TestCase {
        name: name.to_string(),
        category: TestCategory::Unit,
        test_function: test_fn,
        setup: None,
        teardown: None,
        timeout: Some(std::time::Duration::from_secs(30)),
        expected_outcome: ExpectedOutcome::Success,
        tags: vec!["lexer".to_string()],
    }
}
```

### 2. Parser Unit Tests (`tests/unit/parser_tests.rs`)

```rust
use crate::parser::*;
use crate::ast::*;
use crate::tests::framework::*;

pub struct ParserTests;

impl ParserTests {
    pub fn all_tests() -> Vec<TestCase> {
        vec![
            test_case("expressions", Self::test_expression_parsing),
            test_case("statements", Self::test_statement_parsing),
            test_case("functions", Self::test_function_parsing),
            test_case("classes", Self::test_class_parsing),
            test_case("data_types", Self::test_data_type_parsing),
            test_case("precedence", Self::test_operator_precedence),
            test_case("error_recovery", Self::test_error_recovery),
            test_case("edge_cases", Self::test_edge_cases),
        ]
    }
    
    fn test_expression_parsing(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            ("42", "literal integer"),
            ("3.14", "literal number"),
            ("\"hello\"", "literal string"),
            ("true", "literal boolean"),
            ("x", "variable"),
            ("x + y", "binary expression"),
            ("x * y + z", "binary with precedence"),
            ("(x + y) * z", "parenthesized expression"),
            ("f(x, y)", "function call"),
            ("obj.method()", "method call"),
            ("list[index]", "index expression"),
            ("[1, 2, 3]", "list literal"),
            ("[[1, 2], [3, 4]]", "matrix literal"),
            ("x > 0 ? y : z", "conditional expression"),
        ];
        
        for (input, description) in test_cases {
            let mut parser = Parser::new(input);
            match parser.parse_expression() {
                Ok(expr) => {
                    // Verify the expression is well-formed
                    if !Self::is_valid_expression(&expr) {
                        return Ok(TestResult::Failed(format!(
                            "Invalid expression structure for {} ({}): {:?}",
                            description, input, expr
                        )));
                    }
                }
                Err(e) => {
                    return Ok(TestResult::Failed(format!(
                        "Parse error for {} ({}): {}", description, input, e
                    )));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_function_parsing(ctx: &TestContext) -> Result<TestResult, TestError> {
        let input = r#"
            function add(a: integer, b: integer) -> integer {
                return a + b;
            }
            
            async function fetchData(url: string) -> string {
                let response = http.get(url);
                return response;
            }
        "#;
        
        let mut parser = Parser::new(input);
        match parser.parse_program() {
            Ok(program) => {
                if program.declarations.len() != 2 {
                    return Ok(TestResult::Failed(format!(
                        "Expected 2 function declarations, got {}", 
                        program.declarations.len()
                    )));
                }
                
                // Check first function
                if let Declaration::Function(ref func) = program.declarations[0] {
                    if func.name.name != "add" {
                        return Ok(TestResult::Failed(
                            "First function name should be 'add'".to_string()
                        ));
                    }
                    
                    if func.parameters.len() != 2 {
                        return Ok(TestResult::Failed(
                            "First function should have 2 parameters".to_string()
                        ));
                    }
                    
                    if func.is_async {
                        return Ok(TestResult::Failed(
                            "First function should not be async".to_string()
                        ));
                    }
                } else {
                    return Ok(TestResult::Failed(
                        "First declaration should be a function".to_string()
                    ));
                }
                
                // Check second function
                if let Declaration::Function(ref func) = program.declarations[1] {
                    if func.name.name != "fetchData" {
                        return Ok(TestResult::Failed(
                            "Second function name should be 'fetchData'".to_string()
                        ));
                    }
                    
                    if !func.is_async {
                        return Ok(TestResult::Failed(
                            "Second function should be async".to_string()
                        ));
                    }
                } else {
                    return Ok(TestResult::Failed(
                        "Second declaration should be a function".to_string()
                    ));
                }
            }
            Err(e) => {
                return Ok(TestResult::Failed(format!("Parse error: {}", e)));
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_operator_precedence(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            ("1 + 2 * 3", "should parse as 1 + (2 * 3)"),
            ("a && b || c", "should parse as (a && b) || c"),
            ("x < y && y < z", "should parse as (x < y) && (y < z)"),
            ("f() + g() * h()", "should parse as f() + (g() * h())"),
            ("!x && y", "should parse as (!x) && y"),
            ("-x * y", "should parse as (-x) * y"),
        ];
        
        for (input, expected_behavior) in test_cases {
            let mut parser = Parser::new(input);
            match parser.parse_expression() {
                Ok(expr) => {
                    // Verify precedence by checking AST structure
                    if !Self::has_correct_precedence(&expr, input) {
                        return Ok(TestResult::Failed(format!(
                            "Incorrect precedence for '{}': {}", input, expected_behavior
                        )));
                    }
                }
                Err(e) => {
                    return Ok(TestResult::Failed(format!(
                        "Parse error for '{}': {}", input, e
                    )));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_error_recovery(ctx: &TestContext) -> Result<TestResult, TestError> {
        let input = r#"
            function valid1() {
                return 42;
            }
            
            function invalid {  // Missing parentheses
                return "error";
            }
            
            function valid2() {
                return true;
            }
        "#;
        
        let mut parser = Parser::new(input);
        parser.set_error_recovery(true);
        
        match parser.parse_program() {
            Ok(program) => {
                // Should have recovered and parsed valid functions
                if program.declarations.len() < 2 {
                    return Ok(TestResult::Failed(
                        "Parser should have recovered and found valid functions".to_string()
                    ));
                }
            }
            Err(_) => {
                // Check if parser collected errors but continued
                let errors = parser.get_collected_errors();
                if errors.is_empty() {
                    return Ok(TestResult::Failed(
                        "Expected parse errors to be collected".to_string()
                    ));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn is_valid_expression(expr: &Expression) -> bool {
        match expr {
            Expression::Literal(_) => true,
            Expression::Variable(var) => !var.name.name.is_empty(),
            Expression::Binary(bin_expr) => {
                Self::is_valid_expression(&bin_expr.left) && 
                Self::is_valid_expression(&bin_expr.right)
            }
            Expression::Call(call_expr) => {
                call_expr.arguments.iter().all(|arg| Self::is_valid_expression(&arg.expression))
            }
            _ => true, // Simplified validation
        }
    }
    
    fn has_correct_precedence(expr: &Expression, input: &str) -> bool {
        // Simplified precedence checking
        match input {
            "1 + 2 * 3" => {
                // Should be Add(1, Mul(2, 3))
                if let Expression::Binary(bin_expr) = expr {
                    matches!(bin_expr.operator, BinaryOperator::Add) &&
                    matches!(**bin_expr.left, Expression::Literal(_)) &&
                    matches!(**bin_expr.right, Expression::Binary(_))
                } else {
                    false
                }
            }
            _ => true, // Simplified for other cases
        }
    }
}
```

### 3. Semantic Analysis Tests (`tests/unit/semantic_tests.rs`)

```rust
use crate::semantic::*;
use crate::tests::framework::*;

pub struct SemanticTests;

impl SemanticTests {
    pub fn all_tests() -> Vec<TestCase> {
        vec![
            test_case("type_checking", Self::test_type_checking),
            test_case("scope_resolution", Self::test_scope_resolution),
            test_case("inheritance", Self::test_inheritance),
            test_case("async_functions", Self::test_async_functions),
            test_case("generic_types", Self::test_generic_types),
            test_case("error_cases", Self::test_error_cases),
        ]
    }
    
    fn test_type_checking(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            // Valid type cases
            TestCase::valid(r#"
                let x: integer = 42;
                let y: number = 3.14;
                let z: string = "hello";
                let flag: boolean = true;
            "#),
            
            // Type inference cases
            TestCase::valid(r#"
                let x = 42;      // Should infer integer
                let y = 3.14;    // Should infer number
                let z = "hello"; // Should infer string
            "#),
            
            // Function return type checking
            TestCase::valid(r#"
                function add(a: integer, b: integer) -> integer {
                    return a + b;
                }
            "#),
            
            // Invalid type cases
            TestCase::invalid(r#"
                let x: integer = "string"; // Type mismatch
            "#, "TypeError"),
            
            TestCase::invalid(r#"
                function add(a: integer, b: integer) -> string {
                    return a + b; // Wrong return type
                }
            "#, "TypeError"),
        ];
        
        Self::run_semantic_test_cases(test_cases)
    }
    
    fn test_scope_resolution(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            // Valid scope cases
            TestCase::valid(r#"
                let global = 42;
                
                function test() {
                    let local = global + 1;
                    return local;
                }
            "#),
            
            // Variable shadowing
            TestCase::valid(r#"
                let x = 10;
                
                function test() {
                    let x = 20;  // Shadows outer x
                    return x;
                }
            "#),
            
            // Invalid scope cases
            TestCase::invalid(r#"
                function test() {
                    return undefined_variable; // Undefined variable
                }
            "#, "UndefinedVariable"),
            
            TestCase::invalid(r#"
                function test() {
                    let x = x + 1; // Use before definition
                }
            "#, "UndefinedVariable"),
        ];
        
        Self::run_semantic_test_cases(test_cases)
    }
    
    fn test_inheritance(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            // Valid inheritance
            TestCase::valid(r#"
                class Animal {
                    string name;
                    
                    constructor(string name) {
                        this.name = name;
                    }
                    
                    string speak() {
                        return "Some sound";
                    }
                }
                
                class Dog extends Animal {
                    constructor(string name) {
                        base(name);
                    }
                    
                    string speak() {
                        return "Woof!";
                    }
                }
            "#),
            
            // Invalid inheritance cases
            TestCase::invalid(r#"
                class Dog extends UndefinedClass {
                }
            "#, "UndefinedClass"),
            
            TestCase::invalid(r#"
                class Animal {
                    constructor() {}
                }
                
                class Dog extends Animal {
                    constructor() {
                        // Missing base() call
                    }
                }
            "#, "MissingBaseCall"),
        ];
        
        Self::run_semantic_test_cases(test_cases)
    }
    
    fn test_async_functions(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_cases = vec![
            // Valid async functions
            TestCase::valid(r#"
                async function fetchData() -> string {
                    let response = await http.get("http://example.com");
                    return response;
                }
                
                async function processData() {
                    let data = await fetchData();
                    println(data);
                }
            "#),
            
            // Invalid async usage
            TestCase::invalid(r#"
                function syncFunction() -> string {
                    let result = await fetchData(); // await in non-async function
                    return result;
                }
            "#, "AwaitInNonAsync"),
            
            TestCase::invalid(r#"
                async function asyncFunction() {
                    let result = fetchData(); // Missing await for async call
                    return result;
                }
            "#, "MissingAwait"),
        ];
        
        Self::run_semantic_test_cases(test_cases)
    }
    
    fn run_semantic_test_cases(test_cases: Vec<SemanticTestCase>) -> Result<TestResult, TestError> {
        for test_case in test_cases {
            let mut compiler = Compiler::new();
            let result = compiler.analyze_semantics(&test_case.source);
            
            match (result, &test_case.expected) {
                (Ok(_), SemanticExpectation::Valid) => {
                    // Test passed
                }
                (Err(errors), SemanticExpectation::Invalid(expected_error)) => {
                    // Check if we got the expected error type
                    if !errors.iter().any(|e| e.to_string().contains(expected_error)) {
                        return Ok(TestResult::Failed(format!(
                            "Expected error containing '{}', but got: {:?}",
                            expected_error, errors
                        )));
                    }
                }
                (Ok(_), SemanticExpectation::Invalid(expected_error)) => {
                    return Ok(TestResult::Failed(format!(
                        "Expected error '{}' but semantic analysis succeeded",
                        expected_error
                    )));
                }
                (Err(errors), SemanticExpectation::Valid) => {
                    return Ok(TestResult::Failed(format!(
                        "Expected valid semantics but got errors: {:?}",
                        errors
                    )));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
}

#[derive(Debug)]
struct SemanticTestCase {
    source: String,
    expected: SemanticExpectation,
}

#[derive(Debug)]
enum SemanticExpectation {
    Valid,
    Invalid(String), // Expected error pattern
}

impl SemanticTestCase {
    fn valid(source: &str) -> Self {
        Self {
            source: source.to_string(),
            expected: SemanticExpectation::Valid,
        }
    }
    
    fn invalid(source: &str, error_pattern: &str) -> Self {
        Self {
            source: source.to_string(),
            expected: SemanticExpectation::Invalid(error_pattern.to_string()),
        }
    }
}
```

## Integration Testing Framework

### 1. End-to-End Compilation Tests (`tests/integration/compilation_tests.rs`)

```rust
use std::path::PathBuf;
use crate::tests::framework::*;

pub struct CompilationTests {
    test_files_dir: PathBuf,
    output_dir: PathBuf,
}

impl CompilationTests {
    pub fn new() -> Self {
        Self {
            test_files_dir: PathBuf::from("tests/clean_files"),
            output_dir: PathBuf::from("tests/wasm"),
        }
    }
    
    pub fn all_tests() -> Vec<TestCase> {
        let mut tests = Vec::new();
        
        // Basic functionality tests
        tests.extend(Self::basic_compilation_tests());
        
        // Advanced feature tests
        tests.extend(Self::advanced_feature_tests());
        
        // Performance tests
        tests.extend(Self::performance_tests());
        
        // Error handling tests
        tests.extend(Self::error_handling_tests());
        
        tests
    }
    
    fn basic_compilation_tests() -> Vec<TestCase> {
        vec![
            compilation_test("hello_world", "Basic hello world program"),
            compilation_test("variables", "Variable declarations and assignments"),
            compilation_test("functions", "Function definitions and calls"),
            compilation_test("conditionals", "If-else statements"),
            compilation_test("loops", "For and while loops"),
            compilation_test("arrays", "Array operations"),
            compilation_test("strings", "String operations"),
            compilation_test("math", "Mathematical operations"),
        ]
    }
    
    fn advanced_feature_tests() -> Vec<TestCase> {
        vec![
            compilation_test("classes", "Class definitions and inheritance"),
            compilation_test("async_functions", "Async/await functionality"),
            compilation_test("generics", "Generic types and functions"),
            compilation_test("error_handling", "Error handling with onError"),
            compilation_test("modules", "Module system"),
        ]
    }
    
    fn run_compilation_test(test_name: &str) -> Result<TestResult, TestError> {
        let input_file = PathBuf::from("tests/clean_files")
            .join(format!("{}.cln", test_name));
        let output_file = PathBuf::from("tests/wasm")
            .join(format!("{}.wasm", test_name));
        
        if !input_file.exists() {
            return Ok(TestResult::Failed(format!(
                "Test file not found: {}", input_file.display()
            )));
        }
        
        // Clean up any existing output
        if output_file.exists() {
            std::fs::remove_file(&output_file).unwrap_or(());
        }
        
        // Compile the Clean Language file
        let mut compiler = Compiler::new();
        match compiler.compile_file(&input_file, &output_file) {
            Ok(()) => {
                // Verify output file was created
                if !output_file.exists() {
                    return Ok(TestResult::Failed(
                        "Compilation succeeded but output file was not created".to_string()
                    ));
                }
                
                // Validate WebAssembly output
                Self::validate_wasm_output(&output_file)
            }
            Err(e) => {
                Ok(TestResult::Failed(format!("Compilation failed: {}", e)))
            }
        }
    }
    
    fn validate_wasm_output(wasm_file: &PathBuf) -> Result<TestResult, TestError> {
        // Read and validate WebAssembly file
        let wasm_bytes = match std::fs::read(wasm_file) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(TestResult::Failed(format!(
                    "Could not read WASM file: {}", e
                )));
            }
        };
        
        // Basic WASM validation
        if !wasm_bytes.starts_with(b"\0asm") {
            return Ok(TestResult::Failed(
                "Invalid WASM magic number".to_string()
            ));
        }
        
        // Check WASM version
        if wasm_bytes.len() < 8 {
            return Ok(TestResult::Failed(
                "WASM file too short".to_string()
            ));
        }
        
        let version = u32::from_le_bytes([
            wasm_bytes[4], wasm_bytes[5], wasm_bytes[6], wasm_bytes[7]
        ]);
        
        if version != 1 {
            return Ok(TestResult::Failed(format!(
                "Unsupported WASM version: {}", version
            )));
        }
        
        // Additional validation using wasmparser (if available)
        #[cfg(feature = "wasmparser")]
        {
            use wasmparser::{Parser, Payload};
            
            let parser = Parser::new(0);
            for payload in parser.parse_all(&wasm_bytes) {
                match payload {
                    Ok(Payload::Version { num, .. }) => {
                        if num != 1 {
                            return Ok(TestResult::Failed(format!(
                                "Invalid WASM version in payload: {}", num
                            )));
                        }
                    }
                    Ok(_) => {}, // Other valid payloads
                    Err(e) => {
                        return Ok(TestResult::Failed(format!(
                            "WASM validation error: {}", e
                        )));
                    }
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
}

fn compilation_test(name: &str, description: &str) -> TestCase {
    TestCase {
        name: format!("compile_{}", name),
        category: TestCategory::Integration,
        test_function: move |_ctx| CompilationTests::run_compilation_test(name),
        setup: None,
        teardown: None,
        timeout: Some(std::time::Duration::from_secs(60)),
        expected_outcome: ExpectedOutcome::Success,
        tags: vec!["compilation".to_string(), "integration".to_string()],
    }
}
```

### 2. WebAssembly Execution Tests (`tests/integration/execution_tests.rs`)

```rust
use wasmtime::*;
use crate::tests::framework::*;

pub struct ExecutionTests {
    engine: Engine,
    store: Store<()>,
}

impl ExecutionTests {
    pub fn new() -> Self {
        let engine = Engine::default();
        let store = Store::new(&engine, ());
        
        Self { engine, store }
    }
    
    pub fn all_tests() -> Vec<TestCase> {
        vec![
            execution_test("hello_world", Self::test_hello_world),
            execution_test("arithmetic", Self::test_arithmetic),
            execution_test("functions", Self::test_functions),
            execution_test("memory", Self::test_memory_operations),
            execution_test("strings", Self::test_string_operations),
            execution_test("async", Self::test_async_execution),
        ]
    }
    
    fn test_hello_world(ctx: &TestContext) -> Result<TestResult, TestError> {
        let wasm_file = "tests/wasm/hello_world.wasm";
        let wasm_bytes = std::fs::read(wasm_file).map_err(|e| {
            TestError::Setup(format!("Could not read WASM file: {}", e))
        })?;
        
        let mut store = Store::new(&Engine::default(), ());
        let module = Module::new(store.engine(), &wasm_bytes).map_err(|e| {
            TestError::Execution(format!("Could not load WASM module: {}", e))
        })?;
        
        // Set up imports (console.log, etc.)
        let mut imports = Vec::new();
        
        // Mock console.log function
        let console_log_type = FuncType::new([ValType::I32, ValType::I32], []);
        let console_log = Func::new(
            &mut store,
            console_log_type,
            |_caller, params, _results| {
                // Extract string from memory and print it
                // This is a simplified implementation
                println!("Hello, World!");
                Ok(())
            }
        );
        imports.push(console_log.into());
        
        let instance = Instance::new(&mut store, &module, &imports).map_err(|e| {
            TestError::Execution(format!("Could not instantiate WASM: {}", e))
        })?;
        
        // Get the start function
        let start_func = instance.get_typed_func::<(), ()>(&mut store, "start").map_err(|e| {
            TestError::Execution(format!("Could not find start function: {}", e))
        })?;
        
        // Execute the function
        start_func.call(&mut store, ()).map_err(|e| {
            TestError::Execution(format!("Execution failed: {}", e))
        })?;
        
        Ok(TestResult::Passed)
    }
    
    fn test_arithmetic(ctx: &TestContext) -> Result<TestResult, TestError> {
        let wasm_file = "tests/wasm/arithmetic.wasm";
        let test_cases = vec![
            (5, 3, 8),   // 5 + 3 = 8
            (10, 4, 14), // 10 + 4 = 14
            (0, 0, 0),   // 0 + 0 = 0
            (-5, 3, -2), // -5 + 3 = -2
        ];
        
        let result = Self::run_wasm_function_tests(wasm_file, "add", test_cases)?;
        Ok(result)
    }
    
    fn test_functions(ctx: &TestContext) -> Result<TestResult, TestError> {
        let wasm_file = "tests/wasm/functions.wasm";
        let wasm_bytes = std::fs::read(wasm_file).map_err(|e| {
            TestError::Setup(format!("Could not read WASM file: {}", e))
        })?;
        
        let mut store = Store::new(&Engine::default(), ());
        let module = Module::new(store.engine(), &wasm_bytes).map_err(|e| {
            TestError::Execution(format!("Could not load WASM module: {}", e))
        })?;
        
        let instance = Instance::new(&mut store, &module, &[]).map_err(|e| {
            TestError::Execution(format!("Could not instantiate WASM: {}", e))
        })?;
        
        // Test multiple functions
        let test_functions = vec![
            ("factorial", vec![5], Some(120)),
            ("fibonacci", vec![10], Some(55)),
            ("is_prime", vec![17], Some(1)), // 1 for true
            ("is_prime", vec![18], Some(0)), // 0 for false
        ];
        
        for (func_name, args, expected_result) in test_functions {
            let func = instance.get_typed_func::<i32, i32>(&mut store, func_name).map_err(|e| {
                return TestError::Execution(format!("Could not find function '{}': {}", func_name, e));
            })?;
            
            let result = func.call(&mut store, args[0]).map_err(|e| {
                return TestError::Execution(format!("Function '{}' execution failed: {}", func_name, e));
            })?;
            
            if let Some(expected) = expected_result {
                if result != expected {
                    return Ok(TestResult::Failed(format!(
                        "Function '{}' returned {}, expected {}", func_name, result, expected
                    )));
                }
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_memory_operations(ctx: &TestContext) -> Result<TestResult, TestError> {
        let wasm_file = "tests/wasm/memory.wasm";
        let wasm_bytes = std::fs::read(wasm_file).map_err(|e| {
            TestError::Setup(format!("Could not read WASM file: {}", e))
        })?;
        
        let mut store = Store::new(&Engine::default(), ());
        let module = Module::new(store.engine(), &wasm_bytes).map_err(|e| {
            TestError::Execution(format!("Could not load WASM module: {}", e))
        })?;
        
        let instance = Instance::new(&mut store, &module, &[]).map_err(|e| {
            TestError::Execution(format!("Could not instantiate WASM: {}", e))
        })?;
        
        // Test memory allocation and deallocation
        let alloc_func = instance.get_typed_func::<i32, i32>(&mut store, "allocate").map_err(|e| {
            TestError::Execution(format!("Could not find allocate function: {}", e))
        })?;
        
        let free_func = instance.get_typed_func::<i32, ()>(&mut store, "deallocate").map_err(|e| {
            TestError::Execution(format!("Could not find deallocate function: {}", e))
        })?;
        
        // Allocate memory
        let ptr = alloc_func.call(&mut store, 1024).map_err(|e| {
            TestError::Execution(format!("Memory allocation failed: {}", e))
        })?;
        
        if ptr == 0 {
            return Ok(TestResult::Failed("Memory allocation returned null pointer".to_string()));
        }
        
        // Deallocate memory
        free_func.call(&mut store, ptr).map_err(|e| {
            TestError::Execution(format!("Memory deallocation failed: {}", e))
        })?;
        
        Ok(TestResult::Passed)
    }
    
    fn run_wasm_function_tests(
        wasm_file: &str,
        function_name: &str,
        test_cases: Vec<(i32, i32, i32)>
    ) -> Result<TestResult, TestError> {
        let wasm_bytes = std::fs::read(wasm_file).map_err(|e| {
            TestError::Setup(format!("Could not read WASM file: {}", e))
        })?;
        
        let mut store = Store::new(&Engine::default(), ());
        let module = Module::new(store.engine(), &wasm_bytes).map_err(|e| {
            TestError::Execution(format!("Could not load WASM module: {}", e))
        })?;
        
        let instance = Instance::new(&mut store, &module, &[]).map_err(|e| {
            TestError::Execution(format!("Could not instantiate WASM: {}", e))
        })?;
        
        let func = instance.get_typed_func::<(i32, i32), i32>(&mut store, function_name).map_err(|e| {
            TestError::Execution(format!("Could not find function '{}': {}", function_name, e))
        })?;
        
        for (a, b, expected) in test_cases {
            let result = func.call(&mut store, (a, b)).map_err(|e| {
                TestError::Execution(format!("Function call failed for ({}, {}): {}", a, b, e))
            })?;
            
            if result != expected {
                return Ok(TestResult::Failed(format!(
                    "Function '{}({}, {})' returned {}, expected {}", 
                    function_name, a, b, result, expected
                )));
            }
        }
        
        Ok(TestResult::Passed)
    }
}

fn execution_test(name: &str, test_fn: TestFunction) -> TestCase {
    TestCase {
        name: format!("execute_{}", name),
        category: TestCategory::Integration,
        test_function: test_fn,
        setup: None,
        teardown: None,
        timeout: Some(std::time::Duration::from_secs(30)),
        expected_outcome: ExpectedOutcome::Success,
        tags: vec!["execution".to_string(), "wasm".to_string()],
    }
}
```

## Performance Testing Framework

### 1. Compilation Performance Tests (`tests/performance/compilation_perf.rs`)

```rust
use std::time::{Duration, Instant};
use crate::tests::framework::*;

pub struct CompilationPerformanceTests;

impl CompilationPerformanceTests {
    pub fn all_tests() -> Vec<TestCase> {
        vec![
            perf_test("small_files", Self::test_small_file_compilation, 
                     PerformanceCriteria {
                         max_compilation_time: Duration::from_millis(100),
                         max_memory_usage: 10 * 1024 * 1024, // 10MB
                         max_output_size: 1024 * 1024, // 1MB
                     }),
            perf_test("medium_files", Self::test_medium_file_compilation,
                     PerformanceCriteria {
                         max_compilation_time: Duration::from_secs(1),
                         max_memory_usage: 50 * 1024 * 1024, // 50MB
                         max_output_size: 5 * 1024 * 1024, // 5MB
                     }),
            perf_test("large_files", Self::test_large_file_compilation,
                     PerformanceCriteria {
                         max_compilation_time: Duration::from_secs(10),
                         max_memory_usage: 200 * 1024 * 1024, // 200MB
                         max_output_size: 20 * 1024 * 1024, // 20MB
                     }),
            perf_test("optimization_levels", Self::test_optimization_performance,
                     PerformanceCriteria {
                         max_compilation_time: Duration::from_secs(30),
                         max_memory_usage: 500 * 1024 * 1024, // 500MB
                         max_output_size: 10 * 1024 * 1024, // 10MB
                     }),
        ]
    }
    
    fn test_small_file_compilation(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_files = vec![
            "tests/clean_files/basic/hello_world.cln",
            "tests/clean_files/basic/variables.cln",
            "tests/clean_files/basic/functions.cln",
        ];
        
        for test_file in test_files {
            let result = Self::measure_compilation_performance(test_file)?;
            
            if result.compilation_time > Duration::from_millis(100) {
                return Ok(TestResult::Failed(format!(
                    "Small file compilation too slow: {:?} for {}", 
                    result.compilation_time, test_file
                )));
            }
            
            if result.memory_usage > 10 * 1024 * 1024 {
                return Ok(TestResult::Failed(format!(
                    "Small file compilation uses too much memory: {} bytes for {}",
                    result.memory_usage, test_file
                )));
            }
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_large_file_compilation(ctx: &TestContext) -> Result<TestResult, TestError> {
        // Generate a large test file
        let large_file_content = Self::generate_large_program(10000); // 10k functions
        let temp_file = "/tmp/large_test.cln";
        std::fs::write(temp_file, large_file_content).map_err(|e| {
            TestError::Setup(format!("Could not write large test file: {}", e))
        })?;
        
        let result = Self::measure_compilation_performance(temp_file)?;
        
        // Clean up
        std::fs::remove_file(temp_file).unwrap_or(());
        
        if result.compilation_time > Duration::from_secs(10) {
            return Ok(TestResult::Failed(format!(
                "Large file compilation too slow: {:?}", result.compilation_time
            )));
        }
        
        if result.memory_usage > 200 * 1024 * 1024 {
            return Ok(TestResult::Failed(format!(
                "Large file compilation uses too much memory: {} bytes",
                result.memory_usage
            )));
        }
        
        Ok(TestResult::Passed)
    }
    
    fn test_optimization_performance(ctx: &TestContext) -> Result<TestResult, TestError> {
        let test_file = "tests/clean_files/performance/optimization_test.cln";
        let optimization_levels = vec![
            OptimizationLevel::None,
            OptimizationLevel::Basic,
            OptimizationLevel::Default,
            OptimizationLevel::Aggressive,
        ];
        
        let mut results = Vec::new();
        
        for opt_level in optimization_levels {
            let mut compiler = Compiler::new();
            compiler.set_optimization_level(opt_level.clone());
            
            let start_time = Instant::now();
            let result = compiler.compile_file(test_file, "/tmp/opt_test.wasm");
            let compilation_time = start_time.elapsed();
            
            match result {
                Ok(()) => {
                    let output_size = std::fs::metadata("/tmp/opt_test.wasm")
                        .map(|m| m.len())
                        .unwrap_or(0);
                    
                    results.push((opt_level, compilation_time, output_size));
                }
                Err(e) => {
                    return Ok(TestResult::Failed(format!(
                        "Compilation failed at optimization level {:?}: {}", opt_level, e
                    )));
                }
            }
        }
        
        // Verify that higher optimization levels produce smaller output
        let none_size = results.iter().find(|(level, _, _)| matches!(level, OptimizationLevel::None))
            .map(|(_, _, size)| *size).unwrap_or(0);
        let aggressive_size = results.iter().find(|(level, _, _)| matches!(level, OptimizationLevel::Aggressive))
            .map(|(_, _, size)| *size).unwrap_or(0);
        
        if aggressive_size >= none_size {
            return Ok(TestResult::Failed(format!(
                "Aggressive optimization should produce smaller output: {} vs {} bytes",
                aggressive_size, none_size
            )));
        }
        
        Ok(TestResult::Passed)
    }
    
    fn measure_compilation_performance(file_path: &str) -> Result<CompilationMetrics, TestError> {
        let mut compiler = Compiler::new();
        
        let start_memory = Self::get_memory_usage();
        let start_time = Instant::now();
        
        let result = compiler.compile_file(file_path, "/tmp/perf_test.wasm");
        
        let compilation_time = start_time.elapsed();
        let end_memory = Self::get_memory_usage();
        
        match result {
            Ok(()) => {
                let output_size = std::fs::metadata("/tmp/perf_test.wasm")
                    .map(|m| m.len() as usize)
                    .unwrap_or(0);
                
                Ok(CompilationMetrics {
                    compilation_time,
                    memory_usage: end_memory.saturating_sub(start_memory),
                    output_size,
                })
            }
            Err(e) => {
                Err(TestError::Execution(format!("Compilation failed: {}", e)))
            }
        }
    }
    
    fn generate_large_program(function_count: usize) -> String {
        let mut content = String::new();
        
        for i in 0..function_count {
            content.push_str(&format!(r#"
function func{}(x: integer) -> integer {{
    let result = x * {} + {};
    if result > 1000 {{
        return result % 1000;
    }} else {{
        return result;
    }}
}}
"#, i, i + 1, i * 2));
        }
        
        content.push_str(r#"
start() {
    let sum = 0;
    for i in 0..100 {
        sum = sum + func0(i);
    }
    println sum.toString();
}
"#);
        
        content
    }
    
    fn get_memory_usage() -> usize {
        // Platform-specific memory usage measurement
        #[cfg(target_os = "linux")]
        {
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<usize>() {
                            return kb * 1024; // Convert to bytes
                        }
                    }
                }
            }
        }
        
        // Fallback: return 0 if we can't measure memory
        0
    }
}

#[derive(Debug)]
struct CompilationMetrics {
    compilation_time: Duration,
    memory_usage: usize,
    output_size: usize,
}

fn perf_test(name: &str, test_fn: TestFunction, criteria: PerformanceCriteria) -> TestCase {
    TestCase {
        name: format!("perf_{}", name),
        category: TestCategory::Performance,
        test_function: test_fn,
        setup: None,
        teardown: None,
        timeout: Some(Duration::from_secs(300)), // 5 minutes for performance tests
        expected_outcome: ExpectedOutcome::Performance(criteria),
        tags: vec!["performance".to_string()],
    }
}
```

## Test Runner Implementation

### 1. Parallel Test Execution (`tests/framework/runner.rs`)

```rust
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use crate::tests::framework::*;

pub struct TestRunner {
    config: TestConfig,
    thread_pool: ThreadPool,
}

impl TestRunner {
    pub fn new(config: TestConfig) -> Self {
        let thread_count = if config.parallel_execution {
            num_cpus::get()
        } else {
            1
        };
        
        Self {
            config,
            thread_pool: ThreadPool::new(thread_count),
        }
    }
    
    pub fn run_test_suite(&mut self, suite_name: &str, test_cases: Vec<TestCase>) -> TestResults {
        println!("Running test suite: {}", suite_name);
        
        let total_tests = test_cases.len();
        let results = Arc::new(Mutex::new(TestResults::new()));
        let start_time = Instant::now();
        
        // Filter tests based on pattern
        let filtered_tests: Vec<TestCase> = if let Some(ref pattern) = self.config.filter_pattern {
            test_cases.into_iter()
                .filter(|test| pattern.is_match(&test.name))
                .collect()
        } else {
            test_cases
        };
        
        println!("Running {} of {} tests", filtered_tests.len(), total_tests);
        
        if self.config.parallel_execution {
            self.run_tests_parallel(filtered_tests, results.clone());
        } else {
            self.run_tests_sequential(filtered_tests, results.clone());
        }
        
        let mut final_results = results.lock().unwrap().clone();
        final_results.total_time = start_time.elapsed();
        
        // Print summary
        self.print_test_summary(&final_results);
        
        final_results
    }
    
    fn run_tests_parallel(&mut self, test_cases: Vec<TestCase>, results: Arc<Mutex<TestResults>>) {
        let remaining_tests = Arc::new(Mutex::new(test_cases));
        let mut handles = Vec::new();
        
        for _ in 0..self.thread_pool.size() {
            let remaining = Arc::clone(&remaining_tests);
            let results_ref = Arc::clone(&results);
            let config = self.config.clone();
            
            let handle = thread::spawn(move || {
                loop {
                    let test_case = {
                        let mut tests = remaining.lock().unwrap();
                        tests.pop()
                    };
                    
                    match test_case {
                        Some(test) => {
                            let result = Self::run_single_test(test, &config);
                            
                            {
                                let mut results = results_ref.lock().unwrap();
                                results.add_result(result);
                                
                                if config.verbose_output {
                                    Self::print_test_result(&result);
                                }
                                
                                if config.fail_fast && result.status == TestStatus::Failed {
                                    break;
                                }
                            }
                        }
                        None => break, // No more tests
                    }
                }
            });
            
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }
    }
    
    fn run_tests_sequential(&self, test_cases: Vec<TestCase>, results: Arc<Mutex<TestResults>>) {
        for test_case in test_cases {
            let result = Self::run_single_test(test_case, &self.config);
            
            {
                let mut results = results.lock().unwrap();
                results.add_result(result.clone());
                
                if self.config.verbose_output {
                    Self::print_test_result(&result);
                }
                
                if self.config.fail_fast && result.status == TestStatus::Failed {
                    break;
                }
            }
        }
    }
    
    fn run_single_test(test_case: TestCase, config: &TestConfig) -> TestResult {
        let start_time = Instant::now();
        
        // Setup
        let test_context = if let Some(setup) = &test_case.setup {
            match setup() {
                Ok(ctx) => ctx,
                Err(e) => {
                    return TestResult {
                        name: test_case.name,
                        status: TestStatus::Failed,
                        message: Some(format!("Setup failed: {}", e)),
                        duration: start_time.elapsed(),
                        category: test_case.category,
                    };
                }
            }
        } else {
            TestContext::default()
        };
        
        // Run test with timeout
        let test_result = if let Some(timeout) = test_case.timeout {
            Self::run_with_timeout(test_case.test_function, &test_context, timeout)
        } else {
            (test_case.test_function)(&test_context)
        };
        
        // Teardown
        if let Some(teardown) = &test_case.teardown {
            if let Err(e) = teardown(&test_context) {
                eprintln!("Warning: Teardown failed for {}: {}", test_case.name, e);
            }
        }
        
        let duration = start_time.elapsed();
        
        match test_result {
            Ok(TestResult::Passed) => {
                TestResult {
                    name: test_case.name,
                    status: TestStatus::Passed,
                    message: None,
                    duration,
                    category: test_case.category,
                }
            }
            Ok(TestResult::Failed(message)) => {
                TestResult {
                    name: test_case.name,
                    status: TestStatus::Failed,
                    message: Some(message),
                    duration,
                    category: test_case.category,
                }
            }
            Err(TestError::Timeout) => {
                TestResult {
                    name: test_case.name,
                    status: TestStatus::Timeout,
                    message: Some("Test timed out".to_string()),
                    duration,
                    category: test_case.category,
                }
            }
            Err(e) => {
                TestResult {
                    name: test_case.name,
                    status: TestStatus::Failed,
                    message: Some(format!("Test error: {}", e)),
                    duration,
                    category: test_case.category,
                }
            }
        }
    }
    
    fn run_with_timeout(
        test_fn: TestFunction,
        context: &TestContext,
        timeout: Duration
    ) -> Result<crate::tests::framework::TestResult, TestError> {
        let (sender, receiver) = std::sync::mpsc::channel();
        
        let context_clone = context.clone();
        thread::spawn(move || {
            let result = test_fn(&context_clone);
            let _ = sender.send(result);
        });
        
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(TestError::Timeout),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(TestError::Execution("Test thread panicked".to_string()))
            }
        }
    }
    
    fn print_test_result(result: &TestResult) {
        let status_symbol = match result.status {
            TestStatus::Passed => "✓",
            TestStatus::Failed => "✗",
            TestStatus::Timeout => "⏰",
        };
        
        let status_color = match result.status {
            TestStatus::Passed => "\x1b[32m", // Green
            TestStatus::Failed => "\x1b[31m", // Red
            TestStatus::Timeout => "\x1b[33m", // Yellow
        };
        
        print!("{}{} {}\x1b[0m", status_color, status_symbol, result.name);
        
        if let Some(ref message) = result.message {
            print!(" - {}", message);
        }
        
        println!(" ({:.2}ms)", result.duration.as_millis());
    }
    
    fn print_test_summary(&self, results: &TestResults) {
        println!("\n{}", "=".repeat(50));
        println!("Test Summary");
        println!("{}", "=".repeat(50));
        println!("Total tests: {}", results.total_tests());
        println!("Passed: \x1b[32m{}\x1b[0m", results.passed_count());
        println!("Failed: \x1b[31m{}\x1b[0m", results.failed_count());
        println!("Timeouts: \x1b[33m{}\x1b[0m", results.timeout_count());
        println!("Total time: {:.2}s", results.total_time.as_secs_f64());
        
        if results.failed_count() > 0 {
            println!("\nFailed tests:");
            for result in &results.results {
                if result.status == TestStatus::Failed {
                    println!("  - {}: {}", result.name, 
                           result.message.as_deref().unwrap_or("Unknown failure"));
                }
            }
        }
    }
}

struct ThreadPool {
    size: usize,
}

impl ThreadPool {
    fn new(size: usize) -> Self {
        Self { size }
    }
    
    fn size(&self) -> usize {
        self.size
    }
}

#[derive(Debug, Clone)]
pub struct TestResults {
    pub results: Vec<TestResult>,
    pub total_time: Duration,
}

impl TestResults {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            total_time: Duration::default(),
        }
    }
    
    pub fn add_result(&mut self, result: TestResult) {
        self.results.push(result);
    }
    
    pub fn merge(&mut self, other: TestResults) {
        self.results.extend(other.results);
        self.total_time += other.total_time;
    }
    
    pub fn total_tests(&self) -> usize {
        self.results.len()
    }
    
    pub fn passed_count(&self) -> usize {
        self.results.iter().filter(|r| r.status == TestStatus::Passed).count()
    }
    
    pub fn failed_count(&self) -> usize {
        self.results.iter().filter(|r| r.status == TestStatus::Failed).count()
    }
    
    pub fn timeout_count(&self) -> usize {
        self.results.iter().filter(|r| r.status == TestStatus::Timeout).count()
    }
    
    pub fn success_rate(&self) -> f64 {
        if self.results.is_empty() {
            0.0
        } else {
            self.passed_count() as f64 / self.total_tests() as f64
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub status: TestStatus,
    pub message: Option<String>,
    pub duration: Duration,
    pub category: TestCategory,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestStatus {
    Passed,
    Failed,
    Timeout,
}

#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error("Test setup failed: {0}")]
    Setup(String),
    
    #[error("Test execution failed: {0}")]
    Execution(String),
    
    #[error("Test timed out")]
    Timeout,
    
    #[error("Test framework error: {0}")]
    Framework(String),
}

#[derive(Debug, Clone, Default)]
pub struct TestContext {
    pub temp_dir: Option<std::path::PathBuf>,
    pub compiler: Option<Compiler>,
    pub test_data: std::collections::HashMap<String, String>,
}
```

## Best Practices for Claude

When working with Clean Language testing:

1. **Comprehensive Coverage**: Test all compiler phases and edge cases
2. **Fast Feedback**: Prioritize fast-running tests for development workflow
3. **Parallel Execution**: Leverage parallel testing for performance
4. **Error Testing**: Specifically test error conditions and recovery
5. **Performance Monitoring**: Track compilation and execution performance
6. **Regression Testing**: Maintain tests for previously fixed bugs
7. **Cross-Platform Testing**: Test on different target platforms
8. **Integration Testing**: Test the complete compilation pipeline

This testing documentation provides the foundation for maintaining high code quality and reliability in the Clean Language compiler.