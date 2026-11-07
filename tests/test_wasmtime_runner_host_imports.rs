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
    assert!(!out.status.success(), "calling http_get must fail the run");
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
