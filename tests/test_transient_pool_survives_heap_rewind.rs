//! Regression test: the transient arena must not overlap memory the main heap
//! hands out after a `__heap_ptr` rewind.
//!
//! The singleshot string-builder rewrite builds helper strings in the
//! transient arena. The arena's pool used to be `__malloc`-ed lazily on the
//! first `__transient_scope_enter`, i.e. on the main heap. Any rewind of
//! `__heap_ptr` below it (the compiler's `scope_pop`, the host's
//! `mem_scope_pop`, a per-request reset) gave those bytes back to the heap
//! while `TRANSIENT_BASE` still pointed at them, so the next string built
//! in the pool overwrote live heap data.
//!
//! That corrupted the source fragments frame.server builds for its
//! dev-capture helpers, and every `endpoints:` block failed to compile
//! (clean-language-compiler-old #15).
//!
//! The test runs a program that uses the singleshot builder, rewinds
//! `__heap_ptr` to its initial value the way a host does between requests,
//! claims the memory above it, runs the program again, and checks that the
//! claimed memory is untouched.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Linker, Module, Store, Val};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

const CLAIMED_BYTES: usize = 70_000;
const FILL: u8 = 0xAB;

#[test]
fn transient_pool_survives_heap_rewind() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");

    let mut source = String::from(
        "start:\n\tstring a = build()\n\tinteger n = a.length()\n\nfunctions:\n\tstring build()\n\t\tstring s = \"\"\n",
    );
    for i in 0..12 {
        source.push_str(&format!(
            "\t\ts = s + \"line {i} of the fragment that grows past the first buffer size\\n\"\n"
        ));
    }
    source.push_str("\t\treturn s\n");
    fs::write(&main, source).expect("write repro.cln");

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
        panic!("repro program must compile");
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

    let heap_ptr = instance
        .get_global(&mut store, "__heap_ptr")
        .expect("__heap_ptr export");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("memory export");
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .expect("_start export");

    let initial_heap = heap_ptr.get(&mut store).i32().expect("i32 heap ptr");

    // First run: uses the transient arena (and, before the fix, allocated
    // its pool on the main heap).
    start.call(&mut store, ()).expect("first run");

    // Host-style rewind to the initial heap start, then claim the memory
    // above it as a host or a later allocation would.
    heap_ptr
        .set(&mut store, Val::I32(initial_heap))
        .expect("rewind __heap_ptr");
    let claimed_start = initial_heap as usize;
    let claimed_end = claimed_start + CLAIMED_BYTES;
    let pages_needed = (claimed_end as u64).div_ceil(65_536);
    let current_pages = memory.size(&store);
    if pages_needed > current_pages {
        memory
            .grow(&mut store, pages_needed - current_pages)
            .expect("grow memory for the claimed block");
    }
    memory.data_mut(&mut store)[claimed_start..claimed_end].fill(FILL);
    heap_ptr
        .set(&mut store, Val::I32(claimed_end as i32))
        .expect("move __heap_ptr past the claimed block");

    // Second run: builds the same strings again.
    start.call(&mut store, ()).expect("second run");

    let data = &memory.data(&store)[claimed_start..claimed_end];
    if let Some(offset) = data.iter().position(|&b| b != FILL) {
        panic!(
            "the transient arena wrote into memory the heap had handed out: \
             first changed byte at {} (heap start {})",
            claimed_start + offset,
            initial_heap
        );
    }
}
