use crate::error::CompilerError;
use crate::testing::{TestResults, TestSuiteResult, TestCaseResult, TestStatus, PerformanceMetrics};
use std::path::Path;
use std::fs;
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// Test reporter for generating various report formats
pub struct TestReporter {
    format: TestFormat,
}

/// Available report formats
#[derive(Debug, Clone)]
pub enum TestFormat {
    /// Human-readable console output
    Console,
    /// JSON format for tool integration
    Json,
    /// JUnit XML format for CI/CD integration
    JUnit,
    /// HTML format for web viewing
    Html,
    /// TAP (Test Anything Protocol) format
    Tap,
    /// Custom format with template
    Custom(String),
}

/// Complete test report structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub summary: TestSummary,
    pub suite_reports: Vec<SuiteReport>,
    pub performance_analysis: PerformanceAnalysis,
    pub failure_analysis: FailureAnalysis,
    pub coverage_report: Option<CoverageReport>,
    pub timestamp: String,
    pub duration: Duration,
    pub environment: EnvironmentInfo,
}

/// High-level test summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub success_rate: f64,
    pub total_time: Duration,
    pub fastest_test: Option<String>,
    pub slowest_test: Option<String>,
}

/// Individual suite report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    pub name: String,
    pub summary: TestSummary,
    pub test_details: Vec<TestDetail>,
    pub performance_metrics: Option<SuitePerformanceMetrics>,
}

/// Detailed test information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDetail {
    pub name: String,
    pub status: String,
    pub duration: Duration,
    pub message: Option<String>,
    pub output: Option<String>,
    pub error_details: Option<String>,
    pub performance_data: Option<TestPerformanceData>,
    pub tags: Vec<String>,
}

/// Performance analysis across all tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAnalysis {
    pub compilation_stats: CompilationStats,
    pub execution_stats: ExecutionStats,
    pub memory_stats: MemoryStats,
    pub size_stats: SizeStats,
    pub trends: Vec<PerformanceTrend>,
}

/// Compilation performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationStats {
    pub average_time: Duration,
    pub median_time: Duration,
    pub min_time: Duration,
    pub max_time: Duration,
    pub percentiles: std::collections::HashMap<String, Duration>,
}

/// Execution performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStats {
    pub average_time: Duration,
    pub median_time: Duration,
    pub throughput_ops_per_sec: f64,
}

/// Memory usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub average_peak_mb: f64,
    pub max_peak_mb: usize,
    pub total_allocated_mb: usize,
}

/// Binary size statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeStats {
    pub average_size: usize,
    pub min_size: usize,
    pub max_size: usize,
    pub total_size: usize,
    pub compression_ratios: Vec<f64>,
}

/// Performance trend data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceTrend {
    pub metric_name: String,
    pub values: Vec<f64>,
    pub trend_direction: TrendDirection,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrendDirection {
    Improving,
    Degrading,
    Stable,
    Unknown,
}

/// Failure analysis report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAnalysis {
    pub failure_categories: std::collections::HashMap<String, usize>,
    pub common_failure_patterns: Vec<FailurePattern>,
    pub error_distribution: std::collections::HashMap<String, usize>,
    pub flaky_tests: Vec<String>,
}

/// Common failure pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub pattern: String,
    pub occurrences: usize,
    pub affected_tests: Vec<String>,
    pub suggested_fix: Option<String>,
}

/// Code coverage report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub line_coverage: f64,
    pub branch_coverage: f64,
    pub function_coverage: f64,
    pub uncovered_lines: Vec<UncoveredLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UncoveredLine {
    pub file: String,
    pub line_number: usize,
    pub content: String,
}

/// Test environment information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub os: String,
    pub architecture: String,
    pub rust_version: String,
    pub compiler_version: String,
    pub cpu_cores: usize,
    pub memory_gb: f64,
    pub environment_variables: std::collections::HashMap<String, String>,
}

/// Suite-specific performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuitePerformanceMetrics {
    pub total_compilation_time: Duration,
    pub total_execution_time: Duration,
    pub memory_usage: usize,
    pub cache_hit_rate: f64,
}

/// Individual test performance data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPerformanceData {
    pub compilation_time: Duration,
    pub execution_time: Duration,
    pub memory_peak: usize,
    pub binary_size: usize,
    pub custom_metrics: std::collections::HashMap<String, f64>,
}

impl TestReporter {
    /// Create a new test reporter
    pub fn new(format: TestFormat) -> Self {
        Self { format }
    }

    /// Generate a report from test results
    pub fn generate_report(&self, report: &TestReport, output_dir: &Path) -> Result<(), CompilerError> {
        // Ensure output directory exists
        fs::create_dir_all(output_dir)
            .map_err(|e| CompilerError::io_error(format!("Failed to create output directory: {}", e), None, None))?;

        match self.format {
            TestFormat::Console => self.generate_console_report(report),
            TestFormat::Json => self.generate_json_report(report, output_dir),
            TestFormat::JUnit => self.generate_junit_report(report, output_dir),
            TestFormat::Html => self.generate_html_report(report, output_dir),
            TestFormat::Tap => self.generate_tap_report(report, output_dir),
            TestFormat::Custom(ref template) => self.generate_custom_report(report, output_dir, template),
        }
    }

    /// Generate console report
    fn generate_console_report(&self, report: &TestReport) -> Result<(), CompilerError> {
        println!("\n🏁 Clean Language Compiler Test Report");
        println!("=====================================");
        println!("Generated at: {}", report.timestamp);
        println!("Total duration: {:?}", report.duration);
        
        self.print_summary(&report.summary);
        self.print_performance_analysis(&report.performance_analysis);
        self.print_failure_analysis(&report.failure_analysis);
        
        if report.summary.failed > 0 || report.summary.errors > 0 {
            self.print_failed_tests(report);
        }

        Ok(())
    }

    /// Generate JSON report
    fn generate_json_report(&self, report: &TestReport, output_dir: &Path) -> Result<(), CompilerError> {
        let json = serde_json::to_string_pretty(report)
            .map_err(|e| CompilerError::testing_error(format!("Failed to serialize report: {}", e), None, None))?;

        let output_path = output_dir.join("test-results.json");
        fs::write(&output_path, json)
            .map_err(|e| CompilerError::io_error(format!("Failed to write JSON report: {}", e), None, None))?;

        println!("JSON report generated: {}", output_path.display());
        Ok(())
    }

    /// Generate JUnit XML report
    fn generate_junit_report(&self, report: &TestReport, output_dir: &Path) -> Result<(), CompilerError> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuites tests=\"{}\" failures=\"{}\" errors=\"{}\" time=\"{:.3}\">\n",
            report.summary.total_tests,
            report.summary.failed,
            report.summary.errors,
            report.duration.as_secs_f64()
        ));

        for suite in &report.suite_reports {
            xml.push_str(&format!(
                "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" time=\"{:.3}\">\n",
                suite.name,
                suite.summary.total_tests,
                suite.summary.failed,
                suite.summary.errors,
                suite.summary.total_time.as_secs_f64()
            ));

            for test in &suite.test_details {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
                    test.name, suite.name, test.duration.as_secs_f64()
                ));

                match test.status.as_str() {
                    "Failed" => {
                        xml.push_str(">\n");
                        xml.push_str(&format!(
                            "      <failure message=\"{}\">{}</failure>\n",
                            test.message.as_deref().unwrap_or("Test failed"),
                            test.output.as_deref().unwrap_or("")
                        ));
                        xml.push_str("    </testcase>\n");
                    }
                    "Error" => {
                        xml.push_str(">\n");
                        xml.push_str(&format!(
                            "      <error message=\"{}\">{}</error>\n",
                            test.message.as_deref().unwrap_or("Test error"),
                            test.error_details.as_deref().unwrap_or("")
                        ));
                        xml.push_str("    </testcase>\n");
                    }
                    "Skipped" => {
                        xml.push_str(">\n");
                        xml.push_str("      <skipped/>\n");
                        xml.push_str("    </testcase>\n");
                    }
                    _ => {
                        xml.push_str("/>\n");
                    }
                }
            }

            xml.push_str("  </testsuite>\n");
        }

        xml.push_str("</testsuites>\n");

        let output_path = output_dir.join("junit-results.xml");
        fs::write(&output_path, xml)
            .map_err(|e| CompilerError::io_error(format!("Failed to write JUnit report: {}", e), None, None))?;

        println!("JUnit XML report generated: {}", output_path.display());
        Ok(())
    }

    /// Generate HTML report
    fn generate_html_report(&self, report: &TestReport, output_dir: &Path) -> Result<(), CompilerError> {
        let html = self.generate_html_content(report)?;
        
        let output_path = output_dir.join("test-report.html");
        fs::write(&output_path, html)
            .map_err(|e| CompilerError::io_error(format!("Failed to write HTML report: {}", e), None, None))?;

        println!("HTML report generated: {}", output_path.display());
        Ok(())
    }

    /// Generate TAP report
    fn generate_tap_report(&self, report: &TestReport, output_dir: &Path) -> Result<(), CompilerError> {
        let mut tap = String::new();
        tap.push_str(&format!("1..{}\n", report.summary.total_tests));

        let mut test_count = 0;
        for suite in &report.suite_reports {
            for test in &suite.test_details {
                test_count += 1;
                match test.status.as_str() {
                    "Passed" => {
                        tap.push_str(&format!("ok {} - {}\n", test_count, test.name));
                    }
                    "Failed" => {
                        tap.push_str(&format!("not ok {} - {}\n", test_count, test.name));
                        if let Some(ref message) = test.message {
                            tap.push_str(&format!("  # FAILED: {}\n", message));
                        }
                    }
                    "Error" => {
                        tap.push_str(&format!("not ok {} - {}\n", test_count, test.name));
                        if let Some(ref error) = test.error_details {
                            tap.push_str(&format!("  # ERROR: {}\n", error));
                        }
                    }
                    "Skipped" => {
                        tap.push_str(&format!("ok {} - {} # SKIP\n", test_count, test.name));
                    }
                    _ => {
                        tap.push_str(&format!("not ok {} - {} # UNKNOWN\n", test_count, test.name));
                    }
                }
            }
        }

        let output_path = output_dir.join("test-results.tap");
        fs::write(&output_path, tap)
            .map_err(|e| CompilerError::io_error(format!("Failed to write TAP report: {}", e), None, None))?;

        println!("TAP report generated: {}", output_path.display());
        Ok(())
    }

    /// Generate custom report using template
    fn generate_custom_report(&self, _report: &TestReport, _output_dir: &Path, _template: &str) -> Result<(), CompilerError> {
        // Placeholder for custom template processing
        Err(CompilerError::testing_error("Custom report format not implemented", None, None))
    }

    /// Generate HTML content
    fn generate_html_content(&self, report: &TestReport) -> Result<String, CompilerError> {
        let mut html = String::new();
        
        html.push_str("<!DOCTYPE html>\n<html>\n<head>\n");
        html.push_str("<title>Clean Language Compiler Test Report</title>\n");
        html.push_str("<style>\n");
        html.push_str(CSS_CONTENT);
        html.push_str("</style>\n");
        html.push_str("</head>\n<body>\n");
        
        html.push_str("<h1>Clean Language Compiler Test Report</h1>\n");
        html.push_str(&format!("<p>Generated: {}</p>\n", report.timestamp));
        html.push_str(&format!("<p>Duration: {:?}</p>\n", report.duration));
        
        // Summary section
        html.push_str("<div class='summary'>\n");
        html.push_str("<h2>Summary</h2>\n");
        html.push_str(&format!("<p>Total Tests: {}</p>\n", report.summary.total_tests));
        html.push_str(&format!("<p class='passed'>Passed: {}</p>\n", report.summary.passed));
        html.push_str(&format!("<p class='failed'>Failed: {}</p>\n", report.summary.failed));
        html.push_str(&format!("<p class='skipped'>Skipped: {}</p>\n", report.summary.skipped));
        html.push_str(&format!("<p class='errors'>Errors: {}</p>\n", report.summary.errors));
        html.push_str(&format!("<p>Success Rate: {:.1}%</p>\n", report.summary.success_rate));
        html.push_str("</div>\n");
        
        // Suite details
        html.push_str("<div class='suites'>\n");
        html.push_str("<h2>Test Suites</h2>\n");
        
        for suite in &report.suite_reports {
            html.push_str("<div class='suite'>\n");
            html.push_str(&format!("<h3>{}</h3>\n", suite.name));
            html.push_str(&format!("<p>Tests: {} | Passed: {} | Failed: {} | Errors: {} | Skipped: {}</p>\n",
                          suite.summary.total_tests, suite.summary.passed, suite.summary.failed, 
                          suite.summary.errors, suite.summary.skipped));
            
            html.push_str("<table class='test-table'>\n");
            html.push_str("<tr><th>Test</th><th>Status</th><th>Duration</th><th>Message</th></tr>\n");
            
            for test in &suite.test_details {
                let status_class = test.status.to_lowercase();
                html.push_str(&format!(
                    "<tr class='{}'><td>{}</td><td>{}</td><td>{:.3}s</td><td>{}</td></tr>\n",
                    status_class,
                    test.name,
                    test.status,
                    test.duration.as_secs_f64(),
                    test.message.as_deref().unwrap_or("")
                ));
            }
            
            html.push_str("</table>\n");
            html.push_str("</div>\n");
        }
        
        html.push_str("</div>\n");
        html.push_str("</body>\n</html>");
        
        Ok(html)
    }

    /// Print summary to console
    fn print_summary(&self, summary: &TestSummary) {
        println!("\n📊 Test Summary");
        println!("  Total Tests: {}", summary.total_tests);
        println!("  ✅ Passed: {} ({:.1}%)", summary.passed, 
                 if summary.total_tests > 0 { (summary.passed as f64 / summary.total_tests as f64) * 100.0 } else { 0.0 });
        println!("  ❌ Failed: {} ({:.1}%)", summary.failed,
                 if summary.total_tests > 0 { (summary.failed as f64 / summary.total_tests as f64) * 100.0 } else { 0.0 });
        println!("  ⏭️ Skipped: {} ({:.1}%)", summary.skipped,
                 if summary.total_tests > 0 { (summary.skipped as f64 / summary.total_tests as f64) * 100.0 } else { 0.0 });
        println!("  💥 Errors: {} ({:.1}%)", summary.errors,
                 if summary.total_tests > 0 { (summary.errors as f64 / summary.total_tests as f64) * 100.0 } else { 0.0 });
        println!("  🎯 Success Rate: {:.1}%", summary.success_rate);
        println!("  ⏱️ Total Time: {:?}", summary.total_time);
        
        if let Some(ref fastest) = summary.fastest_test {
            println!("  🏃 Fastest Test: {}", fastest);
        }
        if let Some(ref slowest) = summary.slowest_test {
            println!("  🐌 Slowest Test: {}", slowest);
        }
    }

    /// Print performance analysis
    fn print_performance_analysis(&self, analysis: &PerformanceAnalysis) {
        println!("\n⚡ Performance Analysis");
        println!("  Compilation:");
        println!("    Average: {:?}", analysis.compilation_stats.average_time);
        println!("    Median: {:?}", analysis.compilation_stats.median_time);
        println!("    Range: {:?} - {:?}", analysis.compilation_stats.min_time, analysis.compilation_stats.max_time);
        
        println!("  Memory:");
        println!("    Average Peak: {:.1} MB", analysis.memory_stats.average_peak_mb);
        println!("    Max Peak: {} MB", analysis.memory_stats.max_peak_mb);
        
        println!("  Binary Sizes:");
        println!("    Average: {} bytes", analysis.size_stats.average_size);
        println!("    Range: {} - {} bytes", analysis.size_stats.min_size, analysis.size_stats.max_size);
    }

    /// Print failure analysis
    fn print_failure_analysis(&self, analysis: &FailureAnalysis) {
        if analysis.failure_categories.is_empty() {
            return;
        }

        println!("\n🔍 Failure Analysis");
        println!("  Failure Categories:");
        
        let mut categories: Vec<_> = analysis.failure_categories.iter().collect();
        categories.sort_by(|a, b| b.1.cmp(a.1));
        
        for (category, count) in categories {
            println!("    {}: {} occurrences", category, count);
        }

        if !analysis.common_failure_patterns.is_empty() {
            println!("  Common Patterns:");
            for pattern in &analysis.common_failure_patterns {
                println!("    {} ({} occurrences)", pattern.pattern, pattern.occurrences);
                if let Some(ref fix) = pattern.suggested_fix {
                    println!("      Suggested fix: {}", fix);
                }
            }
        }

        if !analysis.flaky_tests.is_empty() {
            println!("  Flaky Tests:");
            for test in &analysis.flaky_tests {
                println!("    {}", test);
            }
        }
    }

    /// Print failed tests
    fn print_failed_tests(&self, report: &TestReport) {
        println!("\n❌ Failed Tests");
        
        for suite in &report.suite_reports {
            let failed_tests: Vec<_> = suite.test_details.iter()
                .filter(|test| matches!(test.status.as_str(), "Failed" | "Error"))
                .collect();
                
            if !failed_tests.is_empty() {
                println!("  Suite: {}", suite.name);
                
                for test in failed_tests {
                    println!("    {} - {}", test.status, test.name);
                    if let Some(ref message) = test.message {
                        println!("      Message: {}", message);
                    }
                    if let Some(ref output) = test.output {
                        println!("      Output: {}", output.trim());
                    }
                }
            }
        }
    }
}

impl TestReport {
    /// Create a test report from results
    pub fn from_results(results: &TestResults) -> Self {
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let environment = EnvironmentInfo::collect();
        
        // Convert suite results
        let suite_reports: Vec<SuiteReport> = results.suite_results.iter()
            .map(|(name, suite_result)| {
                Self::create_suite_report(name.clone(), suite_result)
            })
            .collect();

        // Create summary
        let summary = TestSummary {
            total_tests: results.total_tests,
            passed: results.passed,
            failed: results.failed,
            skipped: results.skipped,
            errors: results.errors,
            success_rate: if results.total_tests > 0 {
                (results.passed as f64 / results.total_tests as f64) * 100.0
            } else {
                0.0
            },
            total_time: results.execution_time,
            fastest_test: None, // Would need to compute from suite results
            slowest_test: None,  // Would need to compute from suite results
        };

        // Create performance analysis
        let performance_analysis = Self::create_performance_analysis(&results.performance_metrics);
        
        // Create failure analysis
        let failure_analysis = Self::create_failure_analysis(&suite_reports);

        Self {
            summary,
            suite_reports,
            performance_analysis,
            failure_analysis,
            coverage_report: None,
            timestamp,
            duration: results.execution_time,
            environment,
        }
    }

    fn create_suite_report(name: String, suite_result: &TestSuiteResult) -> SuiteReport {
        let summary = TestSummary {
            total_tests: suite_result.total_tests,
            passed: suite_result.passed,
            failed: suite_result.failed,
            skipped: suite_result.skipped,
            errors: suite_result.errors,
            success_rate: if suite_result.total_tests > 0 {
                (suite_result.passed as f64 / suite_result.total_tests as f64) * 100.0
            } else {
                0.0
            },
            total_time: suite_result.execution_time,
            fastest_test: None,
            slowest_test: None,
        };

        let test_details: Vec<TestDetail> = suite_result.test_results.iter()
            .map(|test_result| {
                TestDetail {
                    name: test_result.name.clone(),
                    status: format!("{:?}", test_result.status),
                    duration: test_result.execution_time,
                    message: test_result.message.clone(),
                    output: test_result.output.clone(),
                    error_details: test_result.error_details.as_ref().map(|e| e.to_string()),
                    performance_data: test_result.performance_data.as_ref().map(|perf| {
                        TestPerformanceData {
                            compilation_time: perf.compilation_time,
                            execution_time: Duration::default(), // Would need to be tracked
                            memory_peak: perf.memory_peak,
                            binary_size: perf.binary_size,
                            custom_metrics: perf.custom_metrics.clone(),
                        }
                    }),
                    tags: Vec::new(), // Would need to be passed from test case
                }
            })
            .collect();

        SuiteReport {
            name,
            summary,
            test_details,
            performance_metrics: None,
        }
    }

    fn create_performance_analysis(metrics: &PerformanceMetrics) -> PerformanceAnalysis {
        let compilation_stats = if !metrics.compilation_times.is_empty() {
            let mut times = metrics.compilation_times.clone();
            times.sort();
            
            let sum: Duration = times.iter().sum();
            let average = sum / times.len() as u32;
            let median = times[times.len() / 2];
            let min = times[0];
            let max = times[times.len() - 1];

            CompilationStats {
                average_time: average,
                median_time: median,
                min_time: min,
                max_time: max,
                percentiles: std::collections::HashMap::new(),
            }
        } else {
            CompilationStats {
                average_time: Duration::default(),
                median_time: Duration::default(),
                min_time: Duration::default(),
                max_time: Duration::default(),
                percentiles: std::collections::HashMap::new(),
            }
        };

        let memory_stats = MemoryStats {
            average_peak_mb: if !metrics.memory_usage.is_empty() {
                metrics.memory_usage.iter().sum::<usize>() as f64 / metrics.memory_usage.len() as f64 / 1024.0 / 1024.0
            } else {
                0.0
            },
            max_peak_mb: metrics.memory_usage.iter().max().copied().unwrap_or(0) / 1024 / 1024,
            total_allocated_mb: metrics.memory_usage.iter().sum::<usize>() / 1024 / 1024,
        };

        let size_stats = SizeStats {
            average_size: if !metrics.binary_sizes.is_empty() {
                metrics.binary_sizes.iter().sum::<usize>() / metrics.binary_sizes.len()
            } else {
                0
            },
            min_size: metrics.binary_sizes.iter().min().copied().unwrap_or(0),
            max_size: metrics.binary_sizes.iter().max().copied().unwrap_or(0),
            total_size: metrics.binary_sizes.iter().sum(),
            compression_ratios: Vec::new(),
        };

        PerformanceAnalysis {
            compilation_stats,
            execution_stats: ExecutionStats {
                average_time: Duration::default(),
                median_time: Duration::default(),
                throughput_ops_per_sec: 0.0,
            },
            memory_stats,
            size_stats,
            trends: Vec::new(),
        }
    }

    fn create_failure_analysis(suite_reports: &[SuiteReport]) -> FailureAnalysis {
        let mut failure_categories = std::collections::HashMap::new();
        let mut flaky_tests = Vec::new();

        for suite in suite_reports {
            for test in &suite.test_details {
                match test.status.as_str() {
                    "Failed" => {
                        *failure_categories.entry("Test Failure".to_string()).or_insert(0) += 1;
                    }
                    "Error" => {
                        *failure_categories.entry("Test Error".to_string()).or_insert(0) += 1;
                    }
                    _ => {}
                }
            }
        }

        FailureAnalysis {
            failure_categories,
            common_failure_patterns: Vec::new(),
            error_distribution: std::collections::HashMap::new(),
            flaky_tests,
        }
    }
}

impl EnvironmentInfo {
    fn collect() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            rust_version: std::env::var("RUSTC_VERSION").unwrap_or_else(|_| "unknown".to_string()),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            cpu_cores: num_cpus::get(),
            memory_gb: 8.0, // Would need system detection
            environment_variables: std::env::vars().collect(),
        }
    }
}

// Default CSS content for HTML reports
const CSS_CONTENT: &str = r#"
body { font-family: Arial, sans-serif; margin: 20px; }
.summary { background: #f5f5f5; padding: 15px; border-radius: 5px; margin-bottom: 20px; }
.suite { margin-bottom: 30px; }
.test-table { width: 100%; border-collapse: collapse; }
.test-table th, .test-table td { border: 1px solid #ddd; padding: 8px; text-align: left; }
.test-table th { background-color: #f2f2f2; }
.passed { color: green; }
.failed { color: red; }
.error { color: orange; }
.skipped { color: blue; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_reporter_creation() {
        let reporter = TestReporter::new(TestFormat::Console);
        assert!(matches!(reporter.format, TestFormat::Console));
    }

    #[test]
    fn test_environment_info() {
        let env = EnvironmentInfo::collect();
        assert!(!env.os.is_empty());
        assert!(!env.architecture.is_empty());
        assert!(env.cpu_cores > 0);
    }

    #[test]
    fn test_json_report_generation() {
        let reporter = TestReporter::new(TestFormat::Json);
        let temp_dir = TempDir::new().unwrap();
        
        let report = create_mock_report();
        let result = reporter.generate_json_report(&report, temp_dir.path());
        
        assert!(result.is_ok());
        assert!(temp_dir.path().join("test-results.json").exists());
    }

    fn create_mock_report() -> TestReport {
        TestReport {
            summary: TestSummary {
                total_tests: 5,
                passed: 4,
                failed: 1,
                skipped: 0,
                errors: 0,
                success_rate: 80.0,
                total_time: Duration::from_secs(10),
                fastest_test: Some("fast_test".to_string()),
                slowest_test: Some("slow_test".to_string()),
            },
            suite_reports: Vec::new(),
            performance_analysis: PerformanceAnalysis {
                compilation_stats: CompilationStats {
                    average_time: Duration::from_millis(500),
                    median_time: Duration::from_millis(450),
                    min_time: Duration::from_millis(100),
                    max_time: Duration::from_millis(1000),
                    percentiles: std::collections::HashMap::new(),
                },
                execution_stats: ExecutionStats {
                    average_time: Duration::from_millis(100),
                    median_time: Duration::from_millis(90),
                    throughput_ops_per_sec: 1000.0,
                },
                memory_stats: MemoryStats {
                    average_peak_mb: 50.0,
                    max_peak_mb: 100,
                    total_allocated_mb: 500,
                },
                size_stats: SizeStats {
                    average_size: 1024,
                    min_size: 512,
                    max_size: 2048,
                    total_size: 5120,
                    compression_ratios: Vec::new(),
                },
                trends: Vec::new(),
            },
            failure_analysis: FailureAnalysis {
                failure_categories: std::collections::HashMap::new(),
                common_failure_patterns: Vec::new(),
                error_distribution: std::collections::HashMap::new(),
                flaky_tests: Vec::new(),
            },
            coverage_report: None,
            timestamp: "2023-01-01 00:00:00 UTC".to_string(),
            duration: Duration::from_secs(10),
            environment: EnvironmentInfo::collect(),
        }
    }
}