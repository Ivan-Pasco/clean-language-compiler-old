//! Regression test: a built-in namespace wrapper must not wrap a plugin
//! bridge wrapper (clean-language-compiler-old #12).
//!
//! When a program calls `_db_query` directly, the bridge registration
//! registers the raw `_db_query` import plus an expand_strings wrapper
//! under the same name. The no-plugin fallback that builds `db.query` then
//! looked `_db_query` up by name, got the wrapper instead of the import,
//! and wrapped it again: each string argument was expanded twice, and two
//! extra values were left on the stack at the end of the function, so the
//! module failed validation ("values remaining on stack at end of block").

use std::fs;
use tempfile::TempDir;

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

#[test]
fn db_query_fallback_does_not_rewrap_the_bridge_wrapper() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(
        &main,
        "external:\n\tstring _db_query(string sql, string params)\n\nstart:\n\tstring result = _db_query(\"SELECT 1\", \"{}\")\n\tprintl(result)\n",
    )
    .expect("write main.cln");

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
        panic!("a direct _db_query call must compile to a valid module");
    });

    wasmparser::validate(&wasm).expect("generated module must validate");
}
