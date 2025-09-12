use crate::error::CompilerError;
use crate::testing::{TestSuite, TestCaseBuilder, CompilationExpectation};

/// Integration tests for end-to-end compiler functionality
pub struct IntegrationTests;

impl IntegrationTests {
    /// Create integration test suite
    pub fn create_suite() -> Result<TestSuite, CompilerError> {
        let mut suite = TestSuite::new("integration_tests", "End-to-end integration tests");

        // Add comprehensive integration tests
        let test_cases = vec![
            Self::hello_world_test()?,
            Self::fibonacci_test()?,
            Self::class_inheritance_test()?,
            Self::async_operations_test()?,
            Self::error_handling_test()?,
        ];

        for test_case in test_cases {
            suite.add_test(test_case);
        }

        Ok(suite)
    }

    fn hello_world_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("integration_hello_world")
            .description("Basic hello world end-to-end test")
            .execution_test(
                r#"
                function main() {
                    print("Hello, Clean Language!");
                }
                "#,
                vec![],
                "Hello, Clean Language!"
            )
            .tag("integration")
            .tag("basic")
            .build()
    }

    fn fibonacci_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("integration_fibonacci")
            .description("Recursive fibonacci implementation test")
            .execution_test(
                r#"
                function fibonacci(n: integer) -> integer {
                    if (n <= 1) {
                        return n;
                    }
                    return fibonacci(n - 1) + fibonacci(n - 2);
                }

                function main() {
                    print(fibonacci(10));
                }
                "#,
                vec![],
                "55"
            )
            .tag("integration")
            .tag("recursion")
            .build()
    }

    fn class_inheritance_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("integration_class_inheritance")
            .description("Class inheritance and method override test")
            .execution_test(
                r#"
                class Animal {
                    constructor(name: string) {
                        this.name = name;
                    }

                    speak() -> string {
                        return "Some generic animal sound";
                    }
                }

                class Dog extends Animal {
                    constructor(name: string) {
                        base(name);
                    }

                    speak() -> string {
                        return "Woof!";
                    }
                }

                function main() {
                    let dog = new Dog("Buddy");
                    print(dog.speak());
                }
                "#,
                vec![],
                "Woof!"
            )
            .tag("integration")
            .tag("classes")
            .tag("inheritance")
            .build()
    }

    fn async_operations_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("integration_async_operations")
            .description("Async/await operations test")
            .compilation_test(
                r#"
                async function fetchData() -> string {
                    return "async data";
                }

                async function main() {
                    let data = await fetchData();
                    print(data);
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("integration")
            .tag("async")
            .build()
    }

    fn error_handling_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("integration_error_handling")
            .description("Error handling with try/catch test")
            .compilation_test(
                r#"
                function riskyOperation() -> integer {
                    throw new Error("Something went wrong");
                }

                function main() {
                    try {
                        let result = riskyOperation();
                        print(result);
                    } catch (error) {
                        print("Caught error: " + error.message);
                    }
                }
                "#,
                CompilationExpectation::Success
            )
            .tag("integration")
            .tag("error-handling")
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_suite_creation() {
        let suite = IntegrationTests::create_suite().unwrap();
        assert_eq!(suite.name, "integration_tests");
        assert!(suite.test_count() > 0);
    }
}