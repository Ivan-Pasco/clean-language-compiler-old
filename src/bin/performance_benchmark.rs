//! Performance benchmarking tool for Clean Language Compiler
//!
//! Measures compilation performance and detects regressions

use clean_language_compiler::parser::CleanParser;
use clean_language_compiler::semantic::SemanticAnalyzer;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct BenchmarkResult {
    name: String,
    parse_time: Duration,
    semantic_time: Duration,
    total_time: Duration,
    lines_per_second: f64,
    success: bool,
    errors: Vec<String>,
}

impl BenchmarkResult {
    fn new(name: String) -> Self {
        Self {
            name,
            parse_time: Duration::ZERO,
            semantic_time: Duration::ZERO,
            total_time: Duration::ZERO,
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
    let total_start = Instant::now();

    // Benchmark parsing
    let parse_start = Instant::now();
    let ast = match CleanParser::parse_program(&content) {
        Ok(ast) => {
            result.parse_time = parse_start.elapsed();
            ast
        }
        Err(e) => {
            result.parse_time = parse_start.elapsed();
            result.errors.push(format!("Parse error: {e}"));
            return result;
        }
    };

    // Benchmark semantic analysis
    let semantic_start = Instant::now();
    let mut analyzer = SemanticAnalyzer::new();
    match analyzer.analyze(&ast) {
        Ok(_) => {
            result.semantic_time = semantic_start.elapsed();
            result.success = true;
        }
        Err(e) => {
            result.semantic_time = semantic_start.elapsed();
            result.errors.push(format!("Semantic error: {e}"));
        }
    }

    result.total_time = total_start.elapsed();
    if result.total_time.as_secs_f64() > 0.0 {
        result.lines_per_second = line_count as f64 / result.total_time.as_secs_f64();
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

    let mut results = Vec::new();

    for file in test_files {
        results.push(benchmark_file(file));
    }

    results
}

fn print_benchmark_report(results: &[BenchmarkResult]) {
    println!("🚀 CLEAN LANGUAGE COMPILER PERFORMANCE BENCHMARKS");
    println!("==================================================");
    println!();

    let mut total_files = 0;
    let mut successful_files = 0;
    let mut total_parse_time = Duration::ZERO;
    let mut total_semantic_time = Duration::ZERO;
    let mut total_compilation_time = Duration::ZERO;

    for result in results {
        total_files += 1;
        if result.success {
            successful_files += 1;
        }

        total_parse_time += result.parse_time;
        total_semantic_time += result.semantic_time;
        total_compilation_time += result.total_time;

        let status = if result.success { "✅" } else { "❌" };
        let file_name = result.name.split('/').last().unwrap_or(&result.name);

        println!(
            "{} {} | Parse: {:.2}ms | Semantic: {:.2}ms | Total: {:.2}ms | Speed: {:.0} lines/sec",
            status,
            file_name,
            result.parse_time.as_secs_f64() * 1000.0,
            result.semantic_time.as_secs_f64() * 1000.0,
            result.total_time.as_secs_f64() * 1000.0,
            result.lines_per_second
        );

        if !result.success {
            for error in &result.errors {
                println!("   Error: {error}");
            }
        }
    }

    println!();
    println!("📊 SUMMARY");
    println!("----------");
    println!("Files processed: {successful_files}/{total_files}");
    println!(
        "Average parse time: {:.2}ms",
        total_parse_time.as_secs_f64() * 1000.0 / f64::from(total_files)
    );
    println!(
        "Average semantic time: {:.2}ms",
        total_semantic_time.as_secs_f64() * 1000.0 / f64::from(total_files)
    );
    println!(
        "Average total time: {:.2}ms",
        total_compilation_time.as_secs_f64() * 1000.0 / f64::from(total_files)
    );

    // Performance thresholds
    let avg_total_ms = total_compilation_time.as_secs_f64() * 1000.0 / f64::from(total_files);
    if avg_total_ms < 100.0 {
        println!("🎯 Performance: EXCELLENT (< 100ms average)");
    } else if avg_total_ms < 500.0 {
        println!("✅ Performance: GOOD (< 500ms average)");
    } else if avg_total_ms < 1000.0 {
        println!("⚠️  Performance: ACCEPTABLE (< 1s average)");
    } else {
        println!("🔴 Performance: NEEDS IMPROVEMENT (> 1s average)");
    }

    if successful_files == total_files {
        println!("✅ All benchmarks passed!");
    } else {
        println!(
            "❌ {}/{} benchmarks failed",
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

    // Exit with error code if any benchmarks failed
    let failed_count = results.iter().filter(|r| !r.success).count();
    if failed_count > 0 {
        std::process::exit(1);
    }
}
