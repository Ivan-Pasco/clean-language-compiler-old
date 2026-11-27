//! `cln dev-queue list` and `cln dev-queue show` honour the global `--json`
//! flag: stdout carries only JSON (an array for `list`, an object for `show`).

use std::path::Path;
use std::process::{Command, Output};

const QUEUE: &str = r#"[
  {
    "fingerprint": "aaaa1111bbbb2222",
    "first_seen_at": "2026-11-01T10:00:00Z",
    "last_seen_at": "2026-11-02T10:00:00Z",
    "occurrences": 3,
    "error_code": "SEM001",
    "component": "compiler",
    "message": "first message",
    "file_context": "a.cln",
    "compiler_version": "0.33.194",
    "dev_reason": "dev build"
  },
  {
    "fingerprint": "cccc3333dddd4444",
    "first_seen_at": "2026-11-03T10:00:00Z",
    "last_seen_at": "2026-11-04T10:00:00Z",
    "occurrences": 1,
    "error_code": "SEM002",
    "component": "compiler",
    "message": "second message",
    "file_context": null,
    "compiler_version": "0.33.194",
    "dev_reason": "dev build"
  }
]"#;

fn cln(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .output()
        .expect("cln runs")
}

fn home_with_queue(queue: Option<&str>) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    if let Some(queue) = queue {
        let dir = tmp.path().join(".cleen").join("telemetry");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dev_queue.json"), queue).unwrap();
    }
    tmp
}

fn stdout_json(out: &Output) -> serde_json::Value {
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}); stdout:\n{text}"))
}

#[test]
fn list_json_prints_entries_newest_first() {
    let home = home_with_queue(Some(QUEUE));
    let out = cln(&["dev-queue", "list", "--json"], home.path());
    assert!(out.status.success());
    let json = stdout_json(&out);
    let entries = json.as_array().expect("an array");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["fingerprint"], "cccc3333dddd4444");
    assert_eq!(entries[1]["error_code"], "SEM001");
    assert_eq!(entries[1]["occurrences"], 3);
}

#[test]
fn list_json_on_empty_queue_is_an_empty_array() {
    let home = home_with_queue(None);
    let out = cln(&["dev-queue", "list", "--json"], home.path());
    assert!(out.status.success());
    assert_eq!(stdout_json(&out), serde_json::json!([]));
}

#[test]
fn show_json_prints_one_entry() {
    let home = home_with_queue(Some(QUEUE));
    let out = cln(&["dev-queue", "show", "aaaa", "--json"], home.path());
    assert!(out.status.success());
    let json = stdout_json(&out);
    assert_eq!(json["fingerprint"], "aaaa1111bbbb2222");
    assert_eq!(json["message"], "first message");
    assert_eq!(json["file_context"], "a.cln");
}

#[test]
fn show_json_unknown_prefix_fails_without_stdout_noise() {
    let home = home_with_queue(Some(QUEUE));
    let out = cln(&["dev-queue", "show", "zzzz", "--json"], home.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty());
}

#[test]
fn list_without_json_keeps_the_table() {
    let home = home_with_queue(Some(QUEUE));
    let out = cln(&["dev-queue", "list"], home.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("fingerprint"), "table header; got:\n{text}");
}
