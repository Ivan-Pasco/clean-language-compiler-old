//! Plugin Contracts v2 Phase B (cycle 1) — `clean.abi_version` stamp emission.
//!
//! Asserts that `cln compile --target=plugin` writes a `clean.abi_version`
//! WASM custom section into the produced plugin.wasm:
//!   (a) when the sibling plugin.toml declares `[compatibility].abi_version`,
//!       the stamp matches that value verbatim;
//!   (b) when the sibling plugin.toml omits `[compatibility]` entirely, the
//!       stamp defaults to `DEFAULT_RUNTIME_ABI_VERSION` ("1.0.0") so every
//!       plugin built by this compiler carries an unambiguous ABI tag.
//!
//! See `foundation/spec/plugins/contracts/runtime-abi.md` §4.
//! Cycle 2 will add loader-side refusal of unsupported versions.

use std::process::Command;
use tempfile::TempDir;
use wasmparser::{Parser, Payload};

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

const PLUGIN_SOURCE: &str =
    "functions:\n\tstring expand_block(string name, string attrs, string body)\n\t\treturn body\n";

fn write_plugin_project(root: &std::path::Path, toml_body: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("plugin.toml"), toml_body).unwrap();
    std::fs::write(root.join("src/main.cln"), PLUGIN_SOURCE).unwrap();
}

/// Compile the project rooted at `root` as a plugin, then return the
/// `clean.abi_version` custom-section payload if present.
fn compile_and_read_abi_stamp(root: &std::path::Path) -> Option<String> {
    let status = Command::new(cln_binary())
        .args([
            "compile",
            "src/main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .current_dir(root)
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "cln compile --target=plugin must succeed");

    let bytes = std::fs::read(root.join("plugin.wasm")).expect("plugin.wasm exists");
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Ok(Payload::CustomSection(reader)) = payload {
            if reader.name() == "clean.abi_version" {
                return Some(
                    std::str::from_utf8(reader.data())
                        .expect("stamp payload is UTF-8")
                        .to_string(),
                );
            }
        }
    }
    None
}

#[test]
fn declared_abi_version_is_stamped_verbatim() {
    let tmp = TempDir::new().unwrap();
    let toml_body = "[plugin]\nname = \"test.abi_stamp\"\nversion = \"1.0.0\"\n\n\
                     [compatibility]\nabi_version = \"1.0.0\"\n";
    write_plugin_project(tmp.path(), toml_body);

    let stamp = compile_and_read_abi_stamp(tmp.path())
        .expect("clean.abi_version custom section must be present");
    assert_eq!(
        stamp, "1.0.0",
        "stamp must equal the declared [compatibility].abi_version"
    );
}

#[test]
fn missing_compatibility_section_defaults_to_runtime_abi_version() {
    let tmp = TempDir::new().unwrap();
    let toml_body = "[plugin]\nname = \"test.abi_stamp\"\nversion = \"1.0.0\"\n";
    write_plugin_project(tmp.path(), toml_body);

    let stamp = compile_and_read_abi_stamp(tmp.path())
        .expect("clean.abi_version custom section must be present even without [compatibility]");
    assert_eq!(
        stamp,
        clean_language_compiler::plugins::plugin_abi::DEFAULT_RUNTIME_ABI_VERSION,
        "absent [compatibility] must default to DEFAULT_RUNTIME_ABI_VERSION"
    );
}

#[test]
fn user_code_build_does_not_carry_abi_stamp() {
    // Non-plugin targets must NOT be stamped — the ABI contract is scoped to
    // plugin WASM per runtime-abi.md §4 ("user code doesn't get the stamp").
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.cln"), "start:\n\tprintl(\"hi\")\n").unwrap();

    let status = Command::new(cln_binary())
        .args(["compile", "hello.cln", "-o", "hello.wasm"])
        .current_dir(tmp.path())
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "user-code compile must succeed");

    let bytes = std::fs::read(tmp.path().join("hello.wasm")).expect("hello.wasm exists");
    let has_stamp = Parser::new(0)
        .parse_all(&bytes)
        .any(|p| matches!(p, Ok(Payload::CustomSection(ref r)) if r.name() == "clean.abi_version"));
    assert!(
        !has_stamp,
        "non-plugin builds must not emit a clean.abi_version custom section"
    );
}
