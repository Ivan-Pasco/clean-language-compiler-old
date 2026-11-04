//! `cln check` and the library/MCP entry points must agree with `cln compile`.
//!
//! * `cln check` type-checked only the single file it was given, without
//!   module discovery or `package:` handling, so it rejected projects that
//!   `cln compile` builds: imports were undefined and every framework example
//!   failed with "No plugin registered for block type 'package:'".
//! * Type-check warnings (FUNC004, ...) were dropped: `TypeCheckResult`
//!   always carried an empty diagnostics list.
//! * A `package:` `target: browser` source compiled through the library left
//!   the thread's host-class override set to `browser`, so every later
//!   compile on that thread (MCP server, `cln test`) became a browser build.
//! * `//` comments inside a `plugins:` block were taken as plugin names.

use clean_language_compiler::{
    compile_with_external_plugins_and_opt_level, extract_plugins, set_target_host_class_override,
    target_host_class_override, type_check_with_external_plugins,
};
use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn run_cln(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(args)
        .output()
        .expect("run cln")
}

#[test]
fn check_accepts_a_multi_file_project_that_compiles() {
    let entry = fixture("tests/cln/imports/main_simple.cln");
    let entry = entry.to_str().expect("utf-8 path");
    let out_dir = tempfile::tempdir().expect("tempdir");
    let wasm = out_dir.path().join("main_simple.wasm");

    let compile = run_cln(&["compile", entry, "-o", wasm.to_str().unwrap()]);
    assert!(
        compile.status.success(),
        "cln compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let check = run_cln(&["check", entry]);
    assert!(
        check.status.success(),
        "cln check must accept what cln compile builds:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn check_reports_type_check_warnings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.cln");
    let source = "\
start:
\tprintl(pick(1).toString())

functions:
\tinteger pick(integer a)
\t\tif a > 0
\t\t\treturn 1
";
    std::fs::write(&entry, source).expect("write source");

    let result = type_check_with_external_plugins(source, entry.to_str().unwrap())
        .unwrap_or_else(|errors| panic!("type check failed: {errors:?}"));
    let codes: Vec<_> = result
        .diagnostics
        .iter()
        .filter_map(|w| w.context().and_then(|c| c.error_code.clone()))
        .collect();
    assert!(
        codes.iter().any(|c| c == "FUNC004"),
        "expected a FUNC004 warning, got {codes:?}"
    );

    let check = run_cln(&["check", entry.to_str().unwrap()]);
    assert!(check.status.success(), "warnings must not fail cln check");
    assert!(
        String::from_utf8_lossy(&check.stderr).contains("warning[FUNC004]"),
        "cln check must print the warning:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let json = run_cln(&["--json", "check", entry.to_str().unwrap()]);
    let report: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("cln --json check prints JSON");
    let diagnostics = report["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "FUNC004" && d["severity"] == "warning"),
        "JSON diagnostics must list the warning: {report}"
    );
}

#[test]
fn library_compile_restores_the_callers_host_class() {
    let source =
        "package:\n\tname: \"demo\"\n\ttarget: browser\n\nstart:\n\tinteger x = 42\n\tprint(x)\n";

    set_target_host_class_override(None);
    // Whether this compiles does not matter; it must not change the
    // host class seen by the next compile on this thread.
    let _ = compile_with_external_plugins_and_opt_level(source, "pkg_main.cln", 0);
    let after = target_host_class_override();
    set_target_host_class_override(None);

    assert_eq!(
        after, None,
        "a package `target:` must not leak into later compiles on the thread"
    );
}

#[test]
fn missing_manifest_entry_file_is_reported() {
    // A `package:` manifest whose `entry:` file does not exist used to be
    // compiled as if it were the entry itself, burying the real problem
    // under unrelated errors.
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = dir.path().join("main.cln");
    std::fs::write(
        &manifest,
        "package:\n\tname: \"demo\"\n\tentry: app_missing.cln\n",
    )
    .expect("write manifest");

    let errors = clean_language_compiler::compile_multi_file_with_memory_tier(
        &manifest,
        vec![dir.path().to_path_buf()],
        0,
        None,
        clean_language_compiler::MemoryTier::Standard,
        false,
    )
    .expect_err("a missing entry file must fail the build");
    let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    assert!(
        messages.iter().any(|m| m.contains("app_missing.cln")),
        "the error must name the missing entry file: {messages:?}"
    );
}

#[test]
fn plugins_block_comments_are_not_plugin_names() {
    let source = "\
plugins:
\tframe.server // HTTP routes
\t// frame.data
\tframe.ui

start:
\tprint(\"hi\")
";
    assert_eq!(extract_plugins(source), vec!["frame.server", "frame.ui"]);
    assert_eq!(
        extract_plugins("plugins: [frame.server, frame.ui] // web\n"),
        vec!["frame.server", "frame.ui"]
    );
}
