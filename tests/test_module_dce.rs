//! A compiled module carries only the functions its exports can reach
//! (`src/codegen/module_dce.rs`).
//!
//! Every runtime helper (string, list, JSON, validator, ...) was emitted into
//! every module: `print("hi")` compiled to 22.7 KB with 509 defined
//! functions.

use std::sync::{Arc, Mutex};
use wasmparser::{Parser, Payload};
use wasmtime::{Caller, Engine, Extern, Linker, Module, Store};

fn defined_function_count(wasm: &[u8]) -> u32 {
    let mut functions = 0;
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::FunctionSection(reader) = payload.expect("valid module") {
            functions = reader.count();
        }
    }
    functions
}

/// Run `_start` with a working `printl` (every other import traps); returns
/// what it printed.
fn run(wasm: &[u8]) -> String {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).expect("valid module");
    let output = Arc::new(Mutex::new(String::new()));
    let mut linker: Linker<()> = Linker::new(&engine);
    let sink = output.clone();
    linker
        .func_wrap(
            "env",
            "printl",
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
        .expect("define printl");
    linker
        .define_unknown_imports_as_traps(&module)
        .expect("stub imports");
    let mut store = Store::new(&engine, ());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .expect("_start");
    start.call(&mut store, ()).expect("runs");
    let text = output.lock().unwrap().clone();
    text
}

#[test]
fn hello_world_keeps_only_what_it_calls() {
    let wasm = clean_language_compiler::compile("start:\n\tprint(\"hi\")\n").expect("compiles");
    let functions = defined_function_count(&wasm);
    // start, _start and the four exported allocator functions.
    assert!(functions <= 8, "{functions} functions");
    assert!(wasm.len() < 4_000, "{} bytes", wasm.len());
    assert_eq!(run(&wasm), "hi\n");
}

#[test]
fn helpers_reached_through_calls_are_kept() {
    let source = "start:\n\tstring name = \"Ada\"\n\tprint(name.toUpperCase())\n\tlist<integer> xs = [3, 1, 2]\n\tlist<integer> sorted = xs.sort()\n\tprint(sorted.get(0).toString())\n\tprint(twice(21).toString())\n\nfunctions:\n\tinteger twice(integer n)\n\t\treturn n * 2\n";
    let wasm = clean_language_compiler::compile(source).expect("compiles");
    assert_eq!(run(&wasm), "ADA\n1\n42\n");
}
