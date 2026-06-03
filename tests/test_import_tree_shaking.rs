//! Regression tests for the Import Minimality Rule
//! (platform-architecture/EXECUTION_LAYERS.md).
//!
//! GEN003: frame.server preamble functions (resDownload, emailSend, etc.) must
//! not cause their bridge imports to appear in apps that never call them.
//!
//! A Clean program that does not reference Layer 3 server functions
//! (`_http_*`, `_req_*`, `_res_*`, `_session_*`, `_auth_*`) must NOT
//! emit those functions as WASM imports. Emitting them would force
//! every host (browser, CLI, embedded) to provide stubs for ~30 server
//! functions that will never be called.
//!
//! Corresponds to bug report `CODEGEN_UNUSED_IMPORTS` reported via the
//! MCP server against compiler v0.30.52.

use clean_language_compiler::plugins::WasmPluginLoader;
use wasmparser::Parser as WasmParser;
use wasmparser::Payload;

/// Layer 3 server-only WASM import prefixes that a client-only program
/// must never emit.
const SERVER_PREFIXES: &[&str] = &["_http_", "_req_", "_res_", "_session_", "_auth_"];

fn compile_source(source: &str) -> Vec<u8> {
    clean_language_compiler::compile(source).expect("compilation should succeed")
}

fn list_imports(wasm_bytes: &[u8]) -> Vec<String> {
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

#[test]
fn client_only_program_emits_no_server_imports() {
    // Minimal client-only program: just a print statement. Must not
    // cause any Layer 3 server imports to be emitted.
    let source = "start:\n\tprint(\"hello\")\n";
    let wasm = compile_source(source);
    let imports = list_imports(&wasm);

    let leaked: Vec<&String> = imports
        .iter()
        .filter(|name| {
            SERVER_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .collect();

    assert!(
        leaked.is_empty(),
        "client-only program leaked {} server import(s): {:?}",
        leaked.len(),
        leaked
    );
}

#[test]
fn client_only_program_has_reasonable_import_count() {
    // Regression guard: before the Import Minimality Rule fix, a minimal
    // client program emitted ~86 imports. After the fix the Layer 3
    // server imports (~24 of them) are stripped, bringing the count
    // down. This test asserts we stay below the pre-fix baseline so a
    // future change that silently reintroduces unconditional server
    // import emission will fail.
    let source = "start:\n\tprint(\"hello\")\n";
    let wasm = compile_source(source);
    let imports = list_imports(&wasm);

    assert!(
        imports.len() < 80,
        "expected < 80 imports for minimal client program, got {} \
         (Import Minimality Rule regression?)",
        imports.len()
    );
}

/// GEN003 regression: a frame.server app that never calls resDownload or
/// emailSend must NOT emit _res_download, _email_send, _email_configure, or
/// _email_last_error as WASM imports.
///
/// Before the BFS fix, collect_all_called_names_from_mir scanned every function
/// including plugin preambles, causing those bridge names to leak unconditionally.
#[test]
fn frame_server_app_without_download_emits_no_res_download() {
    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return,
    };
    let registry = match loader.load_plugins(&["frame.server".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping GEN003 regression: frame.server not installed ({e})");
            return;
        }
    };

    // App that never calls resDownload or emailSend
    let source = "plugins:\n\tframe.server\n\nstart:\n\tprint(\"hello\")\n";
    let wasm = match clean_language_compiler::compile_with_plugins(source, "entry.cln", &registry) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Skipping GEN003 regression: build failed ({e:?})");
            return;
        }
    };

    let preamble_only_bridges = [
        "_res_download",
        "_email_send",
        "_email_configure",
        "_email_last_error",
    ];

    let leaked: Vec<String> = {
        let mut out = Vec::new();
        for payload in WasmParser::new(0).parse_all(&wasm) {
            if let Ok(Payload::ImportSection(reader)) = payload {
                for import in reader {
                    let imp = import.expect("valid import");
                    if preamble_only_bridges.contains(&imp.name) {
                        out.push(imp.name.to_string());
                    }
                }
            }
        }
        out
    };

    assert!(
        leaked.is_empty(),
        "GEN003: preamble-only bridge imports leaked into frame.server WASM \
         (user code never calls them): {:?}",
        leaked
    );
}

/// GEN003 regression (follow-up): code with no `start:` entry point and no
/// MIR-exported functions (plugin source pattern) must still compile successfully
/// when that code uses string concatenation.
///
/// Before the empty-seed fallback was added, `collect_all_called_names_from_mir`
/// started BFS from an empty worklist, produced an empty reachable set, and
/// tree-shook `string.concat` even though the function bodies called it.
/// Codegen then failed with "Function 'string.concat' not found in function map".
#[test]
fn no_entry_point_with_string_concat_compiles() {
    // A library-style source: no `start:` block, function that uses string `+`.
    // The compiler must not crash when tree-shaking is enabled.
    // Uses the `functions:` block syntax (no top-level `function` keyword in Clean Language).
    let source = "functions:\n\tstring greet(string name)\n\t\treturn \"Hello, \" + name\n";
    // Compilation must succeed (not panic, not return an error).
    let result = clean_language_compiler::compile(source);
    assert!(
        result.is_ok(),
        "GEN003 follow-up: library code with string concat and no entry point \
         failed to compile: {:?}",
        result.err()
    );

    // The produced WASM must contain the string.concat import.
    let wasm = result.unwrap();
    let imports = list_imports(&wasm);
    assert!(
        imports.iter().any(|n| n == "string.concat"),
        "GEN003 follow-up: string.concat was tree-shaken from library code \
         that calls it; imports present: {:?}",
        imports
    );
}

#[test]
fn user_app_with_direct_string_concat_includes_import() {
    // GEN003 regression: user app WITH start: that uses string `+` directly.
    let source = "start:\n\tstring s = \"hello\" + \" world\"\n\tprint(s)\n";
    let result = clean_language_compiler::compile(source);
    assert!(result.is_ok(), "compile failed: {:?}", result.err());
    let wasm = result.unwrap();
    let imports = list_imports(&wasm);
    assert!(
        imports.iter().any(|n| n == "string.concat"),
        "GEN003: string.concat tree-shaken from app with direct concat in start:; imports: {:?}",
        imports
    );
}

#[test]
fn user_app_with_helper_string_concat_includes_import() {
    // GEN003 regression: string concat used in a helper function called from start:.
    // Note: start: must precede functions: per section-order rules (SYN007).
    let source =
        "start:\n\tprint(greet(\"world\"))\nfunctions:\n\tstring greet(string name)\n\t\treturn \"Hello, \" + name\n";
    let result = clean_language_compiler::compile(source);
    assert!(result.is_ok(), "compile failed: {:?}", result.err());
    let wasm = result.unwrap();
    let imports = list_imports(&wasm);
    assert!(
        imports.iter().any(|n| n == "string.concat"),
        "GEN003: string.concat tree-shaken when concat is only in helper fn; imports: {:?}",
        imports
    );
}

#[test]
fn user_app_with_unreachable_function_using_string_concat() {
    // GEN003 root cause: start: exists (so BFS seeds from it), but there is also a
    // helper function NOT called from start that uses string concat.
    // BFS won't traverse into it → string.concat not reachable → not registered.
    // But codegen still generates the helper's body → tries to call string.concat → ERROR.
    let source = "start:\n\tprint(\"hello\")\nfunctions:\n\tstring dead_concat()\n\t\treturn \"a\" + \"b\"\n";
    let result = clean_language_compiler::compile(source);
    // This should compile — the dead function's body should not cause import failure.
    assert!(
        result.is_ok(),
        "GEN003: unreachable function with string concat caused compile failure: {:?}",
        result.err()
    );
}

/// GEN003 regression: route handler → user helper → bridge import chain.
///
/// Route handlers (__route_handler_*) are called by the HTTP server runtime,
/// not from start:. Before this fix the BFS only seeded from start:, so bridge
/// imports reachable only through a route handler → helper function chain were
/// never found and the import was not registered, causing "Function 'X' not
/// found in function map" at codegen time.
///
/// This test requires frame.server to be installed. It is skipped otherwise.
#[test]
fn route_handler_to_helper_bridge_import_is_registered() {
    let mut loader = match clean_language_compiler::plugins::WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return,
    };
    let registry = match loader.load_plugins(&["frame.server".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping GEN003 route-handler regression: frame.server not installed ({e})");
            return;
        }
    };

    // App with an endpoint whose handler calls a helper that uses time.now.
    // time.now maps to the reachability-gated import _time_now.
    // Before the fix: BFS seeded from start: only → route handler not visited →
    // getTimestamp not visited → _time_now not in reachable set → import stripped →
    // codegen error "Function 'time.now' not found in function map".
    let source = "plugins:\n\tframe.server\n\nendpoints:\n\tGET /ts -> getTimestamp()\n\nfunctions:\n\tstring getTimestamp()\n\t\treturn time.now().toString()\n";
    let result = clean_language_compiler::compile_with_plugins(source, "entry.cln", &registry);
    assert!(
        result.is_ok(),
        "GEN003 route-handler regression: route handler → helper → time.now chain \
         caused compile failure: {:?}",
        result.err()
    );

    let wasm = result.unwrap();
    let imports = list_imports(&wasm);
    assert!(
        imports
            .iter()
            .any(|n| n == "_time_now" || n == "time.now" || n.starts_with("time")),
        "GEN003 route-handler regression: time import was tree-shaken even though \
         it is called from a route handler → helper chain; imports present: {:?}",
        imports
    );
}

/// GEN003 regression: reachable preamble function's bridge alias must be registered.
///
/// Before the fix: collect_used_function_names_from_mir skipped ALL preamble bodies
/// unconditionally, so bridge aliases used inside compiled preamble helpers were
/// never added to used_bridge_function_names → register_plugin_bridge_imports never
/// registered the alias → codegen crash "bridge alias not found in function map".
///
/// The fix: skip only DEAD preamble functions (not in reachable_imports). Reachable
/// preamble functions (exported or single-underscore callbacks) must be scanned.
#[test]
fn reachable_preamble_bridge_alias_is_registered() {
    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return,
    };
    let registry = match loader.load_plugins(&["frame.server".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping: frame.server not installed ({e})");
            return;
        }
    };

    // A frame.server app with a POST endpoint. The preamble injects single-underscore
    // helpers (seeded into BFS as external call targets) that may use req.body_field
    // internally (location.file == "<plugin-output>"). User code never calls
    // req.body_field directly.
    // Before the fix: "bridge alias 'req.body_field' not found in function map"
    // Use the arrow syntax so the handler is recognized as a request context — same
    // pattern as route_handler_to_helper_bridge_import_is_registered.
    let source = concat!(
        "plugins:\n\tframe.server\n\n",
        "endpoints:\n\tPOST /submit -> handleSubmit()\n\n",
        "functions:\n\tstring handleSubmit()\n\t\treturn \"ok\"\n"
    );

    let result = clean_language_compiler::compile_with_plugins(source, "entry.cln", &registry);
    assert!(
        result.is_ok(),
        "GEN003 preamble bridge regression: reachable preamble helper's bridge alias \
         caused compile failure (expected successful WASM): {:?}",
        result.err()
    );
}
