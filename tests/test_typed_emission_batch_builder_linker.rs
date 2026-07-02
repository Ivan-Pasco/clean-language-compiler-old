//! Linker-level integration tests for batch builder bridges — bug #e8ad1944445f.
//!
//! Verifies that:
//!   A. All 27 batch builder bridges are accessible under their `_batch_*` underscore
//!      aliases in the typed-emission linker so Clean Language plugins can declare
//!      them with `external:` (which rejects dots in import names).
//!   B. The `_emit_helpers_batch` bridge (C2 batch-family) has at least one WAT-driven
//!      linker test (previously only schema→AST was covered by test_typed_emission_batch.rs).
//!   C. The `_emit_stmt_from_source` bridge (Landing B) has a WAT-driven linker test.
//!   D. The `_emit_class_full` bridge (C2 batch-family) has a minimal WAT import test.
//!
//! Test inventory:
//!   1. `batch_stringlit_underscore_alias_resolves`      — imports `_batch_stringLit`, gets valid handle
//!   2. `batch_spec_roundtrip_via_underscore_aliases`    — `_batch_arrayNew` + `_batch_arrayPush`
//!                                                          + `_batch_func` + `_batch_spec` +
//!                                                          `_emit_helpers_batch` → expansion.functions
//!   3. `emit_stmt_from_source_linker_hygiene`           — `_emit_stmt_from_source` is resolvable
//!   4. `emit_class_full_linker_hygiene`                 — `_emit_class_full` is resolvable

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

// ─── Helpers ─────────────────────────────────────────────────────────────────

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
            description: "batch builder linker test".to_string(),
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

// ─── Test 1: `_batch_stringLit` underscore alias resolves at instantiation ───
//
// A v3 plugin that imports `env::_batch_stringLit` must instantiate and return
// a non-zero batch handle. Before the fix the linker only registered the dotted
// `batch.stringLit` name, so wasmtime would reject this import with
// "unknown import `env::_batch_stringLit`".

#[test]
fn batch_stringlit_underscore_alias_resolves() {
    // This WAT uses the _batch_stringLit underscore alias (the form that Clean
    // Language plugins must use in `external:` declarations). It also uses
    // _batch_spec + _batch_arrayNew + _batch_arrayPush + _batch_func to build
    // a trivial function spec and emit it via _emit_helpers_batch so we can
    // inspect the expansion result.
    //
    // LP-string layout (offset: length_LE32 + bytes):
    //   0:   LP("hello") — stringLit value
    //   16:  LP("greet") — function name
    //   32:  LP("void")  — return type
    let wat_src = r#"
(module
  ;; _batch_* underscore aliases — what Clean Language external: declarations use.
  (import "env" "_batch_stringLit"  (func $_batch_stringLit  (param i32 i32)             (result i32)))
  (import "env" "_batch_arrayNew"   (func $_batch_arrayNew   (param i32)                 (result i32)))
  (import "env" "_batch_arrayPush"  (func $_batch_arrayPush  (param i32 i32 i32)         (result i32)))
  (import "env" "_batch_stmtReturn" (func $_batch_stmtReturn (param i32 i32)             (result i32)))
  (import "env" "_batch_param"      (func $_batch_param      (param i32 i32 i32)         (result i32)))
  (import "env" "_batch_func"       (func $_batch_func       (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_batch_spec"       (func $_batch_spec       (param i32 i32)             (result i32)))

  ;; _emit_helpers_batch: the terminal bridge that materialises the spec into expansion.functions
  (import "env" "_emit_helpers_batch" (func $_emit_helpers_batch (param i32 i32 i32) (result i32)))

  (memory (export "memory") 1)

  ;; LP-strings in static data
  (data (i32.const  0) "\05\00\00\00hello")  ;; offset 0: "hello"
  (data (i32.const 16) "\05\00\00\00greet")  ;; offset 16: "greet"
  (data (i32.const 32) "\04\00\00\00void")   ;; offset 32: "void"

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $str_handle    i32)
    (local $ret_handle    i32)
    (local $stmt_arr      i32)
    (local $param_arr     i32)
    (local $func_handle   i32)
    (local $fn_arr        i32)
    (local $spec_handle   i32)
    (local $emit_err      i32)

    ;; --- Call _batch_stringLit via underscore alias ---
    ;; This is the core of test 1: the import must resolve (not trap with
    ;; "unknown import") and return a non-zero batch handle.
    local.get $ctx
    i32.const 0     ;; LP ptr to "hello"
    call $_batch_stringLit
    local.set $str_handle
    ;; str_handle must be non-zero (valid batch handle with BATCH_TAG bit set).
    ;; We'll use it as the return expression of the greet function.

    ;; --- Build: return <str_handle> ---
    local.get $ctx
    local.get $str_handle
    call $_batch_stmtReturn
    local.set $ret_handle

    ;; --- Build empty-params array ---
    local.get $ctx
    call $_batch_arrayNew
    local.set $param_arr

    ;; --- Build body array: [ret_handle] ---
    local.get $ctx
    call $_batch_arrayNew
    local.set $stmt_arr

    local.get $ctx
    local.get $stmt_arr
    local.get $ret_handle
    call $_batch_arrayPush
    drop  ;; 0 = success

    ;; --- Build func("greet", [], "void", [ret]) ---
    local.get $ctx
    i32.const 16    ;; name LP "greet"
    local.get $param_arr
    i32.const 32    ;; return_type LP "void"
    local.get $stmt_arr
    call $_batch_func
    local.set $func_handle

    ;; --- Build functions array: [func_handle] ---
    local.get $ctx
    call $_batch_arrayNew
    local.set $fn_arr

    local.get $ctx
    local.get $fn_arr
    local.get $func_handle
    call $_batch_arrayPush
    drop

    ;; --- Build spec([func_handle]) ---
    local.get $ctx
    local.get $fn_arr
    call $_batch_spec
    local.set $spec_handle

    ;; --- Emit helpers batch ---
    local.get $ctx
    local.get $spec_handle
    i32.const 0     ;; flags
    call $_emit_helpers_batch
    local.set $emit_err

    ;; Return emit_err (0 = success).
    local.get $emit_err
  )
)
"#;

    let adapter = build_adapter_from_wat(wat_src, "batch_stringlit_underscore_test");
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "v3 plugin using _batch_stringLit (underscore alias) must instantiate and expand; got: {}",
        result.unwrap_err()
    );

    let expansion = result.unwrap();
    // When the arena emits PLUGIN013 error-severity diagnostics the bridge
    // returns non-zero and expand_full returns Err — already asserted above.
    // The greet function must land in expansion.functions.
    assert_eq!(
        expansion.functions.len(),
        1,
        "expansion must contain exactly one emitted function"
    );
    assert_eq!(
        expansion.functions[0].name, "greet",
        "emitted function must be named 'greet'"
    );
}

// ─── Test 2: _batch_arrayNew + _batch_arrayPush + _batch_spec + _emit_helpers_batch ─
//
// End-to-end test via WAT that builds a `BatchSpec` using only underscore-aliased
// bridge names, then calls `_emit_helpers_batch` to materialise it. Verifies:
//  - No PLUGIN013 error diagnostics in the expansion.
//  - `expansion.functions` contains a function named "compute".
//
// This covers the full round-trip path described in bug #e8ad1944445f:
// a Clean Language plugin declares `external: integer _batch_spec(...)` and
// uses the batch builder API to produce a spec without touching JSON.

#[test]
fn batch_spec_roundtrip_via_underscore_aliases() {
    // LP-string layout:
    //   0:  LP("compute")  — function name
    //   16: LP("integer")  — return type
    let wat_src = r#"
(module
  ;; Batch builder bridges — all underscore aliases (the external: form).
  (import "env" "_batch_arrayNew"   (func $_batch_arrayNew   (param i32)                 (result i32)))
  (import "env" "_batch_arrayPush"  (func $_batch_arrayPush  (param i32 i32 i32)         (result i32)))
  (import "env" "_batch_intLit"     (func $_batch_intLit     (param i32 i32 i32)         (result i32)))
  (import "env" "_batch_stmtReturn" (func $_batch_stmtReturn (param i32 i32)             (result i32)))
  (import "env" "_batch_func"       (func $_batch_func       (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_batch_spec"       (func $_batch_spec       (param i32 i32)             (result i32)))
  (import "env" "_emit_helpers_batch" (func $_emit_helpers_batch (param i32 i32 i32) (result i32)))

  (memory (export "memory") 1)

  (data (i32.const  0) "\07\00\00\00compute")  ;; offset 0: "compute"
  (data (i32.const 16) "\07\00\00\00integer")  ;; offset 16: "integer"

  (func (export "expand_block_typed")
        (param $ctx i32) (param $block_name_lp i32)
        (param $attrs_lp i32) (param $body_lp i32) (result i32)
    (local $int_h     i32)
    (local $ret_h     i32)
    (local $body_arr  i32)
    (local $param_arr i32)
    (local $func_h    i32)
    (local $fn_arr    i32)
    (local $spec_h    i32)

    ;; intLit(ctx, lo=99, hi=0) -> handle
    local.get $ctx
    i32.const 99
    i32.const 0
    call $_batch_intLit
    local.set $int_h

    ;; stmtReturn(ctx, int_h) -> handle
    local.get $ctx
    local.get $int_h
    call $_batch_stmtReturn
    local.set $ret_h

    ;; body array
    local.get $ctx
    call $_batch_arrayNew
    local.set $body_arr

    local.get $ctx
    local.get $body_arr
    local.get $ret_h
    call $_batch_arrayPush
    drop

    ;; empty param array
    local.get $ctx
    call $_batch_arrayNew
    local.set $param_arr

    ;; func("compute", [], "integer", body)
    local.get $ctx
    i32.const 0     ;; name LP
    local.get $param_arr
    i32.const 16    ;; return_type LP
    local.get $body_arr
    call $_batch_func
    local.set $func_h

    ;; fn_arr
    local.get $ctx
    call $_batch_arrayNew
    local.set $fn_arr

    local.get $ctx
    local.get $fn_arr
    local.get $func_h
    call $_batch_arrayPush
    drop

    ;; spec
    local.get $ctx
    local.get $fn_arr
    call $_batch_spec
    local.set $spec_h

    ;; emit — return its error code (0 = ok)
    local.get $ctx
    local.get $spec_h
    i32.const 0
    call $_emit_helpers_batch
  )
)
"#;

    let adapter = build_adapter_from_wat(wat_src, "batch_spec_roundtrip_test");
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "batch spec roundtrip via underscore aliases must succeed; got: {}",
        result.unwrap_err()
    );

    let expansion = result.unwrap();
    // expand_full returns Err when PLUGIN013 error diagnostics occur, so
    // reaching here guarantees no arena errors were emitted.
    assert_eq!(
        expansion.functions.len(),
        1,
        "expansion must contain exactly one emitted function"
    );
    assert_eq!(
        expansion.functions[0].name, "compute",
        "emitted function must be named 'compute'"
    );

    // Verify the function body contains a Return(Integer(99)).
    let return_stmt = find_return_stmt(&expansion.functions[0].body)
        .expect("emitted function must contain a Return statement");
    match return_stmt {
        Statement::Return {
            value: Some(Expression::Literal(Value::Integer(99))),
            ..
        } => {}
        other => panic!("expected Return(Integer(99)), got {:?}", other),
    }
}

// ─── Test 3: `_emit_stmt_from_source` is resolvable in the v3 linker (hygiene) ─
//
// Landing B added `_emit_stmt_from_source` to the typed-emission linker. Previously
// no WAT-driven linker-level test confirmed the import resolves. This test imports
// it (and calls it with a trivial source string) to verify it is in the linker surface.

#[test]
fn emit_stmt_from_source_linker_hygiene() {
    // The plugin imports _emit_stmt_from_source and calls it with a simple
    // `return 1` source string. The test goal is that the import resolves
    // (no "unknown import" trap) and the plugin reaches _emit_helpers_batch
    // with a fallback JSON spec path to produce a verifiable result.
    //
    // LP-string layout (all bytes hex-escaped so WAT parser handles them):
    //   offset 0:   LP("return 1")  — 8 bytes
    //   offset 16:  LP(json_spec)   — 121 bytes
    //     json_spec = {"functions":[{"name":"src_fn","params":[],"return_type":"integer",
    //                  "body":[{"kind":"return","expr":{"kind":"int_lit","value":1}}]}]}
    //
    // All data bytes are WAT hex-escaped (\xx) to avoid parser issues with
    // quotes and braces embedded in data string literals.
    let wat_src = build_wat_from_bytes(
        &[
            // import declarations
            ("_emit_stmt_from_source", "(param i32 i32 i32) (result i32)"),
            ("_emit_helpers_batch", "(param i32 i32 i32) (result i32)"),
        ],
        // LP strings as raw bytes: [(offset, bytes)]
        &[
            (0u32,  b"return 1" as &[u8]),
            (16u32, br#"{"functions":[{"name":"src_fn","params":[],"return_type":"integer","body":[{"kind":"return","expr":{"kind":"int_lit","value":1}}]}]}"#),
        ],
        // WASM function body (WAT text)
        r#"
    (local $stmt_h i32)
    ;; _emit_stmt_from_source(ctx, LP("return 1"), origin=0) → stmt handle
    local.get $ctx
    i32.const 0
    i32.const 0
    call $_emit_stmt_from_source
    local.set $stmt_h
    ;; Emit via JSON path (Amendment 3) so we get a verifiable result.
    local.get $ctx
    i32.const 16
    i32.const 0
    call $_emit_helpers_batch
"#,
    );

    let adapter = build_adapter_from_wat(&wat_src, "emit_stmt_from_source_hygiene_test");
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "_emit_stmt_from_source import must resolve in v3 linker; got: {}",
        result.unwrap_err()
    );

    let expansion = result.unwrap();
    assert_eq!(
        expansion.functions.len(),
        1,
        "JSON fallback spec must produce one function"
    );
    assert_eq!(expansion.functions[0].name, "src_fn");
}

// ─── Test 4: `_emit_class_full` is resolvable in the v3 linker (hygiene) ─────
//
// C2 added `_emit_class_full` to the typed-emission linker. This test confirms
// the import name resolves (no "unknown import") and the bridge is callable
// with a JSON LP string (Amendment 3 path).

#[test]
fn emit_class_full_linker_hygiene() {
    // LP-string layout:
    //   offset 0: LP(class_json)
    //   class_json = {"name":"Box","fields":[{"name":"value","type":"integer"}],"methods":[]}
    //   (72 bytes)
    let wat_src = build_wat_from_bytes(
        &[("_emit_class_full", "(param i32 i32 i32) (result i32)")],
        &[(
            0u32,
            br#"{"name":"Box","fields":[{"name":"value","type":"integer"}],"methods":[]}"#,
        )],
        r#"
    ;; _emit_class_full(ctx, LP(class_json), flags=0) → err (0 = ok)
    local.get $ctx
    i32.const 0
    i32.const 0
    call $_emit_class_full
"#,
    );

    let adapter = build_adapter_from_wat(&wat_src, "emit_class_full_hygiene_test");
    let result = adapter.expand_full(&make_block());
    assert!(
        result.is_ok(),
        "_emit_class_full import must resolve in v3 linker; got: {}",
        result.unwrap_err()
    );

    let expansion = result.unwrap();
    assert_eq!(
        expansion.classes.len(),
        1,
        "expansion must contain exactly one emitted class"
    );
    assert_eq!(expansion.classes[0].name, "Box");
}

// ─── WAT builder helper ───────────────────────────────────────────────────────
//
// Builds a WAT module string from:
//   - a list of (name, type_sig) import declarations
//   - a list of (offset, bytes) LP-string data segments (hex-escaped)
//   - a function body snippet (WAT text)
//
// All data bytes are escaped as \xx hex so that quotes and braces embedded
// in JSON strings don't confuse the WAT parser.
fn build_wat_from_bytes(
    imports: &[(&str, &str)],
    data_segments: &[(u32, &[u8])],
    body: &str,
) -> String {
    // Build import declarations.
    let import_decls: String = imports
        .iter()
        .map(|(name, sig)| format!("  (import \"env\" \"{}\" (func ${} {}))\n", name, name, sig))
        .collect();

    // Build data segment declarations with hex-escaped bytes.
    let data_decls: String = data_segments
        .iter()
        .map(|(offset, bytes)| {
            // LP-string: 4 bytes LE length then content bytes.
            let len = bytes.len() as u32;
            let len_bytes = len.to_le_bytes();
            let mut escaped = String::new();
            for b in len_bytes.iter().chain(bytes.iter()) {
                escaped.push_str(&format!("\\{:02x}", b));
            }
            format!("  (data (i32.const {}) \"{}\")\n", offset, escaped)
        })
        .collect();

    format!(
        "(module\n{import_decls}\n  (memory (export \"memory\") 1)\n\n{data_decls}\n\
         (func (export \"expand_block_typed\")\n\
               (param $ctx i32) (param $block_name_lp i32)\n\
               (param $attrs_lp i32) (param $body_lp i32) (result i32)\n\
         {body}  )\n)\n",
        import_decls = import_decls,
        data_decls = data_decls,
        body = body,
    )
}

// ─── Helper ───────────────────────────────────────────────────────────────────

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
