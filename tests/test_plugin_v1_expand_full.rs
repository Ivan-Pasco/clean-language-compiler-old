//! v1 (string-emission) plugin ABI regressions for `WasmPluginAdapter`.
//!
//! 1. The ORM binding header (`<type> <name> =`, prepended by the expander to
//!    `Model.<verb>:` block content) must be stripped before the plugin's
//!    `expand_block` sees the body, and reattached to the plugin's output.
//!    `call_expand` did this but `call_expand_full` — the path the expander
//!    actually uses — did not, so v1 plugins received the header as part of
//!    the body.
//! 2. A negative pointer returned by `expand_block` must surface as an error,
//!    not a host panic (`ptr as usize` used to overflow in `read_result`).

use clean_language_compiler::ast::{Expression, Statement};
use clean_language_compiler::plugins::plugin_abi::{
    PluginCompatibility, PluginExports, PluginHandles, PluginInfo, PluginManifest,
};
use clean_language_compiler::plugins::{FrameworkBlock, FrameworkPlugin, WasmPluginAdapter};
use wasmtime::{Engine, Module};

/// A v1 plugin whose `expand_block` echoes the body it receives — unless the
/// body starts with `i` (the first byte of an `integer n =` binding header),
/// in which case it returns the literal `leaked_header`, so a header that
/// reaches the plugin is visible in the expansion.
const ECHO_PLUGIN_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (data (i32.const 2048) "\0d\00\00\00leaked_header")
  (func (export "expand_block") (param i32 i32 i32) (result i32)
    (if (result i32)
      (i32.eq (i32.load8_u (i32.add (local.get 2) (i32.const 4))) (i32.const 105))
      (then (i32.const 2048))
      (else (local.get 2)))))
"#;

/// A v1 plugin whose `expand_block` returns a negative pointer.
const NEGATIVE_POINTER_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "expand_block") (param i32 i32 i32) (result i32)
    (i32.const -1)))
"#;

fn v1_manifest(name: &str) -> PluginManifest {
    PluginManifest {
        plugin: PluginInfo {
            name: name.to_string(),
            version: "0.0.1".to_string(),
            description: String::new(),
            author: String::new(),
        },
        compatibility: PluginCompatibility::default(),
        handles: PluginHandles {
            blocks: Vec::new(),
            expressions: vec!["Model.count".to_string()],
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

fn adapter(name: &str, wat: &str) -> WasmPluginAdapter {
    let engine = Engine::default();
    let module = Module::new(&engine, wat).expect("test plugin WAT must compile");
    WasmPluginAdapter::new(name.to_string(), v1_manifest(name), module, engine)
        .expect("adapter construction must succeed")
}

fn orm_block(content: &str) -> FrameworkBlock {
    FrameworkBlock {
        name: "Model.count".to_string(),
        content: content.to_string(),
        attributes: Vec::new(),
        location: None,
    }
}

fn inline_statements(
    expansion: clean_language_compiler::plugins::PluginExpansion,
) -> Vec<Statement> {
    expansion.into_inline_statements()
}

#[test]
fn v1_expand_full_strips_and_reattaches_orm_binding_header() {
    let plugin = adapter("test.v1_echo", ECHO_PLUGIN_WAT);
    let expansion = plugin
        .expand_full(&orm_block("integer n =\n__count()"))
        .expect("expand_full must succeed");
    let stmts = inline_statements(expansion);

    assert_eq!(stmts.len(), 1, "expected one statement, got {:?}", stmts);
    match &stmts[0] {
        Statement::VariableDecl {
            name, initializer, ..
        } => {
            assert_eq!(name, "n");
            assert!(
                matches!(initializer, Some(Expression::Call(callee, _)) if callee == "__count"),
                "binding must wrap the plugin's expression, got {:?}",
                initializer
            );
        }
        other => panic!(
            "binding header must be reattached as a variable declaration, got {:?}",
            other
        ),
    }
}

#[test]
fn v1_expand_and_expand_full_agree_on_orm_binding() {
    let plugin = adapter("test.v1_echo", ECHO_PLUGIN_WAT);
    let via_expand = plugin
        .expand(&orm_block("integer n =\n__count()"))
        .expect("expand must succeed");
    let via_full = inline_statements(
        plugin
            .expand_full(&orm_block("integer n =\n__count()"))
            .expect("expand_full must succeed"),
    );
    assert_eq!(format!("{:?}", via_expand), format!("{:?}", via_full));
}

#[test]
fn v1_negative_result_pointer_is_an_error_not_a_panic() {
    let plugin = adapter("test.v1_negative", NEGATIVE_POINTER_WAT);
    let block = FrameworkBlock {
        name: "anything".to_string(),
        content: "body".to_string(),
        attributes: Vec::new(),
        location: None,
    };
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.expand_full(&block)));
    let result = result.expect("a negative plugin result pointer must not panic the host");
    let err = result.expect_err("a negative plugin result pointer must be reported");
    assert!(
        err.to_string().contains("out of bounds"),
        "unexpected error: {}",
        err
    );
}
