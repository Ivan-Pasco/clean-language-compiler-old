//! The exported `malloc` grows memory by exactly the pages it needs and
//! rejects sizes it cannot satisfy instead of moving `__heap_ptr` to a bogus
//! address.
//!
//! - The grow amount was `(new_ptr >> 16) - pages + 1`, one page too many
//!   whenever the allocation ended exactly on a page boundary.
//! - A negative size moved `__heap_ptr` backwards over live data
//!   (`malloc(-100)`), and `malloc(-1)` returned a zero-size block.
//! - An allocation that wrapped past the top of the 32-bit address space set
//!   `__heap_ptr` to the wrapped (low) address.
//!
//! `malloc` returns 0 for every allocation it cannot make (its documented
//! failure value, `native_stdlib::memory::gen_malloc`).

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Instance, Linker, Module, Store, TypedFunc, Val};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

fn instantiate() -> (Store<()>, Instance) {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, "start:\n\tprint(\"x\")\n").expect("write repro.cln");
    let (wasm, _) = compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errors| panic!("minimal program must compile: {errors:?}"));

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

fn malloc(store: &mut Store<()>, instance: &Instance) -> TypedFunc<i32, i32> {
    instance
        .get_typed_func::<i32, i32>(store, "malloc")
        .expect("malloc export")
}

fn heap_ptr(store: &mut Store<()>, instance: &Instance) -> i32 {
    let global = instance
        .get_global(&mut *store, "__heap_ptr")
        .expect("__heap_ptr");
    global.get(store).i32().expect("i32 heap pointer")
}

fn set_heap_ptr(store: &mut Store<()>, instance: &Instance, value: i32) {
    let global = instance
        .get_global(&mut *store, "__heap_ptr")
        .expect("__heap_ptr");
    global.set(store, Val::I32(value)).expect("set __heap_ptr");
}

#[test]
fn malloc_grows_by_exactly_the_pages_it_needs() {
    let (mut store, instance) = instantiate();
    let memory = instance.get_memory(&mut store, "memory").expect("memory");
    let malloc = malloc(&mut store, &instance);

    let pages = memory.size(&store) as i32;
    // The block ends exactly on the boundary one page past the current end.
    set_heap_ptr(&mut store, &instance, pages * 65_536 - 8);
    let ptr = malloc.call(&mut store, 8 + 65_536).expect("malloc call");
    assert_eq!(ptr, pages * 65_536 - 8);
    assert_eq!(
        memory.size(&store) as i32,
        pages + 1,
        "an allocation ending on a page boundary needs one more page"
    );
    assert_eq!(heap_ptr(&mut store, &instance), (pages + 1) * 65_536);
}

#[test]
fn malloc_rejects_negative_sizes() {
    let (mut store, instance) = instantiate();
    let malloc = malloc(&mut store, &instance);
    let before = heap_ptr(&mut store, &instance);
    for size in [-1, -100, i32::MIN] {
        assert_eq!(
            malloc.call(&mut store, size).expect("malloc call"),
            0,
            "malloc({size})"
        );
        assert_eq!(
            heap_ptr(&mut store, &instance),
            before,
            "malloc({size}) must not move __heap_ptr"
        );
    }
}

#[test]
fn malloc_rejects_allocations_past_the_address_space() {
    let (mut store, instance) = instantiate();
    let malloc = malloc(&mut store, &instance);
    // 16 bytes below 4 GiB: 64 more bytes would wrap to address 48.
    set_heap_ptr(&mut store, &instance, -16);
    assert_eq!(malloc.call(&mut store, 64).expect("malloc call"), 0);
    assert_eq!(heap_ptr(&mut store, &instance), -16);
}
