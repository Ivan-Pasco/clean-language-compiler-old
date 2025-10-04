use crate::error::CompilerError;
use crate::testing::{TestCase, TestCaseBuilder, TestTemplates, CompilationExpectation, ErrorExpectation, PerformanceThresholds, BaselineMetrics, ErrorCategory};
use std::path::{Path, PathBuf};
use std::fs;
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// Collection of related test cases
/// 
/// A test suite groups related tests together and provides
/// shared setup/teardown functionality.
#[derive(Debug, Clone)]
pub struct TestSuite {
    pub name: String,
    pub description: String,
    pub test_cases: Vec<TestCase>,
    pub setup: Option<SuiteSetup>,
    pub teardown: Option<SuiteTeardown>,
    pub parallel_safe: bool,
}

/// Setup configuration for a test suite
#[derive(Debug, Clone)]
pub struct SuiteSetup {
    pub setup_commands: Vec<String>,
    pub required_files: Vec<String>,
    pub environment_variables: std::collections::HashMap<String, String>,
}

/// Teardown configuration for a test suite
#[derive(Debug, Clone)]
pub struct SuiteTeardown {
    pub cleanup_commands: Vec<String>,
    pub files_to_remove: Vec<String>,
}

/// Builder for creating test suites
pub struct TestSuiteBuilder {
    suites: Vec<TestSuite>,
    current_suite: Option<TestSuite>,
}

/// Test case configuration loaded from files
#[derive(Debug, Deserialize, Serialize)]
struct TestConfig {
    name: String,
    description: Option<String>,
    test_type: String,
    source: Option<String>,
    source_file: Option<String>,
    expected_output: Option<String>,
    expected_error: Option<String>,
    timeout_seconds: Option<u64>,
    tags: Option<Vec<String>>,
    performance: Option<PerformanceConfig>,
    baseline: Option<BaselineConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PerformanceConfig {
    max_compilation_time_ms: Option<u64>,
    max_execution_time_ms: Option<u64>,
    max_memory_mb: Option<usize>,
    max_binary_size: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BaselineConfig {
    compilation_time_ms: u64,
    execution_time_ms: u64,
    memory_usage_mb: usize,
    binary_size: usize,
    regression_factor: f64,
}

impl TestSuiteBuilder {
    /// Create a new test suite builder
    pub fn new() -> Self {
        Self {
            suites: Vec::new(),
            current_suite: None,
        }
    }

    /// Start building a new test suite
    pub fn suite(mut self, name: &str, description: &str) -> Self {
        // Finalize current suite if exists
        if let Some(suite) = self.current_suite.take() {
            self.suites.push(suite);
        }

        self.current_suite = Some(TestSuite {
            name: name.to_string(),
            description: description.to_string(),
            test_cases: Vec::new(),
            setup: None,
            teardown: None,
            parallel_safe: true,
        });

        self
    }

    /// Add a test case to the current suite
    pub fn test_case(mut self, test_case: TestCase) -> Self {
        if let Some(ref mut suite) = self.current_suite {
            suite.test_cases.push(test_case);
        }
        self
    }

    /// Add setup to the current suite
    pub fn setup(mut self, setup: SuiteSetup) -> Self {
        if let Some(ref mut suite) = self.current_suite {
            suite.setup = Some(setup);
        }
        self
    }

    /// Add teardown to the current suite
    pub fn teardown(mut self, teardown: SuiteTeardown) -> Self {
        if let Some(ref mut suite) = self.current_suite {
            suite.teardown = Some(teardown);
        }
        self
    }

    /// Set parallel safety for current suite
    pub fn parallel_safe(mut self, safe: bool) -> Self {
        if let Some(ref mut suite) = self.current_suite {
            suite.parallel_safe = safe;
        }
        self
    }

    /// Load unit tests from directory
    pub fn load_unit_tests(&mut self, unit_tests_dir: PathBuf) -> Result<(), CompilerError> {
        if !unit_tests_dir.exists() {
            return Ok(()); // Skip if directory doesn't exist
        }

        self.current_suite = Some(TestSuite {
            name: "unit_tests".to_string(),
            description: "Unit tests for compiler components".to_string(),
            test_cases: Vec::new(),
            setup: None,
            teardown: None,
            parallel_safe: true,
        });

        // Load parser tests
        self.load_parser_tests(&unit_tests_dir)?;
        
        // Load semantic analyzer tests
        self.load_semantic_tests(&unit_tests_dir)?;
        
        // Load code generator tests
        self.load_codegen_tests(&unit_tests_dir)?;

        if let Some(suite) = self.current_suite.take() {
            self.suites.push(suite);
        }

        Ok(())
    }

    /// Load integration tests from directory
    pub fn load_integration_tests(&mut self, integration_dir: PathBuf) -> Result<(), CompilerError> {
        if !integration_dir.exists() {
            return Ok(());
        }

        self.current_suite = Some(TestSuite {
            name: "integration_tests".to_string(),
            description: "End-to-end integration tests".to_string(),
            test_cases: Vec::new(),
            setup: None,
            teardown: None,
            parallel_safe: true,
        });

        // Load test files from directory
        self.load_tests_from_directory(&integration_dir, "integration")?;

        if let Some(suite) = self.current_suite.take() {
            self.suites.push(suite);
        }

        Ok(())
    }

    /// Load performance tests from directory
    pub fn load_performance_tests(&mut self, perf_dir: PathBuf) -> Result<(), CompilerError> {
        if !perf_dir.exists() {
            return Ok(());
        }

        self.current_suite = Some(TestSuite {
            name: "performance_tests".to_string(),
            description: "Performance and benchmark tests".to_string(),
            test_cases: Vec::new(),
            setup: None,
            teardown: None,
            parallel_safe: false, // Performance tests should run sequentially
        });

        // Load benchmark tests
        self.load_benchmark_tests(&perf_dir)?;

        if let Some(suite) = self.current_suite.take() {
            self.suites.push(suite);
        }

        Ok(())
    }

    /// Load regression tests from directory
    pub fn load_regression_tests(&mut self, regression_dir: PathBuf) -> Result<(), CompilerError> {
        if !regression_dir.exists() {
            return Ok(());
        }

        self.current_suite = Some(TestSuite {
            name: "regression_tests".to_string(),
            description: "Regression detection tests".to_string(),
            test_cases: Vec::new(),
            setup: None,
            teardown: None,
            parallel_safe: true,
        });

        // Load regression test configurations
        self.load_tests_from_directory(&regression_dir, "regression")?;

        if let Some(suite) = self.current_suite.take() {
            self.suites.push(suite);
        }

        Ok(())
    }

    /// Load parser unit tests
    fn load_parser_tests(&mut self, unit_dir: &Path) -> Result<(), CompilerError> {
        // Basic parsing tests
        let basic_tests = vec![
            TestTemplates::compilation_success(
                "parse_variable_declaration",
                "let x: integer = 42;"
            ).tag("parser").tag("variables"),
            
            TestTemplates::compilation_success(
                "parse_function_declaration", 
                "function add(a: integer, b: integer) -> integer { return a + b; }"
            ).tag("parser").tag("functions"),
            
            TestTemplates::compilation_success(
                "parse_class_declaration",
                r#"
                class Person {
                    constructor(name: string) {
                        this.name = name;
                    }
                }
                "#
            ).tag("parser").tag("classes"),
            
            TestTemplates::syntax_error(
                "parse_invalid_syntax",
                "let x = ;;",
                "unexpected token"
            ),
        ];

        for test_builder in basic_tests {
            let test_case = test_builder.build()?;
            if let Some(ref mut suite) = self.current_suite {
                suite.test_cases.push(test_case);
            }
        }

        Ok(())
    }

    /// Load semantic analyzer tests
    fn load_semantic_tests(&mut self, _unit_dir: &Path) -> Result<(), CompilerError> {
        let semantic_tests = vec![
            TestTemplates::compilation_failure(
                "semantic_undefined_variable",
                r#"
                function test() {
                    return undefined_var;
                }
                "#
            ).tag("semantic").tag("variables"),
            
            TestCaseBuilder::new("semantic_type_mismatch")
                .description("Test type mismatch detection")
                .error_test(
                    r#"
                    function test() {
                        let x: integer = "string";
                    }
                    "#,
                    ErrorExpectation::ErrorCategory(ErrorCategory::Type)
                )
                .tag("semantic")
                .tag("types"),
        ];

        for test_builder in semantic_tests {
            let test_case = test_builder.build()?;
            if let Some(ref mut suite) = self.current_suite {
                suite.test_cases.push(test_case);
            }
        }

        Ok(())
    }

    /// Load code generator tests  
    fn load_codegen_tests(&mut self, _unit_dir: &Path) -> Result<(), CompilerError> {
        let codegen_tests = vec![
            TestTemplates::hello_world("codegen_hello_world"),
            
            TestCaseBuilder::new("codegen_arithmetic")
                .description("Test arithmetic code generation")
                .execution_test(
                    r#"
                    function main() {
                        let result = 2 + 3 * 4;
                        print(result);
                    }
                    "#,
                    vec![],
                    "14"
                )
                .tag("codegen")
                .tag("arithmetic"),
        ];

        for test_builder in codegen_tests {
            let test_case = test_builder.build()?;
            if let Some(ref mut suite) = self.current_suite {
                suite.test_cases.push(test_case);
            }
        }

        Ok(())
    }

    /// Load benchmark tests
    fn load_benchmark_tests(&mut self, perf_dir: &Path) -> Result<(), CompilerError> {
        // Sample performance tests
        let perf_tests = vec![
            TestTemplates::performance_benchmark(
                "compile_time_small_program",
                r#"
                function factorial(n: integer) -> integer {
                    if (n <= 1) return 1;
                    return n * factorial(n - 1);
                }
                
                function main() {
                    print(factorial(10));
                }
                "#,
                1000 // 1 second max compilation time
            ),
            
            TestCaseBuilder::new("large_program_compilation")
                .description("Test compilation time for large program")
                .performance_test(
                    &self.generate_large_program(1000), // 1000 functions
                    PerformanceThresholds {
                        max_compilation_time: Some(Duration::from_secs(5)),
                        max_execution_time: None,
                        max_memory_mb: Some(200),
                        max_binary_size: Some(10 * 1024 * 1024), // 10MB
                        min_throughput_ops_per_sec: None,
                    }
                )
                .tag("performance")
                .tag("large-scale"),
        ];

        for test_builder in perf_tests {
            let test_case = test_builder.build()?;
            if let Some(ref mut suite) = self.current_suite {
                suite.test_cases.push(test_case);
            }
        }

        Ok(())
    }

    /// Load tests from directory with configuration files
    fn load_tests_from_directory(&mut self, dir: &Path, test_type: &str) -> Result<(), CompilerError> {
        let entries = fs::read_dir(dir)
            .map_err(|e| CompilerError::io_error(format!("Failed to read directory: {}", e), None, None))?;

        for entry in entries {
            let entry = entry.map_err(|e| CompilerError::io_error(format!("Failed to read entry: {}", e), None, None))?;
            let path = entry.path();

            if path.extension().map_or(false, |ext| ext == "toml") {
                self.load_test_config(&path, test_type)?;
            } else if path.extension().map_or(false, |ext| ext == "cln") {
                self.load_test_source_file(&path, test_type)?;
            }
        }

        Ok(())
    }

    /// Load test from TOML configuration file
    fn load_test_config(&mut self, config_path: &Path, _test_type: &str) -> Result<(), CompilerError> {
        let content = fs::read_to_string(config_path)
            .map_err(|e| CompilerError::io_error(format!("Failed to read config file: {}", e), None, None))?;

        let config: TestConfig = toml::from_str(&content)
            .map_err(|e| CompilerError::testing_error(format!("Failed to parse test config: {}", e), None, None))?;

        let test_case = self.create_test_from_config(config)?;
        
        if let Some(ref mut suite) = self.current_suite {
            suite.test_cases.push(test_case);
        }

        Ok(())
    }

    /// Load test from Clean Language source file
    fn load_test_source_file(&mut self, source_path: &Path, test_type: &str) -> Result<(), CompilerError> {
        let source = fs::read_to_string(source_path)
            .map_err(|e| CompilerError::io_error(format!("Failed to read source file: {}", e), None, None))?;

        let test_name = source_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown_test");

        // Create a basic compilation test for Clean Language files
        let test_case = TestCaseBuilder::new(&format!("{}_{}", test_type, test_name))
            .description(&format!("Test from file: {}", source_path.display()))
            .compilation_test(&source, CompilationExpectation::Success)
            .tag(test_type)
            .tag("file-based")
            .build()?;

        if let Some(ref mut suite) = self.current_suite {
            suite.test_cases.push(test_case);
        }

        Ok(())
    }

    /// Create test case from configuration
    fn create_test_from_config(&self, config: TestConfig) -> Result<TestCase, CompilerError> {
        let mut builder = TestCaseBuilder::new(&config.name);
        
        if let Some(desc) = config.description {
            builder = builder.description(&desc);
        }

        if let Some(timeout) = config.timeout_seconds {
            builder = builder.timeout(Duration::from_secs(timeout));
        }

        if let Some(tags) = config.tags {
            for tag in tags {
                builder = builder.tag(&tag);
            }
        }

        // Get source code
        let source = if let Some(source) = config.source {
            source
        } else if let Some(source_file) = config.source_file {
            fs::read_to_string(&source_file)
                .map_err(|e| CompilerError::io_error(format!("Failed to read source file: {}", e), None, None))?
        } else {
            return Err(CompilerError::testing_error("No source or source_file specified", None, None));
        };

        // Set test type based on configuration
        builder = match config.test_type.as_str() {
            "compilation" => {
                builder.compilation_test(&source, CompilationExpectation::Success)
            }
            "execution" => {
                let expected_output = config.expected_output.unwrap_or_default();
                builder.execution_test(&source, vec![], &expected_output)
            }
            "error" => {
                let expected_error = config.expected_error.unwrap_or_default();
                builder.error_test(&source, ErrorExpectation::SpecificError(expected_error))
            }
            "performance" => {
                let perf_config = config.performance.unwrap_or(PerformanceConfig {
                    max_compilation_time_ms: Some(1000),
                    max_execution_time_ms: Some(1000),
                    max_memory_mb: Some(100),
                    max_binary_size: Some(1024 * 1024),
                });
                
                let thresholds = PerformanceThresholds {
                    max_compilation_time: perf_config.max_compilation_time_ms.map(Duration::from_millis),
                    max_execution_time: perf_config.max_execution_time_ms.map(Duration::from_millis),
                    max_memory_mb: perf_config.max_memory_mb,
                    max_binary_size: perf_config.max_binary_size,
                    min_throughput_ops_per_sec: None,
                };
                
                builder.performance_test(&source, thresholds)
            }
            "regression" => {
                let baseline_config = config.baseline.ok_or_else(|| {
                    CompilerError::testing_error("Baseline metrics required for regression test", None, None)
                })?;
                
                let baseline = BaselineMetrics {
                    compilation_time_ms: baseline_config.compilation_time_ms,
                    execution_time_ms: baseline_config.execution_time_ms,
                    memory_usage_mb: baseline_config.memory_usage_mb,
                    binary_size: baseline_config.binary_size,
                    allowed_regression_factor: baseline_config.regression_factor,
                };
                
                builder.regression_test(&source, baseline)
            }
            _ => {
                return Err(CompilerError::testing_error(
                    format!("Unknown test type: {}", config.test_type), None, None));
            }
        };

        builder.build()
    }

    /// Generate a large program for performance testing
    fn generate_large_program(&self, function_count: usize) -> String {
        let mut program = String::new();
        
        for i in 0..function_count {
            program.push_str(&format!(
                r#"
                function func_{}(x: integer) -> integer {{
                    return x * {} + {};
                }}
                "#,
                i, i, i + 1
            ));
        }

        program.push_str(
            r#"
            function main() {
                let sum = 0;
                for (let i = 0; i < 100; i++) {
                    sum += func_0(i);
                }
                print(sum);
            }
            "#
        );

        program
    }

    /// Build all test suites
    pub fn build(mut self) -> Result<Vec<TestSuite>, CompilerError> {
        // Finalize current suite if exists
        if let Some(suite) = self.current_suite.take() {
            self.suites.push(suite);
        }

        Ok(self.suites)
    }
}

impl TestSuite {
    /// Create a new test suite
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            test_cases: Vec::new(),
            setup: None,
            teardown: None,
            parallel_safe: true,
        }
    }

    /// Add a test case to the suite
    pub fn add_test(&mut self, test_case: TestCase) {
        self.test_cases.push(test_case);
    }

    /// Get test cases with specific tag
    pub fn tests_with_tag(&self, tag: &str) -> Vec<&TestCase> {
        self.test_cases.iter()
            .filter(|test| test.has_tag(tag))
            .collect()
    }

    /// Get test count
    pub fn test_count(&self) -> usize {
        self.test_cases.len()
    }

    /// Check if suite is empty
    pub fn is_empty(&self) -> bool {
        self.test_cases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_suite_creation() {
        let suite = TestSuite::new("test_suite", "Test description");
        assert_eq!(suite.name, "test_suite");
        assert_eq!(suite.description, "Test description");
        assert!(suite.test_cases.is_empty());
        assert!(suite.parallel_safe);
    }

    #[test]
    fn test_suite_builder() {
        let test_case = TestTemplates::hello_world("hello")
            .build()
            .unwrap();

        let suites = TestSuiteBuilder::new()
            .suite("test_suite", "Test suite")
            .test_case(test_case)
            .parallel_safe(false)
            .build()
            .unwrap();

        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].name, "test_suite");
        assert_eq!(suites[0].test_cases.len(), 1);
        assert!(!suites[0].parallel_safe);
    }

    #[test]
    fn test_suite_with_tag_filtering() {
        let mut suite = TestSuite::new("test_suite", "Test suite");
        
        let test1 = TestCaseBuilder::new("test1")
            .compilation_test("let x = 1;", CompilationExpectation::Success)
            .tag("basic")
            .build()
            .unwrap();
            
        let test2 = TestCaseBuilder::new("test2")
            .compilation_test("let y = 2;", CompilationExpectation::Success)
            .tag("advanced")
            .build()
            .unwrap();

        suite.add_test(test1);
        suite.add_test(test2);

        let basic_tests = suite.tests_with_tag("basic");
        assert_eq!(basic_tests.len(), 1);
        assert_eq!(basic_tests[0].name, "test1");
    }
}