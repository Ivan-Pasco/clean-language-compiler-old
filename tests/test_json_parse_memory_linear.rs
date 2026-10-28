//! Regression test: JSON parsing used memory quadratic in the input size.
//!
//! `__json_parse_string` reserved `4 + <total JSON length>` bytes for every
//! string it decoded (keys and values alike), so a document with N strings
//! reserved N copies of the whole document. A 26 KB website section used
//! 20 MB of heap; parsing two of them exceeded the 32 MB standard memory cap,
//! `malloc` started returning 0 and the parse silently truncated arrays
//! (the website's /syntax and /tutorials pages returned 500).
//!
//! Fix: `__json_parse_string` measures the string's raw span with
//! `__json_skip_string` and allocates only that much (decoded output is never
//! longer than the raw text).
//!
//! The test parses a document of many small objects through the exported
//! `json.textToData` and checks both the heap used and the parsed content.

use std::fs;
use tempfile::TempDir;
use wasmtime::{Engine, Linker, Module, Store};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

fn document(objects: usize) -> String {
    let items: Vec<String> = (0..objects)
        .map(|i| {
            format!(
                "{{\"title\":\"Item number {i}\",\"note\":\"with \\\"quotes\\\" and \\\\ slash\"}}"
            )
        })
        .collect();
    format!("{{\"kind\":\"list\",\"items\":[{}]}}", items.join(","))
}

#[test]
fn parse_memory_grows_linearly_with_input() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(
        &main,
        "start:\n\tprint(json.get(\"{\\\"a\\\":1}\", \"a\"))\n",
    )
    .expect("write");
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
        panic!("program must compile");
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
    let memory = instance.get_memory(&mut store, "memory").expect("memory");
    let heap = instance.get_global(&mut store, "__heap_ptr").expect("heap");
    let malloc = instance
        .get_typed_func::<i32, i32>(&mut store, "malloc")
        .expect("malloc");
    let parse = instance
        .get_typed_func::<i32, i32>(&mut store, "json.textToData")
        .expect("json.textToData");

    let doc = document(400);
    let bytes = doc.as_bytes();
    let ptr = malloc
        .call(&mut store, 4 + bytes.len() as i32)
        .expect("malloc") as usize;
    memory.data_mut(&mut store)[ptr..ptr + 4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    memory.data_mut(&mut store)[ptr + 4..ptr + 4 + bytes.len()].copy_from_slice(bytes);

    let before = heap.get(&mut store).i32().expect("i32") as usize;
    let tree = parse.call(&mut store, ptr as i32).expect("parse");
    let after = heap.get(&mut store).i32().expect("i32") as usize;
    assert_ne!(tree, 0, "document must parse");

    let used = after - before;
    assert!(
        used < bytes.len() * 20,
        "parsing {} bytes used {used} bytes of heap ({}x); must be linear in the input",
        bytes.len(),
        used / bytes.len()
    );

    // Content check: tree is [tag=6][object]; object is [count][key,val]...;
    // the "items" value is [tag=5][array]; array is [count][elems...].
    let read = |store: &Store<()>, p: usize| -> usize {
        u32::from_le_bytes(memory.data(store)[p..p + 4].try_into().unwrap()) as usize
    };
    let object = read(&store, tree as usize + 4);
    let items_value = read(&store, object + 4 + 8 + 4);
    let array = read(&store, items_value + 4);
    assert_eq!(
        read(&store, array),
        400,
        "all 400 array elements must be parsed"
    );
}
