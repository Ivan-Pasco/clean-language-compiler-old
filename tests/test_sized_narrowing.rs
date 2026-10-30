//! Regression test: implicit narrowing between explicitly sized types is a
//! type error (type-system.md §5; clean-language-compiler-old #17).

use std::fs;
use tempfile::TempDir;

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

fn compile(body: &str) -> Result<(), String> {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(&main, format!("start:\n{body}")).expect("write main.cln");
    compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .map(|_| ())
    .map_err(|errors| {
        errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn assert_narrowing(body: &str) {
    let err = compile(body).expect_err("narrowing must not compile");
    assert!(
        err.contains("Narrowing assignment"),
        "unexpected error: {err}"
    );
}

#[test]
fn number_64_to_number_32_is_rejected() {
    assert_narrowing("\tnumber:64 a = 3.5\n\tnumber:32 b = a\n\tprint(b.toString())\n");
}

#[test]
fn wider_sized_integer_to_narrower_is_rejected() {
    assert_narrowing("\tinteger:16 a = 300\n\tinteger:8 b = a\n\tprint(b.toString())\n");
    assert_narrowing("\tinteger:64 a = 3\n\tinteger:32 b = a\n\tprint(b.toString())\n");
}

#[test]
fn widening_unsized_and_literals_are_allowed() {
    compile("\tnumber:32 a = 1.5\n\tnumber:64 b = a\n\tprint(b.toString())\n")
        .expect("widening number");
    compile("\tnumber a = 1.5\n\tnumber:32 b = a\n\tprint(b.toString())\n")
        .expect("unsized to sized");
    compile("\tinteger:64 a = 5\n\tinteger b = a\n\tprint(b.toString())\n")
        .expect("sized to unsized");
    compile("\tinteger:8 a = 5\n\tinteger:16 b = a\n\tprint(b.toString())\n")
        .expect("widening integer");
    compile("\tnumber:32 b = 3.14\n\tprint(b.toString())\n").expect("literal");
    compile("\tinteger:64 a = 7\n\tinteger:32 b = a.toInteger()\n\tprint(b.toString())\n")
        .expect("explicit conversion");
}
