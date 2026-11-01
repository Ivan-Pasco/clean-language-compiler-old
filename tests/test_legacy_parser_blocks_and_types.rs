//! Regression tests for the pest (legacy) parser, which still backs plugin
//! fragments (`CleanParser::parse_single_statement` falls back to it) and the
//! error-recovering parse.
//!
//! 1. A nested block (the body of an `if` inside a `while`/function body) used
//!    to swallow every following line that was indented at least two levels,
//!    so a statement after `if ... break` / `if ... return` landed inside the
//!    `if` (grammar.ebnf `indented_block`: a block holds exactly the lines at
//!    its own indentation level).
//! 2. `list<T>`, `matrix<T>` and `pairs<K, V>` were atomic grammar rules, so
//!    their element types were dropped and every list parsed as `list<any>`
//!    (grammar.ebnf `list_type`, `matrix_type`, `pairs_type`).

use clean_language_compiler::ast::{Statement, Type};
use clean_language_compiler::parser::ErrorRecoveringParser;

fn start_body(src: &str) -> Vec<Statement> {
    let mut parser = ErrorRecoveringParser::new(src, "<legacy-parser-test>");
    let program = parser
        .parse_program()
        .unwrap_or_else(|e| panic!("legacy parser rejected the source: {e}\n{src}"));
    program
        .start_function
        .clone()
        .or_else(|| {
            program
                .functions
                .iter()
                .find(|f| f.name == "start")
                .cloned()
        })
        .expect("start function")
        .body
}

fn while_body(stmts: &[Statement]) -> &[Statement] {
    stmts
        .iter()
        .find_map(|s| match s {
            Statement::While { body, .. } => Some(body.as_slice()),
            _ => None,
        })
        .expect("while statement")
}

fn then_branch(stmt: &Statement) -> &[Statement] {
    match stmt {
        Statement::If { then_branch, .. } => then_branch,
        other => panic!("expected an if statement, got {other:?}"),
    }
}

#[test]
fn statement_after_if_break_stays_in_loop_body() {
    let body = start_body(
        "start:\n\tinteger i = 0\n\twhile i < 5\n\t\tif i == 3\n\t\t\tbreak\n\t\ti = i + 1\n\tprintl(i.toString())\n",
    );
    assert_eq!(body.len(), 3, "start body: decl, while, printl");
    let loop_body = while_body(&body);
    assert_eq!(loop_body.len(), 2, "loop body: if, assignment");
    let inner = then_branch(&loop_body[0]);
    assert_eq!(inner.len(), 1);
    assert!(matches!(inner[0], Statement::Break { .. }));
    assert!(matches!(loop_body[1], Statement::Assignment { .. }));
}

#[test]
fn statement_after_if_return_stays_in_loop_body() {
    let body = start_body(
        "start:\n\tinteger i = 0\n\twhile i < 5\n\t\tif i == 3\n\t\t\treturn 0\n\t\ti = i + 1\n\t\tprintl(i.toString())\n\tprintl(\"end\")\n",
    );
    assert_eq!(body.len(), 3);
    let loop_body = while_body(&body);
    assert_eq!(loop_body.len(), 3, "loop body: if, assignment, printl");
    let inner = then_branch(&loop_body[0]);
    assert_eq!(inner.len(), 1);
    assert!(matches!(inner[0], Statement::Return { .. }));
}

#[test]
fn deeper_block_closes_back_to_each_enclosing_level() {
    let body = start_body(
        "start:\n\tinteger i = 0\n\twhile i < 5\n\t\tif i > 1\n\t\t\tif i == 3\n\t\t\t\tbreak\n\t\t\tprintl(\"inner\")\n\t\ti = i + 1\n\tprintl(\"end\")\n",
    );
    assert_eq!(body.len(), 3);
    let loop_body = while_body(&body);
    assert_eq!(loop_body.len(), 2, "loop body: outer if, assignment");
    let outer = then_branch(&loop_body[0]);
    assert_eq!(outer.len(), 2, "outer if: inner if, printl");
    assert_eq!(then_branch(&outer[0]).len(), 1);
}

#[test]
fn list_matrix_pairs_keep_their_type_arguments() {
    let body = start_body(
        "start:\n\tlist<string> xs = [\"a\"]\n\tmatrix<number> m = [[1.0]]\n\tpairs<string, integer> p = {}\n",
    );
    let types: Vec<&Type> = body
        .iter()
        .map(|s| match s {
            Statement::VariableDecl { type_, .. } => type_,
            other => panic!("expected a declaration, got {other:?}"),
        })
        .collect();
    assert!(
        matches!(types[0], Type::List(inner, _) if **inner == Type::String),
        "list<string> parsed as {:?}",
        types[0]
    );
    assert!(
        matches!(types[1], Type::Matrix(inner) if **inner == Type::Number),
        "matrix<number> parsed as {:?}",
        types[1]
    );
    assert!(
        matches!(types[2], Type::Pairs(k, v) if **k == Type::String && **v == Type::Integer),
        "pairs<string, integer> parsed as {:?}",
        types[2]
    );
}

#[test]
fn list_element_access_keeps_element_type_in_declaration() {
    let body = start_body("start:\n\tlist<string> xs = [\"a\"]\n\tstring s = xs[0]\n");
    match &body[0] {
        Statement::VariableDecl { type_, .. } => {
            assert!(matches!(type_, Type::List(inner, _) if **inner == Type::String))
        }
        other => panic!("expected a declaration, got {other:?}"),
    }
}
