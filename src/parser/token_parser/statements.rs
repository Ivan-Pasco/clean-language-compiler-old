//! Statement parsing methods for the token-driven parser.
//!
//! This module handles all statement-level constructs for `TokenParser`:
//! - `parse_statement` — dispatch table for all statement forms
//! - Control flow: if, while, iterate, break, continue, return
//! - Print and error statements
//! - Later / background async statements
//! - `try_parse_on_error_block` — onError: block suffix

use super::TokenParser;
use crate::ast::{AssignmentTarget, Expression, ResetTarget, Statement, Type};
use crate::error::CompilerError;
use crate::lexer::specification_token::TokenKind;

impl TokenParser {
    pub(super) fn parse_statement(&mut self) -> Result<Statement, CompilerError> {
        self.skip_whitespace();

        match self.current_kind() {
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Break => self.parse_break(),
            TokenKind::Continue => self.parse_continue(),
            // `for` is reserved but has no syntax (grammar.ebnf §1.2).
            TokenKind::For => Err(self.unsupported_syntax_error(
                "`for` loops are not Clean Language syntax: use `iterate item in collection`"
                    .to_string(),
                "iterate item in items\n\tprint(item)",
            )),
            TokenKind::Iterate => self.parse_iterate(),
            TokenKind::Later => self.parse_later_assignment(),
            TokenKind::Background => self.parse_background(),
            TokenKind::Print => {
                // Check if this is a print block: print:
                let saved_cursor = self.cursor;
                self.bump(); // consume print
                self.skip_whitespace();
                if self.check(&TokenKind::Colon) {
                    // Restore to the print token and parse as a PrintBlock.
                    self.cursor = saved_cursor;
                    return self.parse_print_block();
                }
                // Not a block — restore and parse as a regular print statement.
                self.cursor = saved_cursor;
                self.parse_print()
            }
            TokenKind::Error => self.parse_error_statement(),
            TokenKind::Require => self.parse_require(),
            TokenKind::Ensure => self.parse_ensure(),
            TokenKind::Spec => self.parse_spec(),
            TokenKind::Intent => self.parse_intent(),
            TokenKind::Constant => self.parse_constant_apply_block(),
            // Allow Test keyword to be used as a class/type name
            TokenKind::Test => {
                let first_name = "Test".to_string();
                let first_location = self.current().location.clone();
                let first_cursor = self.cursor;
                self.bump(); // consume Test token
                self.skip_whitespace();

                // Check if this is a variable declaration (Test varName = ...)
                if let TokenKind::Identifier(_) = self.current_kind() {
                    // Test is a class type here.
                    self.parse_declaration_tail(
                        Type::Object(first_name),
                        "Expected variable name after type",
                    )
                } else {
                    // Not a variable declaration, might be a function call like Test()
                    // Move cursor back to reparse as expression
                    self.cursor = first_cursor;
                    let expr = self.parse_expression()?;
                    Ok(Statement::Expression {
                        expr,
                        location: Some(first_location),
                    })
                }
            }
            TokenKind::Identifier(name) => {
                let first_name = name.clone();
                self.parse_identifier_statement(first_name)
            }
            TokenKind::StringLiteral(_)
            | TokenKind::IntegerLiteral(_)
            | TokenKind::NumberLiteral(_)
            | TokenKind::True
            | TokenKind::False => {
                // Literal expression statements (e.g., for automatic return)
                // Parse as full expression to handle operators: "hello" + name, 3.14 * x, etc.
                let location = self.current().location.clone();
                let expr = self.parse_expression()?;

                // Check for onError: block / onError <statement>
                if let Some((error_block, _error_loc)) = self.try_parse_on_error_handler()? {
                    return Ok(Statement::OnErrorBlock {
                        expression: expr,
                        error_block,
                        location: Some(location),
                    });
                }

                Ok(Statement::Expression {
                    expr,
                    location: Some(location),
                })
            }
            TokenKind::Description => {
                // Parse description statement: description "text"
                let location = self.current().location.clone();
                self.bump(); // consume 'description'
                self.skip_whitespace();

                // Expect a string literal
                if let TokenKind::StringLiteral(text) = self.current_kind() {
                    let description_text = text.clone();
                    self.bump(); // consume string
                    Ok(Statement::Description {
                        text: description_text,
                        location: Some(location),
                    })
                } else {
                    Err(CompilerError::parse_error(
                        "Expected string literal after 'description'".to_string(),
                        Some(self.current().location.clone()),
                        None,
                    ))
                }
            }
            // reset_statement: "reset" , ( "state" | identifier )
            // grammar.ebnf reset_statement production.
            TokenKind::Reset => self.parse_reset_statement(),
            // standalone_on_error: "onError" , ":" , indented_block
            // (grammar.ebnf §4.7) — same AST node the pest grammar produces.
            TokenKind::OnError => {
                let location = self.current().location.clone();
                match self.try_parse_on_error_block()? {
                    Some((body, _)) => Ok(Statement::StandaloneErrorHandler {
                        body,
                        location: Some(location),
                    }),
                    None => Err(CompilerError::parse_error(
                        "Expected 'onError:' block".to_string(),
                        Some(location),
                        None,
                    )),
                }
            }
            // A contextual keyword used as a name (`step = step + 1`,
            // `rules.trim()`): the same statement forms as an identifier.
            // Keywords with a statement of their own are matched above.
            kind if Self::is_contextual_name(kind) => {
                let first_name = self.current().text.clone();
                self.parse_identifier_statement(first_name)
            }
            _ => {
                let token = self.current();
                Err(CompilerError::parse_error(
                    format!("Unsupported statement type: {:?}", token.kind),
                    Some(token.location.clone()),
                    None,
                ))
            }
        }
    }

    /// The rest of a variable declaration after its type: `name [= value]`
    /// (grammar.ebnf `variable_declaration`). The cursor is at the name.
    fn parse_declaration_tail(
        &mut self,
        type_: Type,
        missing_name: &str,
    ) -> Result<Statement, CompilerError> {
        let Some(name) = self.try_extract_var_name() else {
            return Err(CompilerError::parse_error(
                missing_name.to_string(),
                Some(self.current().location.clone()),
                None,
            ));
        };
        let location = self.current().location.clone();
        self.bump(); // consume the variable name
        self.skip_whitespace();
        let initializer = if self.eat(&TokenKind::Assign) {
            self.skip_whitespace();
            Some(self.parse_assigned_value()?)
        } else {
            None
        };
        Ok(Statement::VariableDecl {
            name,
            type_,
            initializer,
            location: Some(location),
        })
    }

    /// A statement starting with a name (an identifier or a contextual
    /// keyword used as one): a variable declaration, an assignment, an
    /// apply block or an expression statement. The cursor is at the name.
    fn parse_identifier_statement(
        &mut self,
        first_name: String,
    ) -> Result<Statement, CompilerError> {
        // Could be:
        // 1. Type name for variable declaration (e.g., "integer x = 42" or "list<integer> nums = [1,2,3]")
        // 2. Variable assignment (e.g., "x = 42")
        // 3. Expression statement (e.g., "someFunction()" or "x.toString()")

        let first_location = self.current().location.clone();
        let first_cursor = self.cursor; // saved before consuming first_name

        // Check if this is a type name (for variable declaration)
        let is_type_keyword = matches!(
            first_name.as_str(),
            "integer"
                | "number"
                | "string"
                | "boolean"
                | "void"
                | "list"
                | "matrix"
                | "pairs"
                | "any"
        );

        if is_type_keyword {
            // IMPORTANT: Check if this is actually a namespace/method call (e.g., list.add())
            // If followed by a dot, it's NOT a type declaration
            // Save current position (we're AT the type identifier)
            let saved_cursor = self.cursor;

            self.bump(); // consume type identifier
            self.skip_whitespace();

            // If followed by a dot, this is a namespace/method call, not a type declaration
            if self.check(&TokenKind::Dot) {
                // Restore cursor and treat as a regular identifier (not a type keyword)
                // This will allow it to be parsed as a method/namespace call below
                self.cursor = saved_cursor;
                // Fall through to the non-type-keyword handling code
            } else {
                if self.check(&TokenKind::Colon) {
                    // Either a TYPE: apply block or a TYPE:bits declaration;
                    // an integer after the colon means a precision modifier.
                    self.bump(); // consume colon
                    self.skip_whitespace();
                    let is_precision_modifier =
                        matches!(self.current_kind(), TokenKind::IntegerLiteral(_));
                    self.cursor = saved_cursor;
                    if !is_precision_modifier {
                        return self.parse_type_apply_block();
                    }
                }
                // A variable declaration: parse the type (with any precision
                // modifier) from the type identifier.
                self.cursor = saved_cursor;
                let type_ = self.parse_type()?;
                self.skip_whitespace();
                return self.parse_declaration_tail(type_, "Expected variable name after type");
            }
        }

        // Not a type keyword - could be:
        // 1. Function/method apply block (e.g., "print:" or "obj.method:")
        // 2. Custom class name for variable declaration (e.g., "Test varName = ...")
        // 3. Variable assignment (e.g., "x = 42")
        // 4. Expression statement (e.g., "someFunction()" or "x.toString()")

        // Peek ahead to see if this is an apply block
        self.bump(); // consume identifier
        self.skip_whitespace();

        // Check for apply block: identifier: or identifier.method:
        if self.check(&TokenKind::Colon) {
            // Check if identifier is a registered plugin block name (e.g., html, component)
            if self.plugin_keywords.contains(&first_name) {
                // This is a plugin framework block inside a function body
                self.cursor = first_cursor;
                return self.parse_framework_block();
            }
            // This is a function apply block: FUNCTION:
            // Move cursor back to re-parse the identifier
            self.cursor = first_cursor;
            return self.parse_function_apply_block();
        } else if self.check(&TokenKind::Dot) {
            // Could be:
            // 1. Method apply block: OBJECT.METHOD:
            // 2. Property assignment: OBJECT.PROPERTY = VALUE
            // 3. Expression statement: OBJECT.METHOD()
            // Only (1) is decided here; (2) and (3) by the `Dot` branch below.
            // Save current position (after the object name)
            let saved_cursor = self.cursor;

            // Walk the property chain to see if it ends with ':'
            let mut has_colon = false;
            while self.eat(&TokenKind::Dot) {
                self.skip_whitespace();
                if self.at_member_name() {
                    self.bump(); // consume property name
                    self.skip_whitespace();
                    if self.check(&TokenKind::Colon) {
                        has_colon = true;
                        break;
                    } else if self.check(&TokenKind::Assign) {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Restore cursor
            self.cursor = saved_cursor;

            if has_colon {
                // This is a method apply block: OBJECT.METHOD:
                // Move cursor back to re-parse from the object name
                self.cursor = first_cursor;
                return self.parse_method_apply_block();
            }
            // Not an apply block: a property assignment or an expression,
            // handled by the `Dot` branch below.
        }

        // Check if next token is an Identifier or keyword that can be used as a variable name
        // (e.g., "Test test" where "test" is a keyword but can be used as a variable name)
        let next_could_be_var_name =
            self.try_extract_var_name().is_some() || self.at_generic_type_declaration();

        if next_could_be_var_name {
            // Next token is an Identifier - this could be a variable declaration
            // with a custom class type: ClassName varName = ...
            // Move cursor back to re-parse as type
            self.cursor = first_cursor;

            // Parse the type
            let type_ = self.parse_type()?;
            self.skip_whitespace();

            self.parse_declaration_tail(type_, "Expected variable name after custom type")
        } else if self.check(&TokenKind::Assign) {
            // This is a simple assignment: VAR = EXPR
            self.bump(); // consume =
            self.skip_whitespace();
            let value = self.parse_assigned_value()?;

            Ok(Statement::Assignment {
                target: AssignmentTarget::Variable(first_name),
                value,
                location: Some(first_location),
            })
        } else if self.check(&TokenKind::Dot) {
            // Could be a property-chain assignment: `obj.prop = val`
            // or `obj.a.b = val`, or just a method-call expression.
            // foundation/spec/grammar.ebnf `assignment_target`:
            //   identifier , "." , identifier , { "." , identifier }
            // Peek ahead: collect dot-separated identifiers, then check for `=`.
            let mut path: Vec<String> = Vec::new();
            let save_cursor = self.cursor;

            // Consume the leading dot and subsequent identifier segments.
            while self.check(&TokenKind::Dot) {
                self.bump(); // consume .
                if self.at_member_name() {
                    // Any keyword is a member name after `.`.
                    path.push(self.bump().text);
                } else {
                    // Not a member name after the dot — not a property assignment.
                    break;
                }
            }

            self.skip_whitespace();
            if !path.is_empty() && self.check(&TokenKind::Assign) {
                // Property-chain assignment.
                self.bump(); // consume =
                self.skip_whitespace();
                let value = self.parse_assigned_value()?;
                Ok(Statement::Assignment {
                    target: AssignmentTarget::Property {
                        object: first_name,
                        path,
                    },
                    value,
                    location: Some(first_location),
                })
            } else if path.len() == 1 && path[0] == "check" {
                // Validate check statement: `schemaName.check expr:\n\tok: ...\n\terror: ...`
                // Rewind to the dot, consume it, then call parse_validate_check_stmt
                // which will consume the "check" identifier itself.
                self.cursor = save_cursor;
                self.bump(); // consume the "."
                self.skip_whitespace();
                self.parse_validate_check_stmt(first_name, first_location)
            } else {
                // Not an assignment — backtrack to first_name and parse the
                // whole thing as an expression (e.g. `nums.add(4)`, `s.replace(a,b)`).
                // Restoring only to save_cursor (the dot) leaves parse_expression()
                // facing a bare `.`, which causes "Unexpected token: Dot".
                self.cursor = first_cursor;
                let expr = self.parse_expression()?;
                self.expression_statement(expr, first_location)
            }
        } else if self.check(&TokenKind::LeftBracket) {
            // This could be an indexed assignment: VAR[index] = value
            // or an expression statement: VAR[index]
            // Parse the full LHS expression first
            self.cursor = first_cursor; // Go back to the identifier

            let lhs_expr = self.parse_expression()?;
            self.skip_whitespace();

            // Check if this is an assignment
            if self.check(&TokenKind::Assign) {
                self.bump(); // consume =
                self.skip_whitespace();
                let value = self.parse_assigned_value()?;

                // grammar.ebnf `assignment_target`: identifier [ index ].
                // Any other indexed receiver (`m[0][1]`, `f()[0]`) is not
                // assignable.
                if let Expression::ListAccess(list, index) = lhs_expr {
                    if let Expression::Variable(collection) = *list {
                        return Ok(Statement::Assignment {
                            target: AssignmentTarget::Index { collection, index },
                            value,
                            location: Some(first_location),
                        });
                    }
                }
                Err(CompilerError::parse_error(
                    "Unsupported assignment target".to_string(),
                    Some(first_location),
                    Some(
                        "Only a variable, a single list index (`xs[i]`) or a property path (`obj.a.b`) can be assigned to"
                            .to_string(),
                    ),
                ))
            } else {
                // Not an assignment, just an expression statement
                self.expression_statement(lhs_expr, first_location)
            }
        } else {
            // This could be an expression statement with operators: x + 1, x * y, etc.
            // Or just a bare identifier
            // Parse as full expression to handle all cases
            self.cursor = first_cursor; // Go back to the identifier

            let expr = self.parse_expression()?;
            self.expression_statement(expr, first_location)
        }
    }

    /// An expression statement, or `expression onError:` + block
    /// (grammar.ebnf `on_error_block`) when the handler starts on the same
    /// line.
    fn expression_statement(
        &mut self,
        expr: Expression,
        location: crate::ast::SourceLocation,
    ) -> Result<Statement, CompilerError> {
        if self.check(&TokenKind::OnError) {
            if let Some((error_block, _)) = self.try_parse_on_error_handler()? {
                return Ok(Statement::OnErrorBlock {
                    expression: expr,
                    error_block,
                    location: Some(location),
                });
            }
        }
        Ok(Statement::Expression {
            expr,
            location: Some(location),
        })
    }

    /// The value of a declaration or assignment: an expression, or
    /// `expression onError:` + block (grammar.ebnf `expression` includes
    /// `on_error_block`). The HIR builder assigns the value inside an error
    /// region whose handler is the block.
    fn parse_assigned_value(&mut self) -> Result<Expression, CompilerError> {
        let location = self.current().location.clone();
        let expression = self.parse_expression()?;
        if !self.check(&TokenKind::OnError) {
            return Ok(expression);
        }
        match self.try_parse_on_error_handler()? {
            Some((error_handler, _)) => Ok(Expression::OnErrorBlock {
                expression: Box::new(expression),
                error_handler,
                location,
            }),
            None => Ok(expression),
        }
    }

    pub(super) fn parse_return(&mut self) -> Result<Statement, CompilerError> {
        let return_token = self.expect(&TokenKind::Return)?;

        // grammar.ebnf `return_statement = "return" , [ expression ]`: the
        // value, if any, is on the same line. Decide before skipping
        // whitespace, which would also skip the line break.
        let value = if self.statement_ends_after(return_token.location.line) {
            None
        } else {
            self.skip_whitespace();
            Some(self.parse_expression()?)
        };

        Ok(Statement::Return {
            value,
            location: Some(return_token.location),
        })
    }

    pub(super) fn parse_break(&mut self) -> Result<Statement, CompilerError> {
        let break_token = self.expect(&TokenKind::Break)?;
        Ok(Statement::Break {
            location: Some(break_token.location),
        })
    }

    pub(super) fn parse_continue(&mut self) -> Result<Statement, CompilerError> {
        let continue_token = self.expect(&TokenKind::Continue)?;
        Ok(Statement::Continue {
            location: Some(continue_token.location),
        })
    }

    /// Parse require statement: require <condition>
    /// Declares a precondition that must be true at runtime
    pub(super) fn parse_require(&mut self) -> Result<Statement, CompilerError> {
        let require_token = self.expect(&TokenKind::Require)?;
        self.skip_whitespace();

        // Parse the condition expression
        let condition = self.parse_expression()?;

        Ok(Statement::Require {
            condition,
            location: Some(require_token.location),
        })
    }

    /// Parse ensure statement: ensure <condition>
    /// Declares a postcondition on the return value.
    /// Within the condition, `result` refers to the function's return value.
    /// Debug builds emit a WASM trap if the condition is false; release builds strip it.
    pub(super) fn parse_ensure(&mut self) -> Result<Statement, CompilerError> {
        let ensure_token = self.expect(&TokenKind::Ensure)?;
        self.skip_whitespace();

        // Parse the condition expression — `result` is a special identifier in this context
        let condition = self.parse_expression()?;

        Ok(Statement::Ensure {
            condition,
            location: Some(ensure_token.location),
        })
    }

    /// Parse reset statement: `reset state` or `reset <identifier>`
    /// grammar.ebnf: reset_statement = "reset" , ( "state" | identifier ) ;
    pub(super) fn parse_reset_statement(&mut self) -> Result<Statement, CompilerError> {
        let reset_token = self.expect(&TokenKind::Reset)?;
        self.skip_whitespace();

        // Check what follows: "state" keyword → AllState, identifier → Named(name)
        let target = if matches!(self.current_kind(), TokenKind::State) {
            self.bump(); // consume "state"
            ResetTarget::AllState
        } else if let Some(name) = self.try_extract_var_name() {
            let name = name.clone();
            self.bump(); // consume identifier
            ResetTarget::Variable(name)
        } else {
            return Err(CompilerError::parse_error(
                "Expected 'state' or an identifier after 'reset'".to_string(),
                Some(self.current().location.clone()),
                None,
            ));
        };

        Ok(Statement::ResetStmt {
            target,
            location: Some(reset_token.location),
        })
    }

    pub(super) fn parse_if(&mut self) -> Result<Statement, CompilerError> {
        // Capture the if statement's indentation level BEFORE consuming the if token
        let if_indent_level = self.get_current_indent_level();

        let if_token = self.expect(&TokenKind::If)?;
        self.skip_whitespace();

        let condition = self.parse_expression()?;
        self.skip_whitespace();

        let then_branch = self.parse_block(if_indent_level)?;

        self.skip_whitespace();

        self.consume_dedents_to(if_indent_level);

        // Check for else or else if at the same level as the if statement
        let else_branch = if self.eat(&TokenKind::Else) {
            self.skip_whitespace();
            // Check for "else if" pattern - recursively parse as nested if
            if self.check(&TokenKind::If) {
                // Parse the nested if as a single-statement block
                let nested_if = self.parse_if()?;
                Some(vec![nested_if])
            } else {
                // Regular else block
                Some(self.parse_block(if_indent_level)?)
            }
        } else {
            None
        };

        Ok(Statement::If {
            condition,
            then_branch,
            else_branch,
            location: Some(if_token.location),
        })
    }

    /// Parse while statement: while condition
    ///     body
    pub(super) fn parse_while(&mut self) -> Result<Statement, CompilerError> {
        // Capture the while statement's indentation level BEFORE consuming the while token
        let while_indent_level = self.get_current_indent_level();

        let while_token = self.expect(&TokenKind::While)?;
        self.skip_whitespace();

        let condition = self.parse_expression()?;
        self.skip_whitespace();

        let body = self.parse_block(while_indent_level)?;

        self.skip_whitespace();

        self.consume_dedents_to(while_indent_level);

        Ok(Statement::While {
            condition,
            body,
            location: Some(while_token.location),
        })
    }

    /// Parse iterate statement: iterate item in collection or iterate i in start..end
    pub(super) fn parse_iterate(&mut self) -> Result<Statement, CompilerError> {
        let header_level = self.get_current_indent_level();
        let iterate_token = self.expect(&TokenKind::Iterate)?;
        self.skip_whitespace();

        let variable_token = self.expect_identifier()?;
        let iterator = variable_token.text.clone();

        self.skip_whitespace();
        self.expect(&TokenKind::In)?;
        self.skip_whitespace();

        // Parse the start expression
        // This could be a range start (for "iterate i in 0 to 10")
        // or a collection (for "iterate item in myList")
        let start_or_collection = self.parse_expression()?;

        // Check if this is a range iteration (has "to" keyword)
        self.skip_whitespace();
        let is_range = self.check(&TokenKind::To);

        if is_range {
            // Range iteration: iterate i in start to end [inclusive] [step stepValue]
            self.bump(); // consume "to"
            self.skip_whitespace();

            let end = self.parse_expression()?;
            self.skip_whitespace();

            // Optional "inclusive" contextual keyword (grammar.ebnf: not
            // reserved, lexed as an identifier). Ranges always include the
            // end; the keyword only exists for readability.
            if matches!(self.current_kind(), TokenKind::Identifier(s) if s == "inclusive") {
                self.bump();
                self.skip_whitespace();
            }

            // Check for optional "step" clause
            let step = if self.check(&TokenKind::Step) {
                self.bump(); // consume "step"
                self.skip_whitespace();
                Some(self.parse_expression()?)
            } else {
                None
            };

            self.skip_whitespace();
            let body = self.parse_block(header_level)?;

            Ok(Statement::RangeIterate {
                iterator,
                start: start_or_collection,
                end,
                step,
                body,
                location: Some(iterate_token.location),
            })
        } else {
            // Regular collection iteration: iterate item in collection.
            // grammar.ebnf: only `range_iterate_statement` has a `step` clause.
            if self.check(&TokenKind::Step) {
                return Err(CompilerError::parse_error(
                    "'step' is only allowed when iterating over a range".to_string(),
                    Some(self.current().location.clone()),
                    Some("Use `iterate i in start to end step n`, or drop the step.".to_string()),
                ));
            }

            self.skip_whitespace();
            let body = self.parse_block(header_level)?;

            Ok(Statement::Iterate {
                iterator,
                collection: start_or_collection,
                body,
                location: Some(iterate_token.location),
            })
        }
    }

    pub(super) fn parse_print(&mut self) -> Result<Statement, CompilerError> {
        let print_token = self.expect(&TokenKind::Print)?;
        self.skip_whitespace();

        // Check if we have parentheses (function call style) with multiple arguments
        let expression = if self.check(&TokenKind::LeftParen) {
            self.bump(); // consume (
            self.skip_whitespace();

            // Parse comma-separated arguments
            let mut arguments = Vec::new();
            if !self.check(&TokenKind::RightParen) {
                loop {
                    arguments.push(self.parse_expression()?);
                    self.skip_whitespace();

                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                    self.skip_whitespace();
                }
            }

            self.expect(&TokenKind::RightParen)?;

            // If multiple arguments, create a function call expression
            // If single argument, use it directly
            if arguments.len() == 1 {
                // Safe: length checked to be exactly 1 on the line above
                arguments
                    .into_iter()
                    .next()
                    .expect("single argument confirmed by len check")
            } else {
                // Create a function call to represent multi-arg print
                Expression::Call("print".to_string(), arguments)
            }
        } else {
            // No parentheses - parse single expression
            self.parse_expression()?
        };

        // print("Hello")   → newline=true  (default: adds newline)
        // print("Hello") + → newline=false (continuation: stays on same line)
        self.skip_whitespace();
        let newline = if self.check(&TokenKind::Plus) {
            let saved = self.cursor;
            self.bump();
            // Don't call skip_whitespace() here - it eats Newline tokens,
            // hiding the line boundary from the match check below.
            if matches!(
                self.current_kind(),
                TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent(_) | TokenKind::Indent(_)
            ) {
                false // print(...) + stays on same line — no newline
            } else {
                self.cursor = saved;
                true // not a trailing +, treat as default (newline)
            }
        } else {
            true // no + suffix → default form, adds newline
        };

        Ok(Statement::Print {
            expression,
            newline,
            location: Some(print_token.location),
        })
    }

    pub(super) fn parse_error_statement(&mut self) -> Result<Statement, CompilerError> {
        let error_token = self.expect(&TokenKind::Error)?;
        self.skip_whitespace();

        // Expect parentheses with message expression
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();

        // Parse message expression (typically a string literal)
        let message = self.parse_expression()?;
        self.skip_whitespace();

        self.expect(&TokenKind::RightParen)?;

        Ok(Statement::Error {
            message,
            location: Some(error_token.location),
        })
    }

    /// Parse later assignment: later var = start expr
    pub(super) fn parse_later_assignment(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();
        self.expect(&TokenKind::Later)?;
        self.skip_whitespace();

        // Get variable name
        let var_name = self.expect_identifier()?.text.clone();
        self.skip_whitespace();

        // Expect =
        self.expect(&TokenKind::Assign)?;
        self.skip_whitespace();

        // Parse the expression (usually 'start someFunc()')
        let expression = self.parse_expression()?;

        // Emit LaterAssignment so the HIR builder can apply the CPS transform
        // (split the function at this point; remaining statements become a continuation).
        Ok(Statement::LaterAssignment {
            variable: var_name,
            expression,
            location: Some(location),
        })
    }

    /// Parse background statement: background expr
    pub(super) fn parse_background(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();
        self.expect(&TokenKind::Background)?;
        self.skip_whitespace();

        // Parse the expression
        let expression = self.parse_expression()?;

        Ok(Statement::Background {
            expression,
            location: Some(location),
        })
    }

    /// At `<` right after a class name: true when the tokens read
    /// `<TypeArgs> name =` — a variable declaration with a generic class type
    /// such as `PagedResult<Item> page = ...` (grammar.ebnf `type`).
    pub(super) fn at_generic_type_declaration(&self) -> bool {
        if !matches!(self.current_kind(), TokenKind::Less) {
            return false;
        }
        let mut idx = self.cursor;
        let mut depth = 0usize;
        while idx < self.tokens.len() {
            match &self.tokens[idx].kind {
                TokenKind::Less => depth += 1,
                TokenKind::Greater => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Newline | TokenKind::Assign => return false,
                _ => {}
            }
            idx += 1;
        }
        if depth != 0 {
            return false;
        }
        matches!(
            self.tokens.get(idx + 1).map(|t| &t.kind),
            Some(TokenKind::Identifier(_))
        ) && matches!(
            self.tokens.get(idx + 2).map(|t| &t.kind),
            Some(TokenKind::Assign)
        )
    }

    /// Check if the current position has `onError:` and parse the error handling block
    /// Returns Some((error_block, location)) if onError block is present, None otherwise
    pub(super) fn try_parse_on_error_block(
        &mut self,
    ) -> Result<Option<(Vec<Statement>, crate::ast::SourceLocation)>, CompilerError> {
        self.parse_on_error_handler(false)
    }

    /// `onError:` + block, or — after an expression (`allow_statement`) —
    /// `onError <statement>` (grammar.ebnf `on_error_statement`): a print,
    /// return, break, continue or error(...) on the same line, which becomes
    /// a one-statement handler.
    fn try_parse_on_error_handler(
        &mut self,
    ) -> Result<Option<(Vec<Statement>, crate::ast::SourceLocation)>, CompilerError> {
        self.parse_on_error_handler(true)
    }

    fn parse_on_error_handler(
        &mut self,
        allow_statement: bool,
    ) -> Result<Option<(Vec<Statement>, crate::ast::SourceLocation)>, CompilerError> {
        let header_level = self.get_current_indent_level();
        self.skip_whitespace();

        // Check for onError keyword
        if !self.check(&TokenKind::OnError) {
            return Ok(None);
        }

        let error_location = self.current().location.clone();
        self.bump(); // consume onError
        self.skip_whitespace();

        if allow_statement && self.at_on_error_handler_statement() {
            let handler = self.parse_statement()?;
            return Ok(Some((vec![handler], error_location)));
        }

        // Expect colon for block syntax
        if !self.check(&TokenKind::Colon) {
            return Err(CompilerError::parse_error(
                "Expected ':' after 'onError' for error handling block".to_string(),
                Some(self.current().location.clone()),
                Some(
                    "Use 'onError:' for blocks or 'onError <fallback>' for expressions".to_string(),
                ),
            ));
        }

        self.bump(); // consume :
        self.skip_whitespace();

        let error_block = self.parse_block(header_level)?;

        Ok(Some((error_block, error_location)))
    }
}
