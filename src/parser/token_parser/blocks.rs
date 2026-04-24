//! Block parsing methods for the token-driven parser.
//!
//! This module contains all block-level constructs for `TokenParser`:
//! - TypeApplyBlock, ConstantApplyBlock, FunctionApplyBlock, MethodApplyBlock
//! - Framework blocks and plugin declarations
//! - Tests block
//! - State, computed, rules, watch blocks
//! - Build and source top-level blocks
//! - Spec and intent metadata statements
//! - Plugins block
//! - External function declarations

use super::TokenParser;
use crate::ast::{
    ConstantAssignment, Expression, FrameworkAttribute, Statement, TestCase, Type, Value,
    VariableAssignment,
};
use crate::error::CompilerError;
use crate::lexer::specification_token::TokenKind;
use tracing::{debug, trace};

impl TokenParser {
    /// Parse a type apply block: TYPE:\n\tvar1 = value1\n\tvar2 = value2
    /// Example: integer:\n\tcount = 0\n\tmaxSize = 100
    pub(super) fn parse_type_apply_block(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();

        // Parse the type (integer, string, boolean, etc.)
        let type_identifier = self.expect_identifier()?;
        let type_ = match type_identifier.text.as_str() {
            "integer" => Type::Integer,
            "number" => Type::Number,
            "string" => Type::String,
            "boolean" => Type::Boolean,
            "void" => Type::Void,
            other => Type::Object(other.to_string()),
        };

        self.skip_whitespace();

        // Expect ':'
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Parse indented assignments
        let mut assignments = Vec::new();

        // Determine the apply block's indentation level
        let block_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        while !self.is_at_end() {
            tracing::debug!(
                before_skip_ws = ?self.current_kind(),
                "BEFORE skip_whitespace in TypeApplyBlock loop"
            );

            self.skip_whitespace();

            tracing::debug!(
                after_skip_ws = ?self.current_kind(),
                "AFTER skip_whitespace in TypeApplyBlock loop"
            );

            if self.is_at_end() {
                break;
            }

            tracing::debug!(
                current_token = ?self.current_kind(),
                block_level = block_level,
                "TypeApplyBlock loop iteration"
            );

            // Consume Dedent tokens - exit when we see one below our level
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                self.bump();
                self.skip_whitespace();
                tracing::debug!(
                    dedent_level = level,
                    block_level = block_level,
                    "Consumed Dedent token"
                );
                if level < block_level {
                    // Exited the apply block
                    tracing::debug!("Exiting TypeApplyBlock - dedent below block level");
                    break;
                }
            }

            if self.is_at_end() {
                break;
            }

            // Check for Indent at our level
            if matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_level) {
                tracing::debug!("Found Indent at block level, parsing assignment");
                self.bump(); // consume Indent
                self.skip_whitespace();

                // Parse assignment: name = value
                if let TokenKind::Identifier(var_name) = self.current_kind() {
                    let var_name = var_name.clone();
                    tracing::debug!(var_name = %var_name, "Parsing assignment");
                    self.bump();
                    self.skip_whitespace();

                    // Expect '='
                    self.expect(&TokenKind::Assign)?;
                    self.skip_whitespace();

                    // Parse the initializer expression
                    let initializer = self.parse_expression()?;

                    tracing::debug!(
                        var_name = %var_name,
                        after_parse_expr = ?self.current_kind(),
                        "After parsing expression"
                    );

                    assignments.push(VariableAssignment {
                        name: var_name.clone(),
                        initializer: Some(initializer),
                    });
                    tracing::debug!(
                        var_name = %var_name,
                        current_token_after_push = ?self.current_kind(),
                        "Successfully parsed assignment"
                    );
                } else {
                    // Not an assignment, exit block
                    tracing::debug!("Not an identifier, exiting TypeApplyBlock");
                    break;
                }
            } else {
                // No indentation or wrong level - exit block
                tracing::debug!(current_token = ?self.current_kind(), "No indent at block level, exiting TypeApplyBlock");
                break;
            }
        }

        tracing::debug!(
            type_ = ?type_,
            assignments_count = assignments.len(),
            "Parser created TypeApplyBlock statement"
        );

        Ok(Statement::TypeApplyBlock {
            type_,
            assignments,
            location: Some(location),
        })
    }

    /// Parse a constant apply block: constant:\n\tTYPE NAME = value\n\tTYPE NAME2 = value2
    /// Example: constant:\n\tinteger MAX_USERS = 1000\n\tstring API_VERSION = "v2.1"
    pub(super) fn parse_constant_apply_block(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();

        // Consume "constant" keyword
        self.expect(&TokenKind::Constant)?;
        self.skip_whitespace();

        // Expect ':'
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Parse indented constant declarations
        let mut constants = Vec::new();

        // Determine the apply block's indentation level
        let block_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume Dedent tokens - exit when we see one below our level
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                self.bump();
                self.skip_whitespace();
                if level < block_level {
                    // Exited the apply block
                    break;
                }
            }

            if self.is_at_end() {
                break;
            }

            // Check for Indent at our level
            if matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_level) {
                self.bump(); // consume Indent
                self.skip_whitespace();

                // Parse constant declaration: TYPE NAME = value
                // First token is the type
                let type_identifier = self.expect_identifier()?;
                let const_type = match type_identifier.text.as_str() {
                    "integer" => Type::Integer,
                    "number" => Type::Number,
                    "string" => Type::String,
                    "boolean" => Type::Boolean,
                    other => Type::Object(other.to_string()),
                };

                self.skip_whitespace();

                // Second token is the constant name
                let name_token = self.expect_identifier()?;
                let const_name = name_token.text.clone();

                self.skip_whitespace();

                // Expect '='
                self.expect(&TokenKind::Assign)?;
                self.skip_whitespace();

                // Parse the value expression
                let value = self.parse_expression()?;

                constants.push(ConstantAssignment {
                    type_: const_type,
                    name: const_name,
                    value,
                });
            } else {
                // No indentation or wrong level - exit block
                break;
            }
        }

        Ok(Statement::ConstantApplyBlock {
            constants,
            location: Some(location),
        })
    }

    /// Parse a function apply block: FUNCTION:\n\targ1\n\targ2
    /// Example: print:\n\t"Hello"\n\t"World"
    /// Equivalent to: print("Hello"), print("World")
    pub(super) fn parse_function_apply_block(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();

        // Parse the function name (could be identifier or keyword like print/println)
        let function_name = match self.current_kind() {
            TokenKind::Identifier(_) => {
                let token = self.bump();
                token.text.clone()
            }
            TokenKind::Print => {
                self.bump();
                "print".to_string()
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Expected function name for apply block, found {:?}",
                        self.current_kind()
                    ),
                    Some(self.current().location.clone()),
                    None,
                ));
            }
        };
        self.skip_whitespace();

        // Expect ':'
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Parse indented expressions
        let mut expressions = Vec::new();

        // Determine the apply block's indentation level
        let block_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume Dedent tokens - exit when we see one below our level
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                self.bump();
                self.skip_whitespace();
                if level < block_level {
                    // Exited the apply block
                    break;
                }
            }

            if self.is_at_end() {
                break;
            }

            // Check for Indent at our level
            if matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_level) {
                self.bump(); // consume Indent
                self.skip_whitespace();

                // Parse expression
                let expr = self.parse_expression()?;
                expressions.push(expr);
            } else {
                // No indentation or wrong level - exit block
                break;
            }
        }

        Ok(Statement::FunctionApplyBlock {
            function_name,
            expressions,
            location: Some(location),
        })
    }

    /// Parse a print block statement: print:\n\texpr1\n\texpr2
    ///
    /// Each indented expression is printed on its own line (equivalent to
    /// `print(expr) +` for each line).  The parser cursor must be positioned
    /// at the `print` token on entry.
    pub(super) fn parse_print_block(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();

        // Consume the `print` keyword.
        self.expect(&TokenKind::Print)?;
        self.skip_whitespace();

        // Consume the `:` colon.
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip the newline after the colon.
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Determine indentation level from the first indented token.
        let block_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            1
        };

        let mut expressions = Vec::new();

        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume Dedent tokens — exit when we encounter a dedent that
            // falls below our block's indentation level.
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                self.bump();
                self.skip_whitespace();
                if level < block_level {
                    break;
                }
            }

            if self.is_at_end() {
                break;
            }

            // A line at the correct indentation level is a print expression.
            if matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_level) {
                self.bump(); // consume Indent token
                self.skip_whitespace();

                let expr = self.parse_expression()?;
                expressions.push(expr);
            } else {
                // Wrong indentation level or no indentation — exit the block.
                break;
            }
        }

        Ok(Statement::PrintBlock {
            expressions,
            newline: true,
            location: Some(location),
        })
    }

    /// Parse a method apply block: OBJECT.METHOD:\n\targ1\n\targ2
    /// Example: list.push:\n\titem1\n\titem2
    /// Equivalent to: list.push(item1), list.push(item2)
    pub(super) fn parse_method_apply_block(&mut self) -> Result<Statement, CompilerError> {
        let location = self.current().location.clone();

        // Parse the object name
        let object_name = self.expect_identifier()?.text.clone();
        self.skip_whitespace();

        // Parse method chain (object.method1.method2...)
        let mut method_chain = Vec::new();
        while self.eat(&TokenKind::Dot) {
            self.skip_whitespace();
            method_chain.push(self.expect_identifier()?.text.clone());
            self.skip_whitespace();
        }

        // Expect ':'
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Parse indented expressions
        let mut expressions = Vec::new();

        // Determine the apply block's indentation level
        let block_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume Dedent tokens - exit when we see one below our level
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                self.bump();
                self.skip_whitespace();
                if level < block_level {
                    // Exited the apply block
                    break;
                }
            }

            if self.is_at_end() {
                break;
            }

            // Check for Indent at our level
            if matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_level) {
                self.bump(); // consume Indent
                self.skip_whitespace();

                // Parse expression
                let expr = self.parse_expression()?;
                expressions.push(expr);
            } else {
                // No indentation or wrong level - exit block
                break;
            }
        }

        Ok(Statement::MethodApplyBlock {
            object_name,
            method_chain,
            expressions,
            location: Some(location),
        })
    }

    /// Extract raw block content from source text using byte positions.
    ///
    /// Instead of reconstructing text from individual tokens (which destroys HTML/template formatting),
    /// this method uses the original source text and byte positions to extract content verbatim.
    /// It strips the block-level indentation (tabs) from each line.
    pub(super) fn extract_block_content_raw(&mut self, block_indent_level: usize) -> String {
        // Record byte position of first content token
        let content_start_byte = self.current().location.byte_start;

        // Skip through all tokens in the block using the same Indent/Dedent boundary logic
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for dedent that exits the block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < block_indent_level {
                    // Block ended, don't consume the dedent
                    break;
                } else {
                    // Dedent within the block, consume it
                    self.bump();
                    continue;
                }
            }

            // Check for indent at lower level than block
            if let TokenKind::Indent(indent_level) = self.current_kind() {
                if *indent_level < block_indent_level {
                    // Not part of this block
                    break;
                }
                self.bump(); // Consume indent
            }

            // Consume all tokens on the line
            while !self.is_at_end()
                && !matches!(
                    self.current_kind(),
                    TokenKind::Newline | TokenKind::Dedent(_)
                )
            {
                self.bump();
            }

            // Consume newline if present
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
            }
        }

        // Record byte position of the token after the block
        let content_end_byte = self.current().location.byte_start;

        // Extract raw text from source
        let raw_content = match (content_start_byte, content_end_byte) {
            (Some(start), Some(end)) if start < end && end <= self.source_content.len() => {
                self.source_content[start..end].to_string()
            }
            _ => String::new(),
        };

        // Strip block-level indentation from each line and clean up
        let mut content_lines = Vec::new();
        for line in raw_content.lines() {
            // Strip exactly block_indent_level tabs from the start of each line
            let mut stripped = line;
            for _ in 0..block_indent_level {
                stripped = stripped.strip_prefix('\t').unwrap_or(stripped);
            }
            content_lines.push(stripped);
        }

        // Join and trim trailing whitespace
        let content = content_lines.join("\n");
        content.trim_end().to_string()
    }

    /// Parse a framework block or plugin declaration
    ///
    /// Framework blocks use colon: "identifier:", "identifier string:", "identifier identifier:"
    /// Plugin declarations don't require colon: "data User" (when "data" is a plugin keyword)
    pub(super) fn parse_framework_block_or_plugin(
        &mut self,
        is_plugin_keyword: bool,
    ) -> Result<Statement, CompilerError> {
        if !is_plugin_keyword {
            // Standard framework block with colon
            return self.parse_framework_block();
        }

        // Plugin keyword with colon syntax (e.g., "component:" or "html:")
        // should use the standard framework block parser, not the plugin path.
        // The plugin path expects "keyword identifier" (e.g., "data User").
        if self.peek_kind() == Some(&TokenKind::Colon) {
            return self.parse_framework_block();
        }

        // Plugin keyword without colon (e.g., "data User")
        let start_location = self.current().location.clone();

        // Get the plugin keyword (first identifier, e.g., "data")
        let keyword = if let TokenKind::Identifier(name) = self.current_kind() {
            name.clone()
        } else {
            return Err(CompilerError::parse_error(
                "Expected plugin keyword".to_string(),
                Some(start_location),
                None,
            ));
        };

        self.bump(); // Consume keyword
        self.skip_whitespace();

        // Get the declaration name (e.g., "User" in "data User")
        let decl_name = match self.current_kind() {
            TokenKind::Identifier(name) => {
                let n = name.clone();
                self.bump();
                n
            }
            TokenKind::StringLiteral(s) => {
                let n = s.clone();
                self.bump();
                n
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Expected identifier after plugin keyword '{}'", keyword),
                    Some(self.current().location.clone()),
                    Some(format!(
                        "Plugin keyword '{}' should be followed by an identifier",
                        keyword
                    )),
                ));
            }
        };

        // Keep keyword as name, put decl_name in attributes
        // This allows plugins to check "block_name == 'data'" properly
        let block_name = keyword.clone();
        let attributes = vec![FrameworkAttribute {
            name: decl_name,
            value: None,
            location: Some(start_location.clone()),
        }];

        self.skip_whitespace();

        // Optional colon (some plugin declarations might still use it)
        if matches!(self.current_kind(), TokenKind::Colon) {
            self.bump();
            self.skip_whitespace();
        }

        // Expect newline
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Determine the block's indentation level
        let block_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        // Extract raw content from source using byte positions
        let content = self.extract_block_content_raw(block_indent_level);

        debug!(
            block_name = %block_name,
            content_len = content.len(),
            "Parsed plugin declaration block"
        );

        Ok(Statement::FrameworkBlock {
            name: block_name,
            content,
            attributes,
            location: Some(start_location),
        })
    }

    /// Parse a framework block (e.g., endpoints:, data, component, screen "Name":)
    /// Supports patterns: "identifier:", "identifier string:", "identifier identifier:"
    /// These are captured as raw text for plugin expansion
    pub(super) fn parse_framework_block(&mut self) -> Result<Statement, CompilerError> {
        let start_location = self.current().location.clone();

        // Get the block name (first identifier)
        let block_name = if let TokenKind::Identifier(name) = self.current_kind() {
            name.clone()
        } else {
            return Err(CompilerError::parse_error(
                "Expected identifier for framework block name".to_string(),
                Some(start_location),
                None,
            ));
        };

        self.bump(); // Consume first identifier
        self.skip_whitespace();

        // Check for optional second part (string or identifier) before colon
        // This handles patterns like "screen Counter:" or "screen \"Name\":"
        let block_arg = match self.current_kind() {
            TokenKind::StringLiteral(s) => {
                let arg = s.clone();
                self.bump();
                self.skip_whitespace();
                Some(arg)
            }
            TokenKind::Identifier(id) if self.peek_kind() == Some(&TokenKind::Colon) => {
                let arg = id.clone();
                self.bump();
                self.skip_whitespace();
                Some(arg)
            }
            _ => None,
        };

        // Keep block_name as name, put block_arg in attributes if present
        // This allows plugins to check "block_name == 'data'" properly
        let attributes = match block_arg {
            Some(arg) => vec![FrameworkAttribute {
                name: arg,
                value: None,
                location: Some(start_location.clone()),
            }],
            None => vec![],
        };

        // Expect colon
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Expect newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Determine the block's indentation level
        let block_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        // Extract raw content from source using byte positions
        let content = self.extract_block_content_raw(block_indent_level);

        debug!(
            block_name = %block_name,
            content_len = content.len(),
            "Parsed framework block"
        );
        trace!(content = %content, "Framework block content");

        Ok(Statement::FrameworkBlock {
            name: block_name,
            content,
            attributes,
            location: Some(start_location),
        })
    }

    pub(super) fn parse_tests_block(&mut self) -> Result<Vec<TestCase>, CompilerError> {
        trace!(cursor = self.cursor, token = ?self.current_kind(), "Starting tests block parse");

        self.expect(&TokenKind::Tests)?;
        self.skip_whitespace();

        trace!(cursor = self.cursor, token = ?self.current_kind(), "After Tests token");

        // Expect colon after tests keyword
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        trace!(cursor = self.cursor, token = ?self.current_kind(), "After colon");

        let mut tests = Vec::new();

        // Determine the tests block's indentation level
        let tests_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        trace!(
            indent_level = tests_indent_level,
            "Tests block indent level"
        );

        // Parse test cases until we hit a dedent or EOF
        while !self.is_at_end() {
            trace!(cursor = self.cursor, token = ?self.current_kind(), "Tests block iteration");

            self.skip_whitespace();

            if self.is_at_end() {
                trace!("At end after whitespace");
                break;
            }

            // Check for Dedent that exits the tests block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                trace!(dedent_level, tests_indent_level, "Found Dedent");
                if *dedent_level < tests_indent_level {
                    // This Dedent exits the tests block - DON'T consume it
                    trace!("Dedent exits tests block");
                    break;
                }
                // Dedent at our level or higher - consume it and continue
                trace!("Consuming dedent and continuing");
                self.bump();
                self.skip_whitespace();
            }

            // Skip Indent tokens at our level
            if matches!(self.current_kind(), TokenKind::Indent(level) if *level == tests_indent_level)
            {
                trace!("Skipping Indent at tests level");
                self.bump();
                self.skip_whitespace();
            }

            if self.is_at_end() {
                trace!("At end after indent handling");
                break;
            }

            // Check for end of tests block (top-level declarations or dedent below our level)
            if matches!(
                self.current_kind(),
                TokenKind::Start | TokenKind::Functions | TokenKind::Class | TokenKind::Import
            ) {
                trace!(token = ?self.current_kind(), "Found top-level keyword, exiting tests block");
                break;
            }

            // Check again for dedent that exits the block (can appear after consuming previous dedent)
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < tests_indent_level {
                    trace!(dedent_level, tests_indent_level, "Dedent exits block");
                    break;
                }
            }

            trace!(token = ?self.current_kind(), "About to parse test");

            // Parse test case using the "description": expr = expected format
            let saved_cursor = self.cursor;
            match self.parse_test_case_in_block() {
                Ok(test) => {
                    trace!(description = ?test.description, "Parsed test case");
                    tests.push(test);
                }
                Err(e) => {
                    trace!(error = ?e, "Failed to parse test case, skipping line");
                    // Restore cursor and skip to end of line on error
                    self.cursor = saved_cursor;
                    while !matches!(self.current_kind(), TokenKind::Newline | TokenKind::Eof) {
                        self.bump();
                    }
                }
            }
        }

        debug!(test_count = tests.len(), "Finished parsing tests block");
        Ok(tests)
    }

    /// Parse a test case in a `tests:` block.
    ///
    /// Supports two formats:
    /// - Named:     `"description": expr = expected`
    /// - Anonymous: `expr = expected`
    ///
    /// The `=` here is the assertion operator (TokenKind::Assign), not `==`.
    pub(super) fn parse_test_case_in_block(&mut self) -> Result<TestCase, CompilerError> {
        let start_location = self.current().location.clone();

        // Attempt to parse an optional description string followed by ':'
        let description = if let TokenKind::StringLiteral(desc) = self.current_kind() {
            let desc_text = desc.clone();
            let saved = self.cursor;
            self.bump(); // consume the string literal
            self.skip_whitespace();

            if self.check(&TokenKind::Colon) {
                self.bump(); // consume ':'
                self.skip_whitespace();
                Some(desc_text)
            } else {
                // No colon after the string — this string is not a description;
                // restore cursor and parse the whole line as an anonymous test.
                self.cursor = saved;
                None
            }
        } else {
            None
        };

        // Parse the test expression (left-hand side of the assertion)
        let test_expression = self.parse_expression()?;
        self.skip_whitespace();

        // Expect the assertion `=` operator (TokenKind::Assign, not `==`)
        if !self.check(&TokenKind::Assign) {
            return Err(CompilerError::syntax_error(
                "Expected '=' in test assertion (format: expr = expected)",
                Some("Use 'expr = expected_value' syntax in tests: block".to_string()),
                Some(self.current().location.clone()),
            ));
        }
        self.bump(); // consume '='
        self.skip_whitespace();

        // Parse the expected value expression (right-hand side)
        let expected_value = self.parse_expression()?;

        Ok(TestCase {
            description,
            test_expression,
            expected_value,
            location: Some(start_location),
        })
    }

    /// Parse spec statement: spec "path/to/spec"
    /// AI metadata that links a function to its specification document
    pub(super) fn parse_spec(&mut self) -> Result<Statement, CompilerError> {
        let spec_token = self.expect(&TokenKind::Spec)?;
        self.skip_whitespace();

        if let TokenKind::StringLiteral(path) = self.current_kind() {
            let path = path.clone();
            self.bump();
            return Ok(Statement::Spec {
                path,
                location: Some(spec_token.location),
            });
        }

        Err(CompilerError::Syntax {
            context: Box::new(crate::error::ErrorContext {
                message: "Expected string literal after 'spec'".to_string(),
                location: Some(self.current().location.clone()),
                error_code: Some("SYN100".to_string()),
                severity: crate::error::ErrorSeverity::Error,
                ..Default::default()
            }),
        })
    }

    /// Parse intent statement: intent "description of purpose"
    /// AI metadata that describes a function's purpose in natural language
    pub(super) fn parse_intent(&mut self) -> Result<Statement, CompilerError> {
        let intent_token = self.expect(&TokenKind::Intent)?;
        self.skip_whitespace();

        if let TokenKind::StringLiteral(desc) = self.current_kind() {
            let description = desc.clone();
            self.bump();
            return Ok(Statement::Intent {
                description,
                location: Some(intent_token.location),
            });
        }

        Err(CompilerError::Syntax {
            context: Box::new(crate::error::ErrorContext {
                message: "Expected string literal after 'intent'".to_string(),
                location: Some(self.current().location.clone()),
                error_code: Some("SYN101".to_string()),
                severity: crate::error::ErrorSeverity::Error,
                ..Default::default()
            }),
        })
    }

    /// Parse a build: top-level block that configures compiler behaviour.
    ///
    /// ```text
    /// build:
    ///     rules = true
    /// ```
    ///
    /// The `rules` key accepts `true`, `false`, or the string `"development"`.
    /// When omitted the default value is `true` (rules checking is enabled).
    pub(super) fn parse_build_block(&mut self) -> Result<Statement, CompilerError> {
        let build_token = self.current().clone();
        let location = build_token.location.clone();

        // Consume the 'build' keyword
        self.bump();
        self.skip_whitespace();

        // Expect a colon after 'build'
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip optional newline between 'build:' and the indented body
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Determine the indent level of the block body
        let block_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            1
        };

        // Consume the opening indent token
        if matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }

        // Default: rules checking is enabled
        let mut rules_value: Expression = Expression::Literal(Value::Boolean(true));

        // Parse key = value assignments inside the build block
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // A Dedent back to (or past) the enclosing level ends the block
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < block_level {
                    self.bump();
                    break;
                }
            }

            // Any top-level keyword also signals the end of the build block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Plugins
                    | TokenKind::Source
            ) {
                break;
            }

            match self.current_kind() {
                // The only currently supported key inside build: is 'rules'
                TokenKind::Rules => {
                    self.bump(); // consume 'rules'
                    self.skip_whitespace();
                    self.expect(&TokenKind::Assign)?; // consume '='
                    self.skip_whitespace();

                    rules_value = match self.current_kind() {
                        TokenKind::True => {
                            self.bump();
                            Expression::Literal(Value::Boolean(true))
                        }
                        TokenKind::False => {
                            self.bump();
                            Expression::Literal(Value::Boolean(false))
                        }
                        TokenKind::StringLiteral(s) => {
                            let s = s.clone();
                            self.bump();
                            Expression::Literal(Value::String(s))
                        }
                        _ => {
                            let token = self.current().clone();
                            return Err(CompilerError::parse_error(
                                format!(
                                    "Expected true, false, or a string value for 'build rules', found '{}'",
                                    self.current_kind()
                                ),
                                Some(token.location.clone()),
                                None,
                            ));
                        }
                    };
                }
                TokenKind::Indent(_) | TokenKind::Newline => {
                    self.bump();
                }
                _ => {
                    // Skip unknown properties inside the build block to stay forward-compatible
                    self.bump();
                }
            }
        }

        Ok(Statement::BuildBlock {
            rules_enabled: rules_value,
            location: Some(location),
        })
    }

    /// Parse a source: top-level block with spec and version fields
    /// ```text
    /// source:
    ///     spec "pricing/discount-rules"
    ///     version "abc123"
    /// ```
    pub(super) fn parse_source_block(&mut self) -> Result<Statement, CompilerError> {
        let source_token = self.expect(&TokenKind::Source)?;
        self.skip_whitespace();

        // Expect colon after "source"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        let mut spec_path = String::new();
        let mut version: Option<String> = None;

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Get the indent level
        let block_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            1
        };

        // Consume the initial indent
        if matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }

        // Parse key-value pairs inside the block
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for dedent (end of block)
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < block_level {
                    self.bump();
                    break;
                }
            }

            // Check for top-level keywords that end the source block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Plugins
            ) {
                break;
            }

            match self.current_kind() {
                TokenKind::Spec => {
                    self.bump(); // consume "spec"
                    self.skip_whitespace();
                    if let TokenKind::StringLiteral(path) = self.current_kind() {
                        spec_path = path.clone();
                        self.bump();
                    }
                }
                TokenKind::Identifier(name) if name == "version" => {
                    self.bump(); // consume "version"
                    self.skip_whitespace();
                    if let TokenKind::StringLiteral(ver) = self.current_kind() {
                        version = Some(ver.clone());
                        self.bump();
                    }
                }
                TokenKind::Indent(_) => {
                    self.bump();
                }
                TokenKind::Newline => {
                    self.bump();
                }
                _ => {
                    // Skip unknown tokens inside source block
                    self.bump();
                }
            }
        }

        Ok(Statement::SourceBlock {
            spec_path,
            version,
            location: Some(source_token.location),
        })
    }

    /// Parse a state: block containing state variable declarations
    pub(super) fn parse_state_block(&mut self) -> Result<crate::ast::StateBlock, CompilerError> {
        use crate::ast::{StateBlock, StateDeclaration, StateScope};

        // Consume "state" keyword
        self.expect(&TokenKind::State)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        let mut declarations: Vec<StateDeclaration> = Vec::new();
        let mut computed: Vec<crate::ast::ComputedDeclaration> = Vec::new(); // Populated when computed: blocks are parsed

        // Expect indentation for block body
        if !matches!(self.current_kind(), TokenKind::Indent(_))
            && !matches!(self.current_kind(), TokenKind::Newline)
        {
            return Err(CompilerError::parse_error(
                "Expected indented state declarations after 'state:'".to_string(),
                Some(self.current().location.clone()),
                Some("State declarations must be indented".to_string()),
            ));
        }

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Get the indent level
        let block_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            1 // Default to level 1
        };

        // Consume the initial indent
        if matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }

        // Parse state declarations until we see a dedent or different token
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for dedent (end of block)
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < block_level {
                    self.bump();
                    break;
                }
            }

            // Check for other top-level keywords that end the state block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
            ) {
                break;
            }

            // Parse a state declaration: type name = value
            // Look for a type identifier (integer, string, number, boolean, etc.)
            match self.current_kind() {
                TokenKind::Identifier(type_name) => {
                    let location = self.current().location.clone();
                    let type_str = type_name.clone();
                    self.bump(); // consume type

                    self.skip_whitespace();

                    // Get variable name
                    let var_name = if let TokenKind::Identifier(name) = self.current_kind() {
                        let name = name.clone();
                        self.bump();
                        name
                    } else {
                        return Err(CompilerError::parse_error(
                            "Expected variable name in state declaration".to_string(),
                            Some(self.current().location.clone()),
                            Some(
                                "State declarations must have format: type name = value"
                                    .to_string(),
                            ),
                        ));
                    };

                    self.skip_whitespace();

                    // Expect = sign
                    self.expect(&TokenKind::Assign)?;
                    self.skip_whitespace();

                    // Parse initializer expression
                    let initializer = self.parse_expression()?;

                    // Convert type string to Type
                    let type_ = match type_str.as_str() {
                        "integer" => crate::ast::Type::Integer,
                        "number" => crate::ast::Type::Number,
                        "string" => crate::ast::Type::String,
                        "boolean" => crate::ast::Type::Boolean,
                        other => crate::ast::Type::Object(other.to_string()),
                    };

                    // Check for guard clause on next line
                    // Guard syntax: guard <condition> else "message"
                    let guard = self.try_parse_guard_clause(block_level)?;

                    declarations.push(StateDeclaration {
                        name: var_name,
                        type_,
                        initializer,
                        guard,
                        location: Some(location),
                    });

                    // Skip newline after declaration (if guard didn't consume it)
                    if matches!(self.current_kind(), TokenKind::Newline) {
                        self.bump();
                    }

                    // Check for next indent token
                    if let TokenKind::Indent(level) = self.current_kind() {
                        if *level == block_level {
                            self.bump(); // Continue parsing next declaration
                        } else if *level < block_level {
                            // End of block
                            break;
                        }
                    }
                }
                TokenKind::Newline => {
                    self.bump();
                }
                TokenKind::Indent(_) => {
                    self.bump();
                }
                TokenKind::Dedent(_) => {
                    break;
                }
                TokenKind::Computed => {
                    // Parse computed: block for derived state values
                    let computed_decls = self.parse_computed_block(block_level)?;
                    computed.extend(computed_decls);
                }
                TokenKind::Rules => {
                    // Parse rules: block for state invariants
                    let rules_block = self.parse_rules_block(block_level)?;
                    return Ok(StateBlock {
                        declarations,
                        computed,
                        rules: Some(rules_block),
                        scope: StateScope::App,
                        location: None,
                    });
                }
                _ => {
                    // Unknown token, skip and continue
                    break;
                }
            }
        }

        Ok(StateBlock {
            declarations,
            computed,
            rules: None,            // No rules block present
            scope: StateScope::App, // Default to App scope for top-level state
            location: None,
        })
    }

    /// Parse a rules: block containing state invariant expressions
    pub(super) fn parse_rules_block(
        &mut self,
        parent_block_level: usize,
    ) -> Result<crate::ast::RulesBlock, CompilerError> {
        let location = Some(self.current().location.clone());

        // Consume "rules" keyword
        self.expect(&TokenKind::Rules)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        let mut rules = Vec::new();

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Get the indent level for rules (should be deeper than parent block)
        let rules_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            parent_block_level + 1
        };

        // Parse rule expressions until we leave this indent level
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Skip any initial indent
            if let TokenKind::Indent(level) = self.current_kind() {
                if *level >= rules_level {
                    self.bump();
                } else {
                    break; // Dedent past rules block
                }
            }

            // Check for dedent (end of rules block)
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < rules_level {
                    self.bump();
                    break;
                }
            }

            // Check for top-level keywords that end the rules block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
            ) {
                break;
            }

            // Skip newlines between rules
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
                continue;
            }

            // Parse a rule expression (must be boolean)
            if !matches!(
                self.current_kind(),
                TokenKind::Newline | TokenKind::Indent(_) | TokenKind::Dedent(_)
            ) {
                let rule_expr = self.parse_expression()?;
                rules.push(rule_expr);
            }

            // Skip trailing newline
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
            }

            // Check for next line indent
            if let TokenKind::Indent(level) = self.current_kind() {
                if *level < rules_level {
                    break; // End of rules block
                }
            }
        }

        Ok(crate::ast::RulesBlock { rules, location })
    }

    /// Parse a computed: block inside a state block.
    ///
    /// Syntax:
    /// ```text
    /// computed:
    ///     string fullName
    ///         return firstName + " " + lastName
    /// ```
    ///
    /// Each computed declaration consists of:
    /// - A type keyword (integer, string, number, boolean, or a class name)
    /// - A name identifier
    /// - An indented body containing statements (must end with a `return`)
    pub(super) fn parse_computed_block(
        &mut self,
        parent_block_level: usize,
    ) -> Result<Vec<crate::ast::ComputedDeclaration>, CompilerError> {
        // Consume "computed" keyword
        self.expect(&TokenKind::Computed)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        let mut declarations: Vec<crate::ast::ComputedDeclaration> = Vec::new();

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Determine the indent level for computed declarations
        // (should be one level deeper than the parent state block)
        let computed_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            parent_block_level + 1
        };

        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume matching indent tokens
            if let TokenKind::Indent(level) = self.current_kind() {
                if *level >= computed_level {
                    self.bump();
                } else {
                    break;
                }
            }

            // Dedent exits the computed block
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < computed_level {
                    self.bump();
                    break;
                }
            }

            // Top-level section keywords end the computed block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Rules
            ) {
                break;
            }

            // Skip blank lines
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
                continue;
            }

            // Parse the type name: integer | string | number | boolean | <Identifier>
            let decl_location = self.current().location.clone();
            let type_str = match self.current_kind() {
                TokenKind::Identifier(name) => {
                    let s = name.clone();
                    self.bump();
                    s
                }
                _ => {
                    // Not a type declaration — stop parsing computed block
                    break;
                }
            };

            self.skip_whitespace();

            // Parse the computed value name
            let var_name = match self.current_kind() {
                TokenKind::Identifier(name) => {
                    let n = name.clone();
                    self.bump();
                    n
                }
                _ => {
                    return Err(CompilerError::parse_error(
                        "Expected computed value name after type in computed: block".to_string(),
                        Some(self.current().location.clone()),
                        Some(
                            "Computed declarations must have format: <type> <name> (with indented body)"
                                .to_string(),
                        ),
                    ));
                }
            };

            // Convert type string to AST Type
            let type_ = match type_str.as_str() {
                "integer" => crate::ast::Type::Integer,
                "number" => crate::ast::Type::Number,
                "string" => crate::ast::Type::String,
                "boolean" => crate::ast::Type::Boolean,
                other => crate::ast::Type::Object(other.to_string()),
            };

            // Skip newline before the indented body
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
            }

            // Parse the indented body using parse_block()
            let body = self.parse_block()?;

            if body.is_empty() {
                return Err(CompilerError::parse_error(
                    format!(
                        "Computed value '{}' has an empty body — it must contain a return statement",
                        var_name
                    ),
                    Some(decl_location.clone()),
                    Some("Add a return statement: `return <expression>`".to_string()),
                ));
            }

            declarations.push(crate::ast::ComputedDeclaration {
                name: var_name,
                type_,
                body,
                location: Some(decl_location),
            });

            // Skip trailing newline after each declaration
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
            }

            // Check for indent continuing at same computed level
            if let TokenKind::Indent(level) = self.current_kind() {
                if *level < computed_level {
                    break;
                }
            }
        }

        Ok(declarations)
    }

    /// Parse a top-level `watch` block for reactive state observation.
    ///
    /// Syntax:
    /// ```text
    /// watch count:
    ///     print("Count changed")
    ///
    /// watch firstName lastName:
    ///     print("Name changed")
    /// ```
    ///
    /// The `watch` keyword is followed by one or more state variable names and
    /// then a colon. The indented body is executed whenever any of the named
    /// state variables change.
    pub(super) fn parse_watch_block(&mut self) -> Result<crate::ast::WatchBlock, CompilerError> {
        let location = Some(self.current().location.clone());

        // Consume "watch" keyword
        self.expect(&TokenKind::Watch)?;
        self.skip_whitespace();

        // Parse one or more target variable names before the colon
        let mut targets: Vec<String> = Vec::new();
        loop {
            match self.current_kind() {
                TokenKind::Identifier(name) => {
                    targets.push(name.clone());
                    self.bump();
                    self.skip_whitespace();
                }
                TokenKind::Colon => {
                    // End of target list
                    break;
                }
                _ => {
                    return Err(CompilerError::parse_error(
                        format!(
                            "Expected a state variable name or ':' in watch block, found {:?}",
                            self.current_kind()
                        ),
                        Some(self.current().location.clone()),
                        Some(
                            "Watch blocks must have format: watch <name> [<name> ...]: <body>"
                                .to_string(),
                        ),
                    ));
                }
            }
        }

        if targets.is_empty() {
            return Err(CompilerError::parse_error(
                "Watch block must specify at least one state variable to observe".to_string(),
                Some(self.current().location.clone()),
                Some("Usage: watch <variableName>: <body>".to_string()),
            ));
        }

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline before the body
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Parse the indented body
        let body = self.parse_block()?;

        if body.is_empty() {
            return Err(CompilerError::parse_error(
                format!("Watch block for '{}' has an empty body", targets.join(", ")),
                location.clone(),
                Some("Add at least one statement to the watch block body".to_string()),
            ));
        }

        Ok(crate::ast::WatchBlock {
            targets,
            body,
            location,
        })
    }

    /// Try to parse a guard clause following a state declaration
    /// Guard syntax: guard <condition> else "message"
    /// Returns None if no guard is present, or the parsed GuardClause
    pub(super) fn try_parse_guard_clause(
        &mut self,
        block_level: usize,
    ) -> Result<Option<crate::ast::GuardClause>, CompilerError> {
        // Save cursor position to revert if no guard found
        let start_cursor = self.cursor;

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Check for deeper indentation (guard should be indented more than the declaration)
        if let TokenKind::Indent(level) = self.current_kind() {
            if *level > block_level {
                self.bump(); // Consume indent

                // Check for Guard keyword
                if matches!(self.current_kind(), TokenKind::Guard) {
                    let guard_location = self.current().location.clone();
                    self.bump(); // Consume "guard"
                    self.skip_whitespace();

                    // Parse the condition expression
                    let condition = self.parse_expression()?;
                    self.skip_whitespace();

                    // Expect "else" keyword
                    if !matches!(self.current_kind(), TokenKind::Else) {
                        return Err(CompilerError::parse_error(
                            "Expected 'else' after guard condition".to_string(),
                            Some(self.current().location.clone()),
                            Some("Guard syntax: guard <condition> else \"message\"".to_string()),
                        ));
                    }
                    self.bump(); // Consume "else"
                    self.skip_whitespace();

                    // Parse the error message (string literal)
                    let error_message = if let TokenKind::StringLiteral(s) = self.current_kind() {
                        let msg = s.clone();
                        self.bump();
                        msg
                    } else {
                        return Err(CompilerError::parse_error(
                            "Expected string literal for guard error message".to_string(),
                            Some(self.current().location.clone()),
                            Some("Guard syntax: guard <condition> else \"message\"".to_string()),
                        ));
                    };

                    return Ok(Some(crate::ast::GuardClause {
                        condition,
                        error_message,
                        location: Some(guard_location),
                    }));
                }
            }
        }

        // No guard found, revert cursor position
        self.cursor = start_cursor;
        Ok(None)
    }

    /// Parse a plugins: block containing plugin names
    ///
    /// Syntax:
    /// ```text
    /// plugins:
    ///     frame.ui
    ///     frame.data
    /// ```
    pub(super) fn parse_plugins_block(&mut self) -> Result<Vec<String>, CompilerError> {
        // Consume "plugins" keyword
        self.expect(&TokenKind::Plugins)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        let mut plugin_names: Vec<String> = Vec::new();

        // Expect indentation for block body
        if !matches!(self.current_kind(), TokenKind::Indent(_))
            && !matches!(self.current_kind(), TokenKind::Newline)
        {
            return Err(CompilerError::parse_error(
                "Expected indented plugin names after 'plugins:'".to_string(),
                Some(self.current().location.clone()),
                Some("Plugin names must be indented. Example:\nplugins:\n    frame.ui".to_string()),
            ));
        }

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Get the indent level
        let block_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            1 // Default to level 1
        };

        // Consume the initial indent
        if matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }

        // Parse plugin names until we see a dedent or different token
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for dedent (end of block)
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < block_level {
                    self.bump();
                    break;
                }
            }

            // Check for other top-level keywords that end the plugins block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Plugins
            ) {
                break;
            }

            // Parse a plugin name: identifier.identifier (e.g., frame.ui)
            match self.current_kind() {
                TokenKind::Identifier(first_part) => {
                    let mut plugin_name = first_part.clone();
                    self.bump();
                    self.skip_whitespace();

                    // Check for dot notation (plugin names must have dots)
                    if self.eat(&TokenKind::Dot) {
                        self.skip_whitespace();
                        let second_part = self.expect_identifier()?;
                        plugin_name.push('.');
                        plugin_name.push_str(&second_part.text);
                    }

                    debug!(plugin_name = %plugin_name, "Parsed plugin name");
                    plugin_names.push(plugin_name);
                }
                TokenKind::Newline => {
                    self.bump();
                }
                TokenKind::Indent(_) => {
                    self.bump();
                }
                TokenKind::Dedent(_) => {
                    break;
                }
                _ => {
                    // Unknown token, end the block
                    break;
                }
            }
        }

        Ok(plugin_names)
    }

    /// Check if a name looks like a framework plugin name (e.g., "frame.ui", "frame.data")
    /// Plugin names contain dots but don't end with ".cln" (which would be a file import)
    pub(super) fn is_plugin_name(name: &str) -> bool {
        name.contains('.') && !name.ends_with(".cln")
    }

    /// Parse an external function block
    ///
    /// External blocks declare WASM imports (functions provided by the host runtime).
    ///
    /// Syntax:
    /// ```clean
    /// external:
    ///     return_type function_name(params)
    ///
    /// external "module_name":
    ///     return_type function_name(params)
    /// ```
    pub(super) fn parse_external_block(
        &mut self,
    ) -> Result<Vec<crate::ast::ExternalFunction>, CompilerError> {
        let start_location = self.current().location.clone();

        // Consume "external" keyword
        self.bump();
        self.skip_whitespace();

        // Optional module name: external "http":
        let module = if let TokenKind::StringLiteral(s) = self.current_kind() {
            let m = s.clone();
            self.bump();
            self.skip_whitespace();
            m
        } else {
            "env".to_string() // Default WASM import module
        };

        // Consume ":"
        if !self.eat(&TokenKind::Colon) {
            return Err(CompilerError::parse_error(
                "Expected ':' after 'external' or 'external \"module\"'".to_string(),
                Some(self.current().location.clone()),
                Some("External blocks must end with a colon. Example:\nexternal:\n    string get_value()".to_string()),
            ));
        }
        self.skip_whitespace();

        let mut externals: Vec<crate::ast::ExternalFunction> = Vec::new();

        // Expect indentation for block body
        if !matches!(self.current_kind(), TokenKind::Indent(_))
            && !matches!(self.current_kind(), TokenKind::Newline)
        {
            return Err(CompilerError::parse_error(
                "Expected indented function declarations after 'external:'".to_string(),
                Some(self.current().location.clone()),
                Some("External function declarations must be indented. Example:\nexternal:\n    string get_value()".to_string()),
            ));
        }

        // Skip newline if present
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Get the indent level
        let block_level = if let TokenKind::Indent(level) = self.current_kind() {
            *level
        } else {
            1 // Default to level 1
        };

        // Consume the initial indent
        if matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }

        // Parse external function declarations until we see a dedent or different token
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for dedent (end of block)
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level < block_level {
                    self.bump();
                    break;
                }
            }

            // Check for other top-level keywords that end the external block
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Plugins
            ) {
                break;
            }

            // Check for another identifier at top level (might be start of next block)
            if let TokenKind::Identifier(name) = self.current_kind() {
                // Check if it's followed by a colon or is a known top-level pattern
                if matches!(self.peek_kind(), Some(&TokenKind::Colon))
                    || name == "external"
                    || name == "start"
                {
                    break;
                }
            }

            // Parse an external function declaration: return_type name(params)
            match self.current_kind() {
                TokenKind::Identifier(type_name) => {
                    // Check if this is "void" return type
                    let return_type = if type_name == "void" {
                        self.bump();
                        crate::ast::Type::Void
                    } else {
                        self.parse_type()?
                    };

                    self.skip_whitespace();

                    // Get function name
                    let func_name = self.expect_identifier()?.text.clone();
                    self.skip_whitespace();

                    // Parse parameters using external format: type name (not name: type)
                    self.expect(&TokenKind::LeftParen)?;
                    let parameters = self.parse_external_parameter_list()?;

                    debug!(
                        func_name = %func_name,
                        return_type = ?return_type,
                        param_count = parameters.len(),
                        module = %module,
                        "Parsed external function declaration"
                    );

                    externals.push(crate::ast::ExternalFunction {
                        name: func_name,
                        parameters,
                        return_type,
                        module: module.clone(),
                        location: Some(start_location.clone()),
                    });
                }
                TokenKind::Newline => {
                    self.bump();
                }
                TokenKind::Indent(_) => {
                    self.bump();
                }
                TokenKind::Dedent(_) => {
                    break;
                }
                _ => {
                    // Unknown token, end the block
                    break;
                }
            }
        }

        debug!(
            external_count = externals.len(),
            module = %module,
            "Finished parsing external block"
        );

        Ok(externals)
    }

    /// Parse external function parameter list
    ///
    /// External functions use the C-style format: `type name`, not Clean's `name: type`.
    /// Example: `string field_name, integer count` not `field_name: string, count: integer`
    pub(super) fn parse_external_parameter_list(
        &mut self,
    ) -> Result<Vec<crate::ast::Parameter>, CompilerError> {
        let mut parameters = Vec::new();

        self.skip_whitespace();

        while !self.check(&TokenKind::RightParen) && !self.is_at_end() {
            parameters.push(self.parse_external_parameter()?);
            self.skip_whitespace();

            if !self.eat(&TokenKind::Comma) {
                break;
            }
            self.skip_whitespace();
        }

        self.expect(&TokenKind::RightParen)?;

        Ok(parameters)
    }

    /// Parse a single external function parameter: `type name`
    pub(super) fn parse_external_parameter(
        &mut self,
    ) -> Result<crate::ast::Parameter, CompilerError> {
        // Parse type first
        let type_ = self.parse_type()?;
        self.skip_whitespace();

        // Parse parameter name
        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();

        Ok(crate::ast::Parameter {
            name,
            type_,
            default_value: None,
        })
    }
}
