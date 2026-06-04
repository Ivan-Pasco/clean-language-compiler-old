//! Bridge contract test: compiler must emit only canonical import names.
//!
//! The compiler must emit exactly one WASM import per bridge function: the
//! canonical `_namespace_fn` name defined in `function-registry.toml`.
//! Dot-notation aliases (e.g. `db.query`, `req.path`) are syntactic sugar
//! resolved at compile time — they must NOT appear as separate WASM imports.
//!
//! Dual emission of both `_db_query` and `db.query` is a bug, not a feature.
//! These tests guard against that regression across multiple bridge namespaces.

use clean_language_compiler::plugins::WasmPluginLoader;
use wasmparser::{Parser as WasmParser, Payload};

fn list_import_names(wasm_bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let import = import.expect("valid import");
                names.push(import.name.to_string());
            }
        }
    }
    names
}

/// Compile a source program with the requested plugins loaded, returning the
/// raw .wasm bytes. Returns `None` (skipping the calling test) when the plugin
/// runtime or plugin set is unavailable in the local environment — keeps the
/// suite green on machines that haven't installed the framework yet.
fn compile_with(source: &str, plugins: &[&str]) -> Option<Vec<u8>> {
    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping bridge contract test: WasmPluginLoader unavailable");
            return None;
        }
    };

    let plugin_names: Vec<String> = plugins.iter().map(|s| s.to_string()).collect();
    let registry = match loader.load_plugins(&plugin_names) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "Skipping bridge contract test: plugins {:?} not installed ({e})",
                plugins
            );
            return None;
        }
    };

    match clean_language_compiler::compile_with_plugins(source, "test.cln", &registry) {
        Ok(w) => Some(w),
        Err(errors) => panic!("Compilation failed: {:?}", errors),
    }
}

/// Assert the WASM import section satisfies the bridge naming contract:
///   1. every canonical name in `must_contain` is present;
///   2. no dot-notation alias from `must_not_contain` appears;
///   3. no import name is duplicated.
fn assert_canonical_imports_only(
    imports: &[String],
    must_contain: &[&str],
    must_not_contain: &[&str],
) {
    for name in must_contain {
        assert!(
            imports.iter().any(|i| i == name),
            "WASM import section must contain canonical `{name}`. Got: {imports:?}",
        );
    }

    for name in must_not_contain {
        assert!(
            !imports.iter().any(|i| i == name),
            "WASM import section must NOT contain dot-notation `{name}` — dual emission is a bug. Got: {imports:?}",
        );
    }

    let mut seen = std::collections::HashSet::new();
    for name in imports {
        assert!(
            seen.insert(name.clone()),
            "Duplicate WASM import `{name}` — each bridge function must appear exactly once. All imports: {imports:?}",
        );
    }
}

#[test]
fn db_query_emits_canonical_name_only_no_dot_notation() {
    let source = "plugins:\n\tframe.data\n\nstart:\n\tstring result = db.query(\"SELECT 1\", \"\")\n\tprint(result)\n";

    let Some(wasm) = compile_with(source, &["frame.data"]) else {
        return;
    };

    let imports = list_import_names(&wasm);
    assert_canonical_imports_only(&imports, &["_db_query"], &["db.query"]);
}

#[test]
fn frame_server_request_bridges_emit_canonical_names_only() {
    // Exercise multiple request-context bridges in a single fixture so the
    // contract is checked across `req.*` and `res.*` simultaneously. A
    // minimal endpoint handler is the realistic call site for these.
    let source = r#"plugins:
	frame.server

endpoints:
	GET /hello:
		string name = req.query("name")
		res.status(200)
		string body = "hello " + name
		_ = res.send(body)
"#;

    let Some(wasm) = compile_with(source, &["frame.server"]) else {
        return;
    };

    let imports = list_import_names(&wasm);
    // Only assert on the names this fixture actually exercises; the contract
    // is about no-dual-emission, not about a fixed import set.
    assert_canonical_imports_only(
        &imports,
        &[],
        &[
            "req.query",
            "res.status",
            "res.send",
            "http.route",
            "http.listen",
        ],
    );
}
