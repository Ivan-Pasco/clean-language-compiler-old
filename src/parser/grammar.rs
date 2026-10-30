use super::parser_impl::ErrorRecoveringParser;
use crate::ast::{Expression, Program, Statement};
use crate::error::CompilerError;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "parser/grammar.pest"]
pub struct CleanParser;

/// Outcome of `parse_single_statement`, used by the v3 typed-emission bridge
/// `_emit_stmt_from_source` (typed-emission.md §3.11).
pub enum SingleStatementParse {
    /// Parsed exactly one statement.
    Statement(Statement),
    /// Fragment parsed as a valid expression but not a statement — PLUGIN012.
    ExpressionNotStatement,
    /// Fragment failed to parse. `byte_offset` is the parser's reported offset
    /// relative to the fragment (0 if unknown).
    ParseError { message: String, byte_offset: usize },
}

/// Outcome of `parse_single_expression`, used by the v3 typed-emission bridge
/// `_emit_expr_from_source` (typed-emission.md §3.16, Amendment 6).
pub enum SingleExpressionParse {
    /// Parsed exactly one expression.
    Expression(Expression),
    /// Fragment parses as a statement but not a bare expression — PLUGIN016.
    StatementNotExpression,
    /// Fragment failed to parse as an expression. `byte_offset` is the parser's
    /// reported offset relative to the fragment (0 if unknown).
    ParseError { message: String, byte_offset: usize },
}

impl CleanParser {
    /// Static method for parsing program from source string
    pub fn parse_program(source: &str) -> Result<Program, CompilerError> {
        let mut parser = ErrorRecoveringParser::new(source, "");
        parser.parse_program()
    }

    /// Static method for parsing program with file path for better error reporting
    pub fn parse_program_with_file(
        source: &str,
        file_path: &str,
    ) -> Result<Program, CompilerError> {
        let mut parser = ErrorRecoveringParser::new(source, file_path);
        parser.parse_program()
    }

    /// Parse a Clean-source fragment as a single statement (or a small sequence
    /// which is flattened into a synthetic block). Used by the v3 typed-emission
    /// bridge `_emit_stmt_from_source` (typed-emission.md §3.11).
    ///
    /// Strategy: wrap the fragment inside a synthetic `start:` block with the
    /// required indentation, then parse the wrapper as a full program and
    /// extract the resulting statements. This reuses the entire parser without
    /// introducing a new grammar entry point.
    ///
    /// Returns:
    /// - `Statement(s)` on success — for multi-statement fragments, wrapped as
    ///   a block via `Statement::If { cond=true, then_branch=stmts }` matching
    ///   the block-representation convention used by `_stmt_block` in
    ///   `typed_emission::bridges`.
    /// - `ExpressionNotStatement` if the fragment parses as an expression only
    ///   (e.g. `1 + 1`) — the bridge maps this to PLUGIN012.
    /// - `ParseError` with the fragment-relative byte offset for translation.
    pub fn parse_single_statement(source: &str) -> SingleStatementParse {
        let trimmed = source.trim_end_matches(&[' ', '\t', '\n', '\r'][..]);
        if trimmed.trim_start().is_empty() {
            return SingleStatementParse::ParseError {
                message: "empty source fragment".to_string(),
                byte_offset: 0,
            };
        }

        // Indent every non-empty line by 4 spaces so it lives under `start:`.
        let mut indented = String::with_capacity(trimmed.len() + 16);
        for (i, line) in trimmed.lines().enumerate() {
            if i > 0 {
                indented.push('\n');
            }
            if line.is_empty() {
                continue;
            }
            indented.push('\t');
            indented.push_str(line);
        }
        let wrapped = format!("start:\n{}\n", indented);
        let mut parser = ErrorRecoveringParser::new(&wrapped, "<plugin-fragment>");
        let program = match parser.parse_program() {
            Ok(p) => p,
            Err(e) => {
                // Before returning ParseError, probe whether the fragment
                // parses as a valid expression — if so, this is PLUGIN012
                // territory (typed-emission.md §3.11), not a syntax error.
                if is_valid_expression(trimmed) {
                    return SingleStatementParse::ExpressionNotStatement;
                }
                let byte_offset = error_byte_offset(&e)
                    .map(|b| statement_fragment_offset(trimmed, b))
                    .unwrap_or(0);
                return SingleStatementParse::ParseError {
                    message: e.to_string(),
                    byte_offset,
                };
            }
        };

        // The parser stores the `start:` block in `program.start_function`.
        // Multiple synthetic containers are checked as a defense against grammar
        // refactors that move the storage location.
        let body: Vec<Statement> = if let Some(f) = &program.start_function {
            f.body.clone()
        } else if let Some(f) = program.functions.iter().find(|f| f.name == "start") {
            f.body.clone()
        } else {
            return SingleStatementParse::ParseError {
                message: "internal: synthetic start block missing after parse".to_string(),
                byte_offset: 0,
            };
        };

        if body.is_empty() {
            // No statement extracted. Could be an expression-only fragment
            // (which the statement grammar accepts as an Expression statement
            // via the `expression` rule — so this path is rare) or an empty
            // input. Try to detect an expression by re-parsing.
            if is_valid_expression(trimmed) {
                return SingleStatementParse::ExpressionNotStatement;
            }
            return SingleStatementParse::ParseError {
                message: "no statement recognized in fragment".to_string(),
                byte_offset: 0,
            };
        }

        // If the fragment produced exactly one Statement::Expression that is
        // NOT a call/assignment-like form, flag it as ExpressionNotStatement
        // per §3.11 PLUGIN012 contract. Calls, method calls, and property
        // accesses used as effectful statements remain valid.
        if body.len() == 1 {
            if let Statement::Expression { expr, .. } = &body[0] {
                if !is_statement_shaped_expression(expr) {
                    return SingleStatementParse::ExpressionNotStatement;
                }
            }
            return SingleStatementParse::Statement(body.into_iter().next().unwrap());
        }

        // Multiple statements — wrap in the same block convention used by
        // `_stmt_block` so downstream bridges (_emit_function body, _stmt_if)
        // can flatten it uniformly.
        use crate::ast::{Expression, Value};
        SingleStatementParse::Statement(Statement::If {
            condition: Expression::Literal(Value::Boolean(true)),
            then_branch: body,
            else_branch: None,
            location: None,
        })
    }

    /// Parse a Clean-source fragment as a single expression. Used by the v3
    /// typed-emission bridge `_emit_expr_from_source` (typed-emission.md §3.16,
    /// Amendment 6). Mirror of `parse_single_statement`.
    ///
    /// Strategy: wrap the fragment as the initializer of a synthetic typed
    /// variable declaration inside a `start:` block, parse the wrapper as a
    /// full program, and extract the RHS expression. Multiple typed prefixes
    /// (`integer`, `number`, `string`, `boolean`) are tried so the fragment's
    /// concrete type does not need to be known ahead of time.
    ///
    /// Returns:
    /// - `Expression(e)` on success.
    /// - `StatementNotExpression` if the fragment does not parse as an
    ///   expression but does parse as a statement (e.g. `return 42`,
    ///   `x = 1`) — the bridge maps this to PLUGIN016.
    /// - `ParseError` with the fragment-relative byte offset otherwise —
    ///   the bridge maps this to PLUGIN015.
    pub fn parse_single_expression(source: &str) -> SingleExpressionParse {
        let ws: &[char] = &[' ', '\t', '\n', '\r'];
        let trimmed = source.trim_matches(ws);
        // Parser offsets are relative to `trimmed`; report them relative to `source`.
        let leading_ws = source.len() - source.trim_start_matches(ws).len();
        if trimmed.is_empty() {
            return SingleExpressionParse::ParseError {
                message: "empty source fragment".to_string(),
                byte_offset: 0,
            };
        }

        // Try wrapping the fragment as the RHS of a typed variable declaration.
        // The concrete type is unknown, so probe each primitive prefix — if any
        // wrapper parses cleanly, the fragment is a valid expression.
        let mut last_err: Option<(String, usize)> = None;
        for prefix in ["integer", "number", "string", "boolean"] {
            // Wrapped shape: `start:\n\t<prefix> __x = <fragment>\n`
            let wrapper_lhs = format!("{} __x = ", prefix);
            let wrapped = format!("start:\n\t{}{}\n", wrapper_lhs, trimmed);
            // Byte offset of the first fragment byte inside `wrapped`:
            //   "start:\n" + "\t" + wrapper_lhs
            let wrapped_prefix_len = "start:\n".len() + 1 + wrapper_lhs.len();

            let mut parser = ErrorRecoveringParser::new(&wrapped, "<plugin-fragment-expr>");
            let program = match parser.parse_program() {
                Ok(p) => p,
                Err(e) => {
                    let byte_offset = error_byte_offset(&e)
                        .and_then(|b| b.checked_sub(wrapped_prefix_len))
                        .map(|b| leading_ws + b.min(trimmed.len()))
                        .unwrap_or(0);
                    last_err = Some((e.to_string(), byte_offset));
                    continue;
                }
            };

            // Extract the initializer expression from the synthetic variable decl.
            let body: &Vec<Statement> = if let Some(f) = &program.start_function {
                &f.body
            } else if let Some(f) = program.functions.iter().find(|f| f.name == "start") {
                &f.body
            } else {
                continue;
            };

            for stmt in body {
                if let Statement::VariableDecl {
                    initializer: Some(expr),
                    ..
                } = stmt
                {
                    return SingleExpressionParse::Expression(expr.clone());
                }
                // Some grammars route the typed initialiser through an Assignment
                // statement instead of VariableDecl; accept that shape too.
                if let Statement::Assignment { value, .. } = stmt {
                    return SingleExpressionParse::Expression(value.clone());
                }
            }
        }

        // Every wrapper failed. Distinguish "not an expression but IS a
        // statement" (PLUGIN016) from "not parseable at all" (PLUGIN015).
        if let SingleStatementParse::Statement(_) = Self::parse_single_statement(trimmed) {
            return SingleExpressionParse::StatementNotExpression;
        }

        let (message, byte_offset) =
            last_err.unwrap_or_else(|| ("source is not a valid Clean expression".to_string(), 0));
        SingleExpressionParse::ParseError {
            message,
            byte_offset,
        }
    }
}

/// Byte offset (into the parsed source) recorded in a parse error's location.
fn error_byte_offset(error: &CompilerError) -> Option<usize> {
    let location = match error {
        CompilerError::Syntax { context }
        | CompilerError::Type { context }
        | CompilerError::Memory { context }
        | CompilerError::Codegen { context }
        | CompilerError::IO { context }
        | CompilerError::Runtime { context }
        | CompilerError::Validation { context }
        | CompilerError::Module { context }
        | CompilerError::Testing { context } => context.location.as_ref(),
        CompilerError::PluginError { location, .. } => location.as_ref(),
        CompilerError::LexError(_) => None,
    };
    location.and_then(|l| l.byte_start)
}

/// Map a byte offset in the `parse_single_statement` wrapper back to the
/// fragment. The wrapper is `start:\n` followed by every fragment line, with a
/// TAB prepended to each non-empty line, so each line before the error shifts
/// the offset by its TAB.
fn statement_fragment_offset(fragment: &str, wrapped_byte: usize) -> usize {
    let mut wrapped_pos = "start:\n".len();
    let mut fragment_pos = 0;
    for line in fragment.split('\n') {
        // The wrapper is built from `str::lines`, which drops a trailing '\r'.
        let content = line.strip_suffix('\r').unwrap_or(line);
        if !content.is_empty() {
            wrapped_pos += 1; // the TAB
        }
        let line_end = wrapped_pos + content.len();
        if wrapped_byte <= line_end {
            return fragment_pos + wrapped_byte.saturating_sub(wrapped_pos);
        }
        wrapped_pos = line_end + 1; // '\n'
        fragment_pos += line.len() + 1;
    }
    fragment.len()
}

/// Returns true iff an expression, when appearing as a bare statement, is
/// meaningful (has side-effects or a resolved-later shape). Numeric/string/
/// arithmetic-only expressions are rejected as PLUGIN012.
fn is_statement_shaped_expression(expr: &crate::ast::Expression) -> bool {
    use crate::ast::Expression as E;
    matches!(
        expr,
        E::Call(_, _)
            | E::MethodCall { .. }
            | E::StaticMethodCall { .. }
            | E::NamespaceCall { .. }
            | E::PropertyAccess { .. }
            | E::PropertyAssignment { .. }
            | E::ListAssignment { .. }
    )
}

/// Attempt to parse the fragment as a Clean expression. Returns true if it
/// parses cleanly as an expression but not as a statement.
fn is_valid_expression(source: &str) -> bool {
    // Probe the expression parser by wrapping the fragment as the initializer
    // of a variable declaration. If that whole program parses, the fragment
    // is a valid expression; otherwise it isn't. We try multiple wrappers to
    // cover integer/number/string/boolean-shaped fragments without inferring
    // a type — the parser accepts whichever matches.
    for prefix in ["integer x", "number x", "string x", "boolean x"] {
        let wrapped = format!("start:\n\t{} = {}\n", prefix, source.trim());
        let mut parser = ErrorRecoveringParser::new(&wrapped, "<plugin-fragment-expr-probe>");
        if parser.parse_program().is_ok() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: fragment parse errors used to always report byte offset 0
    /// because the offset was scraped from the error's display text, which never
    /// contains it. The offset now comes from the error location.
    #[test]
    fn single_statement_parse_error_reports_fragment_offset() {
        let fragment = "integer x = 1\nprint(x\n";
        match CleanParser::parse_single_statement(fragment) {
            SingleStatementParse::ParseError { byte_offset, .. } => {
                // The error is on the second line (`print(x` is unclosed).
                assert!(
                    byte_offset >= "integer x = 1\n".len() && byte_offset <= fragment.len(),
                    "offset {} must point into the second line of the fragment",
                    byte_offset
                );
            }
            _ => panic!("fragment must fail to parse"),
        }
    }

    #[test]
    fn single_expression_parse_error_reports_fragment_offset() {
        let fragment = "  1 + (2 * ";
        match CleanParser::parse_single_expression(fragment) {
            SingleExpressionParse::ParseError { byte_offset, .. } => {
                assert!(
                    byte_offset > "  1 ".len() && byte_offset <= fragment.len(),
                    "offset {} must point at the unfinished group",
                    byte_offset
                );
            }
            _ => panic!("fragment must fail to parse as an expression"),
        }
    }

    #[test]
    fn statement_offsets_skip_the_wrapper_tabs() {
        // wrapped: "start:\n\tab\n\n\tcd\n"
        let fragment = "ab\n\ncd";
        assert_eq!(statement_fragment_offset(fragment, "start:\n\t".len()), 0);
        assert_eq!(statement_fragment_offset(fragment, "start:\n\ta".len()), 1);
        assert_eq!(
            statement_fragment_offset(fragment, "start:\n\tab\n\n\tc".len()),
            "ab\n\nc".len()
        );
    }
}
