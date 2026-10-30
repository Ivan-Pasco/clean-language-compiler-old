//! `cln` subcommands must exit non-zero when they fail.
//!
//! `debug`, `parse`, `lint` and `run` used to print an error and then return
//! success, so scripts and CI treated failed runs as passing. These tests run
//! the built binary and check the exit status, plus the lint fixes that came
//! with it: directories are scanned for `.cln` files, and `--errors-only`
//! still reports errors (it only hides warnings).

use std::path::Path;
use std::process::{Command, Output};

const VALID_SOURCE: &str = "start:\n\tinteger x = 42\n\tprint(x)\n";
const TYPE_ERROR_SOURCE: &str = "start:\n\tinteger x = \"hello\"\n\tprint(x)\n";

fn cln(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("cln runs")
}

fn combined_output(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn assert_fails(out: &Output, what: &str) {
    assert!(
        !out.status.success(),
        "{what} must exit non-zero; output:\n{}",
        combined_output(out)
    );
}

fn assert_succeeds(out: &Output, what: &str) {
    assert!(
        out.status.success(),
        "{what} must exit zero; output:\n{}",
        combined_output(out)
    );
}

#[test]
fn debug_missing_file_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert_fails(
        &cln(&["debug", "missing.cln"], tmp.path()),
        "debug <missing>",
    );
}

#[test]
fn parse_missing_file_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert_fails(
        &cln(&["parse", "missing.cln"], tmp.path()),
        "parse <missing>",
    );
}

#[test]
fn parse_invalid_source_fails_and_valid_source_succeeds() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    assert_fails(&cln(&["parse", "bad.cln"], tmp.path()), "parse <bad>");
    assert_succeeds(&cln(&["parse", "good.cln"], tmp.path()), "parse <good>");
}

#[test]
fn lint_invalid_path_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert_fails(
        &cln(&["lint", "no/such/path"], tmp.path()),
        "lint <bad path>",
    );
}

#[test]
fn lint_with_errors_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();
    assert_fails(&cln(&["lint", "bad.cln"], tmp.path()), "lint <bad>");
}

#[test]
fn lint_errors_only_still_reports_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();
    let full = cln(&["lint", "bad.cln"], tmp.path());
    let errors_only = cln(&["lint", "--errors-only", "bad.cln"], tmp.path());
    assert_fails(&errors_only, "lint --errors-only <bad>");
    assert!(
        combined_output(&errors_only).contains("Type mismatch")
            || combined_output(&errors_only).contains("error"),
        "--errors-only must still print the compile error; output:\n{}",
        combined_output(&errors_only)
    );
    // The error diagnostics printed with and without --errors-only match.
    assert_eq!(
        String::from_utf8_lossy(&full.stderr),
        String::from_utf8_lossy(&errors_only.stderr),
        "--errors-only must not change the reported errors"
    );
}

#[test]
fn lint_directory_finds_cln_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    let out = cln(&["lint", "."], tmp.path());
    assert_succeeds(&out, "lint <dir>");
    assert!(
        combined_output(&out).contains("Files checked: 1"),
        "lint <dir> must pick up .cln files; output:\n{}",
        combined_output(&out)
    );
}

#[test]
fn run_missing_file_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert_fails(&cln(&["run", "missing.cln"], tmp.path()), "run <missing>");
}

#[test]
fn run_unsupported_extension_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "hello").unwrap();
    std::fs::write(tmp.path().join("noext"), "hello").unwrap();
    assert_fails(&cln(&["run", "notes.txt"], tmp.path()), "run <.txt>");
    assert_fails(&cln(&["run", "noext"], tmp.path()), "run <no extension>");
}

#[test]
fn debug_show_ast_prints_the_ast() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    let out = cln(&["debug", "--show-ast", "good.cln"], tmp.path());
    assert_succeeds(&out, "debug --show-ast <good>");
    let text = combined_output(&out);
    assert!(
        text.contains("AST Structure") && !text.contains("not available"),
        "debug --show-ast must print the AST; output:\n{text}"
    );
}
