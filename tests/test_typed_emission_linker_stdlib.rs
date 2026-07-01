//! Regression test for bug f4b7d6977f05 (TYPED-EMISSION-LINKER-INCOMPLETE).
//!
//! The v3 typed-emission linker previously only registered the 30 typed-emission
//! bridge functions via `register_typed_emission_bridges`. It did NOT register the
//! stdlib env imports (env::print, env::string.concat, env::string_compare,
//! env::float_to_string, etc.) that any non-trivial Clean Language plugin needs.
//!
//! When a plugin declared `expansion_version = "3.0.0"` and its `expand_block_typed`
//! body referenced stdlib functions, wasmtime rejected the plugin at instantiation
//! with "unknown import `env::string.concat`" (or whichever stdlib function was used).
//!
//! The fix: `build_typed_emission_linker` now calls `register_plugin_stdlib_functions`
//! before `register_typed_emission_bridges`, so v3 plugins get the full stdlib surface.
//!
//! Test inventory:
//!
//! 1. `v3_linker_provides_string_concat_import` — instantiation succeeds when plugin imports `env::string.concat`
//! 2. `v3_linker_provides_print_import` — instantiation succeeds when plugin imports `env::print`
//! 3. `v3_plugin_calls_stdlib_and_v3_bridge` — expand_full succeeds calling both `string.concat` and `_emit_error`
//! 4. `v3_plugin_stdlib_result_is_correct` — function emitted via v3 bridges with stdlib call has correct body

use clean_language_compiler::ast::{Expression, FrameworkBlock, SourceLocation, Statement, Value};
use clean_language_compiler::plugins::{
    plugin_abi::{
        PluginBlockConfig, PluginCompatibility, PluginExports, PluginHandles, PluginInfo,
        PluginManifest,
    },
    FrameworkPlugin, WasmPluginAdapter,
};
use std::collections::HashMap;
use wasmtime::{Engine, Module};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_manifest(plugin_name: &str) -> PluginManifest {
    let mut blocks = HashMap::new();
    blocks.insert(
        "test_block".to_string(),
        PluginBlockConfig {
            expand: Some("expand_block_typed".to_string()),
            version: Some(3),
        },
    );
    PluginManifest {
        plugin: PluginInfo {
            name: plugin_name.to_string(),
            version: "0.0.1".to_string(),
            description: "stdlib linker regression test".to_string(),
            author: String::new(),
        },
        compatibility: PluginCompatibility {
            expansion_version: Some("3.0.0".to_string()),
            ..PluginCompatibility::default()
        },
        handles: PluginHandles {
            blocks: vec!["test_block".to_string()],
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
    }
}

fn make_block() -> FrameworkBlock {
    FrameworkBlock {
        name: "test_block".to_string(),
        content: String::new(),
        attributes: Vec::new(),
        location: Some(SourceLocation {
            file: "test".into(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }),
    }
}

fn build_adapter_from_wat(wat_src: &str, plugin_name: &str) -> WasmPluginAdapter {
    let wasm = wat::parse_str(wat_src)
        .unwrap_or_else(|e| panic!("WAT compilation failed for `{}`: {}", plugin_name, e));
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm)
        .unwrap_or_else(|e| panic!("Module::new failed for `{}`: {}", plugin_name, e));
    WasmPluginAdapter::new(
        plugin_name.to_string(),
        make_manifest(plugin_name),
        module,
        engine,
    )
    .unwrap_or_else(|e| panic!("WasmPluginAdapter::new failed for `{}`: {}", plugin_name, e))
}

// ─── Test 1: v3 linker provides env::string.concat ────────────────────────────
//
// A v3 plugin that imports env::string.concat (a stdlib function) must instantiate
// without "unknown import" error. Before fix f4b7d6977f05 this would fail at
// `linker.instantiate(...)` with "unknown import `env::string.concat`".

#[test]
fn v3_linker_provides_string_concat_import() {
    // This WAT imports env::string.concat. If the v3 linker is missing the stdlib
    // registration the module will fail to instantiate — that is the regression.
    //
    // The plugin does NOT call string.concat (it just emits an integer literal),
    // but the *import declaration* being present is enough to trigger the bug.
    let wat_src = r#"
(module
  ;; Stdlib import — this is what was missing from the v3 linker.
  (import "env" "string.concat" (func $concat (param i32 i32) (result i32)))

  ;; v3 typed-emission bridges
  (import "env" "_emit_function" (func $_emit_function (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"    (func $_stmt_block    (param i32 i32) (result i32)))
  (import "env" "_stmt_return"   (func $_stmt_return   (param i32 i32) (result i32)))
  (import "env" "_expr_int_lit"  (func $_expr_int_lit  (param i32 i32 i32) (result i32)))
  (import "env" "_type_integer"  (func $_type_integer  (param i32) (result i32)))

  (memory (export "memory") 1)

  ;; LP-strings: "ok" at 0, "[]" at 16, "[3]" at 32
  (data (i32.const 0)  "\02\00\00\00ok")
  (data (i32.const 16) "\02\00\00\00[]")
  (data (i32.const 32) "\03\00\00\00[3]")

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $int_h i32)
    (local $expr_h i32)
    (local $ret_h i32)
    (local $blk_h i32)

    ;; Emit function named "ok" returning integer 1 — uses only v3 bridges.
    local.get $ctx
    call $_type_integer
    local.set $int_h

    local.get $ctx
    i32.const 1
    i32.const 0
    call $_expr_int_lit
    local.set $expr_h

    local.get $ctx
    local.get $expr_h
    call $_stmt_return
    local.set $ret_h

    local.get $ctx
    i32.const 32
    call $_stmt_block
    local.set $blk_h

    local.get $ctx
    i32.const 0
    i32.const 16
    local.get $int_h
    local.get $blk_h
    i32.const 0
    call $_emit_function
  )
)
"#;

    // If the v3 linker does not include stdlib, `build_adapter_from_wat` will panic
    // at `WasmPluginAdapter::new` (which calls setup_linker for the cached v1 linker)
    // — actually the v3 linker is built per call_expand_typed. The real failure point
    // is `expand_full` → `call_expand_typed` → `build_typed_emission_linker` →
    // `linker.instantiate(...)`. We must call expand_full to trigger it.
    let adapter = build_adapter_from_wat(wat_src, "stdlib_concat_import_test");

    // This triggers build_typed_emission_linker → instantiate. Before the fix this
    // panics with "typed-emission: failed to instantiate plugin: unknown import `env::string.concat`".
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "v3 plugin with env::string.concat import must instantiate successfully; got: {}",
        result.unwrap_err()
    );
}

// ─── Test 2: v3 linker provides env::print ────────────────────────────────────

#[test]
fn v3_linker_provides_print_import() {
    // Similar to test 1 but tests env::print — the most commonly used stdlib import.
    let wat_src = r#"
(module
  ;; Stdlib: env::print (ptr: i32, len: i32)
  (import "env" "print" (func $print (param i32 i32)))

  ;; v3 bridges
  (import "env" "_emit_function" (func $_emit_function (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"    (func $_stmt_block    (param i32 i32) (result i32)))
  (import "env" "_stmt_return"   (func $_stmt_return   (param i32 i32) (result i32)))
  (import "env" "_expr_int_lit"  (func $_expr_int_lit  (param i32 i32 i32) (result i32)))
  (import "env" "_type_integer"  (func $_type_integer  (param i32) (result i32)))

  (memory (export "memory") 1)

  (data (i32.const 0)  "\02\00\00\00hi")   ;; fn name "hi"
  (data (i32.const 16) "\02\00\00\00[]")   ;; empty params
  (data (i32.const 32) "\03\00\00\00[3]")  ;; stmts "[3]"

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $int_h i32)
    (local $expr_h i32)
    (local $ret_h i32)
    (local $blk_h i32)

    local.get $ctx
    call $_type_integer
    local.set $int_h

    local.get $ctx
    i32.const 7
    i32.const 0
    call $_expr_int_lit
    local.set $expr_h

    local.get $ctx
    local.get $expr_h
    call $_stmt_return
    local.set $ret_h

    local.get $ctx
    i32.const 32
    call $_stmt_block
    local.set $blk_h

    local.get $ctx
    i32.const 0
    i32.const 16
    local.get $int_h
    local.get $blk_h
    i32.const 0
    call $_emit_function
  )
)
"#;

    let adapter = build_adapter_from_wat(wat_src, "stdlib_print_import_test");

    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "v3 plugin with env::print import must instantiate successfully; got: {}",
        result.unwrap_err()
    );
}

// ─── Test 3: v3 plugin calls stdlib (string.concat) AND v3 bridge (_emit_error) ─

#[test]
fn v3_plugin_calls_stdlib_and_v3_bridge_succeeds() {
    // This plugin actually CALLS string.concat at runtime (concatenating two LP-strings)
    // and also calls _emit_error with severity=1 (warning, not error — does not abort).
    // Both must resolve and execute. This is the core regression scenario from bug
    // f4b7d6977f05: a plugin body that mixes stdlib calls and v3 bridge calls.
    let wat_src = r#"
(module
  ;; Stdlib imports
  (import "env" "string.concat" (func $concat   (param i32 i32) (result i32)))
  (import "env" "print"         (func $print    (param i32 i32)))

  ;; v3 bridges
  (import "env" "_emit_function" (func $_emit_function (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"    (func $_stmt_block    (param i32 i32) (result i32)))
  (import "env" "_stmt_return"   (func $_stmt_return   (param i32 i32) (result i32)))
  (import "env" "_expr_int_lit"  (func $_expr_int_lit  (param i32 i32 i32) (result i32)))
  (import "env" "_type_integer"  (func $_type_integer  (param i32) (result i32)))
  (import "env" "_emit_error"    (func $_emit_error    (param i32 i32 i32 i32 i32) (result i32)))

  (memory (export "memory") 1)

  ;; LP-strings packed in static data
  ;; offset 0:   LP("fn") — function name
  ;; offset 16:  LP("[]") — empty params
  ;; offset 32:  LP("[3]") — stmts JSON
  ;; offset 64:  LP("W001") — warning code for _emit_error
  ;; offset 96:  LP("stdlib+v3 co-call ok") — warning message
  ;; offset 128: LP("") — empty span
  (data (i32.const 0)   "\02\00\00\00fn")
  (data (i32.const 16)  "\02\00\00\00[]")
  (data (i32.const 32)  "\03\00\00\00[3]")
  (data (i32.const 64)  "\04\00\00\00W001")
  (data (i32.const 96)  "\14\00\00\00stdlib+v3 co-call ok")
  (data (i32.const 128) "\00\00\00\00")

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $int_h i32)
    (local $expr_h i32)
    (local $ret_h i32)
    (local $blk_h i32)
    (local $concat_result i32)

    ;; --- Stdlib call: string.concat("fn", "[]") ---
    ;; This exercises the stdlib path in the v3 linker.
    i32.const 0
    i32.const 16
    call $concat
    local.set $concat_result

    ;; --- v3 bridge: _emit_error(ctx, severity=1, "W001", msg, span) ---
    ;; severity=1 is a warning — does NOT abort the expansion.
    local.get $ctx
    i32.const 1
    i32.const 64
    i32.const 96
    i32.const 128
    call $_emit_error
    drop

    ;; --- Emit function via v3 bridges ---
    local.get $ctx
    call $_type_integer
    local.set $int_h

    local.get $ctx
    i32.const 42
    i32.const 0
    call $_expr_int_lit
    local.set $expr_h

    local.get $ctx
    local.get $expr_h
    call $_stmt_return
    local.set $ret_h

    local.get $ctx
    i32.const 32
    call $_stmt_block
    local.set $blk_h

    local.get $ctx
    i32.const 0   ;; name LP
    i32.const 16  ;; params LP
    local.get $int_h
    local.get $blk_h
    i32.const 0
    call $_emit_function
  )
)
"#;

    let adapter = build_adapter_from_wat(wat_src, "stdlib_and_v3_bridge_test");

    // expand_full triggers call_expand_typed → build_typed_emission_linker → instantiate.
    // Without the fix, instantiation fails because string.concat is not in the linker.
    // With the fix, both string.concat and _emit_error are registered and the call succeeds.
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "v3 plugin calling stdlib (string.concat) and v3 bridge (_emit_error) must succeed; got: {}",
        result.unwrap_err()
    );

    let expansion = result.unwrap();
    assert_eq!(
        expansion.functions.len(),
        1,
        "expansion must contain exactly one emitted function"
    );
    assert_eq!(
        expansion.functions[0].name, "fn",
        "emitted function must be named 'fn'"
    );
}

// ─── Test 4: v3 plugin stdlib call produces correct function body ─────────────

#[test]
fn v3_plugin_emitted_function_body_is_correct() {
    // Confirms that mixing a stdlib call (string.concat, which returns a pointer we
    // discard) with v3 bridge calls produces the correct emitted AST. The emitted
    // function must have a Return statement containing IntegerLiteral(42).
    let wat_src = r#"
(module
  (import "env" "string.concat" (func $concat (param i32 i32) (result i32)))
  (import "env" "_emit_function" (func $_emit_function (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"    (func $_stmt_block    (param i32 i32) (result i32)))
  (import "env" "_stmt_return"   (func $_stmt_return   (param i32 i32) (result i32)))
  (import "env" "_expr_int_lit"  (func $_expr_int_lit  (param i32 i32 i32) (result i32)))
  (import "env" "_type_integer"  (func $_type_integer  (param i32) (result i32)))

  (memory (export "memory") 1)

  (data (i32.const 0)  "\03\00\00\00ans")
  (data (i32.const 16) "\02\00\00\00[]")
  (data (i32.const 32) "\03\00\00\00[3]")

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $int_h i32)
    (local $expr_h i32)
    (local $ret_h i32)
    (local $blk_h i32)

    ;; Stdlib call: concat "ans" + "[]" — result is intentionally discarded.
    ;; Its presence here is the regression trigger (missing import before fix).
    i32.const 0
    i32.const 16
    call $concat
    drop

    ;; Build function body via v3 bridges.
    local.get $ctx
    call $_type_integer
    local.set $int_h

    local.get $ctx
    i32.const 42
    i32.const 0
    call $_expr_int_lit
    local.set $expr_h

    local.get $ctx
    local.get $expr_h
    call $_stmt_return
    local.set $ret_h

    local.get $ctx
    i32.const 32
    call $_stmt_block
    local.set $blk_h

    local.get $ctx
    i32.const 0
    i32.const 16
    local.get $int_h
    local.get $blk_h
    i32.const 0
    call $_emit_function
  )
)
"#;

    let adapter = build_adapter_from_wat(wat_src, "stdlib_body_correct_test");
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "v3 plugin with stdlib import must expand successfully; got: {}",
        result.unwrap_err()
    );

    let expansion = result.unwrap();
    assert_eq!(expansion.functions.len(), 1, "must emit one function");

    let func = &expansion.functions[0];
    assert_eq!(func.name, "ans", "function name must be 'ans'");

    // Locate the Return statement inside the emitted body.
    let return_stmt = find_return_stmt(&func.body)
        .expect("emitted function body must contain a Return statement");

    match return_stmt {
        Statement::Return {
            value: Some(Expression::Literal(Value::Integer(42))),
            ..
        } => {}
        other => panic!("expected Return(Integer(42)), got {:?}", other),
    }
}

// ─── Helper: find Return statement recursively ───────────────────────────────

fn find_return_stmt(stmts: &[Statement]) -> Option<&Statement> {
    for stmt in stmts {
        match stmt {
            Statement::Return { .. } => return Some(stmt),
            Statement::If { then_branch, .. } => {
                if let Some(s) = find_return_stmt(then_branch) {
                    return Some(s);
                }
            }
            _ => {}
        }
    }
    None
}
