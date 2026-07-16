//! Resolver fallback: registry-only bridges resolve without plugin.toml.
//!
//! Some Layer-3 bridges (e.g. `_dev_snapshot`) are declared only in
//! `foundation/platform-architecture/function-registry.toml`. Framework
//! fragments call them either directly (`_dev_snapshot()`) or through the
//! registry-declared alias (`dev.snapshot()`).
//!
//! Before the fix, both call forms failed SEM007 because the resolver only
//! consulted plugin.toml `[bridge]` sections. After the fix, the resolver
//! falls back to the embedded registry when a name misses in the symbol
//! table — but only for `_`-prefixed direct calls and for registry-declared
//! aliases. Unknown names still fail SEM007.

use clean_language_compiler::compile;

fn compile_ok(source: &str) -> Vec<u8> {
    match compile(source) {
        Ok(w) => w,
        Err(errors) => panic!("Compilation failed: {:?}", errors),
    }
}

fn compile_err(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("Expected compilation error, got success"),
        Err(errors) => format!("{:?}", errors),
    }
}

fn wasm_has_import(wasm_bytes: &[u8], name: &str) -> bool {
    use wasmparser::{Parser as WasmParser, Payload};
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let import = import.expect("valid import");
                if import.name == name {
                    return true;
                }
            }
        }
    }
    false
}

/// Direct call `_dev_snapshot()` — bridge is in the embedded registry but
/// not declared in any plugin.toml `[bridge]` section. Must resolve and
/// emit the canonical WASM import.
#[test]
fn direct_bridge_call_from_registry_resolves() {
    let src = "start:\n\tprintl(_dev_snapshot())\n";
    let wasm = compile_ok(src);
    assert!(
        wasm_has_import(&wasm, "_dev_snapshot"),
        "compiler must emit WASM import for registry-only bridge _dev_snapshot"
    );
}

/// Registry alias `dev.snapshot()` — declared in the canonical registry as
/// `aliases = ["dev.snapshot"]` on `_dev_snapshot`. Must resolve to the
/// same underlying bridge (single canonical import — never emit the dotted
/// alias as its own import).
#[test]
fn registry_alias_call_resolves_to_canonical_bridge() {
    let src = "start:\n\tprintl(dev.snapshot())\n";
    let wasm = compile_ok(src);
    assert!(
        wasm_has_import(&wasm, "_dev_snapshot"),
        "compiler must emit canonical import _dev_snapshot for alias dev.snapshot()"
    );
    assert!(
        !wasm_has_import(&wasm, "dev.snapshot"),
        "dot-notation aliases must never emit as separate imports"
    );
}

/// Unknown `_prefix()` call must still fail SEM007. The fix widens
/// resolution to names present in the embedded registry, not to any
/// leading-underscore name.
#[test]
fn unknown_underscore_call_still_fails_sem007() {
    let err = compile_err("start:\n\tprintl(_this_is_definitely_not_a_bridge())\n");
    assert!(
        err.contains("_this_is_definitely_not_a_bridge") && err.contains("SEM007"),
        "unknown _prefixed name must still fail SEM007. Got: {err}"
    );
}

/// Unknown `namespace.function()` where `namespace` isn't a registry alias
/// prefix must still fail. The registry-alias fallback in
/// `resolve_method_call_expression` only kicks in when the receiver name
/// matches a declared alias's namespace prefix.
#[test]
fn unknown_namespace_call_still_fails() {
    let err = compile_err("start:\n\tnotarealnamespace.notarealfn()\n");
    assert!(
        !err.is_empty(),
        "unknown namespace.function call must fail (resolver or codegen)"
    );
}
