//! Regression test for dashboard fingerprints a35979d83604 and
//! e8ad1944445f — the "dotted names in `external:` blocks" SYN001
//! reports.
//!
//! The original reports complained that
//!
//!     external:
//!         integer batch.stringLit(integer ctx, string value)
//!
//! failed to parse with `SYN001: Expected LeftParen, found Dot`. That
//! grammar constraint is intentional — `foundation/spec/grammar.ebnf`
//! defines external function names as `lowercase_identifier`, which
//! excludes dots. Session 4 (0.33.5) closed the cluster the correct way:
//! typed-emission's batch builders are now registered under BOTH the
//! canonical dotted name AND an underscore alias, so plugin authors
//! declare the imports via the underscore form. See the header comment
//! in `src/plugins/typed_emission/batch_builders.rs`.
//!
//! This test locks in that:
//!  1. The dotted-name form still fails with the expected parser error
//!     (guards against a future grammar change that would silently
//!     start accepting dots — which is a semantic change we haven't
//!     approved).
//!  2. The underscore-alias form compiles cleanly and reaches codegen.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::io::Write;

fn compile_source(source: &str) -> Result<(), String> {
    let temp_dir = std::env::temp_dir().join(format!(
        "clean_external_alias_test_{}",
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
    let outcome = match compile_multi_file_with_memory_tier(
        &path,
        vec![temp_dir.clone()],
        0,
        None,
        MemoryTier::Standard,
        false,
    ) {
        Ok(_) => Ok(()),
        Err(errs) => Err(errs
            .iter()
            .map(|e| format!("{e}"))
            .collect::<Vec<_>>()
            .join("\n")),
    };
    let _ = std::fs::remove_dir_all(&temp_dir);
    outcome
}

#[test]
fn dotted_name_in_external_still_rejected() {
    // Reproduces the ORIGINAL report shape. The grammar continues to
    // reject dots in external: identifiers. If this test starts
    // passing (i.e. compile succeeds), the grammar changed silently
    // and needs a spec update per Principle 25.
    let source = "\
external:
\tinteger batch.stringLit(integer ctx, string value)

functions:
\tinteger test(integer ctx)
\t\tinteger h = batch.stringLit(ctx, \"hello\")
\t\treturn h
";
    let err = compile_source(source)
        .expect_err("dotted name in external: block must still be a SYN001 error per grammar.ebnf");
    assert!(
        err.contains("Dot") || err.contains("."),
        "expected parser error mentioning the illegal Dot token, got:\n{err}"
    );
}

#[test]
fn underscore_alias_form_compiles_cleanly() {
    // The Session 4 workaround: plugin authors use the underscore
    // alias. Both names route to the same closure in
    // batch_builders.rs. This form must compile cleanly.
    let source = "\
external:
\tinteger _batch_stringLit(integer ctx, string value)

functions:
\tinteger test(integer ctx)
\t\tinteger h = _batch_stringLit(ctx, \"hello\")
\t\treturn h
";
    compile_source(source).expect("underscore-alias external: declaration must compile cleanly");
}
