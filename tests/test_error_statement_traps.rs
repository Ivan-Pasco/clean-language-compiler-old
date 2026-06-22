//! Regression test for COMPILER-SYNTHETIC-MODULE-START-BLOCK-DROPPED
//! (dashboard fingerprint `4e740d00a254`, reported against 0.30.326+).
//!
//! Background: the parser produces `Statement::Error { message }` for
//! `error("…")`, but the HIR builder had no handler — the variant fell
//! through the wildcard `_ => Ok(HirStatement::Expression { Void })`
//! arm in `build_statement`. Every `error("…")` call lowered to a
//! discarded `void` literal: the message string was DCE'd out of the
//! data section, no host bridge was called, and execution continued
//! past the would-be trap.
//!
//! The dashboard symptom: frame.ui's diagnostic synthetic modules
//! contained `start:\n\terror("…")` to surface plugin-side build
//! failures, but the resulting WASM's `_start` was empty and the
//! diagnostic string was absent from the binary entirely. The
//! "synthetic module start: dropped" framing was a downstream
//! observation; the root cause was that `error()` itself was a no-op.
//!
//! Fix (src/hir/hir_builder.rs `build_block_inner`): explicitly
//! handle `Statement::Error { message }` by emitting two HIR
//! statements:
//!   1. `HirStatement::Print { expression: message, newline: true }`
//!      preserves the message in the data section and prints it.
//!   2. `HirStatement::Require { condition: Literal(false) }`
//!      traps unconditionally (canonical Clean Language trap form).
//!
//! This is a structural test: it asserts the message string literal
//! appears in the compiled WASM's data section. Execution would also
//! confirm the trap fires, but the data-section check is host-agnostic
//! and catches the regression even when the trap-emission path itself
//! has its own bugs.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Parser as WasmParser, Payload};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

#[test]
fn error_statement_preserves_message_in_data_section() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(
        &main,
        "start:\n\terror(\"error-statement-message-must-survive\")\n",
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
        panic!("error() in start: must compile cleanly — it is valid Clean Language syntax");
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

    let needle = b"error-statement-message-must-survive";
    let found = data_blob.windows(needle.len()).any(|w| w == needle);

    assert!(
        found,
        "COMPILER-SYNTHETIC-MODULE-START-BLOCK-DROPPED: the message literal passed to \
         `error(\"…\")` must appear in the compiled WASM's data section. Pre-fix the parser \
         produced `Statement::Error {{ message }}` but the HIR builder swallowed it via the \
         wildcard `_` arm, so the message was DCE'd entirely. Got a {}-byte data blob with \
         no match.",
        data_blob.len()
    );
}
