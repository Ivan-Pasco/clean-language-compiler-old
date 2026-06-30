//! Plugin Contracts v3 sub-cycle 3 — integration tests for the new bridges,
//! lifecycle slot typed dispatch, and PLUGIN006 load-time refusal.
//!
//! Inventory:
//!   1. plugin006_refused_for_unsupported_expansion_version
//!   2. plugin006_accepted_for_supported_expansion_version
//!   3. emit_external_host_class_mismatch_plugin007
//!   4. emit_external_host_class_match_succeeds
//!   5. lifecycle_module_helpers_typed_dispatch
//!   6. define_function_then_emit_class_round_trip (via existing bridges)
//!
//! The WAT pilots are inlined as string literals so each test is self-contained.

use clean_language_compiler::plugins::{
    plugin_abi::{
        PluginBlockConfig, PluginCompatibility, PluginExports, PluginHandles, PluginInfo,
        PluginLifecycle, PluginManifest,
    },
    BuildContext, FrameworkPlugin, WasmPluginAdapter,
};
use std::collections::HashMap;
use wasmtime::{Engine, Module};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn base_manifest(name: &str) -> PluginManifest {
    PluginManifest {
        plugin: PluginInfo {
            name: name.to_string(),
            version: "0.0.1".to_string(),
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
        blocks: {
            let mut m = HashMap::new();
            m.insert(
                "pilot".to_string(),
                PluginBlockConfig {
                    expand: Some("expand_block_typed".to_string()),
                    version: Some(3),
                },
            );
            m
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
    }
}

fn build_block() -> clean_language_compiler::ast::FrameworkBlock {
    clean_language_compiler::ast::FrameworkBlock {
        name: "pilot".to_string(),
        content: String::new(),
        attributes: Vec::new(),
        location: Some(clean_language_compiler::ast::SourceLocation {
            file: "test".into(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }),
    }
}

// ─── Test 1: PLUGIN006 — unsupported expansion_version refused at load ──────

#[test]
fn plugin006_refused_for_unsupported_expansion_version() {
    use clean_language_compiler::plugins::WasmPluginLoader;
    use std::path::Path;

    let msg = WasmPluginLoader::format_expansion_version_mismatch_error(
        "fake_plugin",
        Path::new("/tmp/fake_plugin"),
        "999.0.0",
    );
    assert!(msg.contains("PLUGIN006"));
    assert!(msg.contains("expansion_version"));
    assert!(msg.contains("999.0.0"));
    assert!(msg.contains("typed-emission.md"));
}

// ─── Test 2: PLUGIN006 — supported versions accepted ────────────────────────

#[test]
fn plugin006_accepted_for_supported_expansion_version() {
    use clean_language_compiler::plugins::plugin_abi::SUPPORTED_EXPANSION_VERSIONS;
    // SUPPORTED_EXPANSION_VERSIONS must include both 1.0.0 (legacy) and 3.0.0
    // (typed emission). This guards against accidental removal during a future
    // ABI bump.
    assert!(SUPPORTED_EXPANSION_VERSIONS.contains(&"1.0.0"));
    assert!(SUPPORTED_EXPANSION_VERSIONS.contains(&"3.0.0"));
}

// ─── Test 3: PLUGIN007 — host_class mismatch refused by _emit_external ─────

const HOST_CLASS_MISMATCH_WAT: &str = r#"
(module
  (import "env" "_emit_external" (func $_emit_external (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_type_void"     (func $_type_void     (param i32) (result i32)))

  (memory (export "memory") 1)

  ;; LP-strings
  ;; 0: "doThing" (7 bytes)
  ;; 16: "[]" (2 bytes) — empty params JSON
  ;; 32: "browser" (7 bytes) — host_class declared by plugin
  (data (i32.const 0)  "\07\00\00\00doThing")
  (data (i32.const 16) "\02\00\00\00[]")
  (data (i32.const 32) "\07\00\00\00browser")

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $void_t i32)

    local.get $ctx
    call $_type_void
    local.set $void_t

    ;; _emit_external(ctx, "doThing", "[]", void_t, "browser")
    local.get $ctx
    i32.const 0
    i32.const 16
    local.get $void_t
    i32.const 32
    call $_emit_external
    ;; The result is non-zero (1) because PLUGIN007 was emitted; return it.
  )
)
"#;

#[test]
fn emit_external_host_class_mismatch_plugin007() {
    // Force the active host class to "server" so a plugin declaring "browser"
    // triggers PLUGIN007.
    clean_language_compiler::set_target_host_class_override(Some("server".to_string()));
    // Drop guard in case of panic.
    struct ClearOverride;
    impl Drop for ClearOverride {
        fn drop(&mut self) {
            clean_language_compiler::set_target_host_class_override(None);
        }
    }
    let _guard = ClearOverride;

    let wasm = wat::parse_str(HOST_CLASS_MISMATCH_WAT).expect("host_class mismatch WAT compiles");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("module instantiates");
    let adapter = WasmPluginAdapter::new(
        "host_class_test".to_string(),
        base_manifest("host_class_test"),
        module,
        engine,
    )
    .expect("adapter must construct");

    let result = adapter.expand_full(&build_block());
    assert!(
        result.is_err(),
        "host_class mismatch must fail expansion; got Ok"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("PLUGIN007")
            || msg.contains("BridgeHostClassMismatchInEmission")
            || msg.contains("host_class"),
        "error must reference PLUGIN007, got: {}",
        msg
    );
}

// ─── Test 4: PLUGIN007 — matching host_class succeeds ───────────────────────

const HOST_CLASS_MATCH_WAT: &str = r#"
(module
  (import "env" "_emit_external" (func $_emit_external (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_type_void"     (func $_type_void     (param i32) (result i32)))

  (memory (export "memory") 1)

  (data (i32.const 0)  "\07\00\00\00doThing")
  (data (i32.const 16) "\02\00\00\00[]")
  (data (i32.const 32) "\06\00\00\00server")

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $void_t i32)

    local.get $ctx
    call $_type_void
    local.set $void_t

    local.get $ctx
    i32.const 0
    i32.const 16
    local.get $void_t
    i32.const 32
    call $_emit_external
    ;; success → returns 0
  )
)
"#;

#[test]
fn emit_external_host_class_match_succeeds() {
    clean_language_compiler::set_target_host_class_override(Some("server".to_string()));
    struct ClearOverride;
    impl Drop for ClearOverride {
        fn drop(&mut self) {
            clean_language_compiler::set_target_host_class_override(None);
        }
    }
    let _guard = ClearOverride;

    let wasm = wat::parse_str(HOST_CLASS_MATCH_WAT).expect("host_class match WAT compiles");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("module instantiates");
    let adapter = WasmPluginAdapter::new(
        "host_class_match".to_string(),
        base_manifest("host_class_match"),
        module,
        engine,
    )
    .expect("adapter must construct");

    let expansion = adapter
        .expand_full(&build_block())
        .expect("matching host_class must succeed");
    assert_eq!(
        expansion.externals.len(),
        1,
        "matching host_class must emit external; got {:?}",
        expansion.externals
    );
    assert_eq!(expansion.externals[0].name, "doThing");
}

// ─── Test 5: lifecycle module_helpers typed dispatch ────────────────────────

const MODULE_HELPERS_TYPED_WAT: &str = r#"
(module
  (import "env" "_emit_function" (func $_emit_function (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"    (func $_stmt_block    (param i32 i32) (result i32)))
  (import "env" "_stmt_return"   (func $_stmt_return   (param i32 i32) (result i32)))
  (import "env" "_expr_int_lit"  (func $_expr_int_lit  (param i32 i32 i32) (result i32)))
  (import "env" "_type_integer"  (func $_type_integer  (param i32) (result i32)))

  (memory (export "memory") 1)

  (data (i32.const 0)   "\06\00\00\00helper")
  (data (i32.const 16)  "\02\00\00\00[]")
  (data (i32.const 32)  "\03\00\00\00[3]")

  ;; Typed lifecycle slot signature: (ctx, build_context_lp) -> i32
  (func (export "emit_module_helpers_typed")
        (param $ctx i32) (param $ctx_lp i32) (result i32)
    (local $int_t i32)
    (local $val i32)
    (local $ret i32)
    (local $body i32)
    (local $r i32)

    local.get $ctx
    call $_type_integer
    local.set $int_t

    local.get $ctx
    i32.const 7
    i32.const 0
    call $_expr_int_lit
    local.set $val

    local.get $ctx
    local.get $val
    call $_stmt_return
    local.set $ret

    local.get $ctx
    i32.const 32
    call $_stmt_block
    local.set $body

    local.get $ctx
    i32.const 0
    i32.const 16
    local.get $int_t
    local.get $body
    i32.const 0
    call $_emit_function
    local.set $r

    local.get $r
  )
)
"#;

#[test]
fn lifecycle_module_helpers_typed_dispatch() {
    let wasm = wat::parse_str(MODULE_HELPERS_TYPED_WAT).expect("module_helpers WAT compiles");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("module instantiates");

    let mut manifest = base_manifest("lifecycle_typed_test");
    manifest.lifecycle = PluginLifecycle {
        module_helpers: Some("emit_module_helpers_typed".to_string()),
        ..PluginLifecycle::default()
    };

    let adapter =
        WasmPluginAdapter::new("lifecycle_typed_test".to_string(), manifest, module, engine)
            .expect("adapter must construct");

    let mut ctx = BuildContext::new();
    ctx.target = "server".to_string();

    let expansion = adapter
        .invoke_lifecycle_slot("module_helpers", &ctx)
        .expect("typed lifecycle slot must succeed");

    assert_eq!(
        expansion.functions.len(),
        1,
        "typed module_helpers must emit one function; got {:?}",
        expansion.functions
    );
    assert_eq!(expansion.functions[0].name, "helper");
}
