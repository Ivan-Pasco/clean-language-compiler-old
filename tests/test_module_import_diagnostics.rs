//! Multi-file import diagnostics.
//!
//! * Two files joined into one namespace (file-path imports, `shared:`)
//!   that define a function with the same name are a SEM003 redefinition;
//!   modules imported by name may each declare the same public name. The merge used to keep whichever came first
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
fn same_function_in_two_imported_modules_coexist() {
    // Fixed: this test expected SEM003. Spec decision 2026-11-27 (same public
    // name in two imported modules): modules imported by name are separate
    // namespaces, so `greetings.message()` and `farewells.message()` coexist.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.cln"),
        "import:\n\tgreetings\n\tfarewells\n\nstart:\n\tprintl(greetings.message())\n\tprintl(farewells.message())\n",
    )
    .unwrap();
    for (module, text) in [("greetings", "hello"), ("farewells", "bye")] {
        std::fs::write(
            dir.path().join(format!("{module}.cln")),
            format!("functions:\n\tpublic:\n\t\tstring message()\n\t\t\treturn \"{text}\"\n"),
        )
        .unwrap();
    }
    let (wasm, _) = compile_multi_file_with_memory_tier(
        dir.path().join("main.cln"),
        vec![dir.path().to_path_buf()],
        0,
        None,
        MemoryTier::Standard,
        false,
    )
    .expect("both modules' message() must compile side by side");
    // Each qualified call reaches its own module's string literal.
    let has = |needle: &[u8]| wasm.windows(needle.len()).any(|w| w == needle);
    assert!(has(b"hello") && has(b"bye"));
}

#[test]
fn same_function_in_two_file_path_imports_is_sem003() {
    // Files joined by file-path imports are one namespace (spec 18), so the
    // same public name in both stays a redefinition.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.cln"),
        "import:\n\t\"greetings.cln\"\n\t\"farewells.cln\"\n\nstart:\n\tprintl(message())\n",
    )
    .unwrap();
    for (module, text) in [("greetings", "hello"), ("farewells", "bye")] {
        std::fs::write(
            dir.path().join(format!("{module}.cln")),
            format!("functions:\n\tpublic:\n\t\tstring message()\n\t\t\treturn \"{text}\"\n"),
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
fn private_function_of_two_imported_modules_is_sem005() {
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
        joined.contains("'message' is private and cannot be accessed from outside"),
        "{joined}"
    );
    assert!(
        joined.contains("'greetings'") && joined.contains("'farewells'"),
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
