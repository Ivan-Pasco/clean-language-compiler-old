//! `cln` output modes and argument handling.
//!
//! - `--quiet` shows errors only: diagnostics stay, success chatter goes.
//! - `--json` writes nothing but JSON to stdout.
//! - Colors are off when the output is not a terminal.
//! - Commands fail (exit 1) on bad input instead of falling back to something
//!   else: `cln test` without files, `cln explain` with an unknown code.
//! - `cln parse` only parses; `cln report` prints the GitHub issue command.

use std::path::Path;
use std::process::{Command, Output};

const VALID_SOURCE: &str = "start:\n\tinteger x = 42\n\tprint(x)\n";
const TYPE_ERROR_SOURCE: &str = "start:\n\tinteger x = \"hello\"\n\tprint(x)\n";

fn cln(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(args)
        .current_dir(cwd)
        // Keep any bug-report handling local and deterministic.
        .env("CLEEN_TELEMETRY_FORCE", "publish")
        .env("HOME", cwd)
        .output()
        .expect("cln runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn quiet_still_reports_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();
    let out = cln(&["-q", "compile", "bad.cln", "-o", "bad.wasm"], tmp.path());
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("error"),
        "--quiet must still print the diagnostics; stderr:\n{}",
        stderr(&out)
    );
    assert!(!stdout(&out).contains("Successfully compiled"));
}

#[test]
fn quiet_success_prints_nothing_on_stdout() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    let out = cln(
        &["-q", "compile", "good.cln", "-o", "good.wasm"],
        tmp.path(),
    );
    assert!(out.status.success(), "stderr:\n{}", stderr(&out));
    assert_eq!(stdout(&out), "", "--quiet means errors only");
    assert!(tmp.path().join("good.wasm").exists());
}

#[test]
fn json_mode_emits_only_json_on_stdout() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();

    let ok = cln(
        &["--json", "compile", "good.cln", "-o", "good.wasm"],
        tmp.path(),
    );
    assert!(ok.status.success(), "stderr:\n{}", stderr(&ok));
    let value: serde_json::Value = serde_json::from_str(&stdout(&ok))
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", stdout(&ok)));
    assert_eq!(value["success"], serde_json::json!(true));

    let bad = cln(
        &["--json", "compile", "bad.cln", "-o", "bad.wasm"],
        tmp.path(),
    );
    assert!(!bad.status.success());
    let diagnostics: serde_json::Value = serde_json::from_str(&stdout(&bad))
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", stdout(&bad)));
    assert!(diagnostics.as_array().is_some_and(|d| !d.is_empty()));
}

#[test]
fn lint_and_debug_honour_quiet() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    for args in [
        vec!["-q", "lint", "good.cln"],
        vec!["-q", "debug", "good.cln"],
    ] {
        let out = cln(&args, tmp.path());
        assert!(out.status.success(), "{args:?} stderr:\n{}", stderr(&out));
        assert_eq!(stdout(&out), "", "{args:?}: --quiet means errors only");
    }
}

#[test]
fn lint_debug_and_check_emit_json() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();
    let json_of = |args: &[&str]| -> serde_json::Value {
        let out = cln(args, tmp.path());
        serde_json::from_str(&stdout(&out))
            .unwrap_or_else(|e| panic!("{args:?}: stdout is not JSON ({e}):\n{}", stdout(&out)))
    };

    let lint = json_of(&["--json", "lint", "good.cln"]);
    assert_eq!(lint["success"], true);
    assert_eq!(lint["files"][0]["file"], "good.cln");
    let lint = json_of(&["--json", "lint", "bad.cln"]);
    assert_eq!(lint["success"], false);
    assert!(lint["files"][0]["diagnostics"]
        .as_array()
        .is_some_and(|d| !d.is_empty()));

    let debug = json_of(&["--json", "debug", "good.cln", "--check-style"]);
    assert_eq!(debug["success"], true);
    assert!(debug["wasm_size_bytes"].as_u64().is_some_and(|n| n > 0));
    assert!(debug["style_issues"].is_array());

    // The plugin lint pass is part of `check --json`, not skipped.
    let check = json_of(&["--json", "check", "good.cln"]);
    assert!(check["lint"].is_array(), "{check}");
}

#[test]
fn missing_import_or_plugin_is_not_reported_as_a_toolchain_bug() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Telemetry on, so a toolchain bug would print the issue command.
    std::fs::create_dir_all(tmp.path().join(".cleen/telemetry")).unwrap();
    std::fs::write(
        tmp.path().join(".cleen/telemetry/config.toml"),
        "enabled = true\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("m.cln"),
        "import:\n\tNoSuchModule\n\nstart:\n\tprint(\"x\")\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("p.cln"),
        "plugins:\n\tframe.nonexistent\n\nstart:\n\tprint(\"x\")\n",
    )
    .unwrap();
    for file in ["m.cln", "p.cln"] {
        let out = cln(&["compile", file, "-o", "out.wasm"], tmp.path());
        assert!(!out.status.success());
        assert!(
            !stderr(&out).contains("bug in the Clean Language toolchain"),
            "{file}: a project error was reported as a toolchain bug:\n{}",
            stderr(&out)
        );
    }
}

#[test]
fn no_color_codes_when_not_a_terminal() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    std::fs::write(tmp.path().join("bad.cln"), TYPE_ERROR_SOURCE).unwrap();
    for args in [
        vec!["check", "good.cln"],
        vec!["compile", "bad.cln", "-o", "b.wasm"],
    ] {
        let out = cln(&args, tmp.path());
        let all = format!("{}{}", stdout(&out), stderr(&out));
        assert!(
            !all.contains('\u{1b}'),
            "{args:?} printed ANSI codes:\n{all:?}"
        );
    }
}

#[test]
fn test_without_files_is_an_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = cln(&["test"], tmp.path());
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("no test files given"),
        "stderr:\n{}",
        stderr(&out)
    );
}

#[test]
fn explain_unknown_code_fails_and_known_code_comes_from_the_registry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let unknown = cln(&["explain", "NOPE123"], tmp.path());
    assert!(!unknown.status.success());
    assert!(stderr(&unknown).contains("Unknown error code"));

    let known = cln(&["--json", "explain", "sem003"], tmp.path());
    assert!(known.status.success(), "stderr:\n{}", stderr(&known));
    let value: serde_json::Value = serde_json::from_str(&stdout(&known)).unwrap();
    assert_eq!(value["code"], serde_json::json!("SEM003"));
    assert_eq!(value["name"], serde_json::json!("SymbolRedefinition"));
}

#[test]
fn parse_only_parses() {
    let tmp = tempfile::TempDir::new().unwrap();
    // A type error is not a parse error.
    std::fs::write(tmp.path().join("typed.cln"), TYPE_ERROR_SOURCE).unwrap();
    let out = cln(&["parse", "typed.cln"], tmp.path());
    assert!(out.status.success(), "stderr:\n{}", stderr(&out));
    assert!(stdout(&out).contains("Parsed typed.cln"));
    assert!(!stdout(&out).contains("WASM"));

    std::fs::write(
        tmp.path().join("broken.cln"),
        "start:\n\tinteger x = (1 +\n",
    )
    .unwrap();
    let out = cln(&["parse", "broken.cln"], tmp.path());
    assert!(!out.status.success());
}

#[test]
fn report_prints_the_github_issue_command() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = cln(
        &[
            "report",
            "--error",
            "COM001",
            "--description",
            "it's broken",
            "--component",
            "server",
        ],
        tmp.path(),
    );
    assert!(out.status.success(), "stderr:\n{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains(
        "gh issue create --repo Ivan-Pasco/clean-server-old --label bug --label compiler-old"
    ));
    // The apostrophe in the description is quoted for the shell.
    assert!(text.contains(r"it'\''s broken"), "{text}");
    assert!(!text.contains("submitted") && !text.contains("will be sent"));
}

#[test]
fn removed_dashboard_commands_are_gone() {
    let tmp = tempfile::TempDir::new().unwrap();
    for args in [
        vec!["fixes"],
        vec!["telemetry", "flush"],
        vec!["runtime"],
        vec!["simple-test"],
    ] {
        let out = cln(&args, tmp.path());
        assert!(!out.status.success(), "{args:?} must not exist any more");
    }
}

#[test]
fn compiling_never_enables_telemetry_or_prompts() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("good.cln"), VALID_SOURCE).unwrap();
    let out = cln(&["compile", "good.cln", "-o", "good.wasm"], tmp.path());
    assert!(out.status.success(), "stderr:\n{}", stderr(&out));
    // HOME is the temp dir: no telemetry config may be written.
    assert!(!tmp.path().join(".cleen/telemetry/config.toml").exists());
}
