//! Negative compilation tests — files that must be REJECTED by the compiler.
//!
//! These tests verify that the compiler correctly enforces language rules by
//! rejecting invalid programs. A test passes when compilation fails with the
//! expected error code.

use std::fs;

fn compile_source(
    path: &str,
) -> Result<Vec<u8>, Vec<clean_language_compiler::error::CompilerError>> {
    let source = fs::read_to_string(path).expect("test file must exist");
    clean_language_compiler::compile_with_file(&source, path)
}

fn has_error_code(errors: &[clean_language_compiler::error::CompilerError], code: &str) -> bool {
    errors.iter().any(|e| {
        let msg = format!("{:?}", e);
        msg.contains(code)
    })
}

#[test]
fn class005_ensure_ordering_rejected() {
    let result = compile_source("tests/cln/validation/class005_ensure_ordering.cln");
    assert!(
        result.is_err(),
        "class005_ensure_ordering.cln must be rejected: ensure must appear before other statements"
    );
}

#[test]
fn scope005_screen_state_access_rejected() {
    let result = compile_source("tests/cln/validation/scope005_screen_state_access.cln");
    assert!(
        result.is_err(),
        "scope005_screen_state_access.cln must be rejected: screen-local state cannot be accessed outside its screen"
    );
}

#[test]
fn section_order_invalid_rejected() {
    let result = compile_source("tests/cln/validation/section_order_invalid.cln");
    assert!(
        result.is_err(),
        "section_order_invalid.cln must be rejected: start: section out of order"
    );
}

#[test]
fn syn008_empty_print_block_rejected() {
    let result = compile_source("tests/cln/validation/syn008_empty_print_block.cln");
    assert!(
        result.is_err(),
        "syn008_empty_print_block.cln must be rejected: print: block must contain at least one expression"
    );
    let errors = result.unwrap_err();
    assert!(
        has_error_code(&errors, "SYN008"),
        "expected SYN008 error, got: {:?}",
        errors
    );
}
