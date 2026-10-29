//! Regression test: a declaration glued to the end of a statement line is a
//! syntax error (clean-language-compiler-old #19). `test(car)class Vehicle`
//! used to end the `start:` block silently and begin a class.

use std::fs;
use tempfile::TempDir;

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

fn compile(source: &str) -> Result<(), String> {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(&main, source).expect("write main.cln");
    compile_multi_file_with_memory_tier(&main, vec![tmp.path().to_path_buf()], 2, None, MemoryTier::Standard, false)
        .map(|_| ())
        .map_err(|errors| errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

#[test]
fn class_on_the_same_line_as_a_statement_is_rejected() {
    let err = compile("start:\n\tprint(\"a\")class Vehicle\n\tstring make\n")
        .expect_err("a class glued to a statement must not compile");
    assert!(err.contains("must start on its own line"), "unexpected error: {err}");
}

#[test]
fn class_on_its_own_line_still_compiles() {
    compile("start:\n\tprint(\"a\")\n\nclass Vehicle\n\tstring make\n").expect("a class on its own line compiles");
}
