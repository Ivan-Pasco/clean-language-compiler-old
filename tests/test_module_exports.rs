//! A module exports what hosts look up by name and nothing else
//! (`src/codegen/mir_codegen/exports.rs`).
//!
//! Every import used to be re-exported, together with ~240 compiler runtime
//! helpers (`string.concat`, `list.add`, `validator.*`, ...): a
//! `print("hi")` program had 246 exports.

use std::collections::BTreeSet;
use wasmparser::{ExternalKind, Parser, Payload};

fn exports(wasm: &[u8]) -> BTreeSet<(String, bool)> {
    let mut names = BTreeSet::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ExportSection(reader) = payload.expect("valid module") {
            for export in reader {
                let export = export.expect("export");
                names.insert((export.name.to_string(), export.kind == ExternalKind::Func));
            }
        }
    }
    names
}

fn imports(wasm: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ImportSection(reader) = payload.expect("valid module") {
            for import in reader {
                names.insert(import.expect("import").name.to_string());
            }
        }
    }
    names
}

#[test]
fn minimal_program_exports_only_the_host_interface() {
    let wasm = clean_language_compiler::compile("start:\n\tprint(\"hi\")\n").expect("compiles");
    let names: BTreeSet<String> = exports(&wasm).into_iter().map(|(name, _)| name).collect();
    let expected: BTreeSet<String> = [
        "memory",
        "__heap_ptr",
        "_start",
        "start",
        "malloc",
        "free",
        "scope_push",
        "scope_pop",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(names, expected);
}

#[test]
fn program_functions_are_exported_and_imports_are_not() {
    let source = "start:\n\tprint(double(2))\n\tprint(_on_tick(1))\n\nfunctions:\n\tinteger double(integer n)\n\t\treturn n * 2\n\n\tinteger _on_tick(integer n)\n\t\treturn n + 1\n";
    let wasm = clean_language_compiler::compile(source).expect("compiles");
    let exported = exports(&wasm);
    assert!(exported.contains(&("double".to_string(), true)));
    assert!(exported.contains(&("_on_tick".to_string(), true)));
    for import in imports(&wasm) {
        assert!(
            !exported.iter().any(|(name, _)| *name == import),
            "import '{import}' is re-exported"
        );
    }
}
