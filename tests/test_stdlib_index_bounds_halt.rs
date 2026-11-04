//! Regression test: `.charCodeAt(index)` and `list<integer>.get(index)` must
//! halt on an out-of-bounds index (stdlib-reference.md §4 and §5).
//!
//! `string.charCodeAt` returned 0 for an out-of-range index, and the i32
//! `list.get` read past the list's elements without a bounds check, so both
//! silently produced a value instead of halting.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Instance, Linker, Module, Store};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

const SOURCE: &str = "start:\n\tprint(\"ready\")\n\nfunctions:\n\tinteger codeAt(integer i)\n\t\tstring w = \"abc\"\n\t\treturn w.charCodeAt(i)\n\n\tinteger itemAt(integer i)\n\t\tlist<integer> xs = [10, 20, 30]\n\t\treturn xs.get(i)\n";

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
        panic!("bounds-check program must compile");
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

#[test]
fn char_code_at_halts_out_of_bounds() {
    let (mut store, instance) = instantiate();
    let code_at = instance
        .get_typed_func::<i32, i32>(&mut store, "codeAt")
        .expect("codeAt export");
    assert_eq!(code_at.call(&mut store, 0).expect("in bounds"), 97);
    assert_eq!(code_at.call(&mut store, 2).expect("in bounds"), 99);
    for index in [3, 5, -1] {
        assert!(
            code_at.call(&mut store, index).is_err(),
            "\"abc\".charCodeAt({index}) must halt"
        );
    }
}

#[test]
fn integer_list_get_halts_out_of_bounds() {
    let (mut store, instance) = instantiate();
    let item_at = instance
        .get_typed_func::<i32, i32>(&mut store, "itemAt")
        .expect("itemAt export");
    assert_eq!(item_at.call(&mut store, 0).expect("in bounds"), 10);
    assert_eq!(item_at.call(&mut store, 2).expect("in bounds"), 30);
    for index in [3, 4, -1] {
        assert!(
            item_at.call(&mut store, index).is_err(),
            "[10, 20, 30].get({index}) must halt"
        );
    }
}
