//! Regression test for dashboard fingerprints 6c964defcf04, e3374aae8b90,
//! 44ed914df8e1, 2e0e6d775000 — the SEM005 private-member cluster.
//!
//! Per foundation/spec/semantic-rules.md §Visibility, class methods and
//! fields are private by default. Calls from outside the class must go
//! through a member declared inside a `public:` sub-section of the
//! class's `functions:` (methods) or the class body (fields).
//!
//! Users repeatedly re-filed this error because the bare
//! "`X` is private and cannot be accessed from outside `C`" message did
//! not tell them what to do. Session 4-hygiene (0.33.6) added an
//! actionable `help:` hint that names the required `public:` sub-section
//! and cites the spec section. That hint is the resolution — the
//! underlying visibility rule is working as intended.
//!
//! This test locks in that the hint fires and names both the specific
//! member and the enclosing class.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::io::Write;

fn compile_expect_error(source: &str) -> String {
    let temp_dir = std::env::temp_dir().join(format!(
        "clean_sem005_hint_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("mkdir tmp");
    let path = temp_dir.join("entry.cln");
    {
        let mut f = std::fs::File::create(&path).expect("create tmp source");
        f.write_all(source.as_bytes()).expect("write source");
    }
    let errors = match compile_multi_file_with_memory_tier(
        &path,
        vec![temp_dir.clone()],
        0,
        None,
        MemoryTier::Standard,
        false,
    ) {
        Ok(_) => panic!("expected compile to fail; source:\n{source}"),
        Err(errs) => errs,
    };
    let combined = errors
        .iter()
        .map(|e| format!("{e}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::remove_dir_all(&temp_dir);
    combined
}

/// 6c964defcf04, e3374aae8b90, 2e0e6d775000 — private method call from outside.
#[test]
fn private_method_call_emits_actionable_hint() {
    let source = "\
start:
\tGreeter g = Greeter(\"World\")
\tprint(g.hello())

class Greeter
\tstring who

\tconstructor(string whoParam)
\t\twho = whoParam

\tfunctions:
\t\tstring hello()
\t\t\treturn \"Hello, \" + who
";
    let combined = compile_expect_error(source);
    assert!(
        combined.contains("private"),
        "expected private-member message, got:\n{combined}"
    );
    assert!(
        combined.contains("hello") && combined.contains("Greeter"),
        "expected the diagnostic to name both `hello` and `Greeter`, got:\n{combined}"
    );
    assert!(
        combined.contains("public:"),
        "expected the hint to name the required `public:` sub-section, got:\n{combined}"
    );
}

/// 44ed914df8e1 — private field access from outside (different class name).
#[test]
fn private_method_call_hint_uses_named_class() {
    let source = "\
start:
\tAnimal a = Animal(\"Rex\")
\tprint(a.name())

class Animal
\tstring _name

\tconstructor(string n)
\t\t_name = n

\tfunctions:
\t\tstring name()
\t\t\treturn _name
";
    let combined = compile_expect_error(source);
    assert!(
        combined.contains("private"),
        "expected private-member message, got:\n{combined}"
    );
    assert!(
        combined.contains("Animal") && combined.contains("name"),
        "expected the hint to name both `name` and `Animal`, got:\n{combined}"
    );
}
