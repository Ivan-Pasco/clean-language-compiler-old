//! Regression tests for dashboard fingerprints ff689b2222b3 and 0f628d47cad7.
//!
//! Both fingerprints trace to divergences between the token-driven
//! `SpecificationParser` (which handles `.cln` files) and the Pest-driven
//! `ErrorRecoveringParser` (which handles single-statement fragments passed
//! through the v3 typed-emission bridge `_emit_stmt_from_source`). The
//! divergences broke plugin-emitted code that was otherwise spec-valid Clean:
//!
//! - ff689b2222b3: `_html_escape(number)` inside a plugin fragment failed at
//!   the `argument_item` rule because the Pest `keyword` alternation matched
//!   `number`, blocking `identifier` from matching the same bytes. Fix:
//!   introduced `soft_keyword_identifier` as a fallback rule in `primary`
//!   that accepts the type-name keywords (`number`, `string`, `boolean`,
//!   `list`, `matrix`, `pairs`, `void`, `any`) as bare variable references.
//!   `integer` was already accepted because `"in"` earlier in the keyword
//!   alternation prevents Pest from committing to `"integer"` — leaving the
//!   `keyword` rule unable to match `integer` at all.
//!
//! - 0f628d47cad7: `return Foo().bar()` failed because `primary` tried
//!   `constructor_call` before `chained_method_call` in its alternation and
//!   Pest committed to `constructor_call` after matching `Foo()`, leaving
//!   the trailing `.bar()` as an unexpected `program_item`. Fix: reorder
//!   `primary` so `chained_method_call` precedes `constructor_call`, and
//!   add `constructor_call` to the leading choices inside
//!   `chained_method_call` so uppercase-starting names route through it.
//!
//! Both fixes affect only the Pest grammar path used by
//! `_emit_stmt_from_source`. The token parser used for `.cln` files was
//! already correct — that is why these bugs surfaced only in framework
//! plugins (frame.ui, frame.server) that build Clean source fragments
//! for the compiler to parse.

use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};

fn assert_parses(src: &str) {
    match CleanParser::parse_single_statement(src) {
        SingleStatementParse::Statement(_) => {}
        SingleStatementParse::ExpressionNotStatement => {
            panic!(
                "expected `{}` to parse as a Statement; got ExpressionNotStatement",
                src
            );
        }
        SingleStatementParse::ParseError {
            message,
            byte_offset,
        } => {
            panic!(
                "expected `{}` to parse as a Statement; got ParseError @ byte {}: {}",
                src, byte_offset, message
            );
        }
    }
}

// ─── ff689b2222b3 — soft-keyword identifiers in argument positions ────────

#[test]
fn softkw_number_as_call_argument() {
    // Regression: `_html_escape(number)` failed with
    // "Expected argument_item" because the Pest `keyword` rule matched
    // `number`, blocking `identifier`. Fixed via `soft_keyword_identifier`
    // fallback in `primary`.
    assert_parses("return _html_escape(number)");
}

#[test]
fn softkw_string_as_call_argument() {
    assert_parses("return _html_escape(string)");
}

#[test]
fn softkw_boolean_as_call_argument() {
    assert_parses("return _html_escape(boolean)");
}

#[test]
fn softkw_list_as_call_argument() {
    // `list` gets a slightly different error path ("Expected method_call_segment")
    // because `list` is also a `builtin_class_name`. The fallback resolves it
    // to a bare variable reference in argument position.
    assert_parses("return _html_escape(list)");
}

#[test]
fn softkw_matrix_pairs_void_any_as_call_arguments() {
    for kw in ["matrix", "pairs", "void", "any"] {
        assert_parses(&format!("return _html_escape({})", kw));
    }
}

#[test]
fn softkw_as_bare_variable_reference() {
    // Also covers usage outside a call: `string x = number`.
    assert_parses("string x = number");
    assert_parses("string x = string");
    assert_parses("string x = boolean");
}

#[test]
fn hard_keywords_still_rejected_as_arguments() {
    // Only TYPE keywords are relaxed; control-flow and literal keywords
    // remain reserved.
    for kw in ["return", "if", "else", "not"] {
        let src = format!("return _html_escape({})", kw);
        match CleanParser::parse_single_statement(&src) {
            SingleStatementParse::ParseError { .. } => {}
            other => panic!(
                "hard keyword `{}` must still be rejected as identifier; got: {:?}",
                kw,
                match other {
                    SingleStatementParse::Statement(_) => "Statement",
                    SingleStatementParse::ExpressionNotStatement => "ExpressionNotStatement",
                    _ => unreachable!(),
                }
            ),
        }
    }
}

// ─── 0f628d47cad7 — method-call chain after constructor ────────────────────

#[test]
fn chain_after_constructor_call_in_return() {
    // Regression: `return Foo().bar()` failed because `constructor_call`
    // preceded `chained_method_call` in `primary`.
    assert_parses("return Foo().bar()");
}

#[test]
fn chain_after_constructor_call_in_assignment() {
    assert_parses("string x = Foo().bar()");
}

#[test]
fn chain_after_function_call_still_works() {
    // Baseline: lowercase-starting names were already parsed correctly.
    // Guard against a regression introduced by the reordering.
    assert_parses("return foo().bar()");
    assert_parses("string x = foo().bar()");
}

#[test]
fn plain_constructor_call_still_works() {
    // Guard: unchained `Foo()` must still parse via the plain
    // `constructor_call` alternative.
    assert_parses("return Foo()");
    assert_parses("Foo x = Foo()");
}

#[test]
fn deeper_chain_after_constructor_call() {
    // Two-segment chain: constructor + two method calls.
    assert_parses("return Foo().bar().baz()");
}
