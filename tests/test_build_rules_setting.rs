//! `build: rules = ...` decides whether state `rules:` are checked
//! (specification 1.0 §09 Build Configuration):
//!
//! | value           | development build | `--release` build |
//! |-----------------|-------------------|--------------------|
//! | `true`          | checked           | checked            |
//! | `false`         | not checked       | not checked        |
//! | `"development"` | checked           | not checked        |
//!
//! Grammar: grammar.ebnf `build_block`, `build_field`, `build_rules_value`.

use std::path::Path;
use std::process::{Command, Output};

fn program(setting: &str) -> String {
    format!(
        "build:\n\trules = {setting}\n\nstart:\n\tcount = -5\n\tprint(\"end of start\")\n\n\
         state:\n\tinteger count = 0\n\n\trules:\n\t\tcount >= 0\n"
    )
}

fn compile(dir: &Path, source: &str, release: bool) -> Output {
    let input = dir.join("app.cln");
    std::fs::write(&input, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cln"));
    command
        .args(["compile", "app.cln", "-o", "app.wasm"])
        .current_dir(dir)
        .env("HOME", dir);
    if release {
        command.arg("--release");
    }
    command.output().expect("cln runs")
}

/// Compile `source` and run it; true when the state rule was reported broken.
fn rule_violation_reported(source: &str, release: bool) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let compiled = compile(dir.path(), source, release);
    assert!(
        compiled.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let run = Command::new(env!("CARGO_BIN_EXE_wasmtime_runner"))
        .arg(dir.path().join("app.wasm"))
        .output()
        .expect("wasmtime_runner runs");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("end of start"),
        "start: must run to its end; stdout:\n{stdout}"
    );
    stdout.contains("Rule violation")
}

#[test]
fn rules_true_checks_in_every_build() {
    assert!(rule_violation_reported(&program("true"), false));
    assert!(rule_violation_reported(&program("true"), true));
}

#[test]
fn rules_false_never_checks() {
    assert!(!rule_violation_reported(&program("false"), false));
    assert!(!rule_violation_reported(&program("false"), true));
}

#[test]
fn rules_development_checks_only_in_a_development_build() {
    assert!(rule_violation_reported(&program("\"development\""), false));
    assert!(!rule_violation_reported(&program("\"development\""), true));
}
