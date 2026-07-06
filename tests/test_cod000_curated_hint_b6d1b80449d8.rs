//! Regression test for dashboard fingerprints b6d1b80449d8 and
//! 5cdbcb58ee83 — `Function 'string.toInt' not found in function map`.
//!
//! Session 4-hygiene (0.33.6) shipped `build_did_you_mean_hint` in
//! `src/codegen/mir_codegen/instructions.rs` with a curated legacy-alias
//! table that redirects `string.toInt` (and other common misnames) to
//! their canonical Clean Language equivalents. That closed the codegen
//! side of the parity gap; the resolver-side companion sits in
//! `test_sem007_curated_hint_ceb568e0aaa7.rs`.
//!
//! This test locks in that a call to `.toInt()` on a string variable
//! reaches codegen and the hint fires there naming `string.toInteger`.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::io::Write;

fn compile_expect_error(source: &str) -> String {
    let temp_dir = std::env::temp_dir().join(format!(
        "clean_cod000_hint_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("mkdir tmp");
    let path = temp_dir.join("entry.cln");
    {
        let mut f = std::fs::File::create(&path).expect("create tmp source");
        f.write_all(source.as_bytes()).expect("write source");
    }
    let errors = match compile_multi_file_with_memory_tier(
        &path,
        vec![temp_dir.clone()],
        0,
        None,
        MemoryTier::Standard,
        false,
    ) {
        Ok(_) => panic!("expected compile to fail; source:\n{source}"),
        Err(errs) => errs,
    };
    let combined = errors
        .iter()
        .map(|e| format!("{e}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::remove_dir_all(&temp_dir);
    combined
}

#[test]
fn string_dot_toint_emits_curated_hint() {
    let source = "\
start:
\tstring s = \"42\"
\tinteger n = s.toInt()
\tprint(n.toString())
";
    let combined = compile_expect_error(source);
    assert!(
        combined.contains("string.toInt"),
        "expected the diagnostic to name `string.toInt` (the missing symbol), got:\n{combined}"
    );
    assert!(
        combined.contains("string.toInteger"),
        "expected the curated hint to name `string.toInteger`, got:\n{combined}"
    );
}
