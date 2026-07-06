//! Regression test for dashboard fingerprints ceb568e0aaa7, 8653d059d75d
//! (`Function 'parseInt' not found` at SEM007) and b6d1b80449d8,
//! 5cdbcb58ee83 (`Function 'string.toInt' not found in function map`).
//!
//! Session 4-hygiene (0.33.6) shipped the curated "did you mean?" hint at
//! the codegen COD000 site in mir_codegen/instructions.rs but the parity
//! follow-up noted in §4 was to extend the same table to the resolver's
//! SEM007 emission site — `resolver_impl.rs::resolve_call_expression`.
//! Without the resolver-side hint, `parseInt("42")` at top-level compiled
//! all the way to the resolver and emitted the bare "Function 'parseInt'
//! not found" message with no suggestion, while the same misname at
//! codegen phase (reached via a namespace-method dispatch) got the
//! hint. That parity gap kept the resolver-side reporters
//! (ceb568e0aaa7, 8653d059d75d, fd9d187c30a0) refiling.
//!
//! This test locks in that:
//!  * `parseInt("42")` at top-level emits SEM007 with the curated hint
//!    naming `string.toInteger`.
//!  * `parseFloat("3.14")` emits the curated hint naming `string.toNumber`.
//!  * A random unknown name (`t(5)`) still surfaces a fuzzy fallback
//!    hint from the symbol table rather than being silent.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::io::Write;

fn compile_source_expect_error(source: &str) -> String {
    let temp_dir = std::env::temp_dir().join(format!(
        "clean_sem007_hint_test_{}",
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
fn parseint_at_resolver_emits_curated_hint() {
    let source = "start:\n\tinteger n = parseInt(\"42\")\n\tprint(n.toString())\n";
    let combined = compile_source_expect_error(source);
    assert!(
        combined.contains("not found"),
        "expected \"Function ... not found\" (SEM007) in output, got:\n{combined}"
    );
    assert!(
        combined.contains("string.toInteger"),
        "expected curated hint naming `string.toInteger`, got:\n{combined}"
    );
}

#[test]
fn parsefloat_at_resolver_emits_curated_hint() {
    let source = "start:\n\tnumber n = parseFloat(\"3.14\")\n\tprint(n.toString())\n";
    let combined = compile_source_expect_error(source);
    assert!(
        combined.contains("not found"),
        "expected \"Function ... not found\" (SEM007) in output, got:\n{combined}"
    );
    assert!(
        combined.contains("string.toNumber"),
        "expected curated hint naming `string.toNumber`, got:\n{combined}"
    );
}

#[test]
fn unknown_name_falls_back_to_fuzzy_suggestion() {
    let source = "start:\n\tinteger x = t(5)\n\tprint(x.toString())\n";
    let combined = compile_source_expect_error(source);
    assert!(
        combined.contains("not found"),
        "expected \"Function ... not found\" (SEM007) in output, got:\n{combined}"
    );
    assert!(
        combined.contains("Did you mean"),
        "expected a fuzzy-match `Did you mean?` hint, got:\n{combined}"
    );
}
