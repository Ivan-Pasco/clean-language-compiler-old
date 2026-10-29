//! Plugin fragments go through the pest parser (`parse_single_statement`).
//! grammar.ebnf `list_element` allows a parenthesized expression; the pest
//! rule matches it as `single_line_expression`, which the list parser used to
//! reject with "Unexpected list element" — so `[1, (2 + 3)]` failed inside an
//! endpoint handler while compiling fine at top level (#22 follow-up).

use clean_language_compiler::ast::{Expression, Statement};
use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};

#[test]
fn parenthesized_list_element_parses_in_a_fragment() {
    let source = "list<integer> n = [1, (2 + 3), x]\n";
    match CleanParser::parse_single_statement(source) {
        SingleStatementParse::Statement(Statement::VariableDecl { initializer: Some(init), .. }) => {
            match init {
                Expression::ListLiteral(items) => assert_eq!(items.len(), 3),
                other => panic!("expected a list literal, got {other:?}"),
            }
        }
        SingleStatementParse::Statement(other) => panic!("expected a variable declaration, got {other:?}"),
        _ => panic!("fragment with a parenthesized list element must parse"),
    }
}
