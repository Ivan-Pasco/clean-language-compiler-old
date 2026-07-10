//! Regression test for COM001 crypto.sha256 via library entry point
//! (fingerprint `ee1a83f99a474c098524b3f36b3031e773079db957d2c7916f778343ae4f8b52`,
//! reported against compiler 0.33.20).
//!
//! Background: `cln compile ...` (the CLI) routes crypto.sha256 through
//! `compile_multi_file_release` / `compile_multi_file_with_memory_tier`
//! and produces valid WASM. The library entry point
//! `compile_with_external_plugins_and_opt_level` (used by Rust integration
//! tests via `clean_language_compiler::compile*`) routes the same source
//! through `compile_with_plugins_and_opt_level` and emitted invalid WASM:
//!   "type mismatch: values remaining on stack at end of block"
//!
//! The fix consolidates both entry points to the multi-file compiler +
//! `lower_tast_to_mir_release` codegen path, eliminating the divergence.
//! This test drives the previously-failing entry point directly.

use std::fs;
use std::path::PathBuf;

#[test]
fn crypto_sha256_via_library_entry_point() {
    // Skip if frame.auth plugin is not installed locally — this test needs
    // a real plugin registry to reproduce the codegen divergence.
    let plugin_dir = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".cleen/plugins/frame.auth"))
        .filter(|p| p.exists());
    if plugin_dir.is_none() {
        eprintln!("SKIP: frame.auth plugin not installed at ~/.cleen/plugins/frame.auth");
        return;
    }

    let source =
        "plugins:\n\tframe.auth\n\nstart:\n\tstring h = crypto.sha256(\"hello\")\n\tprint(h)\n";

    // Write source to a temp file so plugin discovery can find it.
    let tmpdir = std::env::temp_dir().join(format!("com001_repro_{}", std::process::id()));
    fs::create_dir_all(&tmpdir).expect("mkdir tmpdir");
    let path = tmpdir.join("main.cln");
    fs::write(&path, source).expect("write source");

    let result = clean_language_compiler::compile_with_external_plugins_and_opt_level(
        source,
        path.to_str().unwrap(),
        2,
    );

    let _ = fs::remove_dir_all(&tmpdir);

    match result {
        Ok(wasm) => {
            assert!(
                !wasm.is_empty(),
                "library entry point returned empty WASM — fix incomplete"
            );
            eprintln!(
                "COM001 fixed: library entry point produced {} bytes of valid WASM",
                wasm.len()
            );
        }
        Err(errors) => {
            let joined: Vec<String> = errors.iter().map(|e| format!("{:?}", e)).collect();
            panic!(
                "COM001 still reproduces via library entry point: {}",
                joined.join("\n")
            );
        }
    }
}
