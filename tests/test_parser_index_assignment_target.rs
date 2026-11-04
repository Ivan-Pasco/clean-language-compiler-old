//! Index assignment targets (grammar.ebnf `assignment_target`):
//!
//! ```ebnf
//! assignment_target = identifier
//!                   , [ ( "[" , additive_expression , "]" )
//!                     | ( "." , identifier , { "." , identifier } ) ] ;
//! ```
//!
//! 1. `m[0][1] = 9` is not an assignment target. The token parser used to
//!    accept it as an `Expression::ListAssignment`, a node with no lowering,
//!    so the write was silently dropped and the program printed the old value.
//!    It must be a syntax error.
//! 2. The pest parser (plugin fragments fall back to it) built the same
//!    unlowered node for `xs[0] = 5`; it must build the
//!    `AssignmentTarget::Index` assignment the token parser builds.

use clean_language_compiler::ast::{AssignmentTarget, Statement};
use clean_language_compiler::parser::ErrorRecoveringParser;

#[test]
fn nested_index_assignment_is_a_syntax_error() {
    let src = "start:\n\tlist<list<integer>> m = [[1, 2], [3]]\n\tm[0][1] = 9\n\tprint(m[0][1])\n";
    let err = clean_language_compiler::parse_to_ast(src, "nested_index.cln")
        .expect_err("`m[0][1] = 9` is not a grammar.ebnf assignment_target");
    assert!(
        err.iter()
            .any(|e| e.to_string().contains("Unsupported assignment target")),
        "unexpected errors: {err:?}"
    );
}

#[test]
fn single_index_assignment_still_parses() {
    let src = "start:\n\tlist<integer> xs = [1, 2]\n\txs[0] = 9\n";
    let program = clean_language_compiler::parse_to_ast(src, "index.cln").expect("valid source");
    let body = &program.start_function.expect("start").body;
    assert!(
        body.iter().any(|s| matches!(
            s,
            Statement::Assignment {
                target: AssignmentTarget::Index { collection, .. },
                ..
            } if collection == "xs"
        )),
        "{body:?}"
    );
}

#[test]
fn pest_parser_builds_index_assignment() {
    let src = "start:\n\tlist<integer> xs = [1, 2]\n\txs[0] = 9\n";
    let mut parser = ErrorRecoveringParser::new(src, "<pest-index-assignment>");
    let program = parser.parse_program().expect("valid source");
    let body = program
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
        .body;
    assert!(
        body.iter().any(|s| matches!(
            s,
            Statement::Assignment {
                target: AssignmentTarget::Index { collection, .. },
                ..
            } if collection == "xs"
        )),
        "{body:?}"
    );
}
