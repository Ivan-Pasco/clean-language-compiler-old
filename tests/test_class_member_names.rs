//! Class methods and constructors have their own internal names
//! (`Counter::bump`), so a method never replaces a top-level function of the
//! same name (`src/codegen/mir_codegen/exports.rs`).
//!
//! All functions used to share one name table: with a method `bump` and a
//! top-level `bump`, the export `bump` went to the method and the top-level
//! function, called only by the host through that export, was removed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use wasmparser::{Parser, Payload};
use wasmtime::{Caller, Engine, Extern, Instance, Linker, Module, Store};

/// Export name → function index.
fn function_exports(wasm: &[u8]) -> BTreeMap<String, u32> {
    let mut exports = BTreeMap::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ExportSection(reader) = payload.expect("valid module") {
            for export in reader {
                let export = export.expect("export");
                if export.kind == wasmparser::ExternalKind::Func {
                    exports.insert(export.name.to_string(), export.index);
                }
            }
        }
    }
    exports
}

/// Instantiate with working `print` / `printl` (every other import traps);
/// the returned buffer collects what they print.
fn instantiate(wasm: &[u8]) -> (Store<()>, Instance, Arc<Mutex<String>>) {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).expect("valid module");
    let output = Arc::new(Mutex::new(String::new()));
    let mut linker: Linker<()> = Linker::new(&engine);
    for name in ["print", "printl"] {
        let sink = output.clone();
        linker
            .func_wrap(
                "env",
                name,
                move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
                    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                        panic!("memory export");
                    };
                    let bytes = &memory.data(&caller)[ptr as usize..(ptr + len) as usize];
                    let mut out = sink.lock().unwrap();
                    out.push_str(std::str::from_utf8(bytes).expect("utf-8"));
                    out.push('\n');
                },
            )
            .expect("define print");
    }
    linker
        .define_unknown_imports_as_traps(&module)
        .expect("stub imports");
    let mut store = Store::new(&engine, ());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    (store, instance, output)
}

const METHOD_AND_HOST_CALLED_FUNCTION: &str = "start:
\tCounter c = Counter()
\tprint(c.bump().toString())
\tprint(c.bump().toString())

class Counter
\tpublic:
\t\tinteger count

\tconstructor()
\t\tcount = 0

\tfunctions:
\t\tpublic:
\t\t\tinteger bump()
\t\t\t\tcount = count + 1
\t\t\t\treturn count

functions:
\tinteger bump()
\t\treturn 70
";

#[test]
fn top_level_function_called_only_by_the_host_keeps_its_export() {
    let wasm = clean_language_compiler::compile(METHOD_AND_HOST_CALLED_FUNCTION).expect("compiles");
    let exports = function_exports(&wasm);
    assert!(exports.contains_key("bump"), "bump is not exported");
    assert!(
        !exports.keys().any(|name| name.contains("::")),
        "internal member names are exported: {exports:?}"
    );

    let (mut store, instance, output) = instantiate(&wasm);
    instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .expect("_start")
        .call(&mut store, ())
        .expect("runs");
    assert_eq!(output.lock().unwrap().as_str(), "1\n2\n");
    let bump = instance
        .get_typed_func::<(), i32>(&mut store, "bump")
        .expect("the export `bump` is the top-level `integer bump()`");
    assert_eq!(bump.call(&mut store, ()).expect("runs"), 70);
}

#[test]
fn method_without_a_top_level_namesake_keeps_its_bare_export() {
    let source = "start:
\tCounter c = Counter()
\tprint(c.bump().toString())

class Counter
\tpublic:
\t\tinteger count

\tconstructor()
\t\tcount = 0

\tfunctions:
\t\tpublic:
\t\t\tinteger bump()
\t\t\t\tcount = count + 1
\t\t\t\treturn count
";
    let wasm = clean_language_compiler::compile(source).expect("compiles");
    let exports = function_exports(&wasm);
    assert!(exports.contains_key("bump"), "bump is not exported");
    assert!(
        exports.contains_key("constructor"),
        "constructor is not exported"
    );
    assert!(
        !exports.keys().any(|name| name.contains("::")),
        "internal member names are exported: {exports:?}"
    );
    let (mut store, instance, _) = instantiate(&wasm);
    instance
        .get_typed_func::<i32, i32>(&mut store, "bump")
        .expect("the export `bump` is the method (it takes `this`)");
}

#[test]
fn event_handler_export_is_the_dispatch_function() {
    let source = "start:
\tinstance.onMount()

state:
\tToolbar instance = Toolbar()

class Toolbar
\tevents:
\t\tonMount():
\t\t\tprintl(\"mounted\")
\t\tfmt_bold():
\t\t\tprintl(\"bold clicked\")
";
    let wasm = clean_language_compiler::compile(source).expect("compiles");
    let (mut store, instance, output) = instantiate(&wasm);
    instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .expect("_start")
        .call(&mut store, ())
        .expect("runs");
    instance
        .get_typed_func::<(), ()>(&mut store, "fmt_bold")
        .expect("fmt_bold export")
        .call(&mut store, ())
        .expect("handler runs");
    assert_eq!(output.lock().unwrap().as_str(), "mounted\nbold clicked\n");
}
