//! Regression test for SYN001-LIST-GENERIC-REGRESSION (fingerprint 3d5a3082).
//!
//! `CleanParser::parse_single_statement("list<integer> xs = []\n")` fails
//! with "SYN001: Expected one of: type annotation, method_call_segment,
//! logical_op, comparison_op_is, additive_op, multiplicative_op, power_op
//! — fragment begins with `list<T> IDENT = []`".
//!
//! End-to-end shape: `cln compile` succeeds because the CLI routes through
//! the `SpecificationParser`. Plugin bridges (`_emit_stmt_from_source`),
//! LSP snippet checks, and `cln parse` all route through the pest
//! CleanParser and fail.
//!
//! The fix must land in the pest grammar OR its `parse_single_statement`
//! wrapper so `list<T>` is recognised as a variable declaration in that
//! entry point too.

use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};

#[test]
fn list_generic_var_decl_parses_via_parse_single_statement() {
    let src = "list<integer> xs = []\n";
    let result = CleanParser::parse_single_statement(src);
    match result {
        SingleStatementParse::Statement(_) => {}
        SingleStatementParse::ExpressionNotStatement => panic!(
            "expected Statement (list<T> var decl), got ExpressionNotStatement"
        ),
        SingleStatementParse::ParseError { message, byte_offset } => panic!(
            "regression: list<integer> var decl fails to parse via CleanParser.\n\
             byte_offset={}\nmessage={}",
            byte_offset, message
        ),
    }
}

#[test]
fn list_generic_var_decl_via_full_block_still_parses() {
    // Sanity: the full `start:` block including the same statement should
    // work — this is what `cln compile` sees. If this ALSO fails, the bug
    // is upstream of parse_single_statement (in the grammar itself) and
    // the fix has to reach the CLI path too.
    let src = "start:\n\tlist<integer> xs = []\n\txs.add(1)\n";
    let result = CleanParser::parse_program(src);
    assert!(
        result.is_ok(),
        "list<T> var decl in a full start: block should parse — cln compile works, \
         this parser entry should too. err: {:?}",
        result.err()
    );
}
