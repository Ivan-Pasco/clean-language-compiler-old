//! Regression test for **CODEGEN-WASM-STACK-MISMATCH** /
//! **CODEGEN_STACK_REMAINING** (introduced by the 0.30.286
//! CLIENT_BUILD_ENTRY_LEAK fix, fixed in 0.30.293).
//!
//! Background: when a Clean Language program calls a plugin bridge that is
//! declared for a host class the current build does not target (e.g. a
//! `target: server` build calling a `hosts = ["browser"]` bridge from
//! frame.ui), the compiler substitutes a no-op stub for the bridge so the
//! build still succeeds. The original 0.30.286 implementation registered
//! that stub as a local WASM function during `register_plugin_bridge_imports`
//! — which runs **mid-import-phase**. WASM's funcidx space puts every
//! imported function before any local function, so registering a local
//! mid-phase shifted the indices of every subsequent import. The downstream
//! `Call(N)` for a function whose index moved would then fail wasmparser
//! validation with "values remaining on stack at end of block".
//!
//! This test compiles a small program that triggers exactly that case
//! (frame.ui's `_ui_shortcut_register` has `hosts = ["browser"]` while the
//! build targets the server host class) and asserts the output is a valid
//! WASM module. Before the 0.30.293 fix this compile aborts at
//! `wasmparser` validation.

use clean_language_compiler::compile_with_plugins;
use clean_language_compiler::plugins::WasmPluginLoader;

#[test]
fn host_mismatched_stub_does_not_shift_subsequent_import_indices() {
    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!(
                "Skipping CODEGEN-WASM-STACK-MISMATCH regression: WasmPluginLoader unavailable"
            );
            return;
        }
    };
    let registry = match loader.load_plugins(&["frame.ui".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "Skipping CODEGEN-WASM-STACK-MISMATCH regression: frame.ui not installed ({e})"
            );
            return;
        }
    };

    // _ui_shortcut_register declares hosts = ["browser"] in frame.ui's
    // plugin.toml. A server-target build (which is the default when no
    // explicit `target:` is given) triggers the host-mismatch stub path.
    //
    // Calling it in a `return` expression forces an import for the stub +
    // many additional plugin bridges that frame.ui auto-injects, so any
    // funcidx shift will cause a downstream Call() to land on the wrong
    // function and fail wasmparser validation.
    let source = "\
plugins:
\tframe.ui

functions:
\tinteger test()
\t\treturn _ui_shortcut_register(\"a\", \"b\", \"c\")
";

    let result = compile_with_plugins(source, "regression_test.cln", &registry);

    match result {
        Ok(wasm_bytes) => {
            assert!(!wasm_bytes.is_empty(), "expected a non-empty .wasm payload");
            assert!(
                wasm_bytes.starts_with(b"\0asm"),
                "expected a WASM magic header — got {:?}",
                &wasm_bytes[..wasm_bytes.len().min(4)]
            );
            // `compile_with_plugins` runs wasmparser validation before
            // returning Ok; the very fact that we got bytes here proves
            // the produced module is well-formed.
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("Error: {e}");
            }
            panic!(
                "CODEGEN-WASM-STACK-MISMATCH regression: host-mismatched bridge \
                 stub shifted subsequent import funcidx and produced invalid WASM"
            );
        }
    }
}
