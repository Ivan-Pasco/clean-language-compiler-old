//! `diagnose_source` checks an editor buffer as the real file: imports resolve
//! relative to the file's directory (even when the buffer differs from what is
//! on disk or the file does not exist yet), errors and warnings are returned
//! apart, and every diagnostic carries its error code.

use clean_language_compiler::diagnose_source;

const MODULE: &str = "functions:\n\tpublic:\n\t\tinteger twice(integer n)\n\t\t\treturn n * 2\n";
const USES_MODULE: &str = "import:\n\tmathutil\n\nstart:\n\tprintl(mathutil.twice(4).toString())\n";

#[test]
fn imports_resolve_relative_to_the_real_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mathutil.cln"), MODULE).unwrap();
    let entry = dir.path().join("main.cln");
    // The buffer is not on disk at all.
    let result = diagnose_source(USES_MODULE, entry.to_str().unwrap());
    assert!(
        result.errors.is_empty(),
        "import must resolve next to the file; got {:?}",
        result.errors
    );
}

#[test]
fn imports_resolve_when_the_buffer_differs_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mathutil.cln"), MODULE).unwrap();
    let entry = dir.path().join("main.cln");
    std::fs::write(&entry, "start:\n\tprintl(\"saved\")\n").unwrap();
    let result = diagnose_source(USES_MODULE, entry.to_str().unwrap());
    assert!(result.errors.is_empty(), "got {:?}", result.errors);
}

#[test]
fn a_missing_import_is_an_error_with_a_code_or_message() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.cln");
    let result = diagnose_source(USES_MODULE, entry.to_str().unwrap());
    assert!(!result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn warnings_are_returned_apart_from_errors_and_carry_codes() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.cln");
    let source = "\
start:
\tprintl(pick(1).toString())

functions:
\tinteger pick(integer a)
\t\tif a > 0
\t\t\treturn 1
\t\treturn
";
    let result = diagnose_source(source, entry.to_str().unwrap());
    assert!(result.errors.is_empty(), "got {:?}", result.errors);
    let codes: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| w.context().and_then(|c| c.error_code.clone()))
        .collect();
    assert!(
        codes.iter().any(|c| c == "FUNC005"),
        "expected a FUNC005 warning, got {codes:?}"
    );
}

#[test]
fn errors_name_the_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.cln");
    let result = diagnose_source("start:\n\tinteger x = \"a\"\n", entry.to_str().unwrap());
    assert!(!result.errors.is_empty());
    let located: Vec<_> = result
        .errors
        .iter()
        .filter_map(|e| e.context().and_then(|c| c.location.as_ref()))
        .collect();
    assert!(
        located.iter().all(|l| l.file.ends_with("main.cln")),
        "errors must name main.cln, got {located:?}"
    );
}
