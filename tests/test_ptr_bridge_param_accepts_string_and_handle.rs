//! type-system.md §9b: a `ptr` bridge parameter takes a string or an opaque
//! byte handle (the integer `req.body_bytes()` returns), passed unboxed.
//! frame.storage's `storage.write(path, bytes)` maps to `_fs_write_bytes`
//! (params ["string", "ptr"]). Skipped when frame.storage is not installed.

use clean_language_compiler::compile_with_external_plugins_and_opt_level;

fn installed(plugin: &str) -> bool {
    std::env::var_os("HOME")
        .map(|home| {
            std::path::Path::new(&home)
                .join(".cleen/plugins")
                .join(plugin)
                .join("plugin.toml")
                .exists()
        })
        .unwrap_or(false)
}

#[test]
fn storage_write_accepts_text_and_byte_handle() {
    if !installed("frame.storage") {
        eprintln!("Skipping: frame.storage not installed");
        return;
    }
    let source = "\
plugins:
\tframe.storage

start:
\tinteger a = storage.write(\"a.txt\", \"text\")
\tstring read = storage.read(\"a.txt\")
\tinteger b = storage.write(\"b.txt\", read)
\tinteger handle = 0
\tinteger c = storage.write(\"c.txt\", handle)
\tprintl((a + b + c).toString())
";
    let wasm = compile_with_external_plugins_and_opt_level(source, "storage_ptr.cln", 0)
        .unwrap_or_else(|errors| {
            panic!(
                "a string and an integer handle must both type-check for a ptr parameter: {:?}",
                errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
            )
        });
    assert!(wasm.starts_with(b"\0asm"));
}
