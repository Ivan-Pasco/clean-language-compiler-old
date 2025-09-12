use crate::error::CompilerError;
use crate::testing::{TestSuite, TestCaseBuilder, CompilationExpectation, ErrorExpectation, ErrorCategory};

/// Unit tests for individual compiler components
pub struct UnitTests;

impl UnitTests {
    /// Create unit test suite for parser
    pub fn create_parser_suite() -> Result<TestSuite, CompilerError> {
        let mut suite = TestSuite::new("parser_unit_tests", "Unit tests for the parser component");

        let test_cases = vec![
            Self::parse_variable_declaration()?,
            Self::parse_function_declaration()?,
            Self::parse_class_declaration()?,
            Self::parse_invalid_syntax()?,
        ];

        for test_case in test_cases {
            suite.add_test(test_case);
        }

        Ok(suite)
    }

    /// Create unit test suite for semantic analyzer
    pub fn create_semantic_suite() -> Result<TestSuite, CompilerError> {
        let mut suite = TestSuite::new("semantic_unit_tests", "Unit tests for semantic analysis");

        let test_cases = vec![
            Self::semantic_type_checking()?,
            Self::semantic_variable_scoping()?,
            Self::semantic_function_resolution()?,
            Self::semantic_class_inheritance()?,
        ];

        for test_case in test_cases {
            suite.add_test(test_case);
        }

        Ok(suite)
    }

    /// Create unit test suite for code generator
    pub fn create_codegen_suite() -> Result<TestSuite, CompilerError> {
        let mut suite = TestSuite::new("codegen_unit_tests", "Unit tests for code generation");

        let test_cases = vec![
            Self::codegen_arithmetic_operations()?,
            Self::codegen_function_calls()?,
            Self::codegen_control_flow()?,
            Self::codegen_memory_management()?,
        ];

        for test_case in test_cases {
            suite.add_test(test_case);
        }

        Ok(suite)
    }

    // Parser unit tests
    fn parse_variable_declaration() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("parse_variable_declaration")
            .description("Test parsing variable declarations")
            .compilation_test(
                "let x: integer = 42;",
                CompilationExpectation::Success
            )
            .tag("unit")
            .tag("parser")
            .tag("variables")
            .build()
    }

    fn parse_function_declaration() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("parse_function_declaration")
            .description("Test parsing function declarations")
            .compilation_test(
                r#"
                function add(a: integer, b: integer) -> integer {
                    return a + b;
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("unit")
            .tag("parser")
            .tag("functions")
            .build()
    }

    fn parse_class_declaration() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("parse_class_declaration")
            .description("Test parsing class declarations")
            .compilation_test(
                r#"
                class Person {
                    constructor(name: string, age: integer) {
                        this.name = name;
                        this.age = age;
                    }

                    greet() -> string {
                        return "Hello, " + this.name;
                    }
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("unit")
            .tag("parser")
            .tag("classes")
            .build()
    }

    fn parse_invalid_syntax() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("parse_invalid_syntax")
            .description("Test parser error handling for invalid syntax")
            .error_test(
                "let x = ;",
                ErrorExpectation::ErrorCategory(ErrorCategory::Syntax)
            )
            .tag("unit")
            .tag("parser")
            .tag("error-handling")
            .build()
    }

    // Semantic analyzer unit tests
    fn semantic_type_checking() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("semantic_type_checking")
            .description("Test type mismatch detection")
            .error_test(
                r#"
                function test() {
                    let x: integer = "not a number";
                }
                "#,
                ErrorExpectation::ErrorCategory(ErrorCategory::Type)
            )
            .tag("unit")
            .tag("semantic")
            .tag("types")
            .build()
    }

    fn semantic_variable_scoping() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("semantic_variable_scoping")
            .description("Test variable scoping rules")
            .error_test(
                r#"
                function outer() {
                    let x = 10;
                    function inner() {
                        return y; // undefined variable
                    }
                }
                "#,
                ErrorExpectation::ErrorCategory(ErrorCategory::Semantic)
            )
            .tag("unit")
            .tag("semantic")
            .tag("scoping")
            .build()
    }

    fn semantic_function_resolution() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("semantic_function_resolution")
            .description("Test function call resolution")
            .compilation_test(
                r#"
                function helper() -> integer {
                    return 42;
                }

                function main() {
                    let result = helper();
                    return result;
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("unit")
            .tag("semantic")
            .tag("functions")
            .build()
    }

    fn semantic_class_inheritance() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("semantic_class_inheritance")
            .description("Test class inheritance validation")
            .compilation_test(
                r#"
                class Base {
                    constructor(value: integer) {
                        this.value = value;
                    }
                }

                class Derived extends Base {
                    constructor(value: integer, extra: string) {
                        base(value);
                        this.extra = extra;
                    }
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("unit")
            .tag("semantic")
            .tag("inheritance")
            .build()
    }

    // Code generator unit tests
    fn codegen_arithmetic_operations() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("codegen_arithmetic_operations")
            .description("Test arithmetic operation code generation")
            .execution_test(
                r#"
                function main() {
                    let a = 10;
                    let b = 5;
                    print(a + b);
                    print(a - b);
                    print(a * b);
                    print(a / b);
                }
                "#,
                vec![],
                "15\n5\n50\n2"
            )
            .tag("unit")
            .tag("codegen")
            .tag("arithmetic")
            .build()
    }

    fn codegen_function_calls() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("codegen_function_calls")
            .description("Test function call code generation")
            .execution_test(
                r#"
                function square(x: integer) -> integer {
                    return x * x;
                }

                function main() {
                    print(square(5));
                    print(square(10));
                }
                "#,
                vec![],
                "25\n100"
            )
            .tag("unit")
            .tag("codegen")
            .tag("functions")
            .build()
    }

    fn codegen_control_flow() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("codegen_control_flow")
            .description("Test control flow code generation")
            .execution_test(
                r#"
                function main() {
                    for (let i = 1; i <= 3; i++) {
                        if (i % 2 == 0) {
                            print("even: " + i);
                        } else {
                            print("odd: " + i);
                        }
                    }
                }
                "#,
                vec![],
                "odd: 1\neven: 2\nodd: 3"
            )
            .tag("unit")
            .tag("codegen")
            .tag("control-flow")
            .build()
    }

    fn codegen_memory_management() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("codegen_memory_management")
            .description("Test memory management code generation")
            .compilation_test(
                r#"
                function main() {
                    let arr = [1, 2, 3, 4, 5];
                    let obj = new Person("Alice");
                    // Memory should be properly managed
                }
                
                class Person {
                    constructor(name: string) {
                        this.name = name;
                    }
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("unit")
            .tag("codegen")
            .tag("memory")
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_suite_creation() {
        let suite = UnitTests::create_parser_suite().unwrap();
        assert_eq!(suite.name, "parser_unit_tests");
        assert!(suite.test_count() > 0);
    }

    #[test]
    fn test_semantic_suite_creation() {
        let suite = UnitTests::create_semantic_suite().unwrap();
        assert_eq!(suite.name, "semantic_unit_tests");
        assert!(suite.test_count() > 0);
    }

    #[test]
    fn test_codegen_suite_creation() {
        let suite = UnitTests::create_codegen_suite().unwrap();
        assert_eq!(suite.name, "codegen_unit_tests");
        assert!(suite.test_count() > 0);
    }
}