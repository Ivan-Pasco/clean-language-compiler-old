//! Regression test: pushing past a list's capacity corrupted memory.
//!
//! `list.add` (used by `push`/`add`) stored the new element at
//! `list_ptr + 16 + size * 4` without checking the capacity, and the caller
//! discarded the pointer it returned. An empty list starts with room for 8
//! elements, so the 9th push wrote past the list's block into whatever was
//! allocated next (in `while_outer_list_push_escape.cln`, the strings being
//! pushed: `first=` printed empty and `total_len` was garbage).
//!
//! Fix: `list.add` moves a full list to a block with double the capacity and
//! returns the new address; the MIR builder writes the returned pointer back
//! into the receiver variable or field.
//!
//! The test grows two lists that were allocated back to back, so an
//! overflowing push on the first lands on the second's header, then sums
//! both back out: every element of both lists must survive each growth step.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Linker, Module, Store};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

const SOURCE: &str = "start:\n\tprint(checksum(3).toString())\n\nfunctions:\n\tinteger checksum(integer n)\n\t\tlist<integer> xs = []\n\t\tlist<integer> ys = []\n\t\tinteger i = 0\n\t\twhile i < n\n\t\t\txs.push(i)\n\t\t\tys.push(1)\n\t\t\ti = i + 1\n\t\tinteger total = 0\n\t\titerate v in xs\n\t\t\ttotal = total + v\n\t\tinteger ones = 0\n\t\titerate w in ys\n\t\t\tones = ones + w\n\t\treturn total * 1000 + xs.length() + ones * 1000000\n";

#[test]
fn push_past_capacity_keeps_every_element() {
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
        panic!("list push program must compile");
    });

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).expect("valid module");
    let mut linker: Linker<()> = Linker::new(&engine);
    // Loop bodies are bracketed by memory scope calls; hosts implement them
    // as no-ops (clean-server host-bridge memory.rs).
    linker
        .func_wrap("memory_runtime", "mem_scope_push", || {})
        .expect("mem_scope_push");
    linker
        .func_wrap("memory_runtime", "mem_scope_pop", || {})
        .expect("mem_scope_pop");
    linker
        .define_unknown_imports_as_traps(&module)
        .expect("stub imports");
    let mut store = Store::new(&engine, ());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");

    // Function parameters and returns of type integer compile to i32 here.
    let checksum = instance
        .get_typed_func::<i32, i32>(&mut store, "checksum")
        .expect("checksum export");

    for n in [0i32, 1, 8, 9, 16, 17, 100, 400] {
        let got = checksum.call(&mut store, n).expect("checksum call");
        let expected = (n * (n - 1) / 2) * 1000 + n + n * 1_000_000;
        assert_eq!(
            got, expected,
            "pushing {n} integers onto two lists must keep every element of both"
        );
    }
}
