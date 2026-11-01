//! Regression test: a direct call to the canonical bridge name `_time_now()`
//! (the form plugin-emitted helpers use, e.g. frame.auth's `session_create`)
//! resolved to the raw `() -> i64` host import instead of the `time.now`
//! wrapper that wraps the result to a Clean `integer`. Storing the i64 result
//! in an `integer` local produced a module that failed WASM validation with
//! "type mismatch: expected i32, found i64".

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn direct_time_now_call_yields_integer() {
    let dir = std::env::temp_dir().join(format!(
        "clean_time_now_direct_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    let entry = dir.join("main.cln");
    fs::write(
        &entry,
        "start:\n\tprintl(expiry(3).toString())\n\nfunctions:\n\tinteger expiry(integer minutes)\n\t\tinteger e = _time_now() + (minutes * 60)\n\t\treturn e\n",
    )
    .expect("write source");

    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &entry,
        vec![dir.clone()],
        0,
        None,
        clean_language_compiler::MemoryTier::Standard,
        false,
    );
    let _ = fs::remove_dir_all(&dir);

    if let Err(errors) = result {
        let combined = errors
            .iter()
            .map(|e| format!("{e}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("direct _time_now() call failed to compile:\n{combined}");
    }
}
