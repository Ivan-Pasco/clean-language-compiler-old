//! JSON-RPC request handling of `cln mcp-server`.
//!
//! - `ping` is answered (MCP requires it).
//! - A request with `"id": null` is a request, not a notification, and gets
//!   a reply; only a missing `id` makes a notification.
//! - `tools/call` without `arguments` works for tools with no required
//!   parameters.
//! - `get_specification` works outside the compiler source tree (the spec is
//!   embedded).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Send newline-delimited requests and return one parsed response per
/// request that expects one, in order.
fn exchange(requests: &[&str], expected_responses: usize) -> Vec<serde_json::Value> {
    let home = tempfile::tempdir().expect("tempdir");
    let cwd = tempfile::tempdir().expect("tempdir");
    let mut server = Command::new(env!("CARGO_BIN_EXE_cln"))
        .arg("mcp-server")
        .env("HOME", home.path())
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start cln mcp-server");
    let mut stdin = server.stdin.take().expect("stdin");
    for request in requests {
        writeln!(stdin, "{request}").expect("send request");
    }
    stdin.flush().expect("flush");
    drop(stdin);

    let reader = BufReader::new(server.stdout.take().expect("stdout"));
    let responses: Vec<serde_json::Value> = reader
        .lines()
        .map(|l| serde_json::from_str(&l.expect("line")).expect("JSON response"))
        .collect();
    let _ = server.wait();
    assert_eq!(
        responses.len(),
        expected_responses,
        "unexpected responses: {responses:?}"
    );
    responses
}

#[test]
fn ping_is_answered() {
    let responses = exchange(&[r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#], 1);
    assert_eq!(responses[0]["id"], 7);
    assert_eq!(responses[0]["result"], serde_json::json!({}));
}

#[test]
fn null_id_gets_a_reply_and_missing_id_does_not() {
    let responses = exchange(
        &[
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        ],
        1,
    );
    assert!(responses[0]["id"].is_null());
    assert!(responses[0]["result"].is_object());
}

#[test]
fn wrong_jsonrpc_version_is_rejected() {
    let responses = exchange(&[r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#], 1);
    assert_eq!(responses[0]["error"]["code"], -32600);
}

#[test]
fn tools_call_without_arguments_and_embedded_specification() {
    let responses = exchange(
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_quick_reference"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_specification"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_specification","arguments":{"section":"Expressions"}}}"#,
        ],
        3,
    );
    for response in &responses {
        assert!(response["error"].is_null(), "unexpected error: {response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
    }
    let toc = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(toc.contains("Overview"), "table of contents: {toc}");
    let section = responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        section.contains("Operator Precedence"),
        "section: {section}"
    );
}
