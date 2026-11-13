//! `print` and `printl` in wasmtime_runner write a string's bytes to stdout
//! unchanged. Strings are byte sequences (stdlib-reference.md §4: "Characters
//! are bytes"), so printing one byte of a multi-byte UTF-8 character writes
//! that byte; it is not an error. A range outside the module's memory still
//! traps.
//!
//! Regression: the runner decoded the bytes as UTF-8 and trapped with
//! "incomplete utf-8 byte sequence" on `iterate ch in "héllo"` / `print(ch)`.
//! The check is a Rust test because the regression pins compare stdout as
//! text, and each output line here is a lone byte.

use std::path::Path;
use std::process::{Command, Output};

fn run_wasm(wasm: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wasmtime_runner"))
        .arg(wasm)
        .output()
        .expect("run wasmtime_runner")
}

/// The bytes between the runner's output markers.
fn program_output(stdout: &[u8]) -> &[u8] {
    let start_marker = b"--- Output ---\n";
    let end_marker = b"--- End Output ---";
    let start = stdout
        .windows(start_marker.len())
        .position(|w| w == start_marker)
        .expect("output start marker")
        + start_marker.len();
    let end = stdout[start..]
        .windows(end_marker.len())
        .position(|w| w == end_marker)
        .expect("output end marker")
        + start;
    &stdout[start..end]
}

#[test]
fn printing_each_byte_of_a_multibyte_string_writes_the_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("bytes.cln");
    let wasm = dir.path().join("bytes.wasm");
    std::fs::write(
        &source,
        "start:\n\titerate ch in \"h\u{e9}llo\"\n\t\tprint(ch)\n\tprint(\"done\")\n",
    )
    .expect("write source");
    let compiled = Command::new(env!("CARGO_BIN_EXE_cln"))
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&wasm)
        .output()
        .expect("run cln");
    assert!(
        compiled.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = run_wasm(&wasm);
    assert!(
        run.status.success(),
        "runner failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        program_output(&run.stdout),
        b"h\n\xc3\n\xa9\nl\nl\no\ndone\n".as_slice()
    );
}

#[test]
fn print_and_printl_write_raw_bytes_and_trap_outside_memory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let module = |body: &str| {
        format!(
            r#"(module
                (import "env" "print" (func $print (param i32 i32)))
                (import "env" "printl" (func $printl (param i32 i32)))
                (memory (export "memory") 1)
                (data (i32.const 16) "a\c3\a9\ff")
                (func (export "start") {body}))"#
        )
    };

    let raw = dir.path().join("raw.wasm");
    let wat = module(
        "i32.const 17 i32.const 1 call $printl
         i32.const 16 i32.const 4 call $print",
    );
    std::fs::write(&raw, wat::parse_str(&wat).expect("valid wat")).expect("write wasm");
    let run = run_wasm(&raw);
    assert!(
        run.status.success(),
        "runner failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        program_output(&run.stdout),
        b"\xc3\na\xc3\xa9\xff".as_slice()
    );

    let outside = dir.path().join("outside.wasm");
    let wat = module("i32.const 65534 i32.const 4 call $print");
    std::fs::write(&outside, wat::parse_str(&wat).expect("valid wat")).expect("write wasm");
    let run = run_wasm(&outside);
    assert!(!run.status.success(), "a range outside memory must trap");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(report.contains("out of bounds"), "runner output: {report}");
}
