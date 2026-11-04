//! Regression test: the built-in `db.*` functions carry their
//! stdlib-reference §9 return types without the frame.data plugin.
//!
//! Without a plugin, `db.query` & co. resolved through the `db` namespace
//! with no signature, so every call was typed Unknown. A `string` target
//! then treated the bridge's length-prefixed text as a boxed `any` and ran
//! it through any-to-string — printing garbage, or trapping once the
//! array/object branch handed the bytes to `_json_encode_v2`.
//! (framework `tests/framework/unit/bridge/db-bridge-test.cln`).
//!
//! Spec: foundation/spec/stdlib-reference.md §9 — `db.query` → string,
//! `db.execute` → integer, `db.begin` / `db.commit` / `db.rollback` → boolean.

#![cfg(feature = "wasmtime-runtime")]

use std::fs;
use tempfile::TempDir;
use wasmtime::{Caller, Engine, Extern, Linker, Module, Store};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

const PROGRAM: &str = r#"start:
	string rows = db.query("SELECT 1 as v", "")
	printl(rows)
	integer changed = db.execute("DELETE FROM t", "")
	printl(changed.toString())
	boolean began = db.begin()
	printl(began.toString())
	boolean committed = db.commit()
	printl(committed.toString())
	boolean rolled = db.rollback()
	printl(rolled.toString())
"#;

const QUERY_RESULT: &str = r#"{"ok":true,"data":{"rows":[{"v":1}],"count":1}}"#;

fn read(caller: &mut Caller<'_, Vec<String>>, ptr: i32, len: i32) -> String {
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .expect("memory");
    let data = memory.data(&*caller);
    String::from_utf8_lossy(&data[ptr as usize..(ptr + len) as usize]).into_owned()
}

#[test]
fn db_builtins_keep_their_return_types_without_plugins() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(&main, PROGRAM).expect("write program");
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
    let mut linker: Linker<Vec<String>> = Linker::new(&engine);
    linker
        .func_wrap(
            "env",
            "printl",
            |mut caller: Caller<'_, Vec<String>>, ptr: i32, len: i32| {
                let line = read(&mut caller, ptr, len);
                caller.data_mut().push(line);
            },
        )
        .unwrap();
    // `_db_query(sql_ptr, sql_len, params_ptr, params_len)` returns the JSON
    // envelope as a length-prefixed string allocated with the module's malloc.
    linker
        .func_wrap(
            "env",
            "_db_query",
            |mut caller: Caller<'_, Vec<String>>, _: i32, _: i32, _: i32, _: i32| -> i32 {
                let malloc = caller
                    .get_export("malloc")
                    .and_then(Extern::into_func)
                    .expect("malloc")
                    .typed::<i32, i32>(&caller)
                    .expect("malloc signature");
                let bytes = QUERY_RESULT.as_bytes();
                let ptr = malloc.call(&mut caller, 4 + bytes.len() as i32).unwrap();
                let memory = caller
                    .get_export("memory")
                    .and_then(Extern::into_memory)
                    .expect("memory");
                let data = memory.data_mut(&mut caller);
                let p = ptr as usize;
                data[p..p + 4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                data[p + 4..p + 4 + bytes.len()].copy_from_slice(bytes);
                ptr
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "env",
            "_db_execute",
            |_: Caller<'_, Vec<String>>, _: i32, _: i32, _: i32, _: i32| -> i32 { 3 },
        )
        .unwrap();
    for (name, result) in [("_db_begin", 1), ("_db_commit", 1), ("_db_rollback", 0)] {
        linker
            .func_wrap("env", name, move |_: Caller<'_, Vec<String>>| -> i32 {
                result
            })
            .unwrap();
    }
    linker
        .define_unknown_imports_as_traps(&module)
        .expect("trap the remaining imports");

    let mut store = Store::new(&engine, Vec::new());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    instance
        .get_typed_func::<(), ()>(&mut store, "start")
        .expect("start export")
        .call(&mut store, ())
        .expect("program runs without trapping");

    assert_eq!(
        store.data(),
        &vec![
            QUERY_RESULT.to_string(),
            "3".to_string(),
            "true".to_string(),
            "true".to_string(),
            "false".to_string(),
        ]
    );
}
