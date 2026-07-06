//! Regression test for dashboard fingerprint 7793fbeec120.
//!
//! Error code: FRAME-UI-HTML-STATIC-SEGMENTS-DROPPED (reclassified compiler bug).
//!
//! Symptom: frame.ui's `html:` blocks silently corrupted runtime output on
//! cln 0.33.3+. Everything after the first attribute interpolation was
//! dropped — closing tags, subsequent attributes, sibling elements — and
//! never reached the emitted WASM.
//!
//! Root cause: [ErrorRecoveringParser] (used by `_emit_stmt_from_source`,
//! the v3 typed-emission bridge that parses plugin-generated Clean source
//! fragments) built binary expression trees with a while-loop guarded by
//!
//!   while i < op_stack.len() && i < expr_stack.len()
//!
//! The loop calls `expr_stack.remove(0)` on each iteration, so
//! `expr_stack.len()` shrinks. After the initial `remove(0)` of the first
//! operand, the invariant is `expr_stack.len() = op_stack.len() - i`, so
//! the `i < expr_stack.len()` guard failed one iteration early — dropping
//! the final operand of any chain with 4+ operands (`a + b + c + d`).
//! For a 3-operand chain the loop still saw two live entries and
//! completed correctly, which is why simpler `html:` blocks masked the
//! bug and only 4-operand-plus chains surfaced it.
//!
//! frame.ui's `expand_html_block` generates chains like
//!   __html + "<div>" + inner + "</div>"    (4 operands, 3 `+` ops)
//! Every non-void wrapping element produces 4+ operands, so every non-void
//! element lost its closing tag.
//!
//! Fix: iterate against a stable `op_count` captured before the loop and
//! guard on `!expr_stack.is_empty()` instead of `i < expr_stack.len()`.
//! Applied to all 8 sites in expression_parser.rs (logical, comparison,
//! additive, multiplicative — each in two variants).
//!
//! The `SpecificationParser` path (which handles user `.cln` files) is
//! unaffected because it uses the TokenParser, not the pest-based
//! expression_parser. That's why `cln compile` of hand-written files
//! never surfaced the bug — only plugin-generated source fragments routed
//! through `_emit_stmt_from_source` hit the buggy code path.

use clean_language_compiler::ast::{BinaryOperator, Expression, Statement, Value};
use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};

/// Depth of `Binary(Add, ...)` chain reachable from an expression's outermost
/// node. For `a + b + c + d` (left-associative) this is 3.
fn count_add_chain_depth(expr: &Expression) -> usize {
    let mut current = expr;
    let mut depth = 0;
    while let Expression::Binary(left, BinaryOperator::Add, _right) = current {
        depth += 1;
        current = left;
    }
    depth
}

fn leftmost_variable_name(expr: &Expression) -> Option<&str> {
    let mut current = expr;
    while let Expression::Binary(left, _, _) = current {
        current = left;
    }
    match current {
        Expression::Variable(n) => Some(n),
        _ => None,
    }
}

fn rightmost_string_literal(expr: &Expression) -> Option<&str> {
    match expr {
        Expression::Binary(_, _, right) => rightmost_string_literal(right),
        Expression::Literal(Value::String(s)) => Some(s),
        _ => None,
    }
}

#[test]
fn error_recovering_parser_preserves_4_operand_add_chain() {
    // Mirrors what frame.ui's expand_html_block generates for
    //   html:
    //     <div>hello world</div>
    // via _emit_stmt_from_source. The trailing `+ "</div>"` was previously
    // dropped from the parsed AST.
    let source =
        "string __html = \"\"\n__html = __html + \"<div>\" + \"hello world\" + \"</div>\"\nreturn __html\n";

    let result = CleanParser::parse_single_statement(source);
    let Statement::If { then_branch, .. } = (match result {
        SingleStatementParse::Statement(s) => s,
        SingleStatementParse::ExpressionNotStatement => {
            panic!("expected Statement, got ExpressionNotStatement")
        }
        SingleStatementParse::ParseError {
            message,
            byte_offset,
        } => {
            panic!("expected Statement, got ParseError at {byte_offset}: {message}")
        }
    }) else {
        panic!("expected block-wrapped statements");
    };

    let assignment = then_branch
        .iter()
        .find_map(|s| match s {
            Statement::Assignment { value, .. } => Some(value),
            _ => None,
        })
        .expect("assignment statement must be present");

    // Chain must have depth 3 to represent __html + "<div>" + "hello world" + "</div>".
    // Before the fix: depth was 2 (trailing operand dropped).
    assert_eq!(
        count_add_chain_depth(assignment),
        3,
        "regression: parser dropped a trailing operand from a 4-operand + chain — \
         see dashboard fingerprint 7793fbeec120. Full AST: {:#?}",
        assignment
    );

    assert_eq!(leftmost_variable_name(assignment), Some("__html"));
    assert_eq!(
        rightmost_string_literal(assignment),
        Some("</div>"),
        "regression: rightmost operand must be the closing tag literal"
    );
}

#[test]
fn error_recovering_parser_preserves_5_operand_add_chain() {
    // A 5-operand chain models attribute-interpolation output like
    //   value='" + "" + _html_escape(val) + "" + "'
    // where the middle `""` operands are structural artifacts of the plugin's
    // string-builder. Before the fix this chain also lost operands, causing
    // the FRAME-UI-HTML-ATTR-INTERP-CORRUPTION sibling report (f80a6277160b).
    let source = "string s = \"a\" + \"b\" + \"c\" + \"d\" + \"e\"\n";
    let result = CleanParser::parse_single_statement(source);
    let stmt = match result {
        SingleStatementParse::Statement(s) => s,
        SingleStatementParse::ExpressionNotStatement => {
            panic!("expected Statement, got ExpressionNotStatement")
        }
        SingleStatementParse::ParseError {
            message,
            byte_offset,
        } => {
            panic!("expected Statement, got ParseError at {byte_offset}: {message}")
        }
    };
    let value = match &stmt {
        Statement::VariableDecl { initializer, .. } => {
            initializer.as_ref().expect("initializer must be present")
        }
        _ => panic!("expected VariableDecl, got {:?}", stmt),
    };
    assert_eq!(
        count_add_chain_depth(value),
        4,
        "regression: 5-operand + chain must produce 4 Add binaries. AST: {:#?}",
        value
    );
    assert_eq!(rightmost_string_literal(value), Some("e"));
}
