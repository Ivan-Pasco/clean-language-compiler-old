//! Plugin fragments (endpoint handler bodies, html: expansions) go through the
//! pest parser via `parse_single_statement`. Its `string` rule was a normal
//! (non-atomic) rule, so implicit WHITESPACE skipping removed spaces right
//! after the opening quote: `"x" + " y"` produced "xy" inside an endpoint
//! while the same code in `start:` produced "x y".

use clean_language_compiler::ast::{Expression, Statement, Value};
use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};

fn string_literals(source: &str) -> Vec<String> {
    let stmt = match CleanParser::parse_single_statement(source) {
        SingleStatementParse::Statement(s) => s,
        _ => panic!("fragment must parse: {source}"),
    };
    let mut out = Vec::new();
    fn walk(e: &Expression, out: &mut Vec<String>) {
        match e {
            Expression::Literal(Value::String(s)) => out.push(s.clone()),
            Expression::Binary(l, _, r) => {
                walk(l, out);
                walk(r, out);
            }
            _ => {}
        }
    }
    if let Statement::VariableDecl {
        initializer: Some(init),
        ..
    } = &stmt
    {
        walk(init, &mut out);
    }
    out
}

#[test]
fn leading_spaces_in_string_literals_are_kept() {
    assert_eq!(
        string_literals("string a = \"x\" + \" y\"\n"),
        vec!["x", " y"]
    );
}

#[test]
fn spaces_only_string_literal_is_kept() {
    assert_eq!(string_literals("string a = \"  \"\n"), vec!["  "]);
}
