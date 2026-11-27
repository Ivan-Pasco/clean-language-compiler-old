//! Regression test for COM001 fingerprint `f26672a3b9a11308` — bare
//! `toString(x)` where `x` is a `number` (f64) produced invalid WASM.
//!
//! Fixed: this test used to require that bare `toString(x)` compiles. Spec 17
//! (method-style syntax, "one name" rule) gives `toString` only the method
//! form `x.toString()`, and compiler 0.33.190 removed the bare built-in
//! (issue #51) — it printed an empty string. The original defect (invalid
//! WASM from an i32-shaped call carrying an f64) can no longer be reached
//! through the bare form; the method form is covered by
//! `test_number_tostring_early_dispatch`. What this test now pins is that the
//! bare form is a clean compile error instead of invalid WASM or empty text.

use std::fs;
use tempfile::TempDir;

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	number x = 3.14
	string s = toString(x)
	print(s) +
"#;

#[test]
fn bare_tostring_on_number_is_rejected() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, REPRO).expect("write repro.cln");

    let errors = clean_language_compiler::compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .expect_err("bare toString(number) is not a built-in (spec 17 one-name rule)");

    let joined = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("Function 'toString' not found"),
        "expected an undeclared-function error, got:\n{joined}"
    );
}
