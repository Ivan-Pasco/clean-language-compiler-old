//! Regression tests for plugin bridge calls.
//!
//! - `email.send(to, subject, html, text)` omits the trailing `from` argument,
//!   which frame.server declares through `param_defaults`. A dot-call on a
//!   plugin namespace resolved through the method-call path, which dropped the
//!   defaults, so the bridge wrapper was called with four arguments and the
//!   module failed WASM validation ("expected i32 but nothing on stack").
//! - `_email_configure` (port) and `_i18n_t_count` (count) take 64-bit
//!   integers (function-registry.toml, HOST_BRIDGE.md). Their imports must use
//!   `i64` for that slot or the host rejects the module at instantiation.
//!
//! The tests need frame.server and frame.locale in ~/.cleen/plugins and skip
//! when they are not installed.

use std::path::PathBuf;
use wasmparser::{Parser, Payload, TypeRef, ValType};

fn plugins_installed(names: &[&str]) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    names.iter().all(|p| {
        PathBuf::from(&home)
            .join(".cleen/plugins")
            .join(p)
            .join("plugin.wasm")
            .exists()
    })
}

fn compile(source: &str) -> Result<Vec<u8>, String> {
    let tmp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let src = tmp.path().join("main.cln");
    std::fs::write(&src, source).map_err(|e| e.to_string())?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cln"))
        .args([
            "compile",
            src.to_str().unwrap(),
            "-o",
            tmp.path().join("out.wasm").to_str().unwrap(),
        ])
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

/// Parameter types of the function the module imports as `env.<name>`.
fn import_params(wasm: &[u8], name: &str) -> Option<Vec<ValType>> {
    let mut types: Vec<Vec<ValType>> = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.ok()? {
            Payload::TypeSection(reader) => {
                for ty in reader.into_iter_err_on_gc_types() {
                    types.push(ty.ok()?.params().to_vec());
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import.ok()?;
                    if import.name == name {
                        if let TypeRef::Func(idx) = import.ty {
                            return types.get(idx as usize).cloned();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn namespace_call_fills_param_defaults() {
    if !plugins_installed(&["frame.server"]) {
        eprintln!("skipping: frame.server not installed");
        return;
    }
    let source = "plugins:\n\tframe.server\n\nstart:\n\tboolean sent = email.send(\"a@example.com\", \"hi\", \"<b>hi</b>\", \"hi\")\n\tprintl(sent)\n";
    let wasm = compile(source).unwrap_or_else(|e| panic!("{e}"));
    let params = import_params(&wasm, "_email_send").expect("module imports _email_send");
    assert_eq!(params.len(), 10, "five (ptr, len) string pairs");
}

#[test]
fn email_configure_port_is_i64() {
    if !plugins_installed(&["frame.server"]) {
        eprintln!("skipping: frame.server not installed");
        return;
    }
    let source = "plugins:\n\tframe.server\n\nserver:\n\tport: 3400\n\tmail:\n\t\thost: \"localhost\"\n\t\tport: 2525\n\t\tsecure: false\n\t\tusername: \"\"\n\t\tpassword: \"\"\n\t\tfrom: \"t@example.com\"\n\nstart:\n\tprintl(\"ready\")\n";
    let wasm = compile(source).unwrap_or_else(|e| panic!("{e}"));
    let params = import_params(&wasm, "_email_configure").expect("module imports _email_configure");
    assert_eq!(
        params[2],
        ValType::I64,
        "port is a 64-bit integer: {params:?}"
    );
}

#[test]
fn i18n_t_count_count_is_i64() {
    if !plugins_installed(&["frame.locale"]) {
        eprintln!("skipping: frame.locale not installed");
        return;
    }
    let source = "plugins:\n\tframe.locale\n\nstart:\n\tinteger n = 3\n\tprintl(tc(\"items.count\", n, \"{}\"))\n";
    let wasm = compile(source).unwrap_or_else(|e| panic!("{e}"));
    let params = import_params(&wasm, "_i18n_t_count").expect("module imports _i18n_t_count");
    assert_eq!(
        params[2],
        ValType::I64,
        "count is a 64-bit integer: {params:?}"
    );
}
