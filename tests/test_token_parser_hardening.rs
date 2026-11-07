//! Regression tests for the token-driven lexer and parser: inputs that used
//! to be accepted silently, rejected wrongly, or reported at the wrong place.
//! Grammar references are to foundation/spec/grammar/grammar.ebnf.

use clean_language_compiler::ast::Program;
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::parse_to_ast;

fn parse(src: &str) -> Result<Program, Vec<CompilerError>> {
    parse_to_ast(src, "hardening.cln")
}

fn parse_err(src: &str) -> CompilerError {
    match parse(src) {
        Ok(_) => panic!("`{}` must be rejected", src.escape_debug()),
        Err(errors) => errors.into_iter().next().expect("at least one error"),
    }
}

fn error_line_col(error: &CompilerError) -> (usize, usize) {
    let location = error
        .context()
        .and_then(|c| c.location.as_ref())
        .unwrap_or_else(|| panic!("error has no location: {}", error));
    (location.line, location.column)
}

/// Lexer errors carry the position of the offending character instead of
/// falling back to 1:1.
#[test]
fn lexer_errors_point_at_the_offending_character() {
    let error = parse_err("start:\n\tinteger x = 1\n\tprint($)\n");
    assert_eq!(error_line_col(&error), (3, 8), "{}", error);

    let error = parse_err("start:\n\tprint(\"open\n");
    assert_eq!(error_line_col(&error).0, 2, "{}", error);

    let error = parse_err("start:\n\tinteger x = 99999999999999999999\n");
    assert_eq!(error_line_col(&error), (2, 14), "{}", error);
}
