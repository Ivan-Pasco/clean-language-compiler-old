//! `value!` halts on none (type-system.md §8, RUN004).
//!
//! Regression: the MIR dead-code pass removed a `!` whose result was never
//! read, together with its check, so `string unused = name!` did not halt.
//! `has_side_effects` now keeps the `Required` check.
//!
//! A halt is a WASM trap, which the `.cln` pins (stdout comparison) cannot
//! express, so the test calls exported functions and checks for the trap.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Instance, Linker, Module, Store};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

const SOURCE: &str = "start:\n\tinteger ready = 1\n\nfunctions:\n\tinteger unwrapText(integer give)\n\t\tstring? name = none\n\t\tif give == 1\n\t\t\tname = \"x\"\n\t\tstring unused = name!\n\t\treturn 1\n";

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
        panic!("the `!` program must compile");
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

fn call(store: &mut Store<()>, instance: &Instance, name: &str, give: i32) -> Option<i32> {
    instance
        .get_typed_func::<i32, i32>(&mut *store, name)
        .unwrap_or_else(|_| panic!("{name} export"))
        .call(&mut *store, give)
        .ok()
}

#[test]
fn required_with_unused_result_halts_on_none() {
    let (mut store, instance) = instantiate();
    assert_eq!(
        call(&mut store, &instance, "unwrapText", 1),
        Some(1),
        "`name!` of a held string returns"
    );
    assert_eq!(
        call(&mut store, &instance, "unwrapText", 0),
        None,
        "`string unused = name!` must halt when name is none, though `unused` is never read"
    );
}
