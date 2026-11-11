//! Regression test: reading `xs[i]` with an index outside `0..length` must
//! halt, as `xs.get(i)` does (stdlib-reference.md §5).
//!
//! The indexed read computed the element address without a bounds check, so
//! an out-of-range index returned whatever memory followed the list.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Instance, Linker, Module, Store};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

const SOURCE: &str = "start:\n\tprint(\"ready\")\n\nfunctions:\n\tinteger itemAt(integer i)\n\t\tlist<integer> xs = [10, 20, 30]\n\t\treturn xs[i]\n\n\tinteger numberAt(integer i)\n\t\tlist<number> xs = list.fill(3, 1.5)\n\t\txs[1] = 2.5\n\t\txs[2] = 3.5\n\t\tnumber v = xs[i]\n\t\treturn (v * 10.0).toInteger()\n";

fn instantiate() -> (Store<()>, Instance) {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, SOURCE).expect("write repro.cln");
    let (wasm, _) = compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("indexed-read program must compile");
    });

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("valid module");
    let mut linker: Linker<()> = Linker::new(&engine);
    linker
        .define_unknown_imports_as_traps(&module)
        .expect("stub imports");
    let mut store = Store::new(&engine, ());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    (store, instance)
}

fn check(export: &str, in_bounds: &[(i32, i32)]) {
    let (mut store, instance) = instantiate();
    let read = instance
        .get_typed_func::<i32, i32>(&mut store, export)
        .unwrap_or_else(|_| panic!("{export} export"));
    for &(index, expected) in in_bounds {
        assert_eq!(read.call(&mut store, index).expect("in bounds"), expected);
    }
    for index in [3, 4, -1, i32::MIN] {
        assert!(
            read.call(&mut store, index).is_err(),
            "{export}({index}) must halt"
        );
    }
}

#[test]
fn integer_list_index_read_halts_out_of_bounds() {
    check("itemAt", &[(0, 10), (1, 20), (2, 30)]);
}

#[test]
fn number_list_index_read_halts_out_of_bounds() {
    check("numberAt", &[(0, 15), (1, 25), (2, 35)]);
}
