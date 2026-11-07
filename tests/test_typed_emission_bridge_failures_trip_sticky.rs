//! A typed-emission bridge that fails must trip the sticky-error flag
//! (typed-emission.md §3.15) so the expansion fails, even when the plugin
//! ignores the bridge's return value. `_expr_call`, `_stmt_call` and
//! `_emit_class` used to return 0/1 silently on malformed input, and the
//! block expanded "successfully" with the call or class missing.

use clean_language_compiler::ast::FrameworkBlock;
use clean_language_compiler::plugins::{
    plugin_abi::{PluginManifest, EMISSION_OPS_HASH},
    FrameworkPlugin, WasmPluginAdapter,
};
use wasmtime::{Engine, Module};

fn manifest() -> PluginManifest {
    toml::from_str(&format!(
        "[plugin]\nname = \"sticky\"\nversion = \"0.0.1\"\n\
         [compatibility]\nexpansion_version = \"3.0.0\"\nemission_ops_hash = \"{}\"\n\
         [handles]\nblocks = [\"pilot\"]\n[exports]\nexpand = \"expand_block_typed\"\n",
        EMISSION_OPS_HASH
    ))
    .unwrap()
}

/// A plugin whose expand export makes one bridge call with a malformed
/// payload, ignores the result and reports success.
fn plugin_calling(import: &str, call: &str) -> String {
    format!(
        r#"
        (module
          {import}
          (memory (export "memory") 1)
          (global (export "__heap_ptr") (mut i32) (i32.const 4096))
          (data (i32.const 64) "\01\00\00\00f")
          (data (i32.const 80) "\07\00\00\00notjson")
          (func (export "expand_block_typed")
                (param $ctx i32) (param i32) (param i32) (param i32) (result i32)
            {call}
            drop
            i32.const 0))
        "#
    )
}

fn expand(wat: &str) -> Result<(), String> {
    let engine = Engine::default();
    let module = Module::new(&engine, wat).expect("compile WAT");
    let adapter =
        WasmPluginAdapter::new("sticky".to_string(), manifest(), module, engine).expect("adapter");
    let block = FrameworkBlock {
        name: "pilot".to_string(),
        content: String::new(),
        attributes: Vec::new(),
        location: None,
    };
    adapter
        .expand_full(&block)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn malformed_call_arguments_fail_the_expansion() {
    for bridge in ["_expr_call", "_stmt_call"] {
        let wat = plugin_calling(
            &format!(r#"(import "env" "{bridge}" (func $b (param i32 i32 i32) (result i32)))"#),
            "local.get $ctx\n i32.const 64\n i32.const 80\n call $b",
        );
        let err = expand(&wat).expect_err(bridge);
        assert!(err.contains(bridge), "{}: {}", bridge, err);
    }
}

#[test]
fn malformed_class_description_fails_the_expansion() {
    let wat = plugin_calling(
        r#"(import "env" "_emit_class" (func $b (param i32 i32 i32 i32) (result i32)))"#,
        "local.get $ctx\n i32.const 64\n i32.const 80\n i32.const 0\n call $b",
    );
    let err = expand(&wat).expect_err("_emit_class");
    assert!(err.contains("_emit_class"), "{}", err);
}
