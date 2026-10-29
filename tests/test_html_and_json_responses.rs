//! Regression tests for HTML and JSON responses in frame.server apps.
//!
//! - #21: `\{` / `\}` inside an `html:` block are literal braces (CSS rules).
//!   The lexer used to reject `\` outright.
//! - framework #4: an `html:` block inside an endpoint handler was never
//!   expanded — plugin-emitted functions (route handlers) were merged into the
//!   program without expanding their own framework blocks — so the route
//!   answered with an empty body.
//! - framework #5: `json(data)` in a frame.server app is the response helper
//!   (frame-server-semantics.md SRV-R003); it used to resolve to plain
//!   `json.encode` and was sent as text/plain.
//!
//! The plugin-backed tests need frame.server and frame.ui built with this
//! compiler in ~/.cleen/plugins and skip when they are not installed.

use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};
use clean_language_compiler::ast::Statement;
use std::path::PathBuf;

fn plugins_installed() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    ["frame.server", "frame.ui"]
        .iter()
        .all(|p| PathBuf::from(&home).join(".cleen/plugins").join(p).join("plugin.wasm").exists())
}

fn compile(source: &str) -> Result<Vec<u8>, String> {
    let tmp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let src = tmp.path().join("main.cln");
    std::fs::write(&src, source).map_err(|e| e.to_string())?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(["compile", "--plugins", src.to_str().unwrap(), "-o", tmp.path().join("out.wasm").to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "cln compile failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    std::fs::read(tmp.path().join("out.wasm")).map_err(|e| e.to_string())
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn html_block_header_attributes_reach_the_plugin() {
    // frame-ui.ebnf: html_block = "html" [ html_var_attribute ] ":" …
    let source = "html var=\"page\":\n\t<p>x</p>\n";
    match CleanParser::parse_single_statement(source) {
        SingleStatementParse::Statement(Statement::FrameworkBlock { name, attributes, .. }) => {
            assert_eq!(name, "html");
            assert_eq!(attributes.len(), 1);
            assert_eq!(attributes[0].name, "var");
            assert_eq!(attributes[0].value.as_deref(), Some("page"));
        }
        other => panic!("expected a framework block, got {:?}", std::mem::discriminant(&other)),
    }
}

#[test]
fn escaped_braces_in_html_block_compile_to_literal_braces() {
    if !plugins_installed() {
        eprintln!("Skipping: frame.server / frame.ui not installed");
        return;
    }
    let source = "plugins:\n\tframe.server\n\tframe.ui\n\nendpoints server:\n\tGET \"/\" :\n\t\treturn html(Css().page())\n\nclass Css\n\tfunctions:\n\t\tpublic:\n\t\t\tstring page()\n\t\t\t\thtml:\n\t\t\t\t\t<style>body \\{ color: #333 \\}</style>\n";
    let wasm = compile(source).expect("escaped braces must compile");
    assert!(contains(&wasm, "color: #333"), "the CSS text must reach the module");
}

#[test]
fn html_block_in_endpoint_is_expanded_into_an_html_response() {
    if !plugins_installed() {
        eprintln!("Skipping: frame.server / frame.ui not installed");
        return;
    }
    let source = "plugins:\n\tframe.server\n\tframe.ui\n\nendpoints server:\n\tGET \"/\" :\n\t\tstring who = \"world\"\n\t\thtml:\n\t\t\t<p>inline {who}</p>\n";
    let wasm = compile(source).expect("an html: block in a handler must compile");
    assert!(contains(&wasm, "inline "), "the page text must be expanded into the handler");
    assert!(contains(&wasm, "text/html"), "the handler must answer with text/html");
}

#[test]
fn json_helper_in_endpoint_is_a_json_response() {
    if !plugins_installed() {
        eprintln!("Skipping: frame.server / frame.ui not installed");
        return;
    }
    let source = "plugins:\n\tframe.server\n\nendpoints server:\n\tGET \"/\" :\n\t\tlist<integer> nums = [1, 2, 3]\n\t\treturn json(nums)\n";
    let wasm = compile(source).expect("json() in a handler must compile");
    let text = String::from_utf8_lossy(&wasm);
    assert!(text.contains("jsonResponse"), "json() must resolve to the jsonResponse helper");
}
