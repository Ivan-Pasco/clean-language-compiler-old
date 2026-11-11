//! `wasmtime_runner` host imports.
//!
//! * The math bridges `cln test` programs import (`math_random`,
//!   `math_min`, ...) are provided, so modules using them load.
//! * Host functions the standalone runner cannot provide (file system, HTTP,
//!   server request context, ...) trap with a clear message instead of
//!   returning a fake 0 that let programs "pass".

use std::process::{Command, Output};

fn run_wat(wat: &str) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let wasm = dir.path().join("module.wasm");
    std::fs::write(&wasm, wat::parse_str(wat).expect("valid WAT")).unwrap();
    Command::new(env!("CARGO_BIN_EXE_wasmtime_runner"))
        .arg(&wasm)
        .output()
        .expect("wasmtime_runner runs")
}

#[test]
fn math_bridges_are_provided() {
    let out = run_wat(
        r#"(module
            (import "env" "math_random" (func $random (result f64)))
            (import "env" "math_min" (func $min (param f64 f64) (result f64)))
            (import "env" "math_max" (func $max (param f64 f64) (result f64)))
            (import "env" "math_sign" (func $sign (param f64) (result f64)))
            (import "env" "math_trunc" (func $trunc (param f64) (result f64)))
            (import "env" "math_pi" (func $pi (result f64)))
            (import "env" "math_e" (func $e (result f64)))
            (import "env" "print_float" (func $print (param f64)))
            (memory (export "memory") 1)
            (func (export "start")
                ;; random() is in [0, 1): print 1 when it is
                call $random
                f64.const 1
                f64.lt
                f64.convert_i32_u
                call $print
                f64.const 2 f64.const 3 call $min call $print
                f64.const 2 f64.const 3 call $max call $print
                f64.const -4 call $sign call $print
                f64.const -2.75 call $trunc call $print
                call $pi f64.const 3 f64.gt f64.convert_i32_u call $print
                call $e f64.const 2.7 f64.gt f64.convert_i32_u call $print))"#,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "runner failed: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("123-1-211"), "{stdout}");
}

#[test]
fn host_only_functions_trap_instead_of_returning_zero() {
    let out = run_wat(
        r#"(module
            (import "env" "http_get" (func $get (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "start")
                i32.const 0
                i32.const 0
                call $get
                drop))"#,
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "calling http_get must fail the run"
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        all.contains("`http_get` is not available in wasmtime_runner"),
        "{all}"
    );
}

#[test]
fn print_of_a_range_outside_memory_traps() {
    for import in ["print", "printl"] {
        let out = run_wat(&format!(
            r#"(module
                (import "env" "{import}" (func $print (param i32 i32)))
                (memory (export "memory") 1)
                (func (export "start")
                    i32.const 65530
                    i32.const 100
                    call $print))"#
        ));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{import}: {stdout}{stderr}");
        assert!(!stdout.contains("invalid range"), "{import}: {stdout}");
        assert!(stderr.contains("out of bounds"), "{import}: {stderr}");
    }
}

#[test]
fn stdout_carries_only_the_program_output() {
    let out = run_wat(
        r#"(module
            (import "env" "print_integer" (func $print (param i64)))
            (memory (export "memory") 1)
            (func (export "start")
                i64.const 7
                call $print))"#,
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "--- Output ---\n7--- End Output ---\n"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("Loading WebAssembly file"));
}

/// A test-only module: `_start` only calls `_run_tests`, which prints a line
/// and returns the failure count.
const TEST_ONLY_MODULE: &str = r#"(module
    (import "env" "print_integer" (func $print (param i64)))
    (memory (export "memory") 1)
    (func $run_tests (export "_run_tests") (result i32)
        i64.const 5
        call $print
        i32.const FAILURES)
    (func (export "_start")
        call $run_tests
        drop))"#;

#[test]
fn test_only_module_runs_its_tests_once() {
    let out = run_wat(&TEST_ONLY_MODULE.replace("FAILURES", "0"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert_eq!(stdout, "--- Output ---\n5--- End Output ---\n");
}

#[test]
fn test_only_module_with_failures_fails_the_run() {
    let out = run_wat(&TEST_ONLY_MODULE.replace("FAILURES", "2"));
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).matches('5').count(),
        1,
        "tests ran more than once"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("2 test(s) failed"));
}

#[test]
fn help_and_version_are_options_not_files() {
    for flag in ["--help", "-h", "--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_wasmtime_runner"))
            .arg(flag)
            .output()
            .expect("wasmtime_runner runs");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{flag}: {stdout}");
        assert!(
            stdout.contains("Usage:") || stdout.starts_with("wasmtime_runner "),
            "{flag}: {stdout}"
        );
    }
    let out = Command::new(env!("CARGO_BIN_EXE_wasmtime_runner"))
        .output()
        .expect("wasmtime_runner runs");
    assert_eq!(out.status.code(), Some(1), "no arguments is a usage error");
}
