use crate::error::CompilerError;
use regex;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::path::{Path, PathBuf};

pub mod test_harness;
pub mod test_runner;
pub mod test_suite;
pub mod test_case;
pub mod test_reporter;
pub mod integration_tests;
pub mod unit_tests;
pub mod performance_tests;
pub mod regression_tests;

// Re-export main components
pub use test_harness::{TestHarness, TestHarnessConfig};
pub use test_runner::{TestRunner, TestExecutionMode};
pub use test_suite::{TestSuite, TestSuiteBuilder};
pub use test_case::{TestCase, TestCaseBuilder, TestExpectation, TestTemplates, CompilationExpectation, ErrorExpectation, PerformanceThresholds, BaselineMetrics, ErrorCategory, TestType, TestSetup, TestCleanup};
pub use test_reporter::{TestReporter, TestReport, TestFormat};

/// Main testing framework for Clean Language compiler
/// 
/// Provides comprehensive testing capabilities:
/// - Unit tests for individual components
/// - Integration tests for end-to-end compilation
/// - Performance benchmarks
/// - Regression test detection
/// - Property-based testing
/// - Fuzz testing support
#[derive(Debug)]
pub struct TestingFramework {
    pub config: TestFrameworkConfig,
    pub harness: TestHarness,
    pub suites: HashMap<String, TestSuite>,
    pub results: TestResults,
}

/// Configuration for the testing framework
#[derive(Debug, Clone)]
pub struct TestFrameworkConfig {
    pub test_directory: PathBuf,
    pub output_directory: PathBuf,
    pub parallel_execution: bool,
    pub max_parallel_jobs: usize,
    pub timeout_seconds: u64,
    pub verbose_output: bool,
    pub fail_fast: bool,
    pub collect_coverage: bool,
    pub benchmark_mode: bool,
    pub regression_detection: bool,
    pub random_seed: Option<u64>,
}

/// Overall test results
#[derive(Debug, Default, Clone)]
pub struct TestResults {
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub execution_time: Duration,
    pub suite_results: HashMap<String, TestSuiteResult>,
    pub performance_metrics: PerformanceMetrics,
}

/// Results for a test suite
#[derive(Debug, Default, Clone)]
pub struct TestSuiteResult {
    pub name: String,
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub execution_time: Duration,
    pub test_results: Vec<TestCaseResult>,
}

/// Result for an individual test case
#[derive(Debug, Clone)]
pub struct TestCaseResult {
    pub name: String,
    pub status: TestStatus,
    pub execution_time: Duration,
    pub message: Option<String>,
    pub output: Option<String>,
    pub error_details: Option<CompilerError>,
    pub performance_data: Option<PerformanceData>,
}

/// Test execution status
#[derive(Debug, Clone, PartialEq)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    Error,
    Timeout,
}

/// Performance metrics collection
#[derive(Debug, Default, Clone)]
pub struct PerformanceMetrics {
    pub compilation_times: Vec<Duration>,
    pub memory_usage: Vec<usize>,
    pub binary_sizes: Vec<usize>,
    pub optimization_effectiveness: Vec<f64>,
    pub throughput_metrics: HashMap<String, f64>,
}

/// Individual performance data point
#[derive(Debug, Clone)]
pub struct PerformanceData {
    pub compilation_time: Duration,
    pub memory_peak: usize,
    pub binary_size: usize,
    pub optimization_ratio: f64,
    pub custom_metrics: HashMap<String, f64>,
}

impl TestingFramework {
    /// Create a new testing framework with default configuration
    pub fn new() -> Result<Self, CompilerError> {
        let config = TestFrameworkConfig::default();
        let harness = TestHarness::new(TestHarnessConfig::from(&config))?;
        
        Ok(Self {
            config,
            harness,
            suites: HashMap::new(),
            results: TestResults::default(),
        })
    }

    /// Create a new testing framework with custom configuration
    pub fn with_config(config: TestFrameworkConfig) -> Result<Self, CompilerError> {
        let harness = TestHarness::new(TestHarnessConfig::from(&config))?;
        
        Ok(Self {
            config,
            harness,
            suites: HashMap::new(),
            results: TestResults::default(),
        })
    }

    /// Add a test suite to the framework
    pub fn add_suite(&mut self, suite: TestSuite) -> Result<(), CompilerError> {
        let suite_name = suite.name.clone();
        
        if self.suites.contains_key(&suite_name) {
            return Err(CompilerError::testing_error(
                format!("Test suite '{}' already exists", suite_name),
                None, None
            ));
        }

        self.suites.insert(suite_name, suite);
        Ok(())
    }

    /// Load test suites from directory
    pub fn load_suites_from_directory(&mut self, directory: &Path) -> Result<(), CompilerError> {
        let mut suite_builder = TestSuiteBuilder::new();
        
        // Load unit tests
        suite_builder.load_unit_tests(directory.join("unit"))?;
        
        // Load integration tests
        suite_builder.load_integration_tests(directory.join("integration"))?;
        
        // Load performance tests
        suite_builder.load_performance_tests(directory.join("performance"))?;
        
        // Load regression tests
        suite_builder.load_regression_tests(directory.join("regression"))?;
        
        let suites = suite_builder.build()?;
        
        for suite in suites {
            self.add_suite(suite)?;
        }

        Ok(())
    }

    /// Run all test suites
    pub fn run_all(&mut self) -> Result<TestResults, CompilerError> {
        let start_time = Instant::now();
        self.results = TestResults::default();

        println!("🧪 Running Clean Language Compiler Test Suite");
        println!("===============================================");

        let suite_names: Vec<String> = self.suites.keys().cloned().collect();
        
        for suite_name in suite_names {
            let suite_result = self.run_suite(&suite_name)?;
            self.results.suite_results.insert(suite_name.clone(), suite_result.clone());
            
            // Update overall results
            self.results.total_tests += suite_result.total_tests;
            self.results.passed += suite_result.passed;
            self.results.failed += suite_result.failed;
            self.results.skipped += suite_result.skipped;
            self.results.errors += suite_result.errors;

            // Stop if fail-fast is enabled and we have failures
            if self.config.fail_fast && (suite_result.failed > 0 || suite_result.errors > 0) {
                println!("❌ Stopping execution due to fail-fast mode");
                break;
            }
        }

        self.results.execution_time = start_time.elapsed();
        
        // Generate final report
        self.generate_report()?;
        
        Ok(self.results.clone())
    }

    /// Run a specific test suite
    pub fn run_suite(&mut self, suite_name: &str) -> Result<TestSuiteResult, CompilerError> {
        let suite = self.suites.get(suite_name).ok_or_else(|| {
            CompilerError::testing_error(
                format!("Test suite '{}' not found", suite_name),
                None, None
            )
        })?.clone(); // Clone the suite to avoid borrowing issues

        println!("\n📂 Running test suite: {}", suite.name);
        println!("   Description: {}", suite.description);
        println!("   Tests: {}", suite.test_cases.len());

        let start_time = Instant::now();
        let mut suite_result = TestSuiteResult {
            name: suite_name.to_string(),
            total_tests: suite.test_cases.len(),
            ..Default::default()
        };

        // Run test cases
        if self.config.parallel_execution {
            suite_result.test_results = self.run_tests_parallel(&suite.test_cases)?;
        } else {
            suite_result.test_results = self.run_tests_sequential(&suite.test_cases)?;
        }

        // Calculate suite statistics
        for test_result in &suite_result.test_results {
            match test_result.status {
                TestStatus::Passed => suite_result.passed += 1,
                TestStatus::Failed => suite_result.failed += 1,
                TestStatus::Skipped => suite_result.skipped += 1,
                TestStatus::Error => suite_result.errors += 1,
                TestStatus::Timeout => suite_result.errors += 1,
            }
        }

        suite_result.execution_time = start_time.elapsed();

        // Print suite summary
        self.print_suite_summary(&suite_result);

        Ok(suite_result)
    }

    /// Run test cases sequentially
    fn run_tests_sequential(&mut self, test_cases: &[TestCase]) -> Result<Vec<TestCaseResult>, CompilerError> {
        let mut results = Vec::new();

        for test_case in test_cases {
            let result = self.run_single_test(test_case)?;
            self.print_test_result(&result);
            results.push(result);

            // Stop on first failure if fail-fast is enabled
            if self.config.fail_fast && matches!(results.last().unwrap().status, TestStatus::Failed | TestStatus::Error) {
                break;
            }
        }

        Ok(results)
    }

    /// Run test cases in parallel
    fn run_tests_parallel(&mut self, test_cases: &[TestCase]) -> Result<Vec<TestCaseResult>, CompilerError> {
        // For now, implement sequential execution
        // In a full implementation, this would use thread pools or async execution
        self.run_tests_sequential(test_cases)
    }

    /// Run a single test case
    fn run_single_test(&mut self, test_case: &TestCase) -> Result<TestCaseResult, CompilerError> {
        let start_time = Instant::now();
        
        // Run the test with timeout
        let result = if let Some(timeout) = test_case.timeout {
            self.run_test_with_timeout(test_case, timeout)
        } else {
            self.harness.execute_test(test_case)
        };

        let execution_time = start_time.elapsed();

        match result {
            Ok(test_output) => {
                // Verify expectations
                let status = if self.verify_expectations(test_case, &test_output)? {
                    TestStatus::Passed
                } else {
                    TestStatus::Failed
                };

                Ok(TestCaseResult {
                    name: test_case.name.clone(),
                    status,
                    execution_time,
                    message: test_output.message,
                    output: test_output.output,
                    error_details: None,
                    performance_data: test_output.performance_data,
                })
            }
            Err(error) => {
                Ok(TestCaseResult {
                    name: test_case.name.clone(),
                    status: TestStatus::Error,
                    execution_time,
                    message: Some(error.to_string()),
                    output: None,
                    error_details: Some(error),
                    performance_data: None,
                })
            }
        }
    }

    /// Run test with timeout (simplified implementation)
    fn run_test_with_timeout(&mut self, test_case: &TestCase, _timeout: Duration) -> Result<TestOutput, CompilerError> {
        // In a full implementation, this would use proper timeout handling
        self.harness.execute_test(test_case)
    }

    /// Verify test expectations
    fn verify_expectations(&self, test_case: &TestCase, output: &TestOutput) -> Result<bool, CompilerError> {
        for expectation in &test_case.expectations {
            if !expectation.verify(output)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Print test result
    fn print_test_result(&self, result: &TestCaseResult) {
        let status_icon = match result.status {
            TestStatus::Passed => "✅",
            TestStatus::Failed => "❌",
            TestStatus::Skipped => "⏭️",
            TestStatus::Error => "💥",
            TestStatus::Timeout => "⏰",
        };

        let time_ms = result.execution_time.as_millis();
        println!("  {} {} ({}ms)", status_icon, result.name, time_ms);

        if self.config.verbose_output {
            if let Some(message) = &result.message {
                println!("     Message: {}", message);
            }
            if let Some(output) = &result.output {
                println!("     Output: {}", output);
            }
        }
    }

    /// Print suite summary
    fn print_suite_summary(&self, result: &TestSuiteResult) {
        let total = result.total_tests;
        let passed = result.passed;
        let failed = result.failed;
        let skipped = result.skipped;
        let errors = result.errors;
        let time_ms = result.execution_time.as_millis();

        println!("\n📊 Suite Summary: {}", result.name);
        println!("   Total: {} | Passed: {} | Failed: {} | Skipped: {} | Errors: {} | Time: {}ms",
                 total, passed, failed, skipped, errors, time_ms);

        let success_rate = if total > 0 { (passed as f64 / total as f64) * 100.0 } else { 0.0 };
        println!("   Success Rate: {:.1}%", success_rate);
    }

    /// Generate comprehensive test report
    fn generate_report(&self) -> Result<(), CompilerError> {
        let reporter = TestReporter::new(TestFormat::Console);
        let report = TestReport::from_results(&self.results);
        
        reporter.generate_report(&report, &self.config.output_directory)?;
        
        // Print final summary
        self.print_final_summary();
        
        Ok(())
    }

    /// Print final test summary
    fn print_final_summary(&self) {
        println!("\n🏁 Test Execution Complete");
        println!("==========================");
        
        let total = self.results.total_tests;
        let passed = self.results.passed;
        let failed = self.results.failed;
        let skipped = self.results.skipped;
        let errors = self.results.errors;
        let time_ms = self.results.execution_time.as_millis();

        println!("Total Tests: {}", total);
        println!("Passed: {} ({}%)", passed, if total > 0 { passed * 100 / total } else { 0 });
        println!("Failed: {} ({}%)", failed, if total > 0 { failed * 100 / total } else { 0 });
        println!("Skipped: {} ({}%)", skipped, if total > 0 { skipped * 100 / total } else { 0 });
        println!("Errors: {} ({}%)", errors, if total > 0 { errors * 100 / total } else { 0 });
        println!("Total Time: {}ms", time_ms);

        if failed == 0 && errors == 0 {
            println!("\n🎉 All tests passed!");
        } else {
            println!("\n⚠️  Some tests failed. Check the detailed report for more information.");
        }
    }
}

impl Default for TestFrameworkConfig {
    fn default() -> Self {
        Self {
            test_directory: PathBuf::from("tests"),
            output_directory: PathBuf::from("target/test-results"),
            parallel_execution: true,
            max_parallel_jobs: num_cpus::get(),
            timeout_seconds: 30,
            verbose_output: false,
            fail_fast: false,
            collect_coverage: false,
            benchmark_mode: false,
            regression_detection: false,
            random_seed: None,
        }
    }
}

/// Test output from harness execution
#[derive(Debug)]
pub struct TestOutput {
    pub success: bool,
    pub message: Option<String>,
    pub output: Option<String>,
    pub compilation_result: Option<CompilationResult>,
    pub performance_data: Option<PerformanceData>,
}

/// Compilation result for testing
#[derive(Debug)]
pub struct CompilationResult {
    pub succeeded: bool,
    pub wasm_bytes: Option<Vec<u8>>,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<String>,
    pub compilation_time: Duration,
    pub optimization_metrics: Option<crate::codegen::optimizations::OptimizationMetrics>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_framework_creation() {
        let framework = TestingFramework::new().unwrap();
        assert_eq!(framework.suites.len(), 0);
        assert_eq!(framework.results.total_tests, 0);
    }

    #[test]
    fn test_framework_with_config() {
        let config = TestFrameworkConfig {
            parallel_execution: false,
            verbose_output: true,
            ..Default::default()
        };
        
        let framework = TestingFramework::with_config(config).unwrap();
        assert!(!framework.config.parallel_execution);
        assert!(framework.config.verbose_output);
    }

    #[test]
    fn test_add_duplicate_suite() {
        let mut framework = TestingFramework::new().unwrap();
        
        let suite1 = TestSuite {
            name: "test_suite".to_string(),
            description: "Test suite 1".to_string(),
            test_cases: vec![],
            setup: None,
            teardown: None,
            parallel_safe: true,
        };
        
        let suite2 = TestSuite {
            name: "test_suite".to_string(), // Same name
            description: "Test suite 2".to_string(),
            test_cases: vec![],
            setup: None,
            teardown: None,
            parallel_safe: true,
        };
        
        assert!(framework.add_suite(suite1).is_ok());
        assert!(framework.add_suite(suite2).is_err());
    }

    #[test]
    fn test_performance_metrics() {
        let mut metrics = PerformanceMetrics::default();
        
        metrics.compilation_times.push(Duration::from_millis(100));
        metrics.memory_usage.push(1024);
        metrics.binary_sizes.push(512);
        
        assert_eq!(metrics.compilation_times.len(), 1);
        assert_eq!(metrics.memory_usage[0], 1024);
        assert_eq!(metrics.binary_sizes[0], 512);
    }
}