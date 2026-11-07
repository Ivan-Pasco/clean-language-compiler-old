//! Library compiles of in-memory sources (`compile_with_file`,
//! `type_check_with_external_plugins`) run from a scratch copy of the
//! source. Their diagnostics must name the path the caller gave, not the
//! scratch directory under the system temp dir.

const TYPE_ERROR_SOURCE: &str = "start:\n\tinteger x = \"hello\"\n\tprint(x)\n";

fn files_of(errors: &[clean_language_compiler::error::CompilerError]) -> Vec<String> {
    errors
        .iter()
        .filter_map(|e| e.context())
        .filter_map(|c| c.location.as_ref())
        .map(|l| l.file.clone())
        .collect()
}

#[test]
fn compile_errors_name_the_callers_path() {
    let errors = clean_language_compiler::compile_with_file(TYPE_ERROR_SOURCE, "app/demo.cln")
        .expect_err("type error");
    let files = files_of(&errors);
    assert!(!files.is_empty(), "errors carry a location: {errors:?}");
    for file in files {
        assert_eq!(file, "app/demo.cln");
    }
}

#[test]
fn type_check_errors_name_the_callers_path() {
    let errors =
        clean_language_compiler::type_check_with_external_plugins(TYPE_ERROR_SOURCE, "demo.cln")
            .expect_err("type error");
    let files = files_of(&errors);
    assert!(!files.is_empty(), "errors carry a location: {errors:?}");
    for file in files {
        assert_eq!(file, "demo.cln");
    }
}
