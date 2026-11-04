//! The MCP stdio transport is newline-delimited JSON: every message is one
//! line of JSON. `cln mcp-server` answered every request with an LSP-style
//! `Content-Length:` header, so a spec-conforming client read a non-JSON
//! line before each response. Responses now use the framing of the request.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn start_server() -> std::process::Child {
    let home = tempfile::tempdir().expect("tempdir").keep();
    Command::new(env!("CARGO_BIN_EXE_cln"))
        .arg("mcp-server")
        // Isolated home: no telemetry config, so no background report flush.
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start cln mcp-server")
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;

#[test]
fn newline_delimited_request_gets_a_json_line_response() {
    let mut server = start_server();
    let mut stdin = server.stdin.take().expect("stdin");
    writeln!(stdin, "{INITIALIZE}").expect("send request");
    stdin.flush().expect("flush");

    let mut reader = BufReader::new(server.stdout.take().expect("stdout"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");
    drop(stdin);
    let _ = server.wait();

    let response: serde_json::Value = serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("response line is not JSON ({e}): {line:?}"));
    assert_eq!(response["id"], 1);
    assert!(
        response["result"].is_object(),
        "unexpected response: {response}"
    );
}

#[test]
fn content_length_request_gets_a_content_length_response() {
    let mut server = start_server();
    let mut stdin = server.stdin.take().expect("stdin");
    write!(
        stdin,
        "Content-Length: {}\r\n\r\n{}",
        INITIALIZE.len(),
        INITIALIZE
    )
    .expect("send request");
    stdin.flush().expect("flush");

    let mut reader = BufReader::new(server.stdout.take().expect("stdout"));
    let mut header = String::new();
    reader.read_line(&mut header).expect("read header");
    let length: usize = header
        .trim()
        .strip_prefix("Content-Length:")
        .unwrap_or_else(|| panic!("expected a Content-Length header, got {header:?}"))
        .trim()
        .parse()
        .expect("numeric length");
    let mut blank = String::new();
    reader.read_line(&mut blank).expect("read separator");
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).expect("read body");
    drop(stdin);
    let _ = server.wait();

    let response: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(response["id"], 1);
}
