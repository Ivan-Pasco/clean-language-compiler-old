//! Password hashing bridges are server-only.
//!
//! `_crypto_hash_password` / `_crypto_verify_password` are bcrypt on the
//! server. The browser has no bcrypt, so function-registry.toml declares both
//! with `hosts = ["server"]`: a browser build that reaches either bridge must
//! fail at compile time with BRIDGE-HOST-MISMATCH (bridge-host-classes.md §6),
//! and a server build must still compile.

use std::path::PathBuf;
use std::process::{Command, Output};

fn compile(target: &str) -> Output {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cln/plugins/password_hash_server_only/main.cln");
    std::fs::copy(&src, tmp.path().join("main.cln")).expect("copy fixture");
    Command::new(env!("CARGO_BIN_EXE_cln"))
        .args([
            "compile",
            "main.cln",
            "-o",
            "out.wasm",
            &format!("--target={}", target),
        ])
        .current_dir(tmp.path())
        .env("CLEEN_TELEMETRY_FORCE", "local")
        .output()
        .expect("cln runs")
}

#[test]
fn browser_build_calling_password_hash_fails_with_host_mismatch() {
    let out = compile("browser");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.status.success(),
        "a browser build calling the password-hash bridges must fail: {}",
        combined
    );
    assert!(
        combined.contains("BRIDGE-HOST-MISMATCH"),
        "diagnostic must carry BRIDGE-HOST-MISMATCH: {}",
        combined
    );
    assert!(
        combined.contains("_crypto_hash_password") && combined.contains("_crypto_verify_password"),
        "diagnostic must name both password bridges: {}",
        combined
    );
}

#[test]
fn server_build_calling_password_hash_compiles() {
    let out = compile("server");
    assert!(
        out.status.success(),
        "a server build calling the password-hash bridges must compile: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
