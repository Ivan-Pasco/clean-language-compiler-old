//! `cln test` runs `tests:` blocks with `wasmtime_runner` and fails when a
//! test fails.
//!
//! - The runner is found next to the real `cln` binary when `cln` is a
//!   symlink (`~/.cleen/bin/cln` -> `~/.cleen/versions/<v>/cln`).
//! - Without a runner that reports failures, `cln test` is an error instead
//!   of passing through a runner that cannot see them.
//! - Directory arguments run the `.cln` files inside them.
//! - Tests of a file without `start:` run once.
//! - `--json` writes one JSON document to stdout.

use std::path::Path;
use std::process::{Command, Output};

const FAILING_TESTS: &str = "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b\n\ntests:\n\t\"wrong\": add(1, 2) = 4\n\t\"right\": add(1, 2) = 3\n";
const PASSING_TESTS: &str = "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b\n\ntests:\n\t\"right\": add(1, 2) = 3\n";

fn run(cln: &Path, args: &[&str], cwd: &Path) -> Output {
    Command::new(cln)
        .args(args)
        .current_dir(cwd)
        .env("HOME", cwd)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("cln runs")
}

fn cln(args: &[&str], cwd: &Path) -> Output {
    run(Path::new(env!("CARGO_BIN_EXE_cln")), args, cwd)
}

fn text(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[cfg(unix)]
#[test]
fn failing_tests_fail_through_a_symlinked_cln() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("bin")).unwrap();
    let link = tmp.path().join("bin/cln");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cln"), &link).unwrap();
    std::fs::write(tmp.path().join("t.cln"), FAILING_TESTS).unwrap();

    let out = run(&link, &["test", "t.cln"], tmp.path());
    let (stdout, stderr) = text(&out);
    assert!(!out.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains("1 test(s) failed"), "{stdout}{stderr}");
}

#[cfg(unix)]
#[test]
fn without_wasmtime_runner_cln_test_is_an_error() {
    use std::os::unix::fs::PermissionsExt;

    // A copy of cln with no runner beside it or on PATH, and a clean-runner
    // that exits 0 whatever happens: the run must not pass through it.
    let tmp = tempfile::TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::create_dir(tmp.path().join("bin")).unwrap();
    let alone = tmp.path().join("bin/cln");
    if std::fs::hard_link(env!("CARGO_BIN_EXE_cln"), &alone).is_err() {
        std::fs::copy(env!("CARGO_BIN_EXE_cln"), &alone).unwrap();
    }
    let fake_dir = tmp.path().join(".cleen/clean-runner/9.9.9");
    std::fs::create_dir_all(&fake_dir).unwrap();
    let fake = fake_dir.join("clean-runner");
    std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(tmp.path().join("t.cln"), FAILING_TESTS).unwrap();

    let out = run(&alone, &["test", "t.cln"], tmp.path());
    let (stdout, stderr) = text(&out);
    assert!(!out.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains("wasmtime_runner not found"), "{stderr}");
}

#[test]
fn directory_argument_runs_its_test_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("suite")).unwrap();
    std::fs::write(tmp.path().join("suite/a.cln"), PASSING_TESTS).unwrap();

    let out = cln(&["test", "suite"], tmp.path());
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("PASS: right"), "{stdout}");

    std::fs::write(tmp.path().join("suite/b.cln"), FAILING_TESTS).unwrap();
    let out = cln(&["test", "suite"], tmp.path());
    assert!(
        !out.status.success(),
        "a failing file in the directory fails the run"
    );
}

#[test]
fn tests_of_a_file_without_start_run_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("t.cln"), PASSING_TESTS).unwrap();
    let out = cln(&["test", "t.cln"], tmp.path());
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert_eq!(stdout.matches("PASS: right").count(), 1, "{stdout}");
}

#[test]
fn json_test_output_is_one_json_document() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("t.cln"), FAILING_TESTS).unwrap();
    let out = cln(&["--json", "test", "t.cln"], tmp.path());
    let (stdout, stderr) = text(&out);
    assert!(!out.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{stdout}\nstderr:\n{stderr}"));
    assert_eq!(json["success"], false);
    let file = &json["files"][0];
    assert_eq!(file["file"], "t.cln");
    assert_eq!(file["passed"], false);
    assert_eq!(
        file["output"],
        serde_json::json!(["FAIL: wrong", "PASS: right"])
    );
}
