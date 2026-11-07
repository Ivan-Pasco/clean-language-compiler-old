//! Multi-file import diagnostics.
//!
//! * Two imported files that define a function with the same name are a
//!   SEM003 redefinition. The merge used to keep whichever came first
//!   (HashMap order), so one module silently called the other's function.
//! * An import cycle names the modules and points at the import that closes
//!   the cycle, instead of printing `CompilationModuleId(1) -> ...` at 1:1.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::path::Path;

fn compile_errors(dir: &Path, entry: &str) -> Vec<String> {
    let errors = compile_multi_file_with_memory_tier(
        dir.join(entry),
        vec![dir.to_path_buf()],
        0,
        None,
        MemoryTier::Standard,
        false,
    )
    .expect_err("the project must not compile");
    errors
        .iter()
        .map(|e| {
            let loc = e
                .context()
                .and_then(|c| c.location.as_ref())
                .map(|l| format!(" @{}:{}", l.line, l.column))
                .unwrap_or_default();
            format!("{}{}", e, loc)
        })
        .collect()
}

#[test]
fn same_function_in_two_imported_modules_is_sem003() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.cln"),
        "import:\n\tgreetings\n\tfarewells\n\nstart:\n\tprintl(message())\n",
    )
    .unwrap();
    for (module, text) in [("greetings", "hello"), ("farewells", "bye")] {
        std::fs::write(
            dir.path().join(format!("{module}.cln")),
            format!("functions:\n\tstring message()\n\t\treturn \"{text}\"\n"),
        )
        .unwrap();
    }
    let errors = compile_errors(dir.path(), "main.cln");
    let joined = errors.join("\n");
    assert!(
        joined.contains("Function 'message' is defined in"),
        "{joined}"
    );
    assert!(
        joined.contains("greetings.cln") && joined.contains("farewells.cln"),
        "{joined}"
    );
}

#[test]
fn import_cycle_names_modules_and_import_location() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.cln"),
        "import:\n\tparts\n\nstart:\n\tprintl(\"x\")\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("parts.cln"),
        "import:\n\tpieces\n\nfunctions:\n\tinteger one()\n\t\treturn 1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("pieces.cln"),
        "import:\n\tparts\n\nfunctions:\n\tinteger two()\n\t\treturn 2\n",
    )
    .unwrap();
    let errors = compile_errors(dir.path(), "main.cln");
    let joined = errors.join("\n");
    assert!(joined.contains("Circular dependency detected"), "{joined}");
    assert!(!joined.contains("CompilationModuleId"), "{joined}");
    assert!(
        joined.contains("parts (") && joined.contains("pieces ("),
        "{joined}"
    );
    // The import that closes the cycle is on line 2 of its file.
    assert!(joined.contains("@2:2"), "{joined}");
}
