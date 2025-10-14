//! Comprehensive test runner for Clean Language compiler
//!
//! NOTE: Uses deprecated SemanticAnalyzer.
//! Will be migrated to use modern pipeline before v0.11.0.

#![allow(deprecated)]

use clean_language_compiler::parser::CleanParser;
use clean_language_compiler::semantic::SemanticAnalyzer;
use std::path::Path;
use std::time::Instant;

#[derive(Debug)]
struct TestResult {
    name: String,
    passed: bool,
    duration: std::time::Duration,
    errors: Vec<String>,
    #[allow(dead_code)]
    warnings: Vec<String>,
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

fn run_parser_tests() -> TestSuite {
    let mut suite = TestSuite::new("Parser Tests".to_string());

    // Test basic parsing with correct Clean Language syntax
    let test_cases = vec![
        ("minimal_start", "start()\n\tprint(42)"),
        ("minimal_function", "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b"),
        ("minimal_class", "class Calculator\n\tinteger value\n\tfunctions:\n\t\tvoid setValue(integer newValue)\n\t\t\tvalue = newValue"),
    ];

    for (name, code) in test_cases {
        let start = Instant::now();

        match CleanParser::parse_program(code) {
            Ok(_) => {
                suite.add_test(TestResult {
                    name: name.to_string(),
                    passed: true,
                    duration: start.elapsed(),
                    errors: Vec::new(),
                    warnings: Vec::new(),
                });
            }
            Err(e) => {
                suite.add_test(TestResult {
                    name: name.to_string(),
                    passed: false,
                    duration: start.elapsed(),
                    errors: vec![e.to_string()],
                    warnings: Vec::new(),
                });
            }
        }
    }

    suite
}

fn run_semantic_tests() -> TestSuite {
    let mut suite = TestSuite::new("Semantic Analysis Tests".to_string());

    // Test type checking with correct syntax
    let test_cases = vec![
        ("valid_start", "start()\n\tprint(42)"),
        ("valid_function", "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b"),
        ("valid_class", "class Calculator\n\tinteger value\n\tfunctions:\n\t\tvoid setValue(integer newValue)\n\t\t\tvalue = newValue"),
    ];

    for (name, code) in test_cases {
        let start = Instant::now();

        // Parse first
        match CleanParser::parse_program(code) {
            Ok(ast) => {
                // Then semantic analysis
                let mut analyzer = SemanticAnalyzer::new();
                match analyzer.analyze(&ast) {
                    Ok(_) => {
                        suite.add_test(TestResult {
                            name: name.to_string(),
                            passed: true,
                            duration: start.elapsed(),
                            errors: Vec::new(),
                            warnings: Vec::new(),
                        });
                    }
                    Err(e) => {
                        let expected_failure = name.contains("invalid");
                        suite.add_test(TestResult {
                            name: name.to_string(),
                            passed: expected_failure,
                            duration: start.elapsed(),
                            errors: vec![e.to_string()],
                            warnings: Vec::new(),
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
                    warnings: Vec::new(),
                });
            }
        }
    }

    suite
}

fn run_compilation_tests() -> TestSuite {
    let mut suite = TestSuite::new("Compilation Tests".to_string());

    let test_cases = vec![
        ("simple_start", "start()\n\tprint(42)"),
        ("simple_function", "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b"),
        ("simple_class", "class Calculator\n\tinteger value\n\tfunctions:\n\t\tvoid setValue(integer newValue)\n\t\t\tvalue = newValue"),
    ];

    for (name, code) in test_cases {
        let start = Instant::now();

        // Full compilation pipeline
        match CleanParser::parse_program(code) {
            Ok(ast) => {
                let mut analyzer = SemanticAnalyzer::new();
                match analyzer.analyze(&ast) {
                    Ok(_) => {
                        // For now, just test that parsing and semantic analysis works
                        // HIR generation can be added later when the API is stable
                        suite.add_test(TestResult {
                            name: name.to_string(),
                            passed: true,
                            duration: start.elapsed(),
                            errors: Vec::new(),
                            warnings: Vec::new(),
                        });
                    }
                    Err(e) => {
                        suite.add_test(TestResult {
                            name: name.to_string(),
                            passed: false,
                            duration: start.elapsed(),
                            errors: vec![e.to_string()],
                            warnings: Vec::new(),
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
                    warnings: Vec::new(),
                });
            }
        }
    }

    suite
}

fn run_integration_tests() -> TestSuite {
    let mut suite = TestSuite::new("Integration Tests".to_string());

    // Test complete files from test directory
    let test_files = vec![
        "tests/clean_files/00_minimal.cln",
        "tests/clean_files/01_hello_world.cln",
        "tests/clean_files/10_functions_basic.cln",
    ];

    for file_path in test_files {
        if Path::new(file_path).exists() {
            let start = Instant::now();
            let name = file_path.split('/').last().unwrap_or(file_path);

            // Read and compile the file
            match std::fs::read_to_string(file_path) {
                Ok(content) => match CleanParser::parse_program(&content) {
                    Ok(_) => {
                        suite.add_test(TestResult {
                            name: name.to_string(),
                            passed: true,
                            duration: start.elapsed(),
                            errors: Vec::new(),
                            warnings: Vec::new(),
                        });
                    }
                    Err(e) => {
                        suite.add_test(TestResult {
                            name: name.to_string(),
                            passed: false,
                            duration: start.elapsed(),
                            errors: vec![e.to_string()],
                            warnings: Vec::new(),
                        });
                    }
                },
                Err(e) => {
                    suite.add_test(TestResult {
                        name: name.to_string(),
                        passed: false,
                        duration: start.elapsed(),
                        errors: vec![e.to_string()],
                        warnings: Vec::new(),
                    });
                }
            }
        }
    }

    suite
}

fn main() {
    println!("🧪 Clean Language Compiler - Comprehensive Test Suite");
    println!("{}", "=".repeat(60));

    let start_time = Instant::now();
    let mut all_suites = Vec::new();

    // Run all test suites
    all_suites.push(run_parser_tests());
    all_suites.push(run_semantic_tests());
    all_suites.push(run_compilation_tests());
    all_suites.push(run_integration_tests());

    let total_time = start_time.elapsed();

    // Print results
    println!("\n📊 Test Results Summary:");
    println!("{}", "=".repeat(40));

    let mut total_passed = 0;
    let mut total_failed = 0;

    for suite in &all_suites {
        println!("{}", suite.summary());
        total_passed += suite.passed_count;
        total_failed += suite.failed_count;

        // Print detailed results for failed tests
        for test in &suite.tests {
            if !test.passed {
                println!("  ❌ {}: {}", test.name, test.errors.join(", "));
            }
        }
    }

    println!("\n🎯 Overall Results:");
    println!("  ✅ Passed: {}", total_passed);
    println!("  ❌ Failed: {}", total_failed);
    println!("  ⏱️  Total Time: {:.2?}", total_time);

    if total_failed > 0 {
        println!("\n🚨 Some tests failed! Please review the errors above.");
        std::process::exit(1);
    } else {
        println!("\n🎉 All tests passed successfully!");
    }
}
