use crate::error::CompilerError;
use crate::testing::{
    TestingFramework, TestFrameworkConfig, TestRunner, TestRunnerConfig, 
    TestExecutionMode, TestFilter, TestFormat, TestReporter
};
use std::path::PathBuf;
use std::time::Duration;

/// CLI interface for the testing framework
pub struct TestCLI {
    framework: TestingFramework,
}

/// CLI configuration for testing
#[derive(Debug, Clone)]
pub struct TestCLIConfig {
    pub test_directory: Option<PathBuf>,
    pub output_directory: Option<PathBuf>,
    pub parallel: bool,
    pub jobs: Option<usize>,
    pub timeout: Option<u64>,
    pub fail_fast: bool,
    pub verbose: bool,
    pub filter: Option<String>,
    pub tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub test_types: Vec<String>,
    pub format: String,
    pub benchmark: bool,
    pub regression: bool,
    pub retry: bool,
    pub seed: Option<u64>,
}

impl TestCLI {
    /// Create a new test CLI
    pub fn new(config: TestCLIConfig) -> Result<Self, CompilerError> {
        let framework_config = TestFrameworkConfig {
            test_directory: config.test_directory.unwrap_or_else(|| PathBuf::from("tests")),
            output_directory: config.output_directory.unwrap_or_else(|| PathBuf::from("target/test-results")),
            parallel_execution: config.parallel,
            max_parallel_jobs: config.jobs.unwrap_or_else(num_cpus::get),
            timeout_seconds: config.timeout.unwrap_or(30),
            verbose_output: config.verbose,
            fail_fast: config.fail_fast,
            collect_coverage: false, // TODO: Implement coverage collection
            benchmark_mode: config.benchmark,
            regression_detection: config.regression,
            random_seed: config.seed,
        };

        let framework = TestingFramework::with_config(framework_config)?;
        
        Ok(Self { framework })
    }

    /// Run all tests
    pub fn run_all(&mut self) -> Result<(), CompilerError> {
        println!("🧪 Clean Language Compiler Test Suite");
        println!("====================================");
        
        // Load test suites
        self.load_test_suites()?;
        
        // Run tests
        let results = self.framework.run_all()?;
        
        // Print summary
        self.print_final_summary(&results);
        
        // Exit with appropriate code
        if results.failed > 0 || results.errors > 0 {
            std::process::exit(1);
        }
        
        Ok(())
    }

    /// Run tests with filter
    pub fn run_filtered(&mut self, config: &TestCLIConfig) -> Result<(), CompilerError> {
        println!("🧪 Clean Language Compiler Test Suite (Filtered)");
        println!("=================================================");
        
        // Load test suites
        self.load_test_suites()?;
        
        // Create test filter
        let filter = self.create_filter(config);
        
        // Configure runner
        let runner_config = self.create_runner_config(config);
        let runner = TestRunner::new(runner_config);
        
        // Get all test cases and filter them
        let all_tests = self.collect_all_test_cases();
        let filtered_tests = if let Some(filter) = filter {
            self.apply_filter(all_tests, filter)
        } else {
            all_tests
        };

        println!("Running {} filtered tests...", filtered_tests.len());
        
        // Run filtered tests
        let results = runner.run_tests(filtered_tests)?;
        
        // Print results
        self.print_test_results(&results);
        
        Ok(())
    }

    /// Run benchmark tests only
    pub fn run_benchmarks(&mut self) -> Result<(), CompilerError> {
        println!("🚀 Clean Language Compiler Benchmark Suite");
        println!("==========================================");
        
        // Load performance tests
        self.load_test_suites()?;
        
        // Filter for benchmark tests
        let config = TestCLIConfig {
            tags: vec!["performance".to_string(), "benchmark".to_string()],
            benchmark: true,
            parallel: false, // Benchmarks should run sequentially
            ..Default::default()
        };
        
        self.run_filtered(&config)
    }

    /// Run regression tests only
    pub fn run_regressions(&mut self) -> Result<(), CompilerError> {
        println!("📊 Clean Language Compiler Regression Suite");
        println!("===========================================");
        
        // Load regression tests
        self.load_test_suites()?;
        
        // Filter for regression tests
        let config = TestCLIConfig {
            tags: vec!["regression".to_string()],
            regression: true,
            ..Default::default()
        };
        
        self.run_filtered(&config)
    }

    /// Generate test report
    pub fn generate_report(&self, format: TestFormat, output_dir: &PathBuf) -> Result<(), CompilerError> {
        let reporter = TestReporter::new(format);
        let report = crate::testing::TestReport::from_results(&self.framework.results);
        
        reporter.generate_report(&report, output_dir)?;
        
        Ok(())
    }

    /// List available tests
    pub fn list_tests(&mut self) -> Result<(), CompilerError> {
        println!("📋 Available Tests");
        println!("==================");
        
        self.load_test_suites()?;
        
        for (suite_name, suite) in &self.framework.suites {
            println!("\n📂 Suite: {}", suite_name);
            println!("   Description: {}", suite.description);
            println!("   Tests: {}", suite.test_cases.len());
            
            for test_case in &suite.test_cases {
                println!("   • {} - {}", test_case.name, test_case.description);
                if !test_case.tags.is_empty() {
                    println!("     Tags: {}", test_case.tags.join(", "));
                }
            }
        }
        
        Ok(())
    }

    /// Show test statistics
    pub fn show_stats(&mut self) -> Result<(), CompilerError> {
        println!("📈 Test Suite Statistics");
        println!("=======================");
        
        self.load_test_suites()?;
        
        let total_suites = self.framework.suites.len();
        let total_tests: usize = self.framework.suites.values()
            .map(|suite| suite.test_cases.len())
            .sum();
        
        let mut tag_counts = std::collections::HashMap::new();
        let mut type_counts = std::collections::HashMap::new();
        
        for suite in self.framework.suites.values() {
            for test_case in &suite.test_cases {
                // Count tags
                for tag in &test_case.tags {
                    *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                }
                
                // Count test types
                let test_type = match test_case.test_type {
                    crate::testing::TestType::Compilation { .. } => "compilation",
                    crate::testing::TestType::Execution { .. } => "execution",
                    crate::testing::TestType::Performance { .. } => "performance",
                    crate::testing::TestType::ErrorCondition { .. } => "error",
                    crate::testing::TestType::Regression { .. } => "regression",
                };
                *type_counts.entry(test_type.to_string()).or_insert(0) += 1;
            }
        }
        
        println!("Total Suites: {}", total_suites);
        println!("Total Tests: {}", total_tests);
        println!();
        
        println!("Test Types:");
        for (test_type, count) in type_counts {
            println!("  {}: {}", test_type, count);
        }
        println!();
        
        println!("Popular Tags:");
        let mut tag_vec: Vec<_> = tag_counts.iter().collect();
        tag_vec.sort_by(|a, b| b.1.cmp(a.1));
        for (tag, count) in tag_vec.iter().take(10) {
            println!("  {}: {}", tag, count);
        }
        
        Ok(())
    }

    /// Load all test suites
    fn load_test_suites(&mut self) -> Result<(), CompilerError> {
        let test_dir = &self.framework.config.test_directory;
        self.framework.load_suites_from_directory(test_dir)?;
        Ok(())
    }

    /// Create test filter from CLI config
    fn create_filter(&self, config: &TestCLIConfig) -> Option<TestFilter> {
        if config.filter.is_none() && config.tags.is_empty() && 
           config.exclude_tags.is_empty() && config.test_types.is_empty() {
            return None;
        }

        let mut filter = TestFilter::new();
        
        if let Some(ref pattern) = config.filter {
            filter = filter.name_contains(pattern);
        }
        
        for tag in &config.tags {
            filter = filter.with_tag(tag);
        }
        
        for tag in &config.exclude_tags {
            filter = filter.without_tag(tag);
        }
        
        for test_type in &config.test_types {
            filter = filter.test_type(test_type);
        }
        
        Some(filter)
    }

    /// Create runner config from CLI config
    fn create_runner_config(&self, config: &TestCLIConfig) -> TestRunnerConfig {
        let execution_mode = if config.parallel {
            TestExecutionMode::Parallel
        } else if let Some(seed) = config.seed {
            TestExecutionMode::Randomized { seed: Some(seed) }
        } else {
            TestExecutionMode::Sequential
        };

        TestRunnerConfig {
            execution_mode,
            max_parallel_jobs: config.jobs.unwrap_or_else(num_cpus::get),
            timeout_seconds: config.timeout.unwrap_or(30),
            retry_failed_tests: config.retry,
            max_retries: 2,
            stop_on_first_failure: config.fail_fast,
            collect_detailed_metrics: config.benchmark,
        }
    }

    /// Collect all test cases from all suites
    fn collect_all_test_cases(&self) -> Vec<crate::testing::TestCase> {
        let mut all_tests = Vec::new();
        
        for suite in self.framework.suites.values() {
            all_tests.extend(suite.test_cases.clone());
        }
        
        all_tests
    }

    /// Apply filter to test cases
    fn apply_filter(&self, tests: Vec<crate::testing::TestCase>, filter: TestFilter) -> Vec<crate::testing::TestCase> {
        tests.into_iter()
            .filter(|test| self.test_matches_filter(test, &filter))
            .collect()
    }

    /// Check if test matches filter (simplified implementation)
    fn test_matches_filter(&self, test: &crate::testing::TestCase, filter: &TestFilter) -> bool {
        // Check name pattern
        if let Some(ref pattern) = filter.name_pattern {
            if !test.name.contains(pattern) {
                return false;
            }
        }

        // Check required tags
        if !filter.tags.is_empty() {
            let has_required_tag = filter.tags.iter()
                .any(|tag| test.has_tag(tag));
            if !has_required_tag {
                return false;
            }
        }

        // Check excluded tags
        if !filter.exclude_tags.is_empty() {
            let has_excluded_tag = filter.exclude_tags.iter()
                .any(|tag| test.has_tag(tag));
            if has_excluded_tag {
                return false;
            }
        }

        true
    }

    /// Print test results
    fn print_test_results(&self, results: &[crate::testing::TestCaseResult]) {
        let mut passed = 0;
        let mut failed = 0;
        let mut errors = 0;
        let mut skipped = 0;

        for result in results {
            match result.status {
                crate::testing::TestStatus::Passed => passed += 1,
                crate::testing::TestStatus::Failed => failed += 1,
                crate::testing::TestStatus::Error => errors += 1,
                crate::testing::TestStatus::Skipped => skipped += 1,
                crate::testing::TestStatus::Timeout => errors += 1,
            }
        }

        println!("\n📊 Results Summary");
        println!("==================");
        println!("Total: {}", results.len());
        println!("✅ Passed: {}", passed);
        println!("❌ Failed: {}", failed);
        println!("💥 Errors: {}", errors);
        println!("⏭️ Skipped: {}", skipped);
        
        let success_rate = if results.len() > 0 {
            (passed as f64 / results.len() as f64) * 100.0
        } else {
            0.0
        };
        println!("🎯 Success Rate: {:.1}%", success_rate);
    }

    /// Print final summary
    fn print_final_summary(&self, results: &crate::testing::TestResults) {
        println!("\n🏁 Final Test Summary");
        println!("=====================");
        println!("Total Tests: {}", results.total_tests);
        println!("✅ Passed: {}", results.passed);
        println!("❌ Failed: {}", results.failed);
        println!("💥 Errors: {}", results.errors);
        println!("⏭️ Skipped: {}", results.skipped);
        println!("⏱️ Total Time: {:?}", results.execution_time);
        
        let success_rate = if results.total_tests > 0 {
            (results.passed as f64 / results.total_tests as f64) * 100.0
        } else {
            0.0
        };
        println!("🎯 Success Rate: {:.1}%", success_rate);

        if results.failed > 0 || results.errors > 0 {
            println!("\n⚠️ Some tests failed. See detailed results above.");
        } else {
            println!("\n🎉 All tests passed!");
        }
    }
}

impl Default for TestCLIConfig {
    fn default() -> Self {
        Self {
            test_directory: None,
            output_directory: None,
            parallel: true,
            jobs: None,
            timeout: None,
            fail_fast: false,
            verbose: false,
            filter: None,
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            test_types: Vec::new(),
            format: "console".to_string(),
            benchmark: false,
            regression: false,
            retry: false,
            seed: None,
        }
    }
}

/// Parse test format from string
pub fn parse_test_format(format_str: &str) -> TestFormat {
    match format_str.to_lowercase().as_str() {
        "console" => TestFormat::Console,
        "json" => TestFormat::Json,
        "junit" => TestFormat::JUnit,
        "html" => TestFormat::Html,
        "tap" => TestFormat::Tap,
        _ => TestFormat::Console,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cli_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = TestCLIConfig {
            test_directory: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        
        let cli = TestCLI::new(config);
        assert!(cli.is_ok());
    }

    #[test]
    fn test_format_parsing() {
        assert!(matches!(parse_test_format("console"), TestFormat::Console));
        assert!(matches!(parse_test_format("json"), TestFormat::Json));
        assert!(matches!(parse_test_format("junit"), TestFormat::JUnit));
        assert!(matches!(parse_test_format("html"), TestFormat::Html));
        assert!(matches!(parse_test_format("tap"), TestFormat::Tap));
        assert!(matches!(parse_test_format("unknown"), TestFormat::Console));
    }

    #[test]
    fn test_filter_creation() {
        let cli_config = TestCLIConfig {
            filter: Some("test_name".to_string()),
            tags: vec!["unit".to_string()],
            exclude_tags: vec!["slow".to_string()],
            test_types: vec!["compilation".to_string()],
            ..Default::default()
        };

        let temp_dir = TempDir::new().unwrap();
        let config = TestCLIConfig {
            test_directory: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        
        let cli = TestCLI::new(config).unwrap();
        let filter = cli.create_filter(&cli_config);
        
        assert!(filter.is_some());
        let filter = filter.unwrap();
        assert_eq!(filter.name_pattern, Some("test_name".to_string()));
        assert_eq!(filter.tags, vec!["unit"]);
        assert_eq!(filter.exclude_tags, vec!["slow"]);
        assert_eq!(filter.test_types, vec!["compilation"]);
    }
}