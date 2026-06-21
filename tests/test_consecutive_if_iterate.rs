//! Regression test for RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED (dashboard
//! fingerprint 8954b2bd828f, reported against 0.30.326).
//!
//! Symptom (from the report): two consecutive `if X.length() > 0 / iterate
//! Y in X.split(...)` blocks in the same function — the first branch fires
//! consistently and the second silently does NOT, even though identical
//! diagnostics confirm the guard variable still has non-zero length one
//! statement earlier. Reordering reproduces the drop on whichever branch is
//! second; each block in isolation works fine.
//!
//! The bug could not be reproduced on 0.30.330 across multiple variations
//! (parameters, function returns, class methods, with/without plugins,
//! nested ifs, helper functions). Memory-related fixes between 0.30.326 and
//! 0.30.330 (notably efb9513c exporting free/scope_pop and 3564b5ec routing
//! LP-string reads through a safe fallback) most likely addressed the
//! underlying cause.
//!
//! This test compiles and executes the exact shape from the report via the
//! project's own `wasmtime_runner` binary (which registers the full host
//! bridge) and asserts BOTH branches fire — guarding the pattern against
//! any future regression in iterate-exit / consecutive-if interaction.
//!
//! Companion .cln source:
//!   tests/cln/control/loops/consecutive_if_iterate_split.cln

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::path::PathBuf;
use std::process::Command;

const REGRESSION_SOURCE: &str = "tests/cln/control/loops/consecutive_if_iterate_split.cln";

fn compile_to_wasm(source_path: &str, out_path: &str) {
    let source = PathBuf::from(source_path);
    let search_paths = vec![source.parent().expect("source has parent").to_path_buf()];

    let (wasm, _build_state) = compile_multi_file_with_memory_tier(
        &source,
        search_paths,
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("regression source must compile cleanly");
    });

    std::fs::write(out_path, &wasm).expect("write compiled wasm");
}

fn run_via_wasmtime_runner(wasm_path: &str) -> String {
    // Try a few candidate locations for the wasmtime_runner binary. In
    // `cargo test --release` it lives under target/release; debug builds
    // put it under target/debug. CI may set CARGO_TARGET_DIR.
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let candidates = [
        format!("{}/release/wasmtime_runner", target_dir),
        format!("{}/debug/wasmtime_runner", target_dir),
    ];
    let runner = candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap_or_else(|| {
            panic!(
                "wasmtime_runner binary not found. Run `cargo build --release --bin \
                 wasmtime_runner` before this test, or include it in the build graph. \
                 Looked in: {candidates:?}"
            )
        });

    let output = Command::new(runner)
        .arg(wasm_path)
        .output()
        .expect("invoke wasmtime_runner");

    assert!(
        output.status.success(),
        "wasmtime_runner exited non-zero ({}):\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn consecutive_if_iterate_both_branches_fire() {
    let out_dir = std::env::temp_dir();
    let wasm_out = out_dir.join("consecutive_if_iterate_split_regression.wasm");
    let wasm_out_str = wasm_out.to_string_lossy().to_string();

    compile_to_wasm(REGRESSION_SOURCE, &wasm_out_str);
    let output = run_via_wasmtime_runner(&wasm_out_str);

    // The companion .cln file calls build_errors("stranger\x1fcat", "body\x1fhead")
    // and expects both branches to emit. The first branch yields "U:stranger"
    // and "U:cat"; the second yields "M:body" and "M:head". The BUG
    // (8954b2bd828f) is the second `if` branch silently not firing — its
    // entries would be absent from the printed accumulator.
    //
    // Assert each entry separately so a regression report is clear about
    // which branch dropped.
    assert!(
        output.contains("U:stranger"),
        "first branch's first iteration entry missing — output was:\n{output}"
    );
    assert!(
        output.contains("U:cat"),
        "first branch's second iteration entry missing — output was:\n{output}"
    );
    assert!(
        output.contains("M:body"),
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED: second branch's first entry missing — \
         this is the original bug symptom. Output was:\n{output}"
    );
    assert!(
        output.contains("M:head"),
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED: second branch's second entry missing — \
         this is the original bug symptom. Output was:\n{output}"
    );
    assert!(
        output.contains("DONE"),
        "final DONE sentinel missing — function did not run to completion. Output was:\n{output}"
    );
}
