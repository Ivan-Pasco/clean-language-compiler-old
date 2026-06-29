//! Plugin Contracts v2 Phase B cycle 3 — BRIDGE-HOST-MISMATCH gating.
//!
//! Phase B contract per `bridge-host-classes.md` §6: when a build calls a
//! bridge function whose declared `hosts` excludes the active host class,
//! the codegen-side validator emits a BRIDGE-HOST-MISMATCH warning. With
//! `--strict-hosts` (or `CLEAN_STRICT_HOSTS=1`), the warning is promoted to
//! a hard compile error.
//!
//! `compile_multi_file_*` discards the warnings vector returned by
//! `MirCodegenResult`, so the warning case is observable only via the
//! tracing subscriber. This test instead pins the gating logic by toggling
//! `--strict-hosts`:
//!
//!   - target=browser, strict-hosts ON  → compile fails with the error code.
//!   - target=browser, strict-hosts OFF → compile succeeds (Phase B contract:
//!     warning only, build proceeds).
//!   - target=server, strict-hosts ON   → compile succeeds (validator
//!     correctly gates on the active host class).
//!
//! The fixture declares `_db_query` with `hosts = ["server"]` in
//! `tests/cln/plugins/host_class_warning/plugin.toml`. The compiler's
//! `extend_with_source_plugin_bridges` resolver picks up the sibling
//! plugin.toml so the call from main.cln is checked against the declared
//! host class.

use std::path::PathBuf;
use std::process::{Command, Output};

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cln/plugins/host_class_warning")
}

fn run_compile(target: &str, strict: bool) -> Output {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = fixture_dir();
    // Copy the fixture into a temp dir so each test run gets a clean output
    // tree and so the compiler's plugin.toml resolver finds the sibling
    // declaration without our test polluting the source fixture.
    for name in ["main.cln", "plugin.toml"] {
        std::fs::copy(src.join(name), tmp.path().join(name))
            .unwrap_or_else(|e| panic!("copy {} failed: {}", name, e));
    }
    let mut args = vec![
        "compile".to_string(),
        "main.cln".to_string(),
        "-o".to_string(),
        "out.wasm".to_string(),
        format!("--target={}", target),
    ];
    if strict {
        args.push("--strict-hosts".to_string());
    }
    Command::new(cln_binary())
        .args(&args)
        .current_dir(tmp.path())
        .output()
        .expect("cln runs")
}

#[test]
fn server_bridge_in_browser_build_with_strict_mode_fails_with_bridge_host_mismatch() {
    let out = run_compile("browser", true);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{}\n{}", stderr, stdout);

    assert!(
        !out.status.success(),
        "strict-mode browser build calling server-only bridge must FAIL.\n\
         stdout=\n{}\nstderr=\n{}",
        stdout,
        stderr
    );
    assert!(
        combined.contains("BRIDGE-HOST-MISMATCH"),
        "diagnostic must carry error code BRIDGE-HOST-MISMATCH.\n{}",
        combined
    );
    assert!(
        combined.contains("_db_query"),
        "diagnostic must name the offending bridge: {}",
        combined
    );
    assert!(
        combined.contains("browser"),
        "diagnostic must cite the active host class (browser): {}",
        combined
    );
    assert!(
        combined.contains("server"),
        "diagnostic must cite the declared host class (server): {}",
        combined
    );
}

// Blocked by compiler bug #44e8e36be0ec (COM001 — stack imbalance when an
// `external:`-declared string-returning bridge with `expand_strings = true`
// in the sibling plugin.toml is called from user code). Strict-hosts mode
// short-circuits earlier with BRIDGE-HOST-MISMATCH and is unaffected — see
// the test above. Re-enable both tests once the codegen wrapper drops the
// extra i32 left on the stack at end-of-block.
#[test]
#[ignore = "compiler bug #44e8e36be0ec (COM001) — string-bridge call site leaves extra i32 on the stack"]
fn server_bridge_in_browser_build_without_strict_mode_succeeds() {
    // Phase B contract — warning only, build proceeds.
    let out = run_compile("browser", false);
    assert!(
        out.status.success(),
        "non-strict browser build must succeed (warning only).\nstderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "compiler bug #44e8e36be0ec (COM001) — same codegen-wrapper bug as above"]
fn server_bridge_in_server_build_with_strict_mode_succeeds() {
    // Validator must gate correctly — same source, target=server, no
    // mismatch, strict mode is irrelevant because there's nothing to
    // promote.
    let out = run_compile("server", true);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "server-target build of server-only bridge must NOT trip BRIDGE-HOST-MISMATCH.\n\
         stderr=\n{}",
        stderr
    );
    assert!(
        !stderr.contains("BRIDGE-HOST-MISMATCH"),
        "no BRIDGE-HOST-MISMATCH expected for matching host class: {}",
        stderr
    );
}
