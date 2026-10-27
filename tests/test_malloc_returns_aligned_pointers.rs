//! Regression test: the exported `malloc` must return 8-byte aligned pointers.
//!
//! `foundation/spec/platform/MEMORY_MODEL.md` § Alignment requires heap
//! allocations to be aligned to `ALIGNMENT` (8 bytes). `gen_malloc` rounded
//! the requested *size* up to 8 but returned the current `__heap_ptr`
//! unchanged, so once anything left the heap pointer unaligned every later
//! allocation was misaligned too. clean-server's host bridge logged
//! "HEAP POINTER NOT PROPERLY UPDATED" on every request of the website
//! (malloc returned 136068, then 136151).
//!
//! The test sets `__heap_ptr` to an odd address and calls the exported
//! `malloc` directly.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Linker, Module, Store, Val};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

#[test]
fn malloc_aligns_the_returned_pointer() {
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
    .unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("minimal program must compile");
    });

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("valid module");
    let mut linker: Linker<()> = Linker::new(&engine);
    linker
        .define_unknown_imports_as_traps(&module)
        .expect("stub imports");
    let mut store = Store::new(&engine, ());
    let instance = linker.instantiate(&mut store, &module).expect("instantiate");

    let heap_ptr = instance
        .get_global(&mut store, "__heap_ptr")
        .expect("__heap_ptr export");
    let malloc = instance
        .get_typed_func::<i32, i32>(&mut store, "malloc")
        .expect("malloc export");

    for (start, size) in [(136_151, 4), (136_068, 30), (136_100, 1)] {
        heap_ptr
            .set(&mut store, Val::I32(start))
            .expect("set __heap_ptr");
        let ptr = malloc.call(&mut store, size).expect("malloc call");
        assert_eq!(
            ptr % 8,
            0,
            "malloc({size}) with __heap_ptr={start} returned unaligned pointer {ptr}"
        );
        assert!(ptr >= start, "malloc must not hand out memory below __heap_ptr");
        let after = heap_ptr.get(&mut store).i32().expect("i32 heap ptr");
        assert!(
            after >= ptr + size && after % 8 == 0,
            "__heap_ptr after malloc({size}) must be aligned and past the block, got {after}"
        );
    }
}
