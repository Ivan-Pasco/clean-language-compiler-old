//! Runtime errors without an `onError` handler stop the program with their
//! message (14-error-handling.md; developer decision for 0.33.184).
//!
//! `.toInteger()` / `.toNumber()` of text that is not a number in range and
//! an integer division by zero raise an error. Inside an `onError` region
//! the handler runs (`tests/cln/bugfixes/string_to_integer_raises_on_invalid_text.cln`,
//! `string_to_number_raises_on_invalid_text.cln`,
//! `integer_division_by_zero_raises.cln`); outside one the message is printed
//! and execution traps. A trap cannot be expressed by the `.cln` pins
//! (stdout comparison), so these tests run `_start` and check both.

use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Engine, Extern, Linker, Module, Store};

/// Run `_start` with a working `printl` (every other import traps). Returns
/// what it printed and whether it ran to the end.
fn run(source: &str) -> (String, bool) {
    let wasm = clean_language_compiler::compile(source).expect("compiles");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("valid module");
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
    let completed = start.call(&mut store, ()).is_ok();
    let text = output.lock().unwrap().clone();
    (text, completed)
}

#[test]
fn invalid_integer_text_without_on_error_stops_with_message() {
    let (output, completed) = run(
        "start:\n\tstring text = \"abc\"\n\tinteger n = text.toInteger()\n\tprintl(\"after\")\n",
    );
    assert!(!completed, "the program must stop");
    assert_eq!(output, "'abc' is not a valid integer\n");
}

#[test]
fn integer_out_of_range_without_on_error_stops_with_message() {
    let (output, completed) = run(
        "start:\n\tstring text = \"99999999999\"\n\tinteger n = text.toInteger()\n\tprintl(\"after\")\n",
    );
    assert!(!completed, "the program must stop");
    assert_eq!(output, "'99999999999' is too large for integer\n");
}

#[test]
fn invalid_number_text_without_on_error_stops_with_message() {
    let (output, completed) = run(
        "start:\n\tstring text = \"1.2.3\"\n\tnumber n = text.toNumber()\n\tprintl(\"after\")\n",
    );
    assert!(!completed, "the program must stop");
    assert_eq!(output, "'1.2.3' is not a valid number\n");
}

#[test]
fn integer_division_by_zero_without_on_error_stops_with_message() {
    let (output, completed) =
        run("start:\n\tinteger zero = 0\n\tinteger n = 7 / zero\n\tprintl(\"after\")\n");
    assert!(!completed, "the program must stop");
    assert_eq!(output, "division by zero\n");
}

#[test]
fn integer_remainder_by_zero_without_on_error_stops_with_message() {
    let (output, completed) =
        run("start:\n\tinteger zero = 0\n\tinteger n = 7 % zero\n\tprintl(\"after\")\n");
    assert!(!completed, "the program must stop");
    assert_eq!(output, "division by zero\n");
}
