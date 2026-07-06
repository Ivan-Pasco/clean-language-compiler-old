/// Regression tests for bug #6bdb713e82fc
///
/// ErrorRecoveringParser (used by `_emit_stmt_from_source` for plugin-emitted
/// statement fragments) previously treated block-header keywords like `build`,
/// `state`, `screen`, `source`, `spec`, `intent`, `rules`, `computed`, `watch`,
/// `guard`, `reset`, `require` as HARD reserved words, while SpecificationParser
/// (used by `cln check` for whole-file parsing) treated them as SOFT keywords —
/// legal as identifiers outside block-header positions.
///
/// The fix: removed those soft keywords from the `keyword` rule in grammar.pest
/// so that `identifier = @{ !keyword ~ ... }` now accepts them as valid identifiers.
use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};

// ─── minimal repro from the bug report ───────────────────────────────────────

/// Bug #6bdb713e82fc: `build` could not be used as a variable name.
#[test]
fn error_recovering_parser_accepts_build_as_identifier() {
    let src = "string build = \"b\"\nstring x = build + \"end\"";
    match CleanParser::parse_single_statement(src) {
        SingleStatementParse::Statement(_) => {}
        SingleStatementParse::ExpressionNotStatement => {
            panic!("expected Statement, got ExpressionNotStatement")
        }
        SingleStatementParse::ParseError { message, .. } => {
            panic!("expected Statement, got ParseError: {}", message)
        }
    }
}

// ─── exhaustive soft-keyword coverage ────────────────────────────────────────

/// All block-header soft keywords must be accepted as variable identifiers.
#[test]
fn error_recovering_parser_accepts_all_soft_keywords_as_identifiers() {
    let soft_keywords = [
        "build", "state", "screen", "source", "spec", "intent", "rules", "computed", "watch",
        "guard", "reset", "require",
    ];

    for kw in &soft_keywords {
        // Declare a variable with the soft keyword as its name, then reference it.
        let src = format!("string {} = \"v\"\nstring check_{} = {}", kw, kw, kw);
        match CleanParser::parse_single_statement(&src) {
            SingleStatementParse::Statement(_) => {}
            SingleStatementParse::ExpressionNotStatement => {
                panic!(
                    "soft keyword '{}' produced ExpressionNotStatement — should be Statement",
                    kw
                )
            }
            SingleStatementParse::ParseError { message, .. } => {
                panic!(
                    "soft keyword '{}' produced ParseError: {} — it must be a legal identifier",
                    kw, message
                )
            }
        }
    }
}

// ─── hard reserved words must still be rejected ──────────────────────────────

/// Hard reserved words must still fail to parse as variable names.
#[test]
fn error_recovering_parser_still_rejects_hard_reserved_words() {
    let hard_keywords = ["if", "return", "while"];

    for kw in &hard_keywords {
        let src = format!("string {} = \"x\"", kw);
        match CleanParser::parse_single_statement(&src) {
            // A ParseError is the expected result — these are genuinely reserved.
            SingleStatementParse::ParseError { .. } => {}
            // ExpressionNotStatement is also acceptable: the fragment may be
            // tokenised as an expression by a fallback path.
            SingleStatementParse::ExpressionNotStatement => {}
            SingleStatementParse::Statement(_) => {
                panic!(
                    "hard reserved word '{}' was accepted as an identifier — it must be rejected",
                    kw
                )
            }
        }
    }
}

// ─── whole-program: soft keywords coexist with block headers ─────────────────

/// A whole program that uses `state` and `build` as ordinary variable names
/// AND contains a `state:` block must parse without errors.
///
/// Uses tab indentation throughout (required by the Pest grammar — INDENT = "\t").
#[test]
fn whole_program_soft_keywords_coexist_with_block_headers() {
    // `state` is used as a local variable inside `start:`, while the program
    // also has a top-level `state:` block. Both must be accepted.
    // Indentation is explicit \t per grammar.pest INDENT rule.
    let src = "state:\n\tinteger count = 0\n\nstart:\n\tstring state = \"idle\"\n\tstring build = \"release\"\n\tprint(state)\n";
    match CleanParser::parse_program(src) {
        Ok(_) => {}
        Err(e) => panic!(
            "whole-program parse failed with soft keywords as variables: {}",
            e
        ),
    }
}
