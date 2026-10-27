//! Regression test: a `print:` block printed nothing.
//!
//! The parser produces `Statement::PrintBlock { expressions }` for
//!
//! ```clean
//! print:
//!     "first"
//!     "second"
//! ```
//!
//! but the HIR builder had no handler for it, so the statement fell through
//! the wildcard arm of `build_statement` and lowered to a discarded `void`
//! literal — every expression in the block vanished from the binary and
//! nothing was printed. Per `foundation/spec/stdlib-reference.md` §`print:`
//! Block, each indented expression prints on its own line.
//!
//! Fix (src/hir/hir_builder.rs `build_block_inner`): expand the block into
//! one `HirStatement::Print { newline: true }` per expression.
//!
//! Structural check: every expression's string literal must reach the
//! compiled WASM's data section.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Parser as WasmParser, Payload};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

#[test]
fn print_block_keeps_every_expression() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(
        &main,
        "start:\n\tprint:\n\t\t\"print-block-line-one\"\n\t\t\"print-block-line-two\"\n",
    )
    .expect("write repro.cln");

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
        panic!("a print: block in start: must compile cleanly");
    });

    let mut data_blob: Vec<u8> = Vec::new();
    for payload in WasmParser::new(0).parse_all(&wasm) {
        if let Ok(Payload::DataSection(reader)) = payload {
            for segment in reader {
                let segment = segment.expect("read data segment");
                data_blob.extend_from_slice(segment.data);
            }
        }
    }

    for needle in [&b"print-block-line-one"[..], &b"print-block-line-two"[..]] {
        assert!(
            data_blob.windows(needle.len()).any(|w| w == needle),
            "every expression of a print: block must be compiled and printed; `{}` is missing \
             from the data section, so the block was dropped",
            String::from_utf8_lossy(needle)
        );
    }
}
