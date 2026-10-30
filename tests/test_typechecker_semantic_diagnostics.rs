//! Regression tests for type-checker / validation diagnostics that used to be
//! silently accepted, misreported, or dropped (compiler 0.33.165).
//!
//! * FUNC001 — calling a method that no class in the receiver's hierarchy
//!   declares compiled and dispatched to an unrelated function at runtime.
//! * IDX001 / IDX004 — index-type mismatches were warnings although
//!   foundation/spec/error-codes.md defines them as errors.
//! * Binary-operator type errors were reported at line 0, column 0.
//! * Warnings (FUNC004/FUNC005/FUNC007) were computed but never returned,
//!   and FUNC004 was computed by two stages.

use clean_language_compiler::error::{CompilerError, ErrorContext, ErrorSeverity};
use clean_language_compiler::hir::hir_builder::HirBuilder;
use clean_language_compiler::hir::validation::HirValidator;
use clean_language_compiler::resolver::NameResolver;
use clean_language_compiler::typechecker::TypeChecker;
use clean_language_compiler::{compile_multi_file_with_memory_tier, parse_to_ast, MemoryTier};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_temp_dir() -> std::path::PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "clean_typechecker_diag_{}_{}",
        std::process::id(),
        n
    ))
}

fn compile(source: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    let temp_dir = unique_temp_dir();
    std::fs::create_dir_all(&temp_dir).expect("mkdir tmp");
    let path = temp_dir.join("entry.cln");
    {
        let mut f = std::fs::File::create(&path).expect("create tmp source");
        f.write_all(source.as_bytes()).expect("write source");
    }
    let result = compile_multi_file_with_memory_tier(
        &path,
        vec![temp_dir.clone()],
        0,
        None,
        MemoryTier::Standard,
        false,
    )
    .map(|(wasm, _)| wasm);
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

fn context(e: &CompilerError) -> Option<&ErrorContext> {
    match e {
        CompilerError::Syntax { context }
        | CompilerError::Type { context }
        | CompilerError::Memory { context }
        | CompilerError::Codegen { context }
        | CompilerError::IO { context }
        | CompilerError::Runtime { context }
        | CompilerError::Validation { context }
        | CompilerError::Module { context }
        | CompilerError::Testing { context } => Some(context),
        _ => None,
    }
}

fn code(e: &CompilerError) -> Option<&str> {
    context(e).and_then(|c| c.error_code.as_deref())
}

fn expect_errors(source: &str) -> Vec<CompilerError> {
    match compile(source) {
        Ok(_) => panic!("expected compile to fail; source:\n{source}"),
        Err(errs) => errs,
    }
}

fn render(errors: &[CompilerError]) -> String {
    errors
        .iter()
        .map(|e| format!("[{}] {e}", code(e).unwrap_or("-")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs parse → HIR → validation → resolution → type check and returns every
/// warning reported along the way.
fn pipeline_warnings(source: &str) -> Vec<CompilerError> {
    let ast = parse_to_ast(source, "warn_test.cln").expect("parse");
    let hir_result = HirBuilder::new().build_hir(ast).expect("hir");
    let mut warnings = hir_result.warnings.clone();
    warnings.extend(HirValidator::validate(&hir_result.hir).expect("validation"));
    let resolved = NameResolver::resolve(hir_result.hir).expect("resolve");
    warnings.extend(resolved.warnings);
    let typed = TypeChecker::check(resolved.resolved_hir).expect("type check");
    warnings.extend(typed.warnings);
    warnings
}

#[test]
fn unknown_method_on_class_is_func001_error() {
    let source = "\
start:
\tFoo f = Foo()
\tf.nope()
\tprintl(\"ok\")

class Foo
\tinteger x

\tconstructor()
\t\tx = 1
";
    let errors = expect_errors(source);
    let rendered = render(&errors);
    assert!(
        errors.iter().any(|e| code(e) == Some("FUNC001")
            && e.to_string()
                .contains("Method 'nope' not found on class 'Foo'")),
        "expected FUNC001 \"Method 'nope' not found on class 'Foo'\", got:\n{rendered}"
    );
}

#[test]
fn unknown_method_suggests_close_inherited_name() {
    let source = "\
start:
\tDog d = Dog()
\tprintl(d.speek())

class Animal
\tinteger legs

\tconstructor()
\t\tlegs = 4

\tfunctions:
\t\tpublic:
\t\t\tstring speak()
\t\t\t\treturn \"...\"

class Dog is Animal
\tconstructor()
\t\tbase()
";
    let errors = expect_errors(source);
    let unknown = errors
        .iter()
        .find(|e| code(e) == Some("FUNC001"))
        .unwrap_or_else(|| panic!("expected FUNC001, got:\n{}", render(&errors)));
    let ctx = context(unknown).expect("context");
    assert!(
        ctx.suggestions.iter().any(|s| s.contains("speak")),
        "expected a did-you-mean suggestion naming the inherited `speak`, got: {:?}",
        ctx.suggestions
    );
}

#[test]
fn string_index_into_list_is_idx001_error() {
    let source = "\
start:
\tlist<integer> xs = [1, 2, 3]
\tinteger v = xs[\"a\"]
\tprintl(v.toString())
";
    let errors = expect_errors(source);
    assert!(
        errors.iter().any(|e| code(e) == Some("IDX001")),
        "expected IDX001 error, got:\n{}",
        render(&errors)
    );
}

#[test]
fn index_into_integer_is_idx004_error() {
    let source = "\
start:
\tinteger n = 5
\tinteger v = n[0]
\tprintl(v.toString())
";
    let errors = expect_errors(source);
    assert!(
        errors.iter().any(|e| code(e) == Some("IDX004")),
        "expected IDX004 error, got:\n{}",
        render(&errors)
    );
}

#[test]
fn binary_type_error_reports_a_real_location() {
    let source = "\
start:
\tstring s = \"abc\"
\tboolean b = s.length() and true
\tprintl(b.toString())
";
    let errors = expect_errors(source);
    let located: Vec<_> = errors
        .iter()
        .filter_map(context)
        .filter(|c| c.message.contains("and") || c.message.contains("boolean"))
        .collect();
    assert!(
        !located.is_empty(),
        "expected a binary operator type error, got:\n{}",
        render(&errors)
    );
    for ctx in located {
        let loc = ctx
            .location
            .as_ref()
            .unwrap_or_else(|| panic!("binary type error has no location: {}", ctx.message));
        assert!(
            loc.line > 0,
            "binary type error reported at line 0: {} ({}:{}:{})",
            ctx.message,
            loc.file,
            loc.line,
            loc.column
        );
    }
}

#[test]
fn stage_warnings_are_returned_and_func004_is_reported_once() {
    let source = "\
start:
\tprintl(pick(1).toString())

functions:
\tinteger pick(integer a)
\t\tif a > 0
\t\t\treturn 1
";
    let warnings = pipeline_warnings(source);
    let func004: Vec<_> = warnings
        .iter()
        .filter(|w| code(w) == Some("FUNC004"))
        .collect();
    assert_eq!(
        func004.len(),
        1,
        "expected exactly one FUNC004 warning, got:\n{}",
        render(&warnings)
    );
    assert!(
        func004
            .iter()
            .all(|w| context(w).map(|c| c.severity == ErrorSeverity::Warning) == Some(true)),
        "FUNC004 must be a warning"
    );
}

#[test]
fn method_missing_return_is_func004_warning() {
    let source = "\
start:
\tCounter c = Counter()
\tprintl(c.next(1).toString())

class Counter
\tinteger n

\tconstructor()
\t\tn = 0

\tfunctions:
\t\tpublic:
\t\t\tinteger next(integer step)
\t\t\t\tif step > 0
\t\t\t\t\treturn n + step
";
    let warnings = pipeline_warnings(source);
    let func004 = warnings
        .iter()
        .filter(|w| code(w) == Some("FUNC004"))
        .count();
    assert_eq!(
        func004,
        1,
        "expected exactly one FUNC004 warning for the method, got:\n{}",
        render(&warnings)
    );
}

#[test]
fn warnings_do_not_fail_compilation() {
    let source = "\
start:
\tprintl(pick(1).toString())

functions:
\tinteger pick(integer a)
\t\tif a > 0
\t\t\treturn 1
";
    let warnings = pipeline_warnings(source);
    assert!(
        warnings.iter().any(|w| code(w) == Some("FUNC004")),
        "expected a FUNC004 warning, got:\n{}",
        render(&warnings)
    );
    if let Err(errors) = compile(source) {
        panic!(
            "a program whose only diagnostics are warnings must compile, got:\n{}",
            render(&errors)
        );
    }
}
