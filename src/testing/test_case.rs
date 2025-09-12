use crate::error::CompilerError;
use crate::testing::TestOutput;
use std::time::Duration;
use std::collections::HashMap;

/// Individual test case for the Clean Language compiler
/// 
/// Represents a single test with specific expectations and configuration.
/// Can test compilation, execution, performance, or error conditions.
#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub description: String,
    pub test_type: TestType,
    pub expectations: Vec<TestExpectation>,
    pub timeout: Option<Duration>,
    pub tags: Vec<String>,
    pub setup: Option<TestSetup>,
    pub cleanup: Option<TestCleanup>,
    pub skip_condition: Option<SkipCondition>,
    pub retry_count: usize,
}

/// Types of tests that can be executed
#[derive(Debug, Clone)]
pub enum TestType {
    /// Test compilation only
    Compilation {
        source: String,
        expected_result: CompilationExpectation,
    },
    /// Test compilation and execution
    Execution {
        source: String,
        inputs: Vec<String>,
        expected_output: String,
    },
    /// Test performance characteristics
    Performance {
        source: String,
        performance_thresholds: PerformanceThresholds,
    },
    /// Test error conditions
    ErrorCondition {
        source: String,
        expected_error: ErrorExpectation,
    },
    /// Test for regression detection
    Regression {
        source: String,
        baseline_metrics: BaselineMetrics,
    },
}

/// Expectations for test results
#[derive(Debug, Clone)]
pub enum TestExpectation {
    /// Compilation should succeed
    CompilationSuccess,
    /// Compilation should fail
    CompilationFailure,
    /// Execution should produce specific output
    OutputEquals(String),
    /// Execution should contain specific text
    OutputContains(String),
    /// Execution should match regex pattern
    OutputMatches(String),
    /// Should complete within time limit
    CompletesWithin(Duration),
    /// Should use less than specified memory
    MemoryUsageLessThan(usize),
    /// Binary size should be less than threshold
    BinarySizeLessThan(usize),
    /// Custom expectation with validation function
    Custom(Box<dyn Fn(&TestOutput) -> Result<bool, CompilerError> + Send + Sync>),
}

/// Compilation expectations
#[derive(Debug, Clone)]
pub enum CompilationExpectation {
    /// Compilation should succeed
    Success,
    /// Compilation should fail
    Failure,
    /// Should produce specific number of warnings
    WarningCount(usize),
    /// Should produce specific number of errors
    ErrorCount(usize),
}

/// Error expectations for testing error conditions
#[derive(Debug, Clone)]
pub enum ErrorExpectation {
    /// Should fail to compile
    CompilationFailure,
    /// Should produce error containing specific text
    SpecificError(String),
    /// Should produce error of specific category
    ErrorCategory(ErrorCategory),
}

/// Error categories for testing
#[derive(Debug, Clone)]
pub enum ErrorCategory {
    Syntax,
    Semantic,
    Type,
    Runtime,
    Internal,
}

/// Performance thresholds for benchmarking
#[derive(Debug, Clone)]
pub struct PerformanceThresholds {
    pub max_compilation_time: Option<Duration>,
    pub max_execution_time: Option<Duration>,
    pub max_memory_mb: Option<usize>,
    pub max_binary_size: Option<usize>,
    pub min_throughput_ops_per_sec: Option<f64>,
}

/// Baseline metrics for regression testing
#[derive(Debug, Clone)]
pub struct BaselineMetrics {
    pub compilation_time_ms: u64,
    pub execution_time_ms: u64,
    pub memory_usage_mb: usize,
    pub binary_size: usize,
    pub allowed_regression_factor: f64, // e.g., 1.1 for 10% regression tolerance
}

/// Test setup configuration
#[derive(Debug, Clone)]
pub struct TestSetup {
    pub environment_variables: HashMap<String, String>,
    pub working_directory: Option<String>,
    pub required_files: Vec<String>,
    pub setup_commands: Vec<String>,
}

/// Test cleanup configuration
#[derive(Debug, Clone)]
pub struct TestCleanup {
    pub files_to_remove: Vec<String>,
    pub cleanup_commands: Vec<String>,
}

/// Conditions for skipping tests
#[derive(Debug, Clone)]
pub enum SkipCondition {
    /// Skip if environment variable is set
    EnvironmentVariable(String),
    /// Skip if platform doesn't match
    Platform(String),
    /// Skip if feature is not enabled
    Feature(String),
    /// Custom skip condition
    Custom(Box<dyn Fn() -> bool + Send + Sync>),
}

/// Builder for creating test cases
pub struct TestCaseBuilder {
    name: String,
    description: String,
    test_type: Option<TestType>,
    expectations: Vec<TestExpectation>,
    timeout: Option<Duration>,
    tags: Vec<String>,
    setup: Option<TestSetup>,
    cleanup: Option<TestCleanup>,
    skip_condition: Option<SkipCondition>,
    retry_count: usize,
}

impl TestCaseBuilder {
    /// Create a new test case builder
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            test_type: None,
            expectations: Vec::new(),
            timeout: None,
            tags: Vec::new(),
            setup: None,
            cleanup: None,
            skip_condition: None,
            retry_count: 0,
        }
    }

    /// Set test description
    pub fn description(mut self, description: &str) -> Self {
        self.description = description.to_string();
        self
    }

    /// Set test type to compilation test
    pub fn compilation_test(mut self, source: &str, expected: CompilationExpectation) -> Self {
        self.test_type = Some(TestType::Compilation {
            source: source.to_string(),
            expected_result: expected,
        });
        self
    }

    /// Set test type to execution test
    pub fn execution_test(mut self, source: &str, inputs: Vec<String>, expected_output: &str) -> Self {
        self.test_type = Some(TestType::Execution {
            source: source.to_string(),
            inputs,
            expected_output: expected_output.to_string(),
        });
        self
    }

    /// Set test type to performance test
    pub fn performance_test(mut self, source: &str, thresholds: PerformanceThresholds) -> Self {
        self.test_type = Some(TestType::Performance {
            source: source.to_string(),
            performance_thresholds: thresholds,
        });
        self
    }

    /// Set test type to error condition test
    pub fn error_test(mut self, source: &str, expected_error: ErrorExpectation) -> Self {
        self.test_type = Some(TestType::ErrorCondition {
            source: source.to_string(),
            expected_error,
        });
        self
    }

    /// Set test type to regression test
    pub fn regression_test(mut self, source: &str, baseline: BaselineMetrics) -> Self {
        self.test_type = Some(TestType::Regression {
            source: source.to_string(),
            baseline_metrics: baseline,
        });
        self
    }

    /// Add an expectation
    pub fn expect(mut self, expectation: TestExpectation) -> Self {
        self.expectations.push(expectation);
        self
    }

    /// Set timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Add a tag
    pub fn tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Set test setup
    pub fn setup(mut self, setup: TestSetup) -> Self {
        self.setup = Some(setup);
        self
    }

    /// Set test cleanup
    pub fn cleanup(mut self, cleanup: TestCleanup) -> Self {
        self.cleanup = Some(cleanup);
        self
    }

    /// Set skip condition
    pub fn skip_if(mut self, condition: SkipCondition) -> Self {
        self.skip_condition = Some(condition);
        self
    }

    /// Set retry count
    pub fn retry_count(mut self, count: usize) -> Self {
        self.retry_count = count;
        self
    }

    /// Build the test case
    pub fn build(self) -> Result<TestCase, CompilerError> {
        let test_type = self.test_type.ok_or_else(|| {
            CompilerError::testing_error("Test type must be specified", None, None)
        })?;

        Ok(TestCase {
            name: self.name,
            description: self.description,
            test_type,
            expectations: self.expectations,
            timeout: self.timeout,
            tags: self.tags,
            setup: self.setup,
            cleanup: self.cleanup,
            skip_condition: self.skip_condition,
            retry_count: self.retry_count,
        })
    }
}

impl TestExpectation {
    /// Verify if the expectation is met
    pub fn verify(&self, output: &TestOutput) -> Result<bool, CompilerError> {
        match self {
            TestExpectation::CompilationSuccess => {
                Ok(output.compilation_result.as_ref()
                   .map_or(false, |r| r.succeeded))
            }
            TestExpectation::CompilationFailure => {
                Ok(output.compilation_result.as_ref()
                   .map_or(true, |r| !r.succeeded))
            }
            TestExpectation::OutputEquals(expected) => {
                Ok(output.output.as_ref()
                   .map_or(false, |actual| actual.trim() == expected.trim()))
            }
            TestExpectation::OutputContains(expected) => {
                Ok(output.output.as_ref()
                   .map_or(false, |actual| actual.contains(expected)))
            }
            TestExpectation::OutputMatches(pattern) => {
                let regex = regex::Regex::new(pattern)
                    .map_err(|e| CompilerError::testing_error(
                        format!("Invalid regex pattern: {}", e), None, None))?;
                
                Ok(output.output.as_ref()
                   .map_or(false, |actual| regex.is_match(actual)))
            }
            TestExpectation::CompletesWithin(max_duration) => {
                Ok(output.performance_data.as_ref()
                   .map_or(true, |perf| perf.compilation_time <= *max_duration))
            }
            TestExpectation::MemoryUsageLessThan(max_memory) => {
                Ok(output.performance_data.as_ref()
                   .map_or(true, |perf| perf.memory_peak < *max_memory))
            }
            TestExpectation::BinarySizeLessThan(max_size) => {
                Ok(output.compilation_result.as_ref()
                   .and_then(|r| r.wasm_bytes.as_ref())
                   .map_or(true, |bytes| bytes.len() < *max_size))
            }
            TestExpectation::Custom(validator) => {
                validator(output)
            }
        }
    }
}

impl TestCase {
    /// Check if this test should be skipped
    pub fn should_skip(&self) -> bool {
        if let Some(ref condition) = self.skip_condition {
            match condition {
                SkipCondition::EnvironmentVariable(var) => {
                    std::env::var(var).is_ok()
                }
                SkipCondition::Platform(platform) => {
                    std::env::consts::OS != platform
                }
                SkipCondition::Feature(feature) => {
                    // Check if feature is enabled (simplified)
                    !cfg!(feature = "all-features") // Placeholder logic
                }
                SkipCondition::Custom(check) => check(),
            }
        } else {
            false
        }
    }

    /// Get test tags
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Check if test has specific tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// Predefined test case templates
pub struct TestTemplates;

impl TestTemplates {
    /// Create a basic compilation test
    pub fn compilation_success(name: &str, source: &str) -> TestCaseBuilder {
        TestCaseBuilder::new(name)
            .compilation_test(source, CompilationExpectation::Success)
            .expect(TestExpectation::CompilationSuccess)
    }

    /// Create a compilation failure test
    pub fn compilation_failure(name: &str, source: &str) -> TestCaseBuilder {
        TestCaseBuilder::new(name)
            .compilation_test(source, CompilationExpectation::Failure)
            .expect(TestExpectation::CompilationFailure)
    }

    /// Create a hello world execution test
    pub fn hello_world(name: &str) -> TestCaseBuilder {
        TestCaseBuilder::new(name)
            .description("Basic hello world test")
            .execution_test(
                r#"
                function main() {
                    print("Hello, World!");
                }
                "#,
                vec![],
                "Hello, World!"
            )
            .tag("basic")
            .tag("execution")
    }

    /// Create a syntax error test
    pub fn syntax_error(name: &str, invalid_source: &str, error_message: &str) -> TestCaseBuilder {
        TestCaseBuilder::new(name)
            .description("Test syntax error handling")
            .error_test(invalid_source, ErrorExpectation::SpecificError(error_message.to_string()))
            .tag("error")
            .tag("syntax")
    }

    /// Create a performance benchmark test
    pub fn performance_benchmark(name: &str, source: &str, max_compile_time_ms: u64) -> TestCaseBuilder {
        TestCaseBuilder::new(name)
            .description("Performance benchmark test")
            .performance_test(source, PerformanceThresholds {
                max_compilation_time: Some(Duration::from_millis(max_compile_time_ms)),
                max_execution_time: None,
                max_memory_mb: Some(100),
                max_binary_size: Some(1024 * 1024), // 1MB
                min_throughput_ops_per_sec: None,
            })
            .tag("performance")
            .tag("benchmark")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_builder() {
        let test_case = TestCaseBuilder::new("test_addition")
            .description("Test integer addition")
            .compilation_test("let x = 2 + 3;", CompilationExpectation::Success)
            .expect(TestExpectation::CompilationSuccess)
            .tag("arithmetic")
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        assert_eq!(test_case.name, "test_addition");
        assert_eq!(test_case.description, "Test integer addition");
        assert_eq!(test_case.tags, vec!["arithmetic"]);
        assert_eq!(test_case.timeout, Some(Duration::from_secs(5)));
        assert_eq!(test_case.expectations.len(), 1);
    }

    #[test]
    fn test_template_hello_world() {
        let test_case = TestTemplates::hello_world("hello_test")
            .build()
            .unwrap();

        assert_eq!(test_case.name, "hello_test");
        assert!(test_case.has_tag("basic"));
        assert!(test_case.has_tag("execution"));
    }

    #[test]
    fn test_skip_condition_platform() {
        let mut builder = TestCaseBuilder::new("platform_test")
            .compilation_test("let x = 1;", CompilationExpectation::Success)
            .skip_if(SkipCondition::Platform("nonexistent_os".to_string()));

        let test_case = builder.build().unwrap();
        assert!(test_case.should_skip()); // Should skip because platform doesn't match
    }

    #[test]
    fn test_expectation_output_equals() {
        let expectation = TestExpectation::OutputEquals("Hello, World!".to_string());
        
        let output = TestOutput {
            success: true,
            message: None,
            output: Some("Hello, World!".to_string()),
            compilation_result: None,
            performance_data: None,
        };

        assert!(expectation.verify(&output).unwrap());
    }

    #[test]
    fn test_expectation_output_contains() {
        let expectation = TestExpectation::OutputContains("World".to_string());
        
        let output = TestOutput {
            success: true,
            message: None,
            output: Some("Hello, World! How are you?".to_string()),
            compilation_result: None,
            performance_data: None,
        };

        assert!(expectation.verify(&output).unwrap());
    }
}