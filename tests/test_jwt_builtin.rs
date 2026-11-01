//! Regression test: the Layer 2 `jwt.*` namespace (stdlib-reference.md §11)
//! failed in codegen with "Function 'jwt.sign' not found in function map"
//! when no plugin declared the `_jwt_*` bridges, although the spec makes
//! these portable host-bridge functions available to every program.

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn jwt_functions_compile_without_plugins() {
    let dir = std::env::temp_dir().join(format!(
        "clean_jwt_builtin_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    let entry = dir.join("main.cln");
    fs::write(
        &entry,
        "start:\n\tstring token = jwt.sign(\"{}\", \"secret\", \"HS256\")\n\tprintl(jwt.verify(token, \"secret\", \"HS256\"))\n\tprintl(jwt.decode(token))\n",
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
            panic!("jwt.* failed to compile:\n{combined}");
        }
    };
    for import in ["_jwt_sign", "_jwt_verify", "_jwt_decode"] {
        assert!(
            wasm.windows(import.len()).any(|w| w == import.as_bytes()),
            "compiled module does not import {import}"
        );
    }
}
