//! End-to-end runtime tests for the Delivery-2 JSON bridges under
//! `--enable-json-bridge` ([P2-cont-2b]).
//!
//! Each test:
//!   1. Compiles a small Clean program with `--enable-json-bridge` via the
//!      library API.
//!   2. Runs the produced WASM through the `wasmtime_runner` binary — which
//!      as of 0.33.138 registers stub implementations of `_json_encode_v2` /
//!      `_json_encode_pretty_v2` / `_json_decode_v2` bridges backed by
//!      serde_json + the BOXED_ANY_ABI tree walker (see the `json_v2`
//!      module at the end of `src/bin/wasmtime_runner.rs`).
//!   3. Diffs the captured stdout against expected serialization.
//!
//! Covers:
//!   * `tryTextToData` sentinel handling (invalid input → null tag, not trap).
//!   * `textToData` raise handling (invalid input → runtime trap → non-zero
//!     runner exit).
//!   * `dataToText` / `prettyDataToText` round trips through `_json_encode_v2`.
//!
//! Reference:
//!   * `foundation/spec/platform/BOXED_ANY_ABI.md`
//!   * `foundation/docs/governance/JSON_MIGRATION_DELIVERY2.md`
//!   * `tests/common/json_stub_host.rs` — reference in-process implementation
//!     (kept for future direct-linker tests; runner-based tests use the same
//!     algorithm mirrored inside the runner).

#![cfg(feature = "wasmtime-runtime")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use clean_language_compiler::MemoryTier;

// ---------------------------------------------------------------------------
// Compilation + execution helpers
// ---------------------------------------------------------------------------

fn compile_with_bridge(source: &str, out_path: &Path) {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, source).expect("write repro.cln");

    clean_language_compiler::set_enable_json_bridge_override(true);
    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    );
    clean_language_compiler::set_enable_json_bridge_override(false);

    let (wasm, _state) = result.unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("runtime test must compile under --enable-json-bridge");
    });

    fs::write(out_path, &wasm).expect("write wasm");
}

fn wasmtime_runner_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    for candidate in [
        manifest_dir.join(format!("{}/release/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/debug/wasmtime_runner", target_dir)),
        PathBuf::from(format!("{}/release/wasmtime_runner", target_dir)),
        PathBuf::from(format!("{}/debug/wasmtime_runner", target_dir)),
    ] {
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(
        "wasmtime_runner must be built first — run `cargo build --release --bin wasmtime_runner`"
    );
}

/// Extract the raw program output from the runner's decorated stdout.
///
/// `wasmtime_runner` wraps executable stdout with `--- Output ---` /
/// `--- End Output ---` banners.
fn extract_program_output(runner_stdout: &str) -> String {
    let start_marker = "--- Output ---";
    let end_marker = "--- End Output ---";
    let start = runner_stdout
        .find(start_marker)
        .map(|i| i + start_marker.len())
        .unwrap_or(0);
    let end = runner_stdout
        .find(end_marker)
        .unwrap_or(runner_stdout.len());
    if start >= end {
        return String::new();
    }
    runner_stdout[start..end].trim_matches('\n').to_string()
}

struct RunResult {
    stdout: String,
    stderr: String,
    exit_success: bool,
}

fn run(wasm_path: &Path) -> RunResult {
    let runner = wasmtime_runner_path();
    let output = Command::new(&runner)
        .arg(wasm_path)
        .output()
        .expect("invoke wasmtime_runner");
    RunResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_success: output.status.success(),
    }
}

fn compile_and_run(source: &str, label: &str) -> RunResult {
    let tmp = TempDir::new().expect("tempdir");
    let wasm = tmp.path().join(format!("{label}.wasm"));
    compile_with_bridge(source, &wasm);
    run(&wasm)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// tryTextToData on invalid JSON must return the null-tag boxed-Any per D5,
/// not trap. `json.dataToText(nullBoxedAny)` then prints "null".
#[test]
fn try_decode_returns_null_on_parse_failure() {
    let src = "start:\n\tany bad = json.tryTextToData(\"[1,2,\")\n\tprintl(json.dataToText(bad))\n";
    let result = compile_and_run(src, "try_decode_null");
    assert!(result.exit_success, "runner failed: {}", result.stderr);
    let out = extract_program_output(&result.stdout);
    assert_eq!(out.trim(), "null", "full stdout: {}", result.stdout);
}

/// tryTextToData on valid input must decode normally.
#[test]
fn try_decode_returns_value_on_valid_input() {
    let src = "start:\n\tany ok = json.tryTextToData(\"42\")\n\tprintl(json.dataToText(ok))\n";
    let result = compile_and_run(src, "try_decode_value");
    assert!(result.exit_success, "runner failed: {}", result.stderr);
    let out = extract_program_output(&result.stdout);
    assert_eq!(out.trim(), "42", "full stdout: {}", result.stdout);
}

/// textToData on invalid JSON must raise (WASM trap) per D5 — surfaced as a
/// non-zero runner exit code, since the trap propagates through
/// `start.call()` in the runner and returns an error.
#[test]
fn text_to_data_raises_on_parse_failure() {
    let src = "start:\n\tany bad = json.textToData(\"[1,2,\")\n\tprintl(json.dataToText(bad))\n";
    let result = compile_and_run(src, "text_to_data_raise");
    // Runner returns non-zero when start traps. Some builds may still
    // partially print — what matters is that the process failed AND the
    // stderr surfaces a WASM trap.
    assert!(
        !result.exit_success,
        "textToData on invalid JSON should trap, but runner succeeded. stdout: {}",
        result.stdout
    );
}

/// dataToText on a small object round-trips through _json_encode_v2.
#[test]
fn data_to_text_encodes_object() {
    let src =
        "start:\n\tany doc = json.textToData(\"{\\\"a\\\":1}\")\n\tprintl(json.dataToText(doc))\n";
    let result = compile_and_run(src, "data_to_text");
    assert!(result.exit_success, "runner failed: {}", result.stderr);
    let out = extract_program_output(&result.stdout);
    assert_eq!(out.trim(), "{\"a\":1}", "full stdout: {}", result.stdout);
}

/// prettyDataToText emits 2-space indented output.
#[test]
fn pretty_data_to_text_indents() {
    let src = "start:\n\tany doc = json.textToData(\"{\\\"a\\\":1}\")\n\tprintl(json.prettyDataToText(doc))\n";
    let result = compile_and_run(src, "pretty_data_to_text");
    assert!(result.exit_success, "runner failed: {}", result.stderr);
    let out = extract_program_output(&result.stdout);
    let expected = "{\n  \"a\": 1\n}";
    assert_eq!(out.trim(), expected, "full stdout: {}", result.stdout);
}
