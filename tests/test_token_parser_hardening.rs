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

/// Precision widths outside foundation/spec/type-system.md §2 are rejected
/// instead of being truncated to a u8 (`integer:264` used to become 8 bits).
#[test]
fn invalid_precision_widths_are_rejected() {
    for bad in [
        "integer:264",
        "integer:7",
        "integer:128",
        "number:16",
        "number:32u",
    ] {
        let error = parse_err(&format!("start:\n\t{} x = 5\n", bad));
        assert!(
            error.to_string().contains("Invalid precision"),
            "{}: {}",
            bad,
            error
        );
    }
    for good in [
        "integer:8",
        "integer:16u",
        "integer:32u",
        "integer:64",
        "number:32",
        "number:64",
    ] {
        let src = format!("start:\n\t{} x = 5\n", good);
        assert!(parse(&src).is_ok(), "{} must parse", good);
    }
}

/// A bare `return` followed by another statement at the same level used to
/// fail with "Unexpected token in expression: Indent(n)": the line break was
/// skipped before the no-value check.
#[test]
fn bare_return_followed_by_a_statement() {
    use clean_language_compiler::ast::Statement;
    let program = parse(
        "start:\n\tf()\n\nfunctions:\n\tvoid f()\n\t\treturn\n\t\tprint(1)\n\tinteger g()\n\t\treturn // done\n",
    )
    .unwrap_or_else(|e| panic!("must parse: {:?}", e));
    let f = program.functions.iter().find(|f| f.name == "f").unwrap();
    assert!(matches!(f.body[0], Statement::Return { value: None, .. }));
    assert_eq!(f.body.len(), 2);
    let g = program.functions.iter().find(|f| f.name == "g").unwrap();
    assert!(matches!(g.body[0], Statement::Return { value: None, .. }));
    // A value on the same line is still the return value.
    let program = parse("start:\n\tf()\n\nfunctions:\n\tinteger f()\n\t\treturn /* x */ 5\n")
        .unwrap_or_else(|e| panic!("must parse: {:?}", e));
    let f = program.functions.iter().find(|f| f.name == "f").unwrap();
    assert!(matches!(
        f.body[0],
        Statement::Return { value: Some(_), .. }
    ));
}

/// A block header with no deeper-indented body used to take the following
/// lines at its own level as its body (grammar.ebnf `indented_block`).
#[test]
fn block_header_without_indented_body_is_rejected() {
    for src in [
        "start:\n\tinteger x = 1\n\tif x > 0\n\tprint(\"in\")\n",
        "start:\n\tinteger x = 1\n\twhile x > 0\n\tx = x - 1\n",
        "start:\n\tlist<integer> xs = [1]\n\titerate v in xs\n\tprint(v)\n",
        "start:\n\titerate i in 1 to 3\n\tprint(i)\n",
        "start:\n\tf()\n\nfunctions:\n\tvoid f()\n\tprint(1)\n",
    ] {
        let error = parse_err(src);
        assert!(
            error.to_string().contains("Expected an indented block"),
            "{}: {}",
            src.escape_debug(),
            error
        );
    }
    // An unindented line after `start:` is not its body.
    parse_err("start:\nprint(1)\n");
    // A header with nothing under it before the next section is empty.
    parse("start:\n\t// nothing yet\n\nfunctions:\n\tvoid f()\n\t\tprint(1)\n")
        .unwrap_or_else(|e| panic!("must parse: {:?}", e));
    // Nested bodies still work, including an else branch.
    parse("start:\n\tinteger x = 1\n\tif x > 0\n\t\tprint(1)\n\telse\n\t\tprint(2)\n\tprint(3)\n")
        .unwrap_or_else(|e| panic!("must parse: {:?}", e));
}

/// Statements are separated by line breaks (grammar.ebnf
/// `simple_indented_block`); several on one line used to be accepted.
#[test]
fn one_statement_per_line() {
    for src in [
        "start:\n\tinteger x = 1 integer y = 2\n",
        "start:\n\tinteger x = 1\n\tprint(x) print(x)\n",
        "start:\n\tinteger x = 1\n\tx = 2 x = 3\n",
    ] {
        let error = parse_err(src);
        assert!(
            error
                .to_string()
                .contains("Expected a new line after the statement"),
            "{}: {}",
            src.escape_debug(),
            error
        );
    }
    // Expressions may still span lines inside parentheses, and trailing
    // comments are fine.
    parse(
        "start:\n\tinteger x = max(1,\n\t\t2)\n\tlist<integer> xs = [1, 2]\n\tprint(x) // done\n\tprint(xs[0]) /* c */\n",
    )
    .unwrap_or_else(|e| panic!("must parse: {:?}", e));
}
