use crate::error::CompilerError;
use crate::testing::{TestCase, TestOutput, CompilationResult, PerformanceData};
use crate::parser::CleanParser;
use crate::semantic::SemanticAnalyzer;
use crate::codegen::CodeGenerator;
use crate::stdlib::StandardLibrary;
use std::time::{Duration, Instant};
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;

/// Test harness for executing Clean Language compiler tests
/// 
/// Provides isolated execution environment for testing:
/// - Compilation testing with various configurations
/// - WASM execution and validation
/// - Performance measurement
/// - Error condition testing
/// - Memory safety verification
#[derive(Debug)]
pub struct TestHarness {
    config: TestHarnessConfig,
    temp_directory: PathBuf,
    parser: Parser,
    semantic_analyzer: SemanticAnalyzer,
    code_generator: CodeGenerator,
    stdlib: StandardLibrary,
}

/// Configuration for the test harness
#[derive(Debug, Clone)]
pub struct TestHarnessConfig {
    pub compiler_timeout: Duration,
    pub execution_timeout: Duration,
    pub max_memory_mb: usize,
    pub temp_directory: PathBuf,
    pub keep_temp_files: bool,
    pub enable_optimizations: bool,
    pub collect_performance_data: bool,
    pub validation_mode: ValidationMode,
    pub wasm_runtime: WasmRuntime,
}

/// Validation modes for compiled WASM
#[derive(Debug, Clone)]
pub enum ValidationMode {
    /// No validation
    None,
    /// Basic WASM format validation
    Basic,
    /// Full semantic validation
    Full,
    /// Execution validation with test inputs
    Execution,
}

/// WASM runtime for testing
#[derive(Debug, Clone)]
pub enum WasmRuntime {
    /// Wasmtime runtime
    Wasmtime,
    /// Node.js with WASM support
    Node,
    /// Browser simulation
    Browser,
    /// Custom runtime
    Custom(String),
}

impl TestHarness {
    /// Create a new test harness
    pub fn new(config: TestHarnessConfig) -> Result<Self, CompilerError> {
        // Ensure temp directory exists
        fs::create_dir_all(&config.temp_directory)
            .map_err(|e| CompilerError::io_error(format!("Failed to create temp directory: {}", e), None, None))?;

        let parser = Parser::new();
        let semantic_analyzer = SemanticAnalyzer::new();
        let mut code_generator = CodeGenerator::new();
        let stdlib = StandardLibrary::new()?;

        // Configure code generator
        if config.enable_optimizations {
            code_generator.enable_optimizations();
        }

        Ok(Self {
            config,
            temp_directory: config.temp_directory.clone(),
            parser,
            semantic_analyzer,
            code_generator,
            stdlib,
        })
    }

    /// Execute a test case
    pub fn execute_test(&mut self, test_case: &TestCase) -> Result<TestOutput, CompilerError> {
        let start_time = Instant::now();
        let mut performance_data = if self.config.collect_performance_data {
            Some(PerformanceData {
                compilation_time: Duration::default(),
                memory_peak: 0,
                binary_size: 0,
                optimization_ratio: 0.0,
                custom_metrics: std::collections::HashMap::new(),
            })
        } else {
            None
        };

        match &test_case.test_type {
            crate::testing::TestType::Compilation { source, expected_result } => {
                self.execute_compilation_test(source, expected_result, &mut performance_data)
            }
            crate::testing::TestType::Execution { source, inputs, expected_output } => {
                self.execute_execution_test(source, inputs, expected_output, &mut performance_data)
            }
            crate::testing::TestType::Performance { source, performance_thresholds } => {
                self.execute_performance_test(source, performance_thresholds, &mut performance_data)
            }
            crate::testing::TestType::ErrorCondition { source, expected_error } => {
                self.execute_error_test(source, expected_error, &mut performance_data)
            }
            crate::testing::TestType::Regression { source, baseline_metrics } => {
                self.execute_regression_test(source, baseline_metrics, &mut performance_data)
            }
        }
    }

    /// Execute a compilation test
    fn execute_compilation_test(
        &mut self,
        source: &str,
        expected_result: &crate::testing::CompilationExpectation,
        performance_data: &mut Option<PerformanceData>
    ) -> Result<TestOutput, CompilerError> {
        let compilation_start = Instant::now();
        
        // Compile the source code
        let compilation_result = self.compile_source(source)?;
        
        if let Some(perf) = performance_data {
            perf.compilation_time = compilation_start.elapsed();
            if let Some(ref result) = compilation_result.compilation_result {
                if let Some(ref wasm_bytes) = result.wasm_bytes {
                    perf.binary_size = wasm_bytes.len();
                }
            }
        }

        // Check if compilation result matches expectation
        let success = match expected_result {
            crate::testing::CompilationExpectation::Success => compilation_result.compilation_result.as_ref()
                .map_or(false, |r| r.succeeded),
            crate::testing::CompilationExpectation::Failure => compilation_result.compilation_result.as_ref()
                .map_or(true, |r| !r.succeeded),
            crate::testing::CompilationExpectation::WarningCount(expected_count) => {
                compilation_result.compilation_result.as_ref()
                    .map_or(false, |r| r.warnings.len() == *expected_count)
            }
            crate::testing::CompilationExpectation::ErrorCount(expected_count) => {
                compilation_result.compilation_result.as_ref()
                    .map_or(false, |r| r.errors.len() == *expected_count)
            }
        };

        let message = if success {
            Some("Compilation test passed".to_string())
        } else {
            Some(format!("Compilation test failed: expected {:?}", expected_result))
        };

        Ok(TestOutput {
            success,
            message,
            output: None,
            compilation_result: compilation_result.compilation_result,
            performance_data: performance_data.take(),
        })
    }

    /// Execute an execution test (compile and run)
    fn execute_execution_test(
        &mut self,
        source: &str,
        inputs: &[String],
        expected_output: &str,
        performance_data: &mut Option<PerformanceData>
    ) -> Result<TestOutput, CompilerError> {
        let compilation_start = Instant::now();
        
        // First compile the source
        let compilation_result = self.compile_source(source)?;
        
        if let Some(perf) = performance_data {
            perf.compilation_time = compilation_start.elapsed();
        }

        let comp_result = compilation_result.compilation_result.ok_or_else(|| {
            CompilerError::testing_error("No compilation result available", None, None)
        })?;

        if !comp_result.succeeded {
            return Ok(TestOutput {
                success: false,
                message: Some("Compilation failed".to_string()),
                output: None,
                compilation_result: Some(comp_result),
                performance_data: performance_data.take(),
            });
        }

        let wasm_bytes = comp_result.wasm_bytes.ok_or_else(|| {
            CompilerError::testing_error("No WASM bytes generated", None, None)
        })?;

        // Execute the compiled WASM
        let execution_result = self.execute_wasm(&wasm_bytes, inputs)?;

        let success = execution_result.output.as_ref()
            .map_or(false, |output| output.trim() == expected_output.trim());

        if let Some(perf) = performance_data {
            perf.binary_size = wasm_bytes.len();
            if let Some(ref exec_metrics) = execution_result.performance_metrics {
                perf.custom_metrics.insert("execution_time_ms".to_string(), 
                                         exec_metrics.execution_time.as_millis() as f64);
            }
        }

        Ok(TestOutput {
            success,
            message: if success { 
                Some("Execution test passed".to_string()) 
            } else { 
                Some(format!("Expected: '{}', Got: '{}'", 
                           expected_output, 
                           execution_result.output.unwrap_or_default())) 
            },
            output: execution_result.output,
            compilation_result: Some(comp_result),
            performance_data: performance_data.take(),
        })
    }

    /// Execute a performance test
    fn execute_performance_test(
        &mut self,
        source: &str,
        thresholds: &crate::testing::PerformanceThresholds,
        performance_data: &mut Option<PerformanceData>
    ) -> Result<TestOutput, CompilerError> {
        let total_start = Instant::now();
        
        // Compile with performance monitoring
        let compilation_result = self.compile_source_with_monitoring(source)?;
        
        let comp_result = compilation_result.compilation_result.ok_or_else(|| {
            CompilerError::testing_error("No compilation result available", None, None)
        })?;

        if !comp_result.succeeded {
            return Ok(TestOutput {
                success: false,
                message: Some("Compilation failed in performance test".to_string()),
                output: None,
                compilation_result: Some(comp_result),
                performance_data: performance_data.take(),
            });
        }

        let wasm_bytes = comp_result.wasm_bytes.as_ref().ok_or_else(|| {
            CompilerError::testing_error("No WASM bytes generated", None, None)
        })?;

        // Check performance thresholds
        let mut success = true;
        let mut messages = Vec::new();

        // Check compilation time
        if let Some(max_compile_time) = thresholds.max_compilation_time {
            if comp_result.compilation_time > max_compile_time {
                success = false;
                messages.push(format!("Compilation time {} exceeds threshold {}", 
                                    comp_result.compilation_time.as_millis(),
                                    max_compile_time.as_millis()));
            }
        }

        // Check binary size
        if let Some(max_binary_size) = thresholds.max_binary_size {
            if wasm_bytes.len() > max_binary_size {
                success = false;
                messages.push(format!("Binary size {} exceeds threshold {}", 
                                    wasm_bytes.len(), max_binary_size));
            }
        }

        // Check memory usage (if available)
        if let Some(max_memory) = thresholds.max_memory_mb {
            let estimated_memory = self.estimate_memory_usage(wasm_bytes);
            if estimated_memory > max_memory * 1024 * 1024 {
                success = false;
                messages.push(format!("Memory usage {} exceeds threshold {}", 
                                    estimated_memory, max_memory * 1024 * 1024));
            }
        }

        if let Some(perf) = performance_data {
            perf.compilation_time = comp_result.compilation_time;
            perf.binary_size = wasm_bytes.len();
            perf.memory_peak = self.estimate_memory_usage(wasm_bytes);
        }

        let message = if success {
            Some("Performance test passed".to_string())
        } else {
            Some(messages.join("; "))
        };

        Ok(TestOutput {
            success,
            message,
            output: None,
            compilation_result: Some(comp_result),
            performance_data: performance_data.take(),
        })
    }

    /// Execute an error condition test
    fn execute_error_test(
        &mut self,
        source: &str,
        expected_error: &crate::testing::ErrorExpectation,
        performance_data: &mut Option<PerformanceData>
    ) -> Result<TestOutput, CompilerError> {
        let compilation_start = Instant::now();
        
        // Try to compile the source (expecting it to fail)
        let compilation_result = self.compile_source(source)?;
        
        if let Some(perf) = performance_data {
            perf.compilation_time = compilation_start.elapsed();
        }

        let comp_result = compilation_result.compilation_result.ok_or_else(|| {
            CompilerError::testing_error("No compilation result available", None, None)
        })?;

        // Check if we got the expected error
        let success = match expected_error {
            crate::testing::ErrorExpectation::CompilationFailure => !comp_result.succeeded,
            crate::testing::ErrorExpectation::SpecificError(error_pattern) => {
                comp_result.errors.iter().any(|err| {
                    err.to_string().contains(error_pattern)
                })
            }
            crate::testing::ErrorExpectation::ErrorCategory(category) => {
                comp_result.errors.iter().any(|err| {
                    self.error_matches_category(err, category)
                })
            }
        };

        let message = if success {
            Some("Error test passed".to_string())
        } else {
            Some(format!("Expected error {:?} but got: {:?}", 
                        expected_error, 
                        comp_result.errors))
        };

        Ok(TestOutput {
            success,
            message,
            output: None,
            compilation_result: Some(comp_result),
            performance_data: performance_data.take(),
        })
    }

    /// Execute a regression test
    fn execute_regression_test(
        &mut self,
        source: &str,
        baseline_metrics: &crate::testing::BaselineMetrics,
        performance_data: &mut Option<PerformanceData>
    ) -> Result<TestOutput, CompilerError> {
        let compilation_start = Instant::now();
        
        // Compile and measure current metrics
        let compilation_result = self.compile_source_with_monitoring(source)?;
        
        if let Some(perf) = performance_data {
            perf.compilation_time = compilation_start.elapsed();
        }

        let comp_result = compilation_result.compilation_result.ok_or_else(|| {
            CompilerError::testing_error("No compilation result available", None, None)
        })?;

        if !comp_result.succeeded {
            return Ok(TestOutput {
                success: false,
                message: Some("Compilation failed in regression test".to_string()),
                output: None,
                compilation_result: Some(comp_result),
                performance_data: performance_data.take(),
            });
        }

        // Compare against baseline metrics
        let mut success = true;
        let mut messages = Vec::new();

        // Check compilation time regression
        let time_regression = comp_result.compilation_time.as_millis() as f64 / 
                             baseline_metrics.compilation_time_ms as f64;
        if time_regression > baseline_metrics.allowed_regression_factor {
            success = false;
            messages.push(format!("Compilation time regression: {:.2}x vs baseline", time_regression));
        }

        // Check binary size regression
        if let Some(ref wasm_bytes) = comp_result.wasm_bytes {
            let size_regression = wasm_bytes.len() as f64 / baseline_metrics.binary_size as f64;
            if size_regression > baseline_metrics.allowed_regression_factor {
                success = false;
                messages.push(format!("Binary size regression: {:.2}x vs baseline", size_regression));
            }

            if let Some(perf) = performance_data {
                perf.binary_size = wasm_bytes.len();
                perf.optimization_ratio = 1.0 / size_regression;
            }
        }

        let message = if success {
            Some("Regression test passed".to_string())
        } else {
            Some(messages.join("; "))
        };

        Ok(TestOutput {
            success,
            message,
            output: None,
            compilation_result: Some(comp_result),
            performance_data: performance_data.take(),
        })
    }

    /// Compile source code
    fn compile_source(&mut self, source: &str) -> Result<CompilationTestResult, CompilerError> {
        let start_time = Instant::now();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Parse
        let ast = match self.parser.parse(source) {
            Ok(ast) => ast,
            Err(e) => {
                errors.push(e);
                return Ok(CompilationTestResult {
                    compilation_result: Some(CompilationResult {
                        succeeded: false,
                        wasm_bytes: None,
                        errors,
                        warnings,
                        compilation_time: start_time.elapsed(),
                        optimization_metrics: None,
                    }),
                });
            }
        };

        // Semantic analysis
        let analyzed_ast = match self.semantic_analyzer.analyze(ast) {
            Ok(ast) => ast,
            Err(e) => {
                errors.push(e);
                return Ok(CompilationTestResult {
                    compilation_result: Some(CompilationResult {
                        succeeded: false,
                        wasm_bytes: None,
                        errors,
                        warnings,
                        compilation_time: start_time.elapsed(),
                        optimization_metrics: None,
                    }),
                });
            }
        };

        // Code generation
        let wasm_bytes = match self.code_generator.generate(&analyzed_ast, &self.stdlib) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                errors.push(e);
                None
            }
        };

        let succeeded = errors.is_empty();
        let compilation_time = start_time.elapsed();

        Ok(CompilationTestResult {
            compilation_result: Some(CompilationResult {
                succeeded,
                wasm_bytes,
                errors,
                warnings,
                compilation_time,
                optimization_metrics: None, // Would need to integrate with optimization pipeline
            }),
        })
    }

    /// Compile source with detailed monitoring
    fn compile_source_with_monitoring(&mut self, source: &str) -> Result<CompilationTestResult, CompilerError> {
        // For now, same as regular compilation
        // In a full implementation, this would include memory monitoring, CPU profiling, etc.
        self.compile_source(source)
    }

    /// Execute WASM bytecode
    fn execute_wasm(&self, wasm_bytes: &[u8], inputs: &[String]) -> Result<ExecutionResult, CompilerError> {
        match self.config.wasm_runtime {
            WasmRuntime::Wasmtime => self.execute_with_wasmtime(wasm_bytes, inputs),
            WasmRuntime::Node => self.execute_with_node(wasm_bytes, inputs),
            WasmRuntime::Browser => self.execute_with_browser_sim(wasm_bytes, inputs),
            WasmRuntime::Custom(ref runtime) => self.execute_with_custom(runtime, wasm_bytes, inputs),
        }
    }

    /// Execute with Wasmtime
    fn execute_with_wasmtime(&self, wasm_bytes: &[u8], _inputs: &[String]) -> Result<ExecutionResult, CompilerError> {
        // Write WASM to temp file
        let temp_file = self.temp_directory.join("test.wasm");
        fs::write(&temp_file, wasm_bytes)
            .map_err(|e| CompilerError::io_error(format!("Failed to write WASM file: {}", e), None, None))?;

        // Execute with wasmtime
        let start_time = Instant::now();
        let output = Command::new("wasmtime")
            .arg(&temp_file)
            .output()
            .map_err(|e| CompilerError::execution_error(format!("Failed to execute wasmtime: {}", e), None, None))?;

        let execution_time = start_time.elapsed();

        // Clean up if configured
        if !self.config.keep_temp_files {
            let _ = fs::remove_file(&temp_file);
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ExecutionResult {
            success: output.status.success(),
            output: Some(stdout),
            error: if stderr.is_empty() { None } else { Some(stderr) },
            exit_code: output.status.code(),
            performance_metrics: Some(ExecutionMetrics {
                execution_time,
                memory_usage: 0, // Would need system monitoring
            }),
        })
    }

    /// Execute with Node.js
    fn execute_with_node(&self, _wasm_bytes: &[u8], _inputs: &[String]) -> Result<ExecutionResult, CompilerError> {
        // Placeholder implementation
        Err(CompilerError::testing_error("Node.js execution not implemented", None, None))
    }

    /// Execute with browser simulation
    fn execute_with_browser_sim(&self, _wasm_bytes: &[u8], _inputs: &[String]) -> Result<ExecutionResult, CompilerError> {
        // Placeholder implementation
        Err(CompilerError::testing_error("Browser simulation not implemented", None, None))
    }

    /// Execute with custom runtime
    fn execute_with_custom(&self, _runtime: &str, _wasm_bytes: &[u8], _inputs: &[String]) -> Result<ExecutionResult, CompilerError> {
        // Placeholder implementation
        Err(CompilerError::testing_error("Custom runtime execution not implemented", None, None))
    }

    /// Check if error matches category
    fn error_matches_category(&self, _error: &CompilerError, _category: &crate::testing::ErrorCategory) -> bool {
        // Placeholder implementation
        false
    }

    /// Estimate memory usage from WASM binary
    fn estimate_memory_usage(&self, wasm_bytes: &[u8]) -> usize {
        // Very rough estimation based on binary size
        // A real implementation would analyze the WASM binary structure
        wasm_bytes.len() * 2
    }
}

impl From<&crate::testing::TestFrameworkConfig> for TestHarnessConfig {
    fn from(config: &crate::testing::TestFrameworkConfig) -> Self {
        Self {
            compiler_timeout: Duration::from_secs(config.timeout_seconds),
            execution_timeout: Duration::from_secs(config.timeout_seconds),
            max_memory_mb: 512,
            temp_directory: config.output_directory.join("temp"),
            keep_temp_files: false,
            enable_optimizations: true,
            collect_performance_data: config.benchmark_mode,
            validation_mode: ValidationMode::Basic,
            wasm_runtime: WasmRuntime::Wasmtime,
        }
    }
}

/// Result of compilation testing
#[derive(Debug)]
struct CompilationTestResult {
    compilation_result: Option<CompilationResult>,
}

/// Result of WASM execution
#[derive(Debug)]
struct ExecutionResult {
    success: bool,
    output: Option<String>,
    error: Option<String>,
    exit_code: Option<i32>,
    performance_metrics: Option<ExecutionMetrics>,
}

/// Execution performance metrics
#[derive(Debug)]
struct ExecutionMetrics {
    execution_time: Duration,
    memory_usage: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_harness_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = TestHarnessConfig {
            temp_directory: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let harness = TestHarness::new(config).unwrap();
        assert!(harness.temp_directory.exists());
    }

    impl Default for TestHarnessConfig {
        fn default() -> Self {
            Self {
                compiler_timeout: Duration::from_secs(30),
                execution_timeout: Duration::from_secs(10),
                max_memory_mb: 512,
                temp_directory: PathBuf::from("target/test-temp"),
                keep_temp_files: false,
                enable_optimizations: false,
                collect_performance_data: false,
                validation_mode: ValidationMode::Basic,
                wasm_runtime: WasmRuntime::Wasmtime,
            }
        }
    }
}