//! Expression parsing methods for the token-driven parser.
//!
//! This module contains all expression-related parsing for `TokenParser`:
//! - Primary expressions (literals, variables, array/pairs literals, conditionals)
//! - Operator precedence chain: on_error → default → logical_or → logical_and →
//!   comparison → term → factor → power → unary → postfix → primary
//! - String interpolation

use super::TokenParser;
use crate::ast::{BinaryOperator, Expression, SourceLocation, StringPart, Value};
use crate::error::CompilerError;
use crate::lexer::specification_token::TokenKind;

impl TokenParser {
    pub(super) fn parse_expression(&mut self) -> Result<Expression, CompilerError> {
        self.parse_on_error()
    }

    // Parse onError expressions: expr onError fallback
    // OnError has lowest precedence (below logical OR)
    // Supports chaining: a onError b onError c = (a onError b) onError c (left-associative)
    pub(super) fn parse_on_error(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_logical_or()?;

        // Support chained onError expressions with a while loop
        while self.check(&TokenKind::OnError) {
            // Peek ahead to see if this is onError: (block) or onError expr
            let saved_cursor = self.cursor;
            self.bump(); // consume onError
            self.skip_whitespace();

            // If we see a colon, this is onError: block syntax (handled at statement level)
            // Back up and stop parsing onError at expression level
            if self.check(&TokenKind::Colon) {
                self.cursor = saved_cursor; // restore cursor to before onError
                break;
            }

            // Otherwise, parse fallback expression
            let fallback = self.parse_logical_or()?;
            let location = self.current().location.clone();

            expr = Expression::OnError {
                expression: Box::new(expr),
                fallback: Box::new(fallback),
                location,
            };
        }

        Ok(expr)
    }

    // NOTE: Add logical OR operator support (lowest precedence)
    pub(super) fn parse_logical_or(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_logical_and()?;

        while self.check(&TokenKind::Or) {
            let _op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_logical_and()?;
            expr = Expression::Binary(Box::new(expr), BinaryOperator::Or, Box::new(right));
        }

        Ok(expr)
    }

    // NOTE: Add logical AND operator support
    pub(super) fn parse_logical_and(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_comparison()?;

        while self.check(&TokenKind::And) {
            let _op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_comparison()?;
            expr = Expression::Binary(Box::new(expr), BinaryOperator::And, Box::new(right));
        }

        Ok(expr)
    }

    pub(super) fn parse_comparison(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_term()?;

        while matches!(
            self.current_kind(),
            TokenKind::Equal
                | TokenKind::NotEqual
                | TokenKind::Less
                | TokenKind::Greater
                | TokenKind::LessEqual
                | TokenKind::GreaterEqual
                | TokenKind::Is
        ) {
            let op_token = self.bump();
            self.skip_whitespace();

            // When we see `is`, peek ahead for `not` to handle `value is not null`.
            // `is not` is a single two-token operator meaning inequality / non-null check.
            let op = match &op_token.kind {
                TokenKind::Equal => BinaryOperator::Equal,
                TokenKind::NotEqual => BinaryOperator::NotEqual,
                TokenKind::Less => BinaryOperator::Less,
                TokenKind::Greater => BinaryOperator::Greater,
                TokenKind::LessEqual => BinaryOperator::LessEqual,
                TokenKind::GreaterEqual => BinaryOperator::GreaterEqual,
                TokenKind::Is => {
                    // Check for `is not` two-token operator
                    if self.check(&TokenKind::Not) {
                        self.bump(); // consume `not`
                        self.skip_whitespace();
                        BinaryOperator::Not
                    } else {
                        BinaryOperator::Is
                    }
                }
                _ => unreachable!(),
            };

            let right = self.parse_term()?;
            expr = Expression::Binary(Box::new(expr), op, Box::new(right));
        }

        Ok(expr)
    }

    pub(super) fn parse_term(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_factor()?;

        while matches!(self.current_kind(), TokenKind::Plus | TokenKind::Minus) {
            let op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_factor()?;

            let op = match &op_token.kind {
                TokenKind::Plus => BinaryOperator::Add,
                TokenKind::Minus => BinaryOperator::Subtract,
                _ => unreachable!(),
            };

            expr = Expression::Binary(Box::new(expr), op, Box::new(right));
        }

        Ok(expr)
    }

    pub(super) fn parse_factor(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_power()?;

        while matches!(
            self.current_kind(),
            TokenKind::Multiply | TokenKind::Divide | TokenKind::Modulo
        ) {
            let op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_power()?;

            let op = match &op_token.kind {
                TokenKind::Multiply => BinaryOperator::Multiply,
                TokenKind::Divide => BinaryOperator::Divide,
                TokenKind::Modulo => BinaryOperator::Modulo,
                _ => unreachable!(),
            };

            expr = Expression::Binary(Box::new(expr), op, Box::new(right));
        }

        Ok(expr)
    }

    pub(super) fn parse_power(&mut self) -> Result<Expression, CompilerError> {
        let expr = self.parse_unary()?;

        if self.check(&TokenKind::Power) {
            let _op_token = self.bump();
            self.skip_whitespace();
            // Right associative - recursively parse the right side
            let right = self.parse_power()?;
            return Ok(Expression::Binary(
                Box::new(expr),
                BinaryOperator::Power,
                Box::new(right),
            ));
        }

        Ok(expr)
    }

    // NOTE: Add unary operator support (not, unary -)
    pub(super) fn parse_unary(&mut self) -> Result<Expression, CompilerError> {
        match self.current_kind() {
            TokenKind::Not => {
                let _op_token = self.bump();
                self.skip_whitespace();
                let operand = self.parse_unary()?; // Right-recursive for multiple unary ops
                Ok(Expression::Unary(
                    crate::ast::UnaryOperator::Not,
                    Box::new(operand),
                ))
            }
            TokenKind::Minus => {
                let _op_token = self.bump();
                self.skip_whitespace();
                let operand = self.parse_unary()?;
                Ok(Expression::Unary(
                    crate::ast::UnaryOperator::Negate,
                    Box::new(operand),
                ))
            }
            TokenKind::Plus => {
                // Unary plus is a no-op, just skip it and parse the operand
                let _op_token = self.bump();
                self.skip_whitespace();
                self.parse_unary()
            }
            _ => self.parse_postfix(),
        }
    }

    /// Check whether the current position looks like the start of a named argument:
    /// `identifier ":"` where the colon is NOT followed by another colon (`::` is a
    /// different construct).  Must only be called while inside call parentheses
    /// (`paren_depth > 0`).
    fn is_named_arg_start(&self) -> bool {
        // Current token must be an identifier or a contextual keyword that is allowed
        // as a parameter name.
        let is_name = matches!(
            self.current_kind(),
            TokenKind::Identifier(_)
                | TokenKind::Rules
                | TokenKind::Computed
                | TokenKind::State
                | TokenKind::Guard
                | TokenKind::Watch
                | TokenKind::Reset
                | TokenKind::Screen
                | TokenKind::Source
                | TokenKind::Build
                | TokenKind::Spec
                | TokenKind::Intent
                | TokenKind::Description
                | TokenKind::Input
                | TokenKind::Unit
                | TokenKind::Step
                | TokenKind::Test
                | TokenKind::Error
        );
        if !is_name {
            return false;
        }

        // The very next token (no whitespace between `name` and `:`) must be Colon.
        // We use look_ahead(1) because the lexer does not emit whitespace tokens on
        // a single line — there is nothing between `name` and `:`.
        let next = self.look_ahead(1);
        matches!(next.kind, TokenKind::Colon)
    }

    /// Parse a single call argument, producing either a plain expression or a
    /// `NamedArgBinding { label, value }` when the argument uses the `label: expr`
    /// syntax (grammar.ebnf: `named_argument`).
    fn parse_call_argument(&mut self) -> Result<Expression, CompilerError> {
        if self.is_named_arg_start() {
            // Consume the label identifier.
            let label_token = self.bump();
            let label = label_token.text.clone();
            let location: SourceLocation = label_token.location.clone();

            // Consume the colon.
            self.expect(&TokenKind::Colon)?;
            self.skip_whitespace();

            // Parse the argument value expression.
            let value = self.parse_expression()?;

            Ok(Expression::NamedArgBinding {
                label,
                value: Box::new(value),
                location,
            })
        } else {
            self.parse_expression()
        }
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_primary()?;

        // Handle postfix operations: function calls and member access
        loop {
            self.skip_whitespace();

            match self.current_kind() {
                TokenKind::LeftParen => {
                    // Function call: identifier(args) or method call: expr.method(args)
                    let call_location = self.current().location.clone();
                    self.bump(); // consume (
                    self.paren_depth += 1; // Track that we're inside call parentheses
                    self.skip_whitespace();

                    let mut arguments = Vec::new();
                    if !self.check(&TokenKind::RightParen) {
                        loop {
                            arguments.push(self.parse_call_argument()?);
                            self.skip_whitespace();

                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                            self.skip_whitespace();
                        }
                    }

                    self.expect(&TokenKind::RightParen)?;
                    self.paren_depth -= 1; // Exit call parentheses

                    // Convert expression to function call or method call
                    expr = match expr {
                        Expression::Variable(name) => {
                            // Special case: base() calls in constructors
                            if name == "base" {
                                Expression::BaseCall {
                                    arguments,
                                    location: call_location.clone(),
                                }
                            } else {
                                Expression::Call(name, arguments)
                            }
                        }
                        Expression::PropertyAccess {
                            object,
                            property,
                            location,
                        } => {
                            // Method call: object.method(args)
                            Expression::MethodCall {
                                object,
                                method: property,
                                arguments,
                                location,
                            }
                        }
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Function calls must be on identifiers or property access"
                                    .to_string(),
                                Some(call_location),
                                None,
                            ))
                        }
                    };
                }
                TokenKind::Dot => {
                    // Member access: expr.property (might be followed by () for method call)
                    // Allow keywords as property/method names (e.g., logical.and, logical.or)
                    let dot_location = self.current().location.clone();
                    self.bump(); // consume .
                    self.skip_whitespace();

                    let property_token = self.expect_name()?;
                    let property = property_token.text.clone();

                    // Create PropertyAccess for now
                    // If next token is (, it will be converted to MethodCall in next iteration
                    expr = Expression::PropertyAccess {
                        object: Box::new(expr),
                        property,
                        location: dot_location,
                    };
                }
                TokenKind::LeftBracket => {
                    // Array/List indexing: expr[index]
                    self.bump(); // consume [
                    self.skip_whitespace();

                    let index = self.parse_expression()?;
                    self.skip_whitespace();

                    self.expect(&TokenKind::RightBracket)?;

                    expr = Expression::ListAccess(Box::new(expr), Box::new(index));
                }
                // Bang (!) is no longer a postfix operator; stop parsing postfix chain
                _ => break,
            }
        }

        Ok(expr)
    }

    pub(super) fn parse_primary(&mut self) -> Result<Expression, CompilerError> {
        match self.current_kind() {
            TokenKind::IntegerLiteral(n) => {
                let value = *n;
                self.bump();
                Ok(Expression::Literal(Value::Integer(value)))
            }
            TokenKind::NumberLiteral(n) => {
                let value = *n;
                self.bump();
                Ok(Expression::Literal(Value::Number(value)))
            }
            TokenKind::StringLiteral(s) => {
                let value = s.clone();
                self.bump();
                Ok(Expression::Literal(Value::String(value)))
            }
            TokenKind::InterpolationStart => {
                // Parse string interpolation: "Hello {name}!"
                self.parse_string_interpolation()
            }
            TokenKind::BooleanLiteral(b) => {
                let value = *b;
                self.bump();
                Ok(Expression::Literal(Value::Boolean(value)))
            }
            TokenKind::True => {
                self.bump();
                Ok(Expression::Literal(Value::Boolean(true)))
            }
            TokenKind::False => {
                self.bump();
                Ok(Expression::Literal(Value::Boolean(false)))
            }
            // null-support - None literal parsing
            TokenKind::None => {
                self.bump();
                Ok(Expression::Literal(Value::None))
            }
            TokenKind::Identifier(_) => {
                let name_token = self.expect_identifier()?;
                let name = name_token.text.clone();
                Ok(Expression::Variable(name))
            }
            TokenKind::Start => {
                // start keyword for async execution: start fetchData()
                let location = self.current().location.clone();
                self.bump(); // consume 'start'
                self.skip_whitespace();
                let expression = Box::new(self.parse_expression()?);
                Ok(Expression::StartExpression {
                    expression,
                    location,
                })
            }
            TokenKind::Later => {
                // `later` as an expression prefix: later fetchGreeting()
                // The async scheduling is handled at the host/runtime level.
                // At the WASM/compiler level, `later expr` compiles as a regular call.
                self.bump(); // consume 'later'
                self.skip_whitespace();
                // Parse the following expression (typically a function call)
                self.parse_primary()
            }
            // Allow keywords to be used as identifiers in expressions (for class/type names and variable names)
            TokenKind::Test
            | TokenKind::Error
            | TokenKind::Unit
            | TokenKind::Input
            | TokenKind::Step
            | TokenKind::Description
            | TokenKind::Rules
            | TokenKind::Computed
            | TokenKind::State
            | TokenKind::Guard
            | TokenKind::Watch
            | TokenKind::Reset
            | TokenKind::Screen
            | TokenKind::Source
            | TokenKind::Build
            | TokenKind::Spec
            | TokenKind::Intent => {
                let token = self.bump();
                // Use the actual token text to preserve the exact identifier (e.g., "Test", not "test")
                let name = token.text.clone();
                Ok(Expression::Variable(name))
            }
            TokenKind::This => {
                self.bump();
                Ok(Expression::Variable("this".to_string()))
            }
            TokenKind::LeftParen => {
                self.bump();
                self.paren_depth += 1; // Track that we're inside parentheses
                self.skip_whitespace();
                let expr = self.parse_expression()?;
                self.skip_whitespace();
                self.expect(&TokenKind::RightParen)?;
                self.paren_depth -= 1; // Exit parentheses
                Ok(expr)
            }
            TokenKind::LeftBracket => {
                // Parse array/list literal: [elem1, elem2, ...]
                self.bump(); // consume '['
                self.skip_whitespace();

                let mut elements = Vec::new();

                // Check for empty array
                if matches!(self.current_kind(), TokenKind::RightBracket) {
                    self.bump(); // consume ']'
                    return Ok(Expression::Literal(Value::List(elements)));
                }

                // Parse array elements (as expressions, will be converted to values)
                loop {
                    let elem_expr = self.parse_expression()?;

                    // Convert expression to Value if it's a literal
                    // For now, we support literal values and will handle variables/expressions later in MIR
                    let value = match elem_expr {
                        Expression::Literal(val) => val,
                        _ => {
                            // For now, allow non-literal expressions by wrapping them
                            // This will be handled properly during lowering to MIR
                            // Temporarily store as a placeholder - this needs proper handling in semantic analysis
                            return Err(CompilerError::parse_error(
                                "Array literals currently only support constant literal values".to_string(),
                                Some(self.current().location.clone()),
                                Some("Variables and expressions in arrays will be supported in MIR lowering".to_string()),
                            ));
                        }
                    };

                    elements.push(value);
                    self.skip_whitespace();

                    if matches!(self.current_kind(), TokenKind::Comma) {
                        self.bump(); // consume ','
                        self.skip_whitespace();
                    } else {
                        break;
                    }
                }

                self.expect(&TokenKind::RightBracket)?;
                Ok(Expression::Literal(Value::List(elements)))
            }
            TokenKind::If => {
                // Parse conditional expression: if condition then value else value
                let if_token = self.bump(); // consume 'if'
                let location = if_token.location.clone();
                self.skip_whitespace();

                // Parse condition - use comparison level to avoid parsing "then" as part of condition
                let condition = Box::new(self.parse_comparison()?);
                self.skip_whitespace();

                // Expect 'then' keyword (identifier)
                if let TokenKind::Identifier(id) = self.current_kind() {
                    if id != "then" {
                        return Err(CompilerError::parse_error(
                            format!("Expected 'then' after if condition, found '{}'", id),
                            Some(self.current().location.clone()),
                            None,
                        ));
                    }
                    self.bump(); // consume 'then'
                } else {
                    return Err(CompilerError::parse_error(
                        format!(
                            "Expected 'then' after if condition, found {:?}",
                            self.current_kind()
                        ),
                        Some(self.current().location.clone()),
                        None,
                    ));
                }
                self.skip_whitespace();

                // Parse then expression - use comparison level to avoid parsing "else" as part of then expression
                let then_expr = Box::new(self.parse_comparison()?);
                self.skip_whitespace();

                // Expect 'else' keyword (TokenKind::Else)
                if !self.check(&TokenKind::Else) {
                    return Err(CompilerError::parse_error(
                        format!(
                            "Expected 'else' after then expression, found {:?}",
                            self.current_kind()
                        ),
                        Some(self.current().location.clone()),
                        None,
                    ));
                }
                self.bump(); // consume 'else'
                self.skip_whitespace();

                // Parse else expression - can be full expression since this is the last part
                let else_expr = Box::new(self.parse_comparison()?);

                Ok(Expression::Conditional {
                    condition,
                    then_expr,
                    else_expr,
                    location,
                })
            }
            TokenKind::LeftBrace => {
                // Parse pairs literal: {"key": value, "key2": value2}
                self.bump(); // consume '{'
                self.skip_whitespace();

                let mut pairs = Vec::new();

                // Check for empty pairs
                if matches!(self.current_kind(), TokenKind::RightBrace) {
                    self.bump(); // consume '}'
                    return Ok(Expression::Literal(Value::Pairs(pairs)));
                }

                // Parse key-value pairs
                loop {
                    // Parse key (must be an expression, typically a literal)
                    let key_expr = self.parse_expression()?;
                    let key = match key_expr {
                        Expression::Literal(val) => val,
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Pairs literal keys must be constant literals".to_string(),
                                Some(self.current().location.clone()),
                                None,
                            ));
                        }
                    };

                    self.skip_whitespace();

                    // Expect colon
                    self.expect(&TokenKind::Colon)?;
                    self.skip_whitespace();

                    // Parse value
                    let value_expr = self.parse_expression()?;
                    let value = match value_expr {
                        Expression::Literal(val) => val,
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Pairs literal values currently only support constant literals".to_string(),
                                Some(self.current().location.clone()),
                                Some("Variables and expressions in pairs will be supported in MIR lowering".to_string()),
                            ));
                        }
                    };

                    pairs.push((key, value));
                    self.skip_whitespace();

                    if matches!(self.current_kind(), TokenKind::Comma) {
                        self.bump(); // consume ','
                        self.skip_whitespace();
                    } else {
                        break;
                    }
                }

                self.expect(&TokenKind::RightBrace)?;
                Ok(Expression::Literal(Value::Pairs(pairs)))
            }
            _ => {
                let token = self.current();
                Err(CompilerError::parse_error(
                    format!("Unexpected token in expression: {:?}", token.kind),
                    Some(token.location.clone()),
                    None,
                ))
            }
        }
    }

    pub(super) fn parse_string_interpolation(&mut self) -> Result<Expression, CompilerError> {
        let mut parts = Vec::new();

        // Handle InterpolationStart - token text contains the literal string part before first {
        if let TokenKind::InterpolationStart = self.current_kind() {
            let token = self.bump();
            // Add text part if present
            if !token.text.is_empty() {
                parts.push(StringPart::Text(token.text.clone()));
            }

            // Parse the expression inside {}
            let expr = self.parse_expression()?;
            parts.push(StringPart::Interpolation(expr));

            // Handle InterpolationMid tokens - text between } and next {
            while matches!(self.current_kind(), TokenKind::InterpolationMid) {
                let token = self.bump();
                // Add text part if present
                if !token.text.is_empty() {
                    parts.push(StringPart::Text(token.text.clone()));
                }

                // Parse next expression
                let expr = self.parse_expression()?;
                parts.push(StringPart::Interpolation(expr));
            }

            // Handle InterpolationEnd - token text contains the literal string part after last }
            if let TokenKind::InterpolationEnd = self.current_kind() {
                let token = self.bump();
                // Add final text part if present
                if !token.text.is_empty() {
                    parts.push(StringPart::Text(token.text.clone()));
                }
            } else {
                return Err(CompilerError::parse_error(
                    "Expected end of string interpolation".to_string(),
                    Some(self.current().location.clone()),
                    None,
                ));
            }
        }

        Ok(Expression::StringInterpolation(parts))
    }
}
