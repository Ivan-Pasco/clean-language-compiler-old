//! Performance benchmarking tool for Clean Language Compiler
//!
//! Measures compilation performance and detects regressions

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct BenchmarkResult {
    name: String,
    compile_time: Duration,
    lines_per_second: f64,
    success: bool,
    errors: Vec<String>,
}

impl BenchmarkResult {
    fn new(name: String) -> Self {
        Self {
            name,
            compile_time: Duration::ZERO,
            lines_per_second: 0.0,
            success: false,
            errors: Vec::new(),
        }
    }
}

fn benchmark_file(file_path: &str) -> BenchmarkResult {
    let mut result = BenchmarkResult::new(file_path.to_string());

    if !Path::new(file_path).exists() {
        result.errors.push(format!("File not found: {file_path}"));
        return result;
    }

    let content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(e) => {
            result.errors.push(format!("Failed to read file: {e}"));
            return result;
        }
    };

    let line_count = content.lines().count();

    let start = Instant::now();
    match clean_language_compiler::compile_with_file(&content, file_path) {
        Ok(_) => {
            result.compile_time = start.elapsed();
            result.success = true;
        }
        Err(errors) => {
            result.compile_time = start.elapsed();
            result
                .errors
                .extend(errors.iter().map(|e| format!("Compile error: {e}")));
        }
    }

    if result.compile_time.as_secs_f64() > 0.0 {
        result.lines_per_second = line_count as f64 / result.compile_time.as_secs_f64();
    }

    result
}

fn run_benchmark_suite() -> Vec<BenchmarkResult> {
    let test_files = vec![
        "tests/clean_files/00_minimal.cln",
        "tests/clean_files/01_hello_world.cln",
        "tests/clean_files/10_functions_basic.cln",
        "tests/clean_files/14_classes_basic.cln",
        "tests/clean_files/28_complex_example.cln",
        "tests/clean_files/33_complex_integration.cln",
    ];

    test_files.iter().map(|f| benchmark_file(f)).collect()
}

fn print_benchmark_report(results: &[BenchmarkResult]) {
    println!("CLEAN LANGUAGE COMPILER PERFORMANCE BENCHMARKS");
    println!("==================================================");
    println!();

    let total_files = results.len() as u32;
    let mut successful_files = 0u32;
    let mut total_compile_time = Duration::ZERO;

    for result in results {
        if result.success {
            successful_files += 1;
        }

        total_compile_time += result.compile_time;

        let status = if result.success { "OK" } else { "FAIL" };
        let file_name = result.name.split('/').last().unwrap_or(&result.name);

        println!(
            "{} {} | Compile: {:.2}ms | Speed: {:.0} lines/sec",
            status,
            file_name,
            result.compile_time.as_secs_f64() * 1000.0,
            result.lines_per_second
        );

        for error in &result.errors {
            println!("   Error: {error}");
        }
    }

    println!();
    println!("SUMMARY");
    println!("----------");
    println!("Files processed: {successful_files}/{total_files}");
    let avg_ms = total_compile_time.as_secs_f64() * 1000.0 / f64::from(total_files);
    println!("Average compile time: {avg_ms:.2}ms");

    if avg_ms < 100.0 {
        println!("Performance: EXCELLENT (< 100ms average)");
    } else if avg_ms < 500.0 {
        println!("Performance: GOOD (< 500ms average)");
    } else if avg_ms < 1000.0 {
        println!("Performance: ACCEPTABLE (< 1s average)");
    } else {
        println!("Performance: NEEDS IMPROVEMENT (> 1s average)");
    }

    if successful_files == total_files {
        println!("All benchmarks passed!");
    } else {
        println!(
            "{}/{} benchmarks failed",
            total_files - successful_files,
            total_files
        );
    }
}

fn main() {
    println!("Starting Clean Language Compiler performance benchmarks...");
    println!();

    let results = run_benchmark_suite();
    print_benchmark_report(&results);

    let failed_count = results.iter().filter(|r| !r.success).count();
    if failed_count > 0 {
        std::process::exit(1);
    }
}
