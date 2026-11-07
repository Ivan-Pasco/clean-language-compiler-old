//! plugin-contract.md §5.2: when a plugin declares `[exports].validate`, the
//! compiler calls it once before expanding a block (`PluginRegistry`
//! calls `FrameworkPlugin::validate` ahead of `expand_full`); a non-empty
//! result is the developer-facing error and the block is not expanded.
//!
//! The fixture plugin's `validate_block` rejects a block with a non-empty
//! body ("guard role list must be closed") and accepts an empty one; its
//! `expand_block` returns an empty expansion.

use clean_language_compiler::ast::{FrameworkBlock, SourceLocation};
use clean_language_compiler::plugins::{
    plugin_abi::{PluginExports, PluginHandles, PluginInfo, PluginManifest},
    FrameworkPlugin, WasmPluginAdapter,
};
use wasmtime::{Engine, Module};

const PLUGIN_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  ;; LP strings: 2048 = "guard role list must be closed", 2100 = ""
  (data (i32.const 2048) "\1e\00\00\00guard role list must be closed")
  (data (i32.const 2100) "\00\00\00\00")
  (func (export "validate_block") (param $name i32) (param $attrs i32) (param $body i32) (result i32)
    (if (result i32) (i32.load (local.get $body))
      (then (i32.const 2048))
      (else (i32.const 2100))))
  (func (export "expand_block") (param i32 i32 i32) (result i32)
    (i32.const 2100))
)
"#;

fn adapter(with_validate: bool) -> WasmPluginAdapter {
    let manifest = PluginManifest {
        plugin: PluginInfo {
            name: "validate_fixture".to_string(),
            version: "0.1.0".to_string(),
            description: "validate export fixture".to_string(),
            author: String::new(),
        },
        compatibility: Default::default(),
        handles: PluginHandles {
            blocks: vec!["guarded".to_string()],
            expressions: Vec::new(),
        },
        exports: PluginExports {
            expand: "expand_block".to_string(),
            validate: with_validate.then(|| "validate_block".to_string()),
            ..PluginExports::default()
        },
        blocks: Default::default(),
        bridge: Default::default(),
        language: Default::default(),
        ai: Default::default(),
        paths: Default::default(),
        enforcement: Default::default(),
        memory: Default::default(),
        build: Default::default(),
        lifecycle: Default::default(),
        artifacts: Vec::new(),
        aliases: Default::default(),
    };
    let engine = Engine::default();
    let bytes = wat::parse_str(PLUGIN_WAT).expect("fixture WAT compiles");
    let module = Module::new(&engine, bytes).expect("fixture module loads");
    WasmPluginAdapter::new("validate_fixture".to_string(), manifest, module, engine)
        .expect("adapter construction")
}

fn block(content: &str) -> FrameworkBlock {
    FrameworkBlock {
        name: "guarded".to_string(),
        content: content.to_string(),
        attributes: Vec::new(),
        location: Some(SourceLocation {
            file: "test.cln".into(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }),
        body_location: None,
    }
}

#[test]
fn validate_export_rejects_block_before_expansion() {
    let plugin = adapter(true);
    let error = plugin
        .validate(&block("GET \"/a\" [admin:"))
        .expect_err("the plugin's validate export rejects this block");
    let text = error.to_string();
    assert!(
        text.contains("guard role list must be closed"),
        "the validation message reaches the developer: {}",
        text
    );
}

#[test]
fn validate_export_accepts_valid_block() {
    let plugin = adapter(true);
    plugin
        .validate(&block(""))
        .expect("an empty validate result accepts the block");
    plugin.expand_full(&block("")).expect("the block expands");
}

#[test]
fn block_expands_when_no_validate_export_is_declared() {
    let plugin = adapter(false);
    plugin
        .validate(&block("GET \"/a\" [admin:"))
        .expect("without [exports].validate every block is accepted");
}
