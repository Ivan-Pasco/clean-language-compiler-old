//! `_i18n_configure` is a server-only registry bridge.
//!
//! frame.locale expands a `locale:` block into one `_i18n_configure(default,
//! fallback, detection, path)` call, so the compiler's registry must declare
//! it (otherwise the plugin fails PLUGIN-REGISTRY-DRIFT). It loads translation
//! files from disk, so `hosts = ["server"]`: a server build compiles and
//! imports it with four (ptr, len) string pairs, and a browser build fails
//! with BRIDGE-HOST-MISMATCH. `locale.current` is the grammar's name for
//! `_i18n_locale` (frame-locale.ebnf §3).

use std::path::PathBuf;
use std::process::{Command, Output};

fn compile(target: &str) -> (Output, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cln/plugins/i18n_configure_server_only/main.cln");
    std::fs::copy(&src, tmp.path().join("main.cln")).expect("copy fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_cln"))
        .args([
            "compile",
            "main.cln",
            "-o",
            "out.wasm",
            &format!("--target={}", target),
        ])
        .current_dir(tmp.path())
        .env("CLEEN_TELEMETRY_FORCE", "local")
        .output()
        .expect("cln runs");
    (out, tmp)
}

fn text(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn server_build_imports_i18n_configure_with_four_string_pairs() {
    let (out, tmp) = compile("server");
    assert!(
        out.status.success(),
        "server build must compile: {}",
        text(&out)
    );
    let wasm = std::fs::read(tmp.path().join("out.wasm")).expect("out.wasm");
    let mut func_types: Vec<usize> = Vec::new();
    let mut configure_type = None;
    let mut locale = false;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload {
            Ok(wasmparser::Payload::TypeSection(reader)) => {
                for group in reader {
                    for ty in group.expect("rec group").types() {
                        if let wasmparser::CompositeType::Func(f) = &ty.composite_type {
                            func_types.push(f.params().len());
                        } else {
                            func_types.push(usize::MAX);
                        }
                    }
                }
            }
            Ok(wasmparser::Payload::ImportSection(reader)) => {
                for import in reader {
                    let import = import.expect("import");
                    if let wasmparser::TypeRef::Func(idx) = import.ty {
                        if import.name == "_i18n_configure" {
                            configure_type = Some(idx as usize);
                        }
                        if import.name == "_i18n_locale" {
                            locale = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let idx = configure_type.expect("module must import env._i18n_configure");
    assert_eq!(
        func_types[idx], 8,
        "_i18n_configure takes four strings as (ptr, len) pairs"
    );
    assert!(locale, "locale.current() must resolve to env._i18n_locale");
}

#[test]
fn browser_build_calling_i18n_configure_fails_with_host_mismatch() {
    let (out, _tmp) = compile("browser");
    let combined = text(&out);
    assert!(
        !out.status.success(),
        "a browser build must fail: {}",
        combined
    );
    assert!(
        combined.contains("BRIDGE-HOST-MISMATCH") && combined.contains("_i18n_configure"),
        "diagnostic must carry BRIDGE-HOST-MISMATCH naming _i18n_configure: {}",
        combined
    );
}
