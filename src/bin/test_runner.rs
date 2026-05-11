//! Comprehensive test runner for Clean Language compiler
//!
//! Uses the modern 7-stage compilation pipeline.

use std::path::Path;
use std::time::Instant;

#[derive(Debug)]
struct TestResult {
    name: String,
    passed: bool,
    duration: std::time::Duration,
    errors: Vec<String>,
}

#[derive(Debug)]
struct TestSuite {
    name: String,
    tests: Vec<TestResult>,
    total_duration: std::time::Duration,
    passed_count: usize,
    failed_count: usize,
}

impl TestSuite {
    fn new(name: String) -> Self {
        Self {
            name,
            tests: Vec::new(),
            total_duration: std::time::Duration::ZERO,
            passed_count: 0,
            failed_count: 0,
        }
    }

    fn add_test(&mut self, result: TestResult) {
        if result.passed {
            self.passed_count += 1;
        } else {
            self.failed_count += 1;
        }
        self.total_duration += result.duration;
        self.tests.push(result);
    }

    fn summary(&self) -> String {
        format!(
            "{}: {}/{} tests passed in {:.2?}",
            self.name,
            self.passed_count,
            self.passed_count + self.failed_count,
            self.total_duration
        )
    }
}

fn run_compilation_tests() -> TestSuite {
    let mut suite = TestSuite::new("Compilation Tests".to_string());

    let test_cases = vec![
        ("simple_start", "start:\n\tprint(42)"),
        ("simple_function", "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b"),
        ("simple_class", "class Calculator\n\tinteger value\n\tfunctions:\n\t\tvoid setValue(integer newValue)\n\t\t\tvalue = newValue"),
    ];

    for (name, code) in test_cases {
        let start = Instant::now();
        match clean_language_compiler::compile_with_file(code, name) {
            Ok(_) => {
                suite.add_test(TestResult {
                    name: name.to_string(),
                    passed: true,
                    duration: start.elapsed(),
                    errors: Vec::new(),
                });
            }
            Err(errors) => {
                suite.add_test(TestResult {
                    name: name.to_string(),
                    passed: false,
                    duration: start.elapsed(),
                    errors: errors.iter().map(|e| e.to_string()).collect(),
                });
            }
        }
    }

    suite
}

fn run_integration_tests() -> TestSuite {
    let mut suite = TestSuite::new("Integration Tests".to_string());

    let test_files = vec![
        "tests/clean_files/00_minimal.cln",
        "tests/clean_files/01_hello_world.cln",
        "tests/clean_files/10_functions_basic.cln",
    ];

    for file_path in test_files {
        if Path::new(file_path).exists() {
            let start = Instant::now();
            let name = file_path.split('/').next_back().unwrap_or(file_path);

            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    match clean_language_compiler::compile_with_file(&content, file_path) {
                        Ok(_) => {
                            suite.add_test(TestResult {
                                name: name.to_string(),
                                passed: true,
                                duration: start.elapsed(),
                                errors: Vec::new(),
                            });
                        }
                        Err(errors) => {
                            suite.add_test(TestResult {
                                name: name.to_string(),
                                passed: false,
                                duration: start.elapsed(),
                                errors: errors.iter().map(|e| e.to_string()).collect(),
                            });
                        }
                    }
                }
                Err(e) => {
                    suite.add_test(TestResult {
                        name: name.to_string(),
                        passed: false,
                        duration: start.elapsed(),
                        errors: vec![e.to_string()],
                    });
                }
            }
        }
    }

    suite
}

fn main() {
    println!("Clean Language Compiler - Comprehensive Test Suite");
    println!("{}", "=".repeat(60));

    let start_time = Instant::now();
    let all_suites = vec![run_compilation_tests(), run_integration_tests()];

    let total_time = start_time.elapsed();

    println!("\nTest Results Summary:");
    println!("{}", "=".repeat(40));

    let mut total_passed = 0;
    let mut total_failed = 0;

    for suite in &all_suites {
        println!("{}", suite.summary());
        total_passed += suite.passed_count;
        total_failed += suite.failed_count;

        for test in &suite.tests {
            if !test.passed {
                println!("  FAIL {}: {}", test.name, test.errors.join(", "));
            }
        }
    }

    println!("\nOverall Results:");
    println!("  Passed: {total_passed}");
    println!("  Failed: {total_failed}");
    println!("  Total Time: {total_time:.2?}");

    if total_failed > 0 {
        println!("\nSome tests failed! Please review the errors above.");
        std::process::exit(1);
    } else {
        println!("\nAll tests passed successfully!");
    }
}
