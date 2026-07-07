//! Bug 270f8fc643db — BRIDGE-HOST-MISMATCH host_class inference.
//!
//! `cln compile` used to default the CLI `--target` to `"server"`, so a
//! standalone client entry (`cln compile app/client/main.cln`) that only
//! called browser-restricted bridges failed with BRIDGE-HOST-MISMATCH even
//! though every call target was declared for the browser.
//!
//! Fix: change the CLI default to `--target=auto`, which leaves the
//! `target_host_class_override` unset so the compile path can infer the
//! host class from the reachable bridge functions. When every reachable
//! bridge with a `hosts` field accepts `browser` AND at least one is
//! browser-restricted, the compiler picks `browser`; otherwise it falls
//! back to the historical `server` default.
//!
//! This test pins the fix by compiling a fixture that only calls the
//! browser-restricted `_ui_observe_visible` bridge:
//!
//!   - no `--target`  → inference picks `browser`, compile succeeds.
//!   - `--target=server` → explicit override wins, compile fails
//!                          with BRIDGE-HOST-MISMATCH.

use std::path::PathBuf;
use std::process::{Command, Output};

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cln/plugins/host_class_inference")
}

fn run_compile(target: Option<&str>) -> Output {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = fixture_dir();
    for name in ["main.cln", "plugin.toml"] {
        std::fs::copy(src.join(name), tmp.path().join(name))
            .unwrap_or_else(|e| panic!("copy {} failed: {}", name, e));
    }
    let mut args = vec![
        "compile".to_string(),
        "main.cln".to_string(),
        "-o".to_string(),
        "out.wasm".to_string(),
    ];
    if let Some(t) = target {
        args.push(format!("--target={}", t));
    }
    Command::new(cln_binary())
        .args(&args)
        .current_dir(tmp.path())
        .output()
        .expect("cln runs")
}

#[test]
fn no_target_infers_browser_when_only_browser_bridges_reached() {
    // Regression for 270f8fc643db: with no --target, the compiler must
    // infer host_class=browser (not fall back to `server`) when every
    // reachable bridge with a `hosts` field is browser-restricted.
    let out = run_compile(None);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "compile with no --target must succeed via browser inference; \
         got stdout=\n{}\nstderr=\n{}",
        stdout,
        stderr
    );
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        !combined.contains("BRIDGE-HOST-MISMATCH"),
        "must not emit BRIDGE-HOST-MISMATCH when inference picks browser: {}",
        combined
    );
}

#[test]
fn explicit_target_server_still_reports_mismatch() {
    // Inference must not override an explicit user choice. With
    // --target=server the compiler must still enforce the server class,
    // so a browser-only bridge produces BRIDGE-HOST-MISMATCH.
    let out = run_compile(Some("server"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{}\n{}", stderr, stdout);
    assert!(
        !out.status.success(),
        "explicit --target=server must not be overridden by inference; \
         got stdout=\n{}\nstderr=\n{}",
        stdout,
        stderr
    );
    assert!(
        combined.contains("BRIDGE-HOST-MISMATCH"),
        "diagnostic must carry BRIDGE-HOST-MISMATCH: {}",
        combined
    );
    assert!(
        combined.contains("_ui_observe_visible"),
        "diagnostic must name the offending bridge: {}",
        combined
    );
}
