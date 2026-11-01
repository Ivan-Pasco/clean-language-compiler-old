//! Multi-file merge regressions:
//!
//! * Each module's `start:` body is its own block scope. Two files that both
//!   declare `integer s` in their `start:` used to fail with SEM007
//!   "Variable 's' is already declared in this scope" because the merge
//!   spliced every non-entry start body into the entry start body.
//! * A `screen` declared in an imported module keeps its `state:` visible to
//!   its own functions (spec §23 "State in Screens", SCOPE005). The merge
//!   used to keep only the entry module's screen blocks, so the screen's
//!   functions failed with SEM007 "Undefined variable 'count'".
//!
//! Fixtures: tests/cln/imports/start_scope_*.cln, screen_import_*.cln.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::path::PathBuf;
use std::process::Command;

fn compile_to_wasm(source_path: &str, out_name: &str) -> PathBuf {
    let source = PathBuf::from(source_path);
    let search_paths = vec![source.parent().expect("source has parent").to_path_buf()];
    let (wasm, _build_state) = compile_multi_file_with_memory_tier(
        &source,
        search_paths,
        0,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errors| {
        panic!(
            "{source_path} must compile: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        )
    });
    let out = std::env::temp_dir().join(out_name);
    std::fs::write(&out, wasm).expect("write wasm");
    out
}

fn run_via_wasmtime_runner(wasm_path: &std::path::Path) -> String {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        PathBuf::from(format!("{}/release/wasmtime_runner", target_dir)),
        PathBuf::from(format!("{}/debug/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/release/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/debug/wasmtime_runner", target_dir)),
    ];
    let runner = candidates
        .iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("wasmtime_runner not built. Looked in: {candidates:?}"));
    let output = Command::new(runner)
        .arg(wasm_path)
        .output()
        .expect("invoke wasmtime_runner");
    assert!(
        output.status.success(),
        "wasmtime_runner failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The runner frames program output between these two markers.
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let body = stdout
        .split("--- Output ---")
        .nth(1)
        .and_then(|rest| rest.split("--- End Output ---").next())
        .unwrap_or_else(|| panic!("runner output has no program section:\n{stdout}"));
    body.trim().to_string()
}

#[test]
fn start_locals_with_same_name_in_two_modules_keep_their_own_scope() {
    let wasm = compile_to_wasm(
        "tests/cln/imports/start_scope_main.cln",
        "start_scope_main_regression.wasm",
    );
    let output = run_via_wasmtime_runner(&wasm);
    let lines: Vec<&str> = output.lines().map(str::trim).collect();
    assert_eq!(
        lines,
        vec!["other 2", "main 1", "1"],
        "imported module's start: runs first, each with its own `s`"
    );
}

#[test]
fn screen_state_in_imported_module_is_visible_to_its_functions() {
    let wasm = compile_to_wasm(
        "tests/cln/imports/screen_import_main.cln",
        "screen_import_main_regression.wasm",
    );
    let output = run_via_wasmtime_runner(&wasm);
    assert_eq!(output.trim(), "2");
}
