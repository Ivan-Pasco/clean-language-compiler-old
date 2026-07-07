//! Regression test for bug 99fd4557f821 — COD000 tree-shake gap for preamble helpers.
//!
//! Frame.server (and similar v3 plugins) emit module helpers as preamble
//! functions (location.file == "<plugin-output>"). Each helper's body calls
//! a Layer 3 host bridge such as `_http_respond`. Before the fix, the
//! codegen tree-shook `_http_respond` from the import section whenever the
//! BFS reachability scan did not directly reach the helper's Call site,
//! and codegen then failed with:
//!
//!   Function '_http_respond' (SymbolId(N)) not found in function map
//!   during code generation
//!
//! Fix (mir_codegen/mod.rs): after populating `reachable_imports` from the
//! BFS, run `collect_used_function_names_from_mir` and extend
//! `reachable_imports` with the resulting bridge names. This closes the
//! disagreement between the BFS scan (which excludes server-only bridges
//! found in dead code and in preamble bodies) and the plain-walk scan
//! (which correctly identifies bridges used by any retained function).
//!
//! Surfaced during the 2026-07-06 dashboard triage after the two SYN001
//! parser fixes (ff689b2222b3, 0f628d47cad7) unblocked the endpoint body
//! and let codegen run.

use std::path::PathBuf;

fn frame_server_plugin_available() -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        let root = PathBuf::from(home).join(".cleen/plugins/frame.server");
        return root.exists();
    }
    false
}

fn frame_ui_plugin_available() -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        let root = PathBuf::from(home).join(".cleen/plugins/frame.ui");
        return root.exists();
    }
    false
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
            "cln compile failed:\nstdout=\n{}\nstderr=\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(std::fs::read(tmp.path().join("out.wasm")).unwrap_or_default())
}

#[test]
fn endpoint_returning_string_compiles_without_cod000() {
    if !frame_server_plugin_available() {
        eprintln!("Skipping: frame.server plugin not installed");
        return;
    }
    // Bug 99fd4557f821 minimal shape: endpoint body returns a string directly
    // without calling any of the frame.server preamble helpers (jsonResponse,
    // htmlResponse, etc.). Before the fix, frame.server still emits
    // `jsonResponse` as a preamble helper, and its `_http_respond` call
    // caused the tree-shake gap.
    let source = concat!(
        "plugins:\n",
        "\tframe.server\n",
        "\n",
        "class Foo\n",
        "\tfunctions:\n",
        "\t\tpublic:\n",
        "\t\t\tstring bar()\n",
        "\t\t\t\treturn \"ok\"\n",
        "\n",
        "endpoints server:\n",
        "\tGET \"/api/health\" :\n",
        "\t\treturn Foo().bar()\n",
    );
    let bytes = compile(source).expect("compile must succeed");
    assert!(
        bytes.starts_with(b"\0asm"),
        "output must be a valid WASM module"
    );
}

#[test]
fn endpoint_with_html_class_returning_string_compiles_without_cod000() {
    if !frame_server_plugin_available() || !frame_ui_plugin_available() {
        eprintln!("Skipping: frame.server or frame.ui plugin not installed");
        return;
    }
    // Same shape as above but with an html: block class method — this is
    // the original 17fa6a334e68 dashboard repro. Before the fix it hit the
    // same COD000 tree-shake bug.
    let source = concat!(
        "plugins:\n",
        "\tframe.server\n",
        "\tframe.ui\n",
        "\n",
        "class Hero\n",
        "\tstring title\n",
        "\tstring cta\n",
        "\n",
        "\tconstructor(string titleParam, string ctaParam)\n",
        "\t\ttitle = titleParam\n",
        "\t\tcta = ctaParam\n",
        "\n",
        "\tfunctions:\n",
        "\t\tpublic:\n",
        "\t\t\tstring render()\n",
        "\t\t\t\thtml:\n",
        "\t\t\t\t\t<section>\n",
        "\t\t\t\t\t\t<h1>{title}</h1>\n",
        "\t\t\t\t\t\t<a class=\"btn\">{cta}</a>\n",
        "\t\t\t\t\t</section>\n",
        "\n",
        "endpoints server:\n",
        "\tGET \"/\" :\n",
        "\t\tHero h = Hero(\"Hello world\", \"Get started\")\n",
        "\t\treturn h.render()\n",
    );
    let bytes = compile(source).expect("compile must succeed");
    assert!(
        bytes.starts_with(b"\0asm"),
        "output must be a valid WASM module"
    );
}
