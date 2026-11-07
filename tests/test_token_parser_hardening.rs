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

fn first_statement_value(src: &str) -> clean_language_compiler::ast::Expression {
    use clean_language_compiler::ast::Statement;
    let program = parse(src).unwrap_or_else(|e| panic!("`{}` must parse: {:?}", src, e));
    let start = program
        .functions
        .iter()
        .find(|f| f.name == "start")
        .expect("start function");
    match &start.body[0] {
        Statement::VariableDecl {
            initializer: Some(value),
            ..
        } => value.clone(),
        other => panic!("expected a declaration, got {:?}", other),
    }
}

/// `-9223372036854775808` (i64::MIN) is a valid integer literal; the bare
/// magnitude 2^63 is out of range.
#[test]
fn i64_min_literal() {
    use clean_language_compiler::ast::{Expression, Value};
    assert_eq!(
        first_statement_value("start:\n\tinteger x = -9223372036854775808\n"),
        Expression::Literal(Value::Integer(i64::MIN))
    );
    let error = parse_err("start:\n\tinteger x = 9223372036854775808\n");
    assert!(error.to_string().contains("out of range"), "{}", error);
    let error = parse_err("start:\n\tinteger x = -9223372036854775809\n");
    assert!(error.to_string().contains("Invalid number"), "{}", error);
}

/// Negative numeric literals are list elements (grammar.ebnf
/// `list_element = ... | number_literal`, which carries the sign).
#[test]
fn negative_numbers_in_list_literals() {
    use clean_language_compiler::ast::{Expression, Value};
    assert_eq!(
        first_statement_value("start:\n\tlist<integer> xs = [1, -2, 3]\n"),
        Expression::Literal(Value::List(vec![
            Value::Integer(1),
            Value::Integer(-2),
            Value::Integer(3)
        ]))
    );
    assert_eq!(
        first_statement_value("start:\n\tlist<number> xs = [-1.5]\n"),
        Expression::Literal(Value::List(vec![Value::Number(-1.5)]))
    );
    // Other unary expressions still need parentheses.
    parse_err("start:\n\tinteger a = 1\n\tlist<integer> xs = [-a]\n");
}
