//! Plugin Contracts v2 §2.5 — end-to-end smoke test for the build_state
//! bridges from a real compiled Clean plugin source.
//!
//! Compiles `tests/cln/plugins/build_state_round_trip/main.cln` as a plugin and
//! drives it through the SAME `WasmPluginAdapter` host code production uses,
//! so the full ~50 runtime stubs (mem_alloc, string_compare, string.concat,
//! …) plus the build_state bridges are all in scope. Calls `expand_block`
//! twice via the registry's `expand` dispatch.
//!
//! If either call traps, the bug described in
//! `compiler-frame-ui-v2-followup-trap-and-silent-orchestrator.md` §1
//! is reproduced in isolation — no frame.ui, no framework dependency.
//!
//! Regression coverage for `CMP-PLUGIN-ABI-BUILD-STATE-SET-MISMATCH`:
//! the test plugin source now declares `_build_state_set` and
//! `_build_state_get` via `plugin.toml` `[bridge]` with `expand_strings =
//! true`, matching the canonical contract in
//! `foundation/platform-architecture/function-registry.toml`. The compiler
//! emits the raw `(ptr, len, ptr, len) -> ()` import and a wrapper that
//! adapts LP-pointer call sites. The host adapter at
//! `src/plugins/wasm_adapter.rs::register_build_state_bridges` registers
//! the same shape. Any future drift on either side will trap this test.
//!
//! Why declare bridges via plugin.toml rather than an `external:` block:
//! when the compiler builds the import signature from an `external:` block,
//! `string` params collapse to a single i32 (LP-pointer) and a `void` return
//! threads through `HirType::Void → ConcreteType::Null → MirType::Ptr(Void)`
//! which `mir_type_to_wasm_type_for_import` maps to `WasmType::I32` — a
//! double drift from the canonical contract. plugin.toml's `[bridge]` path
//! goes through `register_plugin_bridge_imports`, which honors
//! `expand_strings = true` and the explicit `returns = "void"`. Shipped
//! plugins (frame.ui, frame.server) all use this path.
//!
//! Related dashboard reports verified non-reproducing on cln 0.30.348 with
//! frame.ui 2.12.34 / frame.server 2.7.10 installed (compile cleanly with no
//! diagnostics — the compiler/plugin combination that originally tripped each
//! one has shipped enough fixes that the symptom no longer fires):
//!
//! - `FRAME_UI_HTML_EXPANSION_LITERAL_NEWLINE_IN_STRING` (2026-05-01) — a
//!   minimal `html: <div>Hello</div>` block in a `plugins: frame.ui` program
//!   compiles cleanly. The original tokenizer crash on plugin-emitted literal
//!   newlines was closed by subsequent lexer commits in the 0.30.99–0.30.348
//!   range (29-character lexer pass-through extensions + plugin-side
//!   escape-handling rework).
//! - `E001` "Plugin 'frame.server' failed to expand 'endpoints:'… Pairs
//!   literal keys must be constant literals" (2026-04-29) — an
//!   `endpoints:`/`GET /test`/`return "ok"` program compiles cleanly. The
//!   original Pairs-literal rejection on plugin output was closed by the
//!   parser/HIR object-literal-field-value preservation work (b343e79a) plus
//!   the plugin's own template rework.

use std::fs;
use std::path::PathBuf;

use clean_language_compiler::ast::SourceLocation;
use clean_language_compiler::plugins::plugin_abi::{
    PluginCompatibility, PluginExports, PluginHandles, PluginInfo, PluginManifest,
};
use clean_language_compiler::plugins::{FrameworkBlock, FrameworkPlugin, WasmPluginAdapter};
use wasmtime::{Engine, Module};

fn synthetic_manifest(name: &str) -> PluginManifest {
    PluginManifest {
        plugin: PluginInfo {
            name: name.to_string(),
            version: "0.0.1".to_string(),
            description: String::new(),
            author: String::new(),
        },
        compatibility: PluginCompatibility::default(),
        handles: PluginHandles {
            blocks: vec!["build_state_round_trip".to_string()],
            expressions: Vec::new(),
        },
        exports: PluginExports {
            expand: "expand_block".to_string(),
            ..PluginExports::default()
        },
        bridge: Default::default(),
        language: Default::default(),
        ai: Default::default(),
        paths: Default::default(),
        enforcement: Default::default(),
        memory: Default::default(),
        build: Default::default(),
        lifecycle: Default::default(),
        artifacts: Vec::new(),
        blocks: Default::default(),
    }
}

fn build_block(content: &str) -> FrameworkBlock {
    FrameworkBlock {
        name: "build_state_round_trip".to_string(),
        content: content.to_string(),
        attributes: Vec::new(),
        location: Some(SourceLocation {
            file: "smoke-test".into(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }),
    }
}

#[test]
fn build_state_round_trip_does_not_trap() {
    // 1. Compile the .cln source to plugin WASM bytes (same path `cleen` uses).
    let source_path = "tests/cln/plugins/build_state_round_trip/main.cln";
    let source = fs::read_to_string(source_path).unwrap_or_else(|e| {
        panic!("plugin source {} unreadable: {}", source_path, e);
    });
    let wasm = clean_language_compiler::compile_for_plugin(&source, source_path).unwrap_or_else(
        |errors| {
            for e in &errors {
                eprintln!("compile error: {}", e);
            }
            panic!("plugin source failed to compile — {} errors", errors.len());
        },
    );

    // 2. Build a WasmPluginAdapter with a synthetic manifest. The adapter
    //    sets up the full host environment that production uses (all
    //    runtime stubs + the build_state bridges).
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("plugin WASM must instantiate");
    let manifest = synthetic_manifest("test.build_state_round_trip");
    let adapter = WasmPluginAdapter::new(
        "test.build_state_round_trip".to_string(),
        manifest,
        module,
        engine,
    )
    .expect("adapter construction must succeed");

    // 3. Call `expand_block` twice via the FrameworkPlugin trait.
    //    First call: key absent → if branch sets the keystore to "first".
    //    Second call: key present → else branch concatenates ";next".
    //    If either call traps, the test fails with the trap diagnostic.
    let first = adapter
        .expand(&build_block("body-1"))
        .expect("first expand_block must not trap");
    let _ = first;
    let second = adapter
        .expand(&build_block("body-2"))
        .expect("second expand_block must not trap");
    let _ = second;
}

/// Sister test: ensure the smoke plugin even compiles to plugin WASM. If
/// `compile_for_plugin` regresses for an `external:` block declaring void
/// returns, this catches it before the round-trip test loads the bytes.
#[test]
fn build_state_round_trip_plugin_compiles() {
    let source_path = "tests/cln/plugins/build_state_round_trip/main.cln";
    let source = fs::read_to_string(source_path).expect("source readable");
    let wasm = clean_language_compiler::compile_for_plugin(&source, source_path);
    assert!(
        wasm.is_ok(),
        "build_state_round_trip.cln must compile as a plugin: {:?}",
        wasm.err()
    );
    let bytes = wasm.unwrap();
    assert!(bytes.len() > 1024, "plugin WASM unexpectedly tiny");

    // Sanity-check the imports include both build_state bridges. The
    // canonical signatures (set: (i32,i32,i32,i32) -> (); get: (i32,i32) -> i32)
    // are validated end-to-end by `build_state_round_trip_does_not_trap`,
    // which fails on any signature drift at instantiation time.
    let bytes_str = String::from_utf8_lossy(&bytes);
    assert!(
        bytes_str.contains("_build_state_set"),
        "plugin must import _build_state_set"
    );
    assert!(
        bytes_str.contains("_build_state_get"),
        "plugin must import _build_state_get"
    );

    // Drop the artifact next to a known temp path so other tests / manual
    // inspection can read it (used by the round-trip test as a fast path).
    let _ = fs::write(PathBuf::from("/tmp/build_state_round_trip.wasm"), &bytes);
}
