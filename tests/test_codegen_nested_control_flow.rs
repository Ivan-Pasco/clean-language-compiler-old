//! Regression tests for two codegen bugs surfaced by the
//! COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS investigation
//! (dashboard fingerprint f80ee96ce507).
//!
//! Both bugs caused statements / branches to be silently dropped from the
//! emitted WASM, and both lived in the MIR → WASM control-flow lowering
//! (`src/codegen/mir_codegen/`). They are independent of each other but
//! shared the same root user-visible symptom in frame.ui: an inner loop
//! that the plugin author bounded with an `if counter > N: break` safety
//! check would run for the full N iterations every call, because the
//! statement that should have advanced the loop's variable (test 1) or
//! the break itself (test 2) never made it into the WASM.
//!
//! Source fixtures live next to the language's other regression .cln
//! files so they get picked up by the unified test runner too:
//!   * tests/cln/control/conditionals/08_stmt_after_nested_if_else.cln
//!   * tests/cln/control/loops/else_break_inside_while.cln

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::path::PathBuf;
use std::process::Command;

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
        panic!("regression source {source_path} must compile cleanly");
    });

    std::fs::write(out_path, wasm).expect("write wasm");
}

fn run_via_wasmtime_runner(wasm_path: &str) -> String {
    // `cargo test` (CI's default) builds the debug binary; `cargo test
    // --release` (local) builds the release one. Try both so CI and dev
    // workflows pass without extra plumbing. CARGO_TARGET_DIR override is
    // also honoured for the same reason as `test_consecutive_if_iterate`.
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        PathBuf::from(format!("{}/release/wasmtime_runner", target_dir)),
        PathBuf::from(format!("{}/debug/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/release/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/debug/wasmtime_runner", target_dir)),
    ];
    let runner = candidates.iter().find(|p| p.exists()).unwrap_or_else(|| {
        panic!(
            "wasmtime_runner not built — `cargo build --bin wasmtime_runner` (debug) or \
             `cargo build --release --bin wasmtime_runner`. Looked in: {candidates:?}"
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
fn stmt_after_nested_if_else_with_nested_branches_emits() {
    // Bug shape: an outer if/else whose body contains an inner if/else
    // where BOTH branches contain another if/else, then a statement at
    // the outer body's level after that inner if/else. The codegen's
    // `collect_jump_targets` stopped at the innermost merge block (the
    // inner if's continue block) instead of chasing the Jump chain
    // through it to the actual merge point the outer body's trailing
    // statement was sitting in, so `generate_branch_block`'s
    // `generated.contains(...)` check short-circuited and the trailing
    // statement was never emitted at all.
    //
    // Fix: `collect_jump_targets` now walks through empty merge blocks
    // via `chase_jump_chain` until it finds a block with user
    // instructions or a non-Jump terminator. See
    // `src/codegen/mir_codegen/control_flow.rs`.
    //
    // This was the root cause of frame.ui's `process_text_node`
    // never advancing its `remaining` variable, which made
    // COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS hang the
    // compiler indefinitely on any project with `.cln` files under
    // `app/ui/web/pages/`.
    let out = std::env::temp_dir().join("stmt_after_nested_if_else_regression.wasm");
    let out_str = out.to_string_lossy().to_string();
    compile_to_wasm(
        "tests/cln/control/conditionals/08_stmt_after_nested_if_else.cln",
        &out_str,
    );
    let output = run_via_wasmtime_runner(&out_str);

    assert!(
        output.contains("inner-a"),
        "the deepest nested true branch must still emit its print — output was:\n{output}"
    );
    assert!(
        output.contains("AFTER"),
        "COMPILER-CODEGEN-DROPS-STATEMENT-AFTER-NESTED-IF: the statement that follows \
         the nested if/else was dropped from the emitted WASM. Output was:\n{output}"
    );
}

#[test]
fn else_break_inside_while_actually_breaks() {
    // Bug shape: `while ... if cond: stmt else: break`. The else block
    // is empty (`break` has no body, just a terminator) and ends in a
    // Jump to the loop's exit. The codegen's `is_continuation_not_else`
    // treated any empty Jump-terminated `false_block` as the if's
    // continuation (i.e. "no else clause"), so the else with the break
    // was dropped from the emitted WASM. The loop's exit was therefore
    // unreachable from the if; without the break, the only way out was
    // the loop guard becoming false, and since the if's then branch
    // assigned the loop variable to a value still satisfying the guard,
    // the loop spun forever.
    //
    // Fix: `is_continuation_not_else` now excludes Jumps that target
    // the innermost loop's exit. See
    // `src/codegen/mir_codegen/control_flow.rs`. Surfaced as the
    // SECOND infinite loop in frame.ui after the first fix landed,
    // inside `find_unescaped_quote`.
    let out = std::env::temp_dir().join("else_break_inside_while_regression.wasm");
    let out_str = out.to_string_lossy().to_string();
    compile_to_wasm(
        "tests/cln/control/loops/else_break_inside_while.cln",
        &out_str,
    );
    let output = run_via_wasmtime_runner(&out_str);

    assert!(
        output.contains("exited cleanly inner=1"),
        "COMPILER-CODEGEN-DROPS-ELSE-BREAK: `else: break` was dropped — \
         the loop ran past the break that should have terminated it. \
         If you see no output at all, the loop is still infinite. \
         Output was:\n{output}"
    );
}
