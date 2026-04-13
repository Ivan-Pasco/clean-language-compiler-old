//! Statement parsing methods for the token-driven parser.
//!
//! This module handles all statement-level constructs for `TokenParser`:
//! - `parse_statement` — dispatch table for all statement forms
//! - Control flow: if, while, for, iterate, break, continue, return
//! - Print and error statements
//! - Later / background async statements
//! - `try_parse_on_error_block` — onError: block suffix

use super::TokenParser;
use crate::ast::{Expression, Statement, Type};
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
            TokenKind::For => self.parse_for(),
            TokenKind::Iterate => self.parse_iterate(),
            TokenKind::Later => self.parse_later_assignment(),
            TokenKind::Background => self.parse_background(),
            TokenKind::Print => {
                // Check if this is a print apply block: print:
                let saved_cursor = self.cursor;
                self.bump(); // consume print
                self.skip_whitespace();
                if self.check(&TokenKind::Colon) {
                    // This is a print apply block
                    self.cursor = saved_cursor; // restore to print token
                    return self.parse_function_apply_block();
                }
                // Not an apply block, restore and parse as regular print
                self.cursor = saved_cursor;
                self.parse_print()
            }
            TokenKind::Error => self.parse_error_statement(),
            TokenKind::Require => self.parse_require(),
            TokenKind::Spec => self.parse_spec(),
            TokenKind::Intent => self.parse_intent(),
            TokenKind::Constant => self.parse_constant_apply_block(),
            // Allow Test keyword to be used as a class/type name
            TokenKind::Test => {
                let first_name = "Test".to_string();
                let first_location = self.current().location.clone();
                self.bump(); // consume Test token
                self.skip_whitespace();

                // Check if this is a variable declaration (Test varName = ...)
                if let TokenKind::Identifier(var_name) = self.current_kind() {
                    let var_name = var_name.clone();
                    let var_location = self.current().location.clone();
                    self.bump(); // consume variable name
                    self.skip_whitespace();

                    // Check for initializer
                    let initializer = if self.eat(&TokenKind::Assign) {
                        self.skip_whitespace();
                        Some(self.parse_expression()?)
                    } else {
                        None
                    };

                    return Ok(Statement::VariableDecl {
                        name: var_name,
                        type_: Type::Object(first_name), // Test is a class type
                        initializer,
                        location: Some(var_location),
                    });
                } else {
                    // Not a variable declaration, might be a function call like Test()
                    // Move cursor back to reparse as expression
                    self.cursor -= 1;
                    let expr = self.parse_expression()?;
                    return Ok(Statement::Expression {
                        expr,
                        location: Some(first_location),
                    });
                }
            }
            TokenKind::Identifier(name) => {
                // Could be:
                // 1. Type name for variable declaration (e.g., "integer x = 42" or "list<integer> nums = [1,2,3]")
                // 2. Variable assignment (e.g., "x = 42")
                // 3. Expression statement (e.g., "someFunction()" or "x.toString()")

                let first_name = name.clone();
                let first_location = self.current().location.clone();

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
                    } else if self.check(&TokenKind::Colon) {
                        // Could be either:
                        // 1. TYPE: apply block (followed by newline/indent)
                        // 2. TYPE:precision variable declaration (followed by integer literal)

                        // Look ahead to distinguish
                        self.bump(); // consume colon
                        self.skip_whitespace();

                        // Check what follows the colon
                        let is_precision_modifier =
                            matches!(self.current_kind(), TokenKind::IntegerLiteral(_));

                        // Restore cursor to the original position (at type identifier)
                        self.cursor = saved_cursor;

                        if is_precision_modifier {
                            // This is a variable declaration with precision modifier: TYPE:bits var = val
                            // Fall through to parse as variable declaration
                        } else {
                            // This is a TYPE: apply block
                            return self.parse_type_apply_block();
                        }

                        // At this point, cursor is positioned AT the type identifier
                        // Parse the type (which will handle precision modifiers)
                        let type_ = self.parse_type()?;
                        self.skip_whitespace();

                        // Next token should be the variable name
                        // Allow contextual keywords (like `rules`, `state`) as variable names
                        if let Some(var_name) = self.try_extract_var_name() {
                            let var_location = self.current().location.clone();
                            self.bump(); // consume variable name
                            self.skip_whitespace();

                            // Check for initializer
                            let initializer = if self.eat(&TokenKind::Assign) {
                                self.skip_whitespace();
                                Some(self.parse_expression()?)
                            } else {
                                None
                            };

                            return Ok(Statement::VariableDecl {
                                name: var_name,
                                type_,
                                initializer,
                                location: Some(var_location),
                            });
                        } else {
                            return Err(CompilerError::parse_error(
                                "Expected variable name after type".to_string(),
                                Some(self.current().location.clone()),
                                None,
                            ));
                        }
                    } else {
                        // Not a colon - this is a regular variable declaration
                        // Restore cursor to the original position
                        self.cursor = saved_cursor;

                        // At this point, cursor is positioned AT the type identifier
                        // Parse the type (which will handle precision modifiers)
                        let type_ = self.parse_type()?;
                        self.skip_whitespace();

                        // Next token should be the variable name
                        // Allow contextual keywords (like `rules`, `state`) as variable names
                        if let Some(var_name) = self.try_extract_var_name() {
                            let var_location = self.current().location.clone();
                            self.bump(); // consume variable name
                            self.skip_whitespace();

                            // Check for initializer
                            let initializer = if self.eat(&TokenKind::Assign) {
                                self.skip_whitespace();
                                Some(self.parse_expression()?)
                            } else {
                                None
                            };

                            return Ok(Statement::VariableDecl {
                                name: var_name,
                                type_,
                                initializer,
                                location: Some(var_location),
                            });
                        } else {
                            return Err(CompilerError::parse_error(
                                "Expected variable name after type".to_string(),
                                Some(self.current().location.clone()),
                                None,
                            ));
                        }
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
                        self.cursor -= 1;
                        return self.parse_framework_block();
                    }
                    // This is a function apply block: FUNCTION:
                    // Move cursor back to re-parse the identifier
                    self.cursor -= 1;
                    return self.parse_function_apply_block();
                } else if self.check(&TokenKind::Dot) {
                    // Could be:
                    // 1. Method apply block: OBJECT.METHOD:
                    // 2. Property assignment: OBJECT.PROPERTY = VALUE
                    // 3. Expression statement: OBJECT.METHOD()
                    // Need to look ahead to see which one
                    // Save current position (after the object name)
                    let saved_cursor = self.cursor;

                    // Try to parse property chain to see if it ends with ':' or '='
                    let mut has_colon = false;
                    let mut has_assign = false;
                    while self.eat(&TokenKind::Dot) {
                        self.skip_whitespace();
                        if matches!(self.current_kind(), TokenKind::Identifier(_)) {
                            self.bump(); // consume property name
                            self.skip_whitespace();
                            if self.check(&TokenKind::Colon) {
                                has_colon = true;
                                break;
                            } else if self.check(&TokenKind::Assign) {
                                has_assign = true;
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
                        self.cursor -= 1;
                        return self.parse_method_apply_block();
                    } else if has_assign {
                        // This is a property assignment: OBJECT.PROPERTY = VALUE
                        // Move cursor back to the identifier
                        self.cursor -= 1;

                        // Parse the object part
                        let object = self.parse_expression()?;
                        self.skip_whitespace();

                        // Extract property name from the parsed expression
                        // We need to check if it's a PropertyAccess
                        if let Expression::PropertyAccess { property, .. } = &object {
                            let property_name = property.clone();

                            // Consume the = token
                            if self.eat(&TokenKind::Assign) {
                                self.skip_whitespace();
                                let value = self.parse_expression()?;

                                // Create PropertyAssignment expression wrapped in Statement::Expression
                                return Ok(Statement::Expression {
                                    expr: Expression::PropertyAssignment {
                                        object: if let Expression::PropertyAccess {
                                            object, ..
                                        } = object
                                        {
                                            object
                                        } else {
                                            Box::new(Expression::Variable(first_name.clone()))
                                        },
                                        property: property_name,
                                        value: Box::new(value),
                                        location: first_location.clone(),
                                    },
                                    location: Some(first_location),
                                });
                            }
                        }

                        return Err(CompilerError::parse_error(
                            "Expected '=' after property access".to_string(),
                            Some(self.current().location.clone()),
                            None,
                        ));
                    }
                    // Not an apply block or property assignment, continue with regular parsing
                }

                // Check if next token is an Identifier or keyword that can be used as a variable name
                // (e.g., "Test test" where "test" is a keyword but can be used as a variable name)
                let next_could_be_var_name = matches!(
                    self.current_kind(),
                    TokenKind::Identifier(_) | TokenKind::Test | TokenKind::Error | TokenKind::Unit
                );

                if next_could_be_var_name {
                    // Next token is an Identifier - this could be a variable declaration
                    // with a custom class type: ClassName varName = ...
                    // Move cursor back to re-parse as type
                    self.cursor -= 1;

                    // Parse the type
                    let type_ = self.parse_type()?;
                    self.skip_whitespace();

                    // Next token should be the variable name (could be identifier or keyword)
                    let var_name = match self.current_kind() {
                        TokenKind::Identifier(name) => name.clone(),
                        TokenKind::Test => "test".to_string(),
                        TokenKind::Error => "error".to_string(),
                        TokenKind::Unit => "unit".to_string(),
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Expected variable name after custom type".to_string(),
                                Some(self.current().location.clone()),
                                None,
                            ));
                        }
                    };
                    let var_location = self.current().location.clone();
                    self.bump(); // consume variable name
                    self.skip_whitespace();

                    // Check for initializer
                    let initializer = if self.eat(&TokenKind::Assign) {
                        self.skip_whitespace();
                        Some(self.parse_expression()?)
                    } else {
                        None
                    };

                    return Ok(Statement::VariableDecl {
                        name: var_name,
                        type_,
                        initializer,
                        location: Some(var_location),
                    });
                } else if self.check(&TokenKind::Assign) {
                    // This is a simple assignment: VAR = EXPR
                    self.bump(); // consume =
                    self.skip_whitespace();
                    let value = self.parse_expression()?;

                    return Ok(Statement::Assignment {
                        target: first_name,
                        value,
                        location: Some(first_location),
                    });
                } else if self.check(&TokenKind::LeftBracket) {
                    // This could be an indexed assignment: VAR[index] = value
                    // or an expression statement: VAR[index]
                    // Parse the full LHS expression first
                    self.cursor -= 1; // Go back to the identifier

                    let lhs_expr = self.parse_expression()?;
                    self.skip_whitespace();

                    // Check if this is an assignment
                    if self.check(&TokenKind::Assign) {
                        self.bump(); // consume =
                        self.skip_whitespace();
                        let value = self.parse_expression()?;

                        // Check if lhs_expr is a list access (e.g., numbers[0])
                        if let Expression::ListAccess(list, index) = lhs_expr {
                            // Create indexed assignment (numbers[0] = 99)
                            return Ok(Statement::Expression {
                                expr: Expression::ListAssignment {
                                    list,
                                    index,
                                    value: Box::new(value),
                                    location: first_location.clone(),
                                },
                                location: Some(first_location),
                            });
                        } else {
                            // Not a list access - unsupported assignment target
                            return Err(CompilerError::parse_error(
                                "Unsupported assignment target".to_string(),
                                Some(first_location),
                                Some(
                                    "Only simple variables and list indices can be assigned to"
                                        .to_string(),
                                ),
                            ));
                        }
                    } else {
                        // Not an assignment, just an expression statement
                        return Ok(Statement::Expression {
                            expr: lhs_expr,
                            location: Some(first_location),
                        });
                    }
                } else {
                    // This could be an expression statement with operators: x + 1, x * y, etc.
                    // Or just a bare identifier
                    // Parse as full expression to handle all cases
                    self.cursor -= 1; // Go back to the identifier

                    let expr = self.parse_expression()?;

                    // Check for onError: block
                    if let Some((error_block, _error_loc)) = self.try_parse_on_error_block()? {
                        return Ok(Statement::OnErrorBlock {
                            expression: expr,
                            error_block,
                            location: Some(first_location),
                        });
                    }

                    return Ok(Statement::Expression {
                        expr,
                        location: Some(first_location),
                    });
                }
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

                // Check for onError: block
                if let Some((error_block, _error_loc)) = self.try_parse_on_error_block()? {
                    return Ok(Statement::OnErrorBlock {
                        expression: expr,
                        error_block,
                        location: Some(location),
                    });
                }

                return Ok(Statement::Expression {
                    expr,
                    location: Some(location),
                });
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
                    return Ok(Statement::Description {
                        text: description_text,
                        location: Some(location),
                    });
                } else {
                    return Err(CompilerError::parse_error(
                        "Expected string literal after 'description'".to_string(),
                        Some(self.current().location.clone()),
                        None,
                    ));
                }
            }
            // Contextual keywords used as variable names in statements
            // (e.g., `rules = rules + ","` or `rules.trim()`)
            TokenKind::Rules
            | TokenKind::Computed
            | TokenKind::State
            | TokenKind::Guard
            | TokenKind::Watch
            | TokenKind::Reset
            | TokenKind::Screen
            | TokenKind::Source
            | TokenKind::Build => {
                let first_name = self.current().text.clone();
                let first_location = self.current().location.clone();
                self.bump(); // consume keyword token
                self.skip_whitespace();

                if self.check(&TokenKind::Assign) {
                    // Simple assignment: keyword = EXPR
                    self.bump(); // consume =
                    self.skip_whitespace();
                    let value = self.parse_expression()?;
                    return Ok(Statement::Assignment {
                        target: first_name,
                        value,
                        location: Some(first_location),
                    });
                } else {
                    // Expression statement (e.g., rules.something())
                    self.cursor -= 1;
                    let expr = self.parse_expression()?;
                    return Ok(Statement::Expression {
                        expr,
                        location: Some(first_location),
                    });
                }
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

    pub(super) fn parse_return(&mut self) -> Result<Statement, CompilerError> {
        let return_token = self.expect(&TokenKind::Return)?;
        self.skip_whitespace();

        // Check if there's a return value expression
        // If we see Newline, Eof, or Dedent, there's no return value
        let value = if !matches!(
            self.current_kind(),
            TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent(_)
        ) {
            Some(self.parse_expression()?)
        } else {
            None
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

    pub(super) fn parse_if(&mut self) -> Result<Statement, CompilerError> {
        // Capture the if statement's indentation level BEFORE consuming the if token
        let if_indent_level = self.get_current_indent_level();

        let if_token = self.expect(&TokenKind::If)?;
        self.skip_whitespace();

        let condition = self.parse_expression()?;
        self.skip_whitespace();

        let then_branch = self.parse_block()?;

        self.skip_whitespace();

        // Consume Dedent tokens until we return to the if statement's own level
        // This allows us to see if there's an else clause at the same level as the if
        loop {
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level; // Copy the value to avoid borrow issues
                if level < if_indent_level {
                    // This Dedent would take us below the if statement's level
                    // Don't consume it - it belongs to a parent block
                    break;
                }
                self.bump();
                self.skip_whitespace();

                // After consuming a dedent, if we've reached the if statement's level, stop
                if level == if_indent_level {
                    break;
                }
            } else {
                // Not a Dedent token - stop
                break;
            }
        }

        // Check for else or else if at the same level as the if statement
        let else_branch = if self.eat(&TokenKind::Else) {
            self.skip_whitespace();
            // DON'T skip indentation - let parse_block() handle it

            // Check for "else if" pattern - recursively parse as nested if
            if self.check(&TokenKind::If) {
                // Parse the nested if as a single-statement block
                let nested_if = self.parse_if()?;
                Some(vec![nested_if])
            } else {
                // Regular else block
                Some(self.parse_block()?)
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

        let body = self.parse_block()?;

        self.skip_whitespace();

        // Consume Dedent tokens until we return to the while statement's own level
        // This ensures proper positioning for the next statement at the same level
        loop {
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level; // Copy the value to avoid borrow issues
                if level < while_indent_level {
                    // This Dedent would take us below the while statement's level
                    // Don't consume it - it belongs to a parent block
                    break;
                }
                self.bump();
                self.skip_whitespace();

                // After consuming a dedent, if we've reached the while statement's level, stop
                if level == while_indent_level {
                    break;
                }
            } else {
                // Not a Dedent token - stop
                break;
            }
        }

        Ok(Statement::While {
            condition,
            body,
            location: Some(while_token.location),
        })
    }

    pub(super) fn parse_for(&mut self) -> Result<Statement, CompilerError> {
        // For is represented as Iterate in Clean Language
        let for_token = self.expect(&TokenKind::For)?;
        self.skip_whitespace();

        let variable_token = self.expect_identifier()?;
        let iterator = variable_token.text.clone();

        self.skip_whitespace();
        self.expect(&TokenKind::In)?;
        self.skip_whitespace();

        let collection = self.parse_expression()?;
        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        let body = self.parse_block()?;

        Ok(Statement::Iterate {
            iterator,
            collection,
            body,
            location: Some(for_token.location),
        })
    }

    /// Parse iterate statement: iterate item in collection or iterate i in start..end
    pub(super) fn parse_iterate(&mut self) -> Result<Statement, CompilerError> {
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
            // Range iteration: iterate i in start to end [step stepValue]
            self.bump(); // consume "to"
            self.skip_whitespace();

            let end = self.parse_expression()?;
            self.skip_whitespace();

            // Check for optional "step" clause
            let step = if self.check(&TokenKind::Step) {
                self.bump(); // consume "step"
                self.skip_whitespace();
                Some(self.parse_expression()?)
            } else {
                None
            };

            self.skip_whitespace();
            let body = self.parse_block()?;

            Ok(Statement::RangeIterate {
                iterator,
                start: start_or_collection,
                end,
                step,
                body,
                location: Some(iterate_token.location),
            })
        } else {
            // Regular collection iteration: iterate item in collection
            // Check if there's an optional "step" clause (shouldn't be used with collections, but handle it)
            let _step = if self.check(&TokenKind::Step) {
                self.bump(); // consume "step"
                self.skip_whitespace();
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };

            self.skip_whitespace();
            let body = self.parse_block()?;

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

        // Consume trailing '+' for print-with-newline: print("Hello") +
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
                true
            } else {
                self.cursor = saved;
                false
            }
        } else {
            false
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

        // Create a variable declaration with the later expression as initializer
        // The type will be inferred from the expression
        Ok(Statement::VariableDecl {
            name: var_name,
            type_: Type::Any, // Type will be inferred from the expression
            initializer: Some(expression),
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

    /// Check if the current position has `onError:` and parse the error handling block
    /// Returns Some((error_block, location)) if onError block is present, None otherwise
    pub(super) fn try_parse_on_error_block(
        &mut self,
    ) -> Result<Option<(Vec<Statement>, crate::ast::SourceLocation)>, CompilerError> {
        self.skip_whitespace();

        // Check for onError keyword
        if !self.check(&TokenKind::OnError) {
            return Ok(None);
        }

        let error_location = self.current().location.clone();
        self.bump(); // consume onError
        self.skip_whitespace();

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

        // Use parse_block() to handle all the indentation logic properly
        // parse_block() will consume the Indent token, parse all statements,
        // and handle Dedent tokens correctly
        let error_block = self.parse_block()?;

        Ok(Some((error_block, error_location)))
    }
}
