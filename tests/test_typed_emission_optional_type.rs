//! `_type_optional(ctx, inner)` builds the nullable type `T?`
//! (typed-emission.md §3.10). Only a `T?` holds none (type-system.md §8), so
//! a plugin that emits a function returning none on some path declares it
//! with this type — frame.data's `first` query returns `Model?`
//! (frame-data-semantics.md DAT-Q001).

use clean_language_compiler::ast::{FrameworkBlock, SourceLocation, Type};
use clean_language_compiler::plugins::{
    plugin_abi::{
        PluginBlockConfig, PluginCompatibility, PluginExports, PluginHandles, PluginInfo,
        PluginManifest,
    },
    FrameworkPlugin, WasmPluginAdapter,
};
use std::collections::HashMap;
use wasmtime::{Engine, Module};

/// Emits `Item? maybe()` whose body returns the string literal "x" — the
/// body is irrelevant here, only the declared return type is checked.
const OPTIONAL_RETURN_WAT: &str = r#"
(module
  (import "env" "_emit_function"   (func $_emit_function   (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"      (func $_stmt_block      (param i32 i32) (result i32)))
  (import "env" "_stmt_return"     (func $_stmt_return     (param i32 i32) (result i32)))
  (import "env" "_expr_string_lit" (func $_expr_string_lit (param i32 i32) (result i32)))
  (import "env" "_type_class_ref"  (func $_type_class_ref  (param i32 i32) (result i32)))
  (import "env" "_type_optional"   (func $_type_optional   (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0)  "\05\00\00\00maybe")
  (data (i32.const 16) "\02\00\00\00[]")
  (data (i32.const 32) "\03\00\00\00[4]")
  (data (i32.const 48) "\04\00\00\00Item")
  (data (i32.const 64) "\01\00\00\00x")
  (func (export "expand_block_typed")
        (param $ctx i32) (param $name i32) (param $attrs i32) (param $body i32)
        (result i32)
    (local $class_h i32) (local $opt_h i32) (local $lit_h i32) (local $ret_h i32) (local $block_h i32)
    ;; handle 1: Item
    (local.set $class_h (call $_type_class_ref (local.get $ctx) (i32.const 48)))
    ;; handle 2: Item? (consumes 1)
    (local.set $opt_h (call $_type_optional (local.get $ctx) (local.get $class_h)))
    ;; handle 3: "x"
    (local.set $lit_h (call $_expr_string_lit (local.get $ctx) (i32.const 64)))
    ;; handle 4: return "x"
    (local.set $ret_h (call $_stmt_return (local.get $ctx) (local.get $lit_h)))
    ;; handle 5: block([4])
    (local.set $block_h (call $_stmt_block (local.get $ctx) (i32.const 32)))
    (drop (call $_emit_function (local.get $ctx) (i32.const 0) (i32.const 16)
                                (local.get $opt_h) (local.get $block_h) (i32.const 0)))
    i32.const 0))
"#;

fn manifest() -> PluginManifest {
    let mut blocks = HashMap::new();
    blocks.insert(
        "pilot".to_string(),
        PluginBlockConfig {
            expand: Some("expand_block_typed".to_string()),
            version: Some(3),
        },
    );
    PluginManifest {
        plugin: PluginInfo {
            name: "optional_type_pilot".to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            author: String::new(),
        },
        compatibility: PluginCompatibility {
            expansion_version: Some("3.0.0".to_string()),
            ..PluginCompatibility::default()
        },
        handles: PluginHandles {
            blocks: vec!["pilot".to_string()],
            expressions: Vec::new(),
        },
        exports: PluginExports {
            expand: "expand_block_typed".to_string(),
            ..PluginExports::default()
        },
        blocks,
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
    }
}

#[test]
fn type_optional_wraps_the_inner_type() {
    let wasm = wat::parse_str(OPTIONAL_RETURN_WAT).expect("WAT must parse");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("module must compile");
    let adapter = WasmPluginAdapter::new(
        "optional_type_pilot".to_string(),
        manifest(),
        module,
        engine,
    )
    .expect("adapter construction must succeed");
    let block = FrameworkBlock {
        name: "pilot".to_string(),
        content: String::new(),
        attributes: Vec::new(),
        location: Some(SourceLocation {
            file: "test".into(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }),
        body_location: None,
    };

    let expansion = adapter.expand_full(&block).expect("expansion must succeed");
    assert_eq!(expansion.functions.len(), 1);
    assert_eq!(
        expansion.functions[0].return_type,
        Type::Nullable(Box::new(Type::Object("Item".to_string()))),
        "_type_optional must build `Item?`"
    );
}
