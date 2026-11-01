//! Regression test: `crypto.hmac` (stdlib-reference.md §10, Layer 2) failed
//! in codegen with "Function 'crypto.hmac' not found in function map" when no
//! plugin supplied it, while every sibling crypto.* function had a built-in
//! wrapper over its host bridge. Argument order follows the bridge contract
//! in function-registry.toml (`_crypto_hmac(data, key, algorithm)`).

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn crypto_hmac_compiles_without_plugins() {
    let dir = std::env::temp_dir().join(format!(
        "clean_crypto_hmac_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    let entry = dir.join("main.cln");
    fs::write(
        &entry,
        "start:\n\tstring h = crypto.hmac(\"message\", \"secret-key\", \"sha256\")\n\tprintl(h)\n",
    )
    .expect("write source");

    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &entry,
        vec![dir.clone()],
        0,
        None,
        clean_language_compiler::MemoryTier::Standard,
        false,
    );
    let _ = fs::remove_dir_all(&dir);

    let wasm = match result {
        Ok((wasm, _build_state)) => wasm,
        Err(errors) => {
            let combined = errors
                .iter()
                .map(|e| format!("{e}"))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("crypto.hmac failed to compile:\n{combined}");
        }
    };
    // The wrapper must call the host bridge, so its import must be present.
    assert!(
        wasm.windows(b"_crypto_hmac".len())
            .any(|w| w == b"_crypto_hmac"),
        "compiled module does not import _crypto_hmac"
    );
}
