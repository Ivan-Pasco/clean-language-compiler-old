//! Every syntax error the parser collects reaches the caller as its own
//! diagnostic (issue #70): the library API, `cln --json check` and the MCP
//! parse tool all see them.

use std::process::Command;

const OUT_OF_ORDER: &str =
    "class A\n\tinteger x\n\nstate:\n\tinteger c = 0\n\nstart:\n\tprint(1)\n";

#[test]
fn parse_to_ast_returns_every_error() {
    let errors = clean_language_compiler::parse_to_ast(OUT_OF_ORDER, "t.cln")
        .expect_err("out-of-order sections must not parse");
    let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    assert_eq!(errors.len(), 2, "{messages:?}");
    assert!(messages[0].contains("'state:' section is out of order"));
    assert!(messages[1].contains("'start:' section is out of order"));
}

#[test]
fn json_check_lists_every_error() {
    let dir = std::env::temp_dir().join(format!("cln_parse_all_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.cln");
    std::fs::write(&file, OUT_OF_ORDER).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(["--json", "check"])
        .arg(&file)
        .output()
        .expect("cln runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!output.status.success());
    assert!(
        stdout.contains("'state:' section is out of order"),
        "{stdout}"
    );
    assert!(
        stdout.contains("'start:' section is out of order"),
        "{stdout}"
    );
}
