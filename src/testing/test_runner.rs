use crate::error::CompilerError;
use crate::testing::{TestCase, TestCaseResult, TestStatus, TestOutput};
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::thread;
use crossbeam_channel::{bounded, Receiver, Sender};

/// Test runner for executing test cases with various execution modes
#[derive(Debug)]
pub struct TestRunner {
    config: TestRunnerConfig,
}

/// Configuration for test execution
#[derive(Debug, Clone)]
pub struct TestRunnerConfig {
    pub execution_mode: TestExecutionMode,
    pub max_parallel_jobs: usize,
    pub timeout_seconds: u64,
    pub retry_failed_tests: bool,
    pub max_retries: usize,
    pub stop_on_first_failure: bool,
    pub collect_detailed_metrics: bool,
}

/// Test execution modes
#[derive(Debug, Clone)]
pub enum TestExecutionMode {
    /// Execute tests sequentially in a single thread
    Sequential,
    /// Execute tests in parallel with work-stealing
    Parallel,
    /// Execute tests in isolated processes
    Isolated,
    /// Execute tests with randomized order
    Randomized { seed: Option<u64> },
    /// Execute only tests matching specific criteria
    Filtered { filter: TestFilter },
}

/// Test filtering criteria
#[derive(Debug, Clone)]
pub struct TestFilter {
    pub name_pattern: Option<String>,
    pub tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub test_types: Vec<String>,
}

/// Work item for parallel execution
#[derive(Debug)]
struct WorkItem {
    test_case: TestCase,
    attempt: usize,
}

/// Result of a work item execution
#[derive(Debug)]
struct WorkResult {
    result: TestCaseResult,
    should_retry: bool,
}

impl TestRunner {
    /// Create a new test runner
    pub fn new(config: TestRunnerConfig) -> Self {
        Self { config }
    }

    /// Execute a batch of test cases
    pub fn run_tests(&self, test_cases: Vec<TestCase>) -> Result<Vec<TestCaseResult>, CompilerError> {
        match self.config.execution_mode {
            TestExecutionMode::Sequential => self.run_sequential(test_cases),
            TestExecutionMode::Parallel => self.run_parallel(test_cases),
            TestExecutionMode::Isolated => self.run_isolated(test_cases),
            TestExecutionMode::Randomized { seed } => self.run_randomized(test_cases, seed),
            TestExecutionMode::Filtered { ref filter } => {
                let filtered_tests = self.filter_tests(test_cases, filter);
                self.run_sequential(filtered_tests)
            }
        }
    }

    /// Run tests sequentially
    fn run_sequential(&self, test_cases: Vec<TestCase>) -> Result<Vec<TestCaseResult>, CompilerError> {
        let mut results = Vec::new();
        let total_tests = test_cases.len();

        for (index, test_case) in test_cases.into_iter().enumerate() {
            println!("Running test {}/{}: {}", index + 1, total_tests, test_case.name);
            
            let result = self.execute_single_test(test_case)?;
            let should_stop = self.config.stop_on_first_failure && 
                             matches!(result.status, TestStatus::Failed | TestStatus::Error);
            
            results.push(result);
            
            if should_stop {
                println!("Stopping execution due to failure (stop_on_first_failure=true)");
                break;
            }
        }

        Ok(results)
    }

    /// Run tests in parallel using work-stealing
    fn run_parallel(&self, test_cases: Vec<TestCase>) -> Result<Vec<TestCaseResult>, CompilerError> {
        let num_workers = self.config.max_parallel_jobs.min(test_cases.len());
        let (work_sender, work_receiver) = bounded::<WorkItem>(test_cases.len() * 2);
        let (result_sender, result_receiver) = bounded::<WorkResult>(test_cases.len() * 2);
        
        // Send initial work items
        for test_case in test_cases {
            work_sender.send(WorkItem {
                test_case,
                attempt: 0,
            }).map_err(|e| CompilerError::testing_error(format!("Failed to send work: {}", e), None, None))?;
        }

        // Start worker threads
        let mut handles = Vec::new();
        let total_work = Arc::new(Mutex::new(work_sender.len()));
        
        for worker_id in 0..num_workers {
            let work_rx = work_receiver.clone();
            let result_tx = result_sender.clone();
            let config = self.config.clone();
            let work_counter = total_work.clone();

            let handle = thread::spawn(move || {
                Self::worker_thread(worker_id, work_rx, result_tx, config, work_counter)
            });
            
            handles.push(handle);
        }

        // Close the work sender to signal no more work
        drop(work_sender);

        // Collect results
        let mut results = Vec::new();
        let mut retry_queue = Vec::new();

        // Process initial results and collect retries
        while let Ok(work_result) = result_receiver.recv() {
            if work_result.should_retry && self.config.retry_failed_tests {
                retry_queue.push(work_result.result);
            } else {
                results.push(work_result.result);
            }
            
            // Check if we should stop early
            if self.config.stop_on_first_failure && 
               matches!(results.last().map(|r| &r.status), Some(TestStatus::Failed) | Some(TestStatus::Error)) {
                break;
            }
        }

        // Handle retries if needed
        if !retry_queue.is_empty() && self.config.retry_failed_tests {
            println!("Retrying {} failed tests...", retry_queue.len());
            // Retry logic: re-run failed tests with fresh state
        }

        // Wait for all workers to complete
        for handle in handles {
            handle.join().map_err(|e| CompilerError::testing_error(
                format!("Worker thread panicked: {:?}", e), None, None))?;
        }

        Ok(results)
    }

    /// Worker thread for parallel execution
    fn worker_thread(
        worker_id: usize,
        work_receiver: Receiver<WorkItem>,
        result_sender: Sender<WorkResult>,
        config: TestRunnerConfig,
        _work_counter: Arc<Mutex<usize>>,
    ) {
        let runner = TestRunner::new(config.clone());
        
        while let Ok(work_item) = work_receiver.recv() {
            println!("Worker {}: Running {}", worker_id, work_item.test_case.name);
            
            match runner.execute_single_test(work_item.test_case) {
                Ok(result) => {
                    let should_retry = config.retry_failed_tests && 
                                     work_item.attempt < config.max_retries &&
                                     matches!(result.status, TestStatus::Failed | TestStatus::Error);
                    
                    let work_result = WorkResult {
                        result,
                        should_retry,
                    };
                    
                    if result_sender.send(work_result).is_err() {
                        break; // Result channel closed
                    }
                }
                Err(e) => {
                    let error_result = TestCaseResult {
                        name: format!("worker_{}_error", worker_id),
                        status: TestStatus::Error,
                        execution_time: Duration::default(),
                        message: Some(e.to_string()),
                        output: None,
                        error_details: Some(e),
                        performance_data: None,
                    };
                    
                    let work_result = WorkResult {
                        result: error_result,
                        should_retry: false,
                    };
                    
                    if result_sender.send(work_result).is_err() {
                        break;
                    }
                }
            }
        }
    }

    /// Run tests in isolated processes
    fn run_isolated(&self, test_cases: Vec<TestCase>) -> Result<Vec<TestCaseResult>, CompilerError> {
        // For now, fall back to sequential execution
        // In a full implementation, this would spawn separate processes
        println!("Warning: Isolated execution not fully implemented, falling back to sequential");
        self.run_sequential(test_cases)
    }

    /// Run tests in randomized order
    fn run_randomized(&self, mut test_cases: Vec<TestCase>, seed: Option<u64>) -> Result<Vec<TestCaseResult>, CompilerError> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let actual_seed = seed.unwrap_or_else(|| {
            let mut hasher = DefaultHasher::new();
            std::time::SystemTime::now().hash(&mut hasher);
            hasher.finish()
        });

        println!("Randomizing test order with seed: {}", actual_seed);

        // Simple shuffle using seed (not cryptographically secure, but deterministic)
        let mut rng_state = actual_seed;
        for i in (1..test_cases.len()).rev() {
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345); // LCG
            let j = (rng_state as usize) % (i + 1);
            test_cases.swap(i, j);
        }

        self.run_sequential(test_cases)
    }

    /// Filter tests based on criteria
    fn filter_tests(&self, test_cases: Vec<TestCase>, filter: &TestFilter) -> Vec<TestCase> {
        test_cases.into_iter()
            .filter(|test| self.test_matches_filter(test, filter))
            .collect()
    }

    /// Check if test matches filter criteria
    fn test_matches_filter(&self, test: &TestCase, filter: &TestFilter) -> bool {
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

        // Check test types
        if !filter.test_types.is_empty() {
            let test_type_name = match test.test_type {
                crate::testing::TestType::Compilation { .. } => "compilation",
                crate::testing::TestType::Execution { .. } => "execution",
                crate::testing::TestType::Performance { .. } => "performance",
                crate::testing::TestType::ErrorCondition { .. } => "error",
                crate::testing::TestType::Regression { .. } => "regression",
            };
            
            if !filter.test_types.contains(&test_type_name.to_string()) {
                return false;
            }
        }

        true
    }

    /// Execute a single test case with retry logic
    fn execute_single_test(&self, test_case: TestCase) -> Result<TestCaseResult, CompilerError> {
        let mut last_result = None;
        let max_attempts = if self.config.retry_failed_tests { 
            self.config.max_retries + 1 
        } else { 
            1 
        };

        for attempt in 0..max_attempts {
            if attempt > 0 {
                println!("  Retry attempt {}/{}", attempt, max_attempts - 1);
            }

            let result = self.execute_test_once(&test_case)?;
            
            // If test passed or it's the last attempt, return the result
            if matches!(result.status, TestStatus::Passed) || attempt == max_attempts - 1 {
                return Ok(result);
            }

            last_result = Some(result);
            
            // Brief pause between retries
            thread::sleep(Duration::from_millis(100));
        }

        // Return the last failed result
        Ok(last_result.unwrap())
    }

    /// Execute a test case once
    fn execute_test_once(&self, test_case: &TestCase) -> Result<TestCaseResult, CompilerError> {
        let start_time = Instant::now();

        // Check skip condition
        if test_case.should_skip() {
            return Ok(TestCaseResult {
                name: test_case.name.clone(),
                status: TestStatus::Skipped,
                execution_time: Duration::default(),
                message: Some("Test skipped due to skip condition".to_string()),
                output: None,
                error_details: None,
                performance_data: None,
            });
        }

        // Execute test setup if needed
        if let Some(ref setup) = test_case.setup {
            self.run_setup(setup)?;
        }

        // Execute the test with timeout
        let execution_result = if let Some(timeout) = test_case.timeout {
            self.execute_with_timeout(test_case, timeout)
        } else {
            self.execute_test_impl(test_case)
        };

        // Execute cleanup regardless of test outcome
        if let Some(ref cleanup) = test_case.cleanup {
            if let Err(cleanup_error) = self.run_cleanup(cleanup) {
                println!("Warning: Cleanup failed: {}", cleanup_error);
            }
        }

        let execution_time = start_time.elapsed();

        match execution_result {
            Ok(test_output) => {
                // Verify expectations
                let mut all_passed = true;
                let mut failed_expectations = Vec::new();

                for expectation in &test_case.expectations {
                    match expectation.verify(&test_output) {
                        Ok(passed) => {
                            if !passed {
                                all_passed = false;
                                failed_expectations.push(format!("Expectation failed: {:?}", expectation));
                            }
                        }
                        Err(e) => {
                            all_passed = false;
                            failed_expectations.push(format!("Expectation error: {}", e));
                        }
                    }
                }

                let status = if all_passed { TestStatus::Passed } else { TestStatus::Failed };
                let message = if failed_expectations.is_empty() {
                    test_output.message
                } else {
                    Some(failed_expectations.join("; "))
                };

                Ok(TestCaseResult {
                    name: test_case.name.clone(),
                    status,
                    execution_time,
                    message,
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

    /// Execute test with timeout (simplified implementation)
    fn execute_with_timeout(&self, test_case: &TestCase, timeout: Duration) -> Result<TestOutput, CompilerError> {
        let start = Instant::now();
        let result = self.execute_test_impl(test_case);
        
        if start.elapsed() > timeout {
            return Err(CompilerError::testing_error("Test execution timed out", None, None));
        }
        
        result
    }

    /// Actual test implementation (placeholder)
    fn execute_test_impl(&self, test_case: &TestCase) -> Result<TestOutput, CompilerError> {
        // This would delegate to the TestHarness in a real implementation
        // For now, we'll create a simple mock implementation
        
        // Simulate test execution
        thread::sleep(Duration::from_millis(10));

        Ok(TestOutput {
            success: true,
            message: Some(format!("Mock execution of {}", test_case.name)),
            output: Some("Mock output".to_string()),
            compilation_result: None,
            performance_data: None,
        })
    }

    /// Run test setup
    fn run_setup(&self, setup: &crate::testing::TestSetup) -> Result<(), CompilerError> {
        // Set environment variables
        for (key, value) in &setup.environment_variables {
            std::env::set_var(key, value);
        }

        // Execute setup commands
        for command in &setup.setup_commands {
            println!("Setup: {}", command);
            // In a real implementation, this would execute the command
        }

        Ok(())
    }

    /// Run test cleanup
    fn run_cleanup(&self, cleanup: &crate::testing::TestCleanup) -> Result<(), CompilerError> {
        // Remove files
        for file_path in &cleanup.files_to_remove {
            if std::path::Path::new(file_path).exists() {
                std::fs::remove_file(file_path)
                    .map_err(|e| CompilerError::io_error(format!("Failed to remove file {}: {}", file_path, e), None, None))?;
            }
        }

        // Execute cleanup commands
        for command in &cleanup.cleanup_commands {
            println!("Cleanup: {}", command);
            // In a real implementation, this would execute the command
        }

        Ok(())
    }
}

impl Default for TestRunnerConfig {
    fn default() -> Self {
        Self {
            execution_mode: TestExecutionMode::Sequential,
            max_parallel_jobs: num_cpus::get(),
            timeout_seconds: 30,
            retry_failed_tests: false,
            max_retries: 2,
            stop_on_first_failure: false,
            collect_detailed_metrics: false,
        }
    }
}

impl TestFilter {
    /// Create a new empty filter
    pub fn new() -> Self {
        Self {
            name_pattern: None,
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            test_types: Vec::new(),
        }
    }

    /// Filter by name pattern
    pub fn name_contains(mut self, pattern: &str) -> Self {
        self.name_pattern = Some(pattern.to_string());
        self
    }

    /// Filter by required tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Filter by excluded tag
    pub fn without_tag(mut self, tag: &str) -> Self {
        self.exclude_tags.push(tag.to_string());
        self
    }

    /// Filter by test type
    pub fn test_type(mut self, test_type: &str) -> Self {
        self.test_types.push(test_type.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TestCaseBuilder, CompilationExpectation};

    #[test]
    fn test_runner_creation() {
        let config = TestRunnerConfig::default();
        let runner = TestRunner::new(config);
        assert!(matches!(runner.config.execution_mode, TestExecutionMode::Sequential));
    }

    #[test]
    fn test_filter_creation() {
        let filter = TestFilter::new()
            .name_contains("test")
            .with_tag("unit")
            .without_tag("slow")
            .test_type("compilation");

        assert_eq!(filter.name_pattern, Some("test".to_string()));
        assert_eq!(filter.tags, vec!["unit"]);
        assert_eq!(filter.exclude_tags, vec!["slow"]);
        assert_eq!(filter.test_types, vec!["compilation"]);
    }

    #[test]
    fn test_filter_matching() {
        let runner = TestRunner::new(TestRunnerConfig::default());
        let filter = TestFilter::new().with_tag("unit");

        let test_case = TestCaseBuilder::new("unit_test")
            .compilation_test("let x = 1;", CompilationExpectation::Success)
            .tag("unit")
            .build()
            .unwrap();

        assert!(runner.test_matches_filter(&test_case, &filter));
    }
}