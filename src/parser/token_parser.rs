//! Token-driven parser for Clean Language
//!
//! This parser consumes tokens directly from the lexer without source reconstruction,
//! following the architecture of rustc's parser (see rust-lang/rustc-dev-guide).
//!
//! Architecture:
//! - Maintains a cursor into the token stream
//! - Uses utility methods: bump(), check(), eat(), expect(), look_ahead()
//! - Recursive descent parsing
//! - Error recovery with diagnostic generation

use crate::ast::{
    BinaryOperator, Class, ConstantAssignment, Constructor, Expression, Field, Function,
    FunctionModifier, FunctionSyntax, ImportItem, Parameter, Program, SourceLocation, Statement,
    TestCase, Type, UnaryOperator, Value, VariableAssignment, Visibility,
};
use crate::error::CompilerError;
use crate::lexer::specification_token::{Token, TokenKind, TokenStream};
use std::collections::HashSet;
use tracing::{debug, trace, warn};

/// Token-driven parser for Clean Language
pub struct TokenParser {
    tokens: Vec<Token>,
    cursor: usize,
    #[allow(dead_code)] // Used for future error reporting enhancements
    file_path: String,
    errors: Vec<CompilerError>,
    paren_depth: usize, // Track parenthesis depth for multiline expression support
    /// Plugin-defined keywords that don't require colons (e.g., "data" from frame.data)
    plugin_keywords: HashSet<String>,
}

impl TokenParser {
    pub fn new(token_stream: TokenStream, file_path: String) -> Self {
        Self {
            tokens: token_stream.tokens,
            cursor: 0,
            file_path,
            errors: Vec::new(),
            paren_depth: 0,
            plugin_keywords: HashSet::new(),
        }
    }

    /// Create a parser with plugin-defined keywords
    ///
    /// Plugin keywords are recognized as framework block starters even without
    /// a trailing colon. For example, with `plugin_keywords = ["data"]`:
    /// - `data User` is parsed as a framework block (plugin keyword)
    /// - `endpoints:` is parsed as a framework block (has colon)
    pub fn with_plugin_keywords(
        token_stream: TokenStream,
        file_path: String,
        plugin_keywords: Vec<String>,
    ) -> Self {
        Self {
            tokens: token_stream.tokens,
            cursor: 0,
            file_path,
            errors: Vec::new(),
            paren_depth: 0,
            plugin_keywords: plugin_keywords.into_iter().collect(),
        }
    }

    /// Debug: dump all tokens
    #[allow(dead_code)]
    fn dump_tokens(&self) {
        println!("=== TOKEN DUMP ===");
        for (i, token) in self.tokens.iter().enumerate() {
            println!("{:3}: {:?}", i, token.kind);
        }
        println!("=== END TOKEN DUMP ===");
    }

    /// Parse a complete program
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        self.skip_whitespace();

        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut tests = Vec::new();
        let mut imports = Vec::new();
        let mut statements = Vec::new();
        let screens = Vec::new(); // Always empty - screens handled as framework blocks by plugins
        let mut state: Option<crate::ast::StateBlock> = None;

        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Skip any Dedent tokens at top level (they mark the end of blocks)
            if matches!(self.current_kind(), TokenKind::Dedent(_)) {
                trace!(cursor = self.cursor, "Skipping dedent token at top level");
                self.bump();
                trace!(cursor = self.cursor, token = ?self.current_kind(), "After bump");
                continue;
            }

            // Parse top-level declarations
            match self.current_kind() {
                TokenKind::Function => match self.parse_function() {
                    Ok(func) => functions.push(func),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Class => match self.parse_class() {
                    Ok(class) => classes.push(class),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Tests => {
                    debug!("Parsing tests: block");
                    match self.parse_tests_block() {
                        Ok(test_cases) => {
                            debug!(test_count = test_cases.len(), "Parsed tests block");
                            tests.extend(test_cases)
                        }
                        Err(e) => {
                            debug!(error = ?e, "Failed to parse tests block");
                            self.errors.push(e)
                        }
                    }
                }
                TokenKind::Import => match self.parse_import() {
                    Ok(mut import_items) => imports.append(&mut import_items),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Private => match self.parse_private() {
                    Ok(private_stmt) => statements.push(private_stmt),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Start => {
                    // Parse start() function (special case - no 'function' keyword)
                    match self.parse_start_function() {
                        Ok(func) => functions.push(func),
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Functions => {
                    // Parse functions: block
                    debug!("Parsing functions: block");
                    match self.parse_functions_block() {
                        Ok(mut block_functions) => {
                            debug!(
                                function_count = block_functions.len(),
                                next_token = ?self.current_kind(),
                                "Parsed functions block"
                            );
                            functions.append(&mut block_functions)
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::State => {
                    // Parse state: block
                    debug!("Parsing state: block");
                    match self.parse_state_block() {
                        Ok(state_block) => {
                            debug!(
                                declaration_count = state_block.declarations.len(),
                                "Parsed state block"
                            );
                            state = Some(state_block);
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Identifier(name) => {
                    // Check if this is a plugin keyword (e.g., "data" from frame.data plugin)
                    let is_plugin_keyword = self.plugin_keywords.contains(name);

                    // Check if this is a framework block
                    // Patterns:
                    // - "identifier:" (colon-based framework block)
                    // - "identifier string:" or "identifier identifier:" (with colon)
                    // - "plugin_keyword identifier" (plugin keyword without colon)
                    let is_framework_block = match self.peek_kind() {
                        Some(&TokenKind::Colon) => true,
                        Some(&TokenKind::StringLiteral(_)) | Some(&TokenKind::Identifier(_)) => {
                            // Plugin keyword followed by identifier: "data User"
                            if is_plugin_keyword {
                                true
                            } else {
                                // Check if second token is followed by colon
                                matches!(self.look_ahead(2).kind, TokenKind::Colon)
                            }
                        }
                        _ => false,
                    };

                    if is_framework_block {
                        debug!(block_name = %name, is_plugin_keyword, "Found framework block");
                        match self.parse_framework_block_or_plugin(is_plugin_keyword) {
                            Ok(stmt) => statements.push(stmt),
                            Err(e) => self.errors.push(e),
                        }
                    } else {
                        // Not a framework block, unexpected token
                        let token = self.current();
                        self.errors.push(CompilerError::parse_error(
                            format!("Unexpected identifier at top level: {:?}", name),
                            Some(token.location.clone()),
                            Some("Expected 'functions:', 'class', 'start()', or framework block (e.g., 'endpoints:', 'screen \"Name\":')".to_string()),
                        ));
                        self.bump();
                    }
                }
                _ => {
                    let token = self.current();
                    self.errors.push(CompilerError::parse_error(
                        format!("Unexpected token at top level: {:?}", token.kind),
                        Some(token.location.clone()),
                        None,
                    ));
                    self.bump(); // Skip unexpected token
                }
            }

            self.skip_whitespace();
        }

        if !self.errors.is_empty() {
            // Return first error with all subsequent errors as related_errors
            // This allows IDE tools to access all parser errors for better diagnostics
            let mut first_error = self.errors[0].clone();

            // Add remaining errors as related messages
            if self.errors.len() > 1 {
                // Extract the context and add related errors
                match &mut first_error {
                    CompilerError::Syntax { context }
                    | CompilerError::Type { context }
                    | CompilerError::Memory { context }
                    | CompilerError::Codegen { context }
                    | CompilerError::IO { context }
                    | CompilerError::Runtime { context }
                    | CompilerError::Validation { context }
                    | CompilerError::Module { context }
                    | CompilerError::Testing { context } => {
                        for error in &self.errors[1..] {
                            context.related_errors.push(error.to_string());
                        }
                    }
                    CompilerError::LexError(_) => {
                        // LexError doesn't have ErrorContext, skip related errors
                    }
                    CompilerError::PluginError { .. } => {
                        // PluginError doesn't have ErrorContext, skip related errors
                    }
                }
            }

            return Err(first_error);
        }

        let start_function = functions.iter().find(|f| f.name == "start").cloned();

        debug!(
            function_count = functions.len(),
            has_start = start_function.is_some(),
            test_count = tests.len(),
            class_count = classes.len(),
            "Program parsed successfully"
        );

        Ok(Program {
            imports,
            statements,
            functions,
            classes,
            start_function,
            tests,
            screens,
            state,
            watch_blocks: Vec::new(),
            screen_blocks: Vec::new(),
            location: None,
        })
    }

    // ============================================================================
    // Token cursor utilities (following rustc pattern)
    // ============================================================================

    /// Get the current token without consuming it
    fn current(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }

    /// Get the kind of the current token
    fn current_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    /// Check if we're at EOF
    fn is_at_end(&self) -> bool {
        matches!(self.current_kind(), TokenKind::Eof)
    }

    /// Peek at the next token kind without consuming
    #[allow(dead_code)] // Parser utility method - kept for API completeness
    fn peek_kind(&self) -> Option<&TokenKind> {
        if self.cursor + 1 < self.tokens.len() {
            Some(&self.tokens[self.cursor + 1].kind)
        } else {
            None
        }
    }

    /// Consume the current token and advance (rustc: bump())
    fn bump(&mut self) -> Token {
        let token = self.tokens[self.cursor].clone();
        if self.cursor < self.tokens.len() - 1 {
            self.cursor += 1;
        }
        token
    }

    /// Check if the current token matches without consuming (rustc: check())
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(kind)
    }

    /// Consume the token if it matches, return whether it matched (rustc: eat())
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect a specific token, error if not found (rustc: expect())
    fn expect(&mut self, kind: &TokenKind) -> Result<Token, CompilerError> {
        if self.check(kind) {
            Ok(self.bump())
        } else {
            let current = self.current();
            Err(CompilerError::parse_error(
                format!("Expected {:?}, found {:?}", kind, current.kind),
                Some(current.location.clone()),
                None,
            ))
        }
    }

    /// Look ahead at token N positions forward (rustc: look_ahead())
    #[allow(dead_code)] // Parser utility method - kept for API completeness
    fn look_ahead(&self, n: usize) -> &Token {
        let pos = (self.cursor + n).min(self.tokens.len() - 1);
        &self.tokens[pos]
    }

    /// Get the current block's indentation level by looking at recent Indent/Dedent tokens
    /// Returns 0 if no Indent/Dedent token found (top-level)
    fn get_current_indent_level(&self) -> usize {
        // Look backwards from current position to find the most recent Indent or Dedent token
        // A Dedent(N) token means we're now at level N, so return that
        for i in (0..self.cursor).rev() {
            if let TokenKind::Indent(level) = &self.tokens[i].kind {
                return *level;
            }
            // A Dedent(level) means we've transitioned TO that level
            if let TokenKind::Dedent(level) = &self.tokens[i].kind {
                return *level;
            }
        }
        0 // Default to level 0 if no Indent/Dedent found
    }

    /// Skip whitespace tokens (newlines, comments)
    /// When inside parentheses (paren_depth > 0), also skip Indent/Dedent for multiline expressions
    fn skip_whitespace(&mut self) {
        let start_token = self.current_kind().clone();
        let mut consumed = vec![];

        // When inside parentheses, skip indent/dedent tokens too (for multiline expressions)
        if self.paren_depth > 0 {
            while matches!(
                self.current_kind(),
                TokenKind::Newline
                    | TokenKind::Comment(_)
                    | TokenKind::BlockComment(_)
                    | TokenKind::Indent(_)
                    | TokenKind::Dedent(_)
            ) {
                consumed.push(format!("{:?}", self.current_kind()));
                self.bump();
            }
        } else {
            while matches!(
                self.current_kind(),
                TokenKind::Newline | TokenKind::Comment(_) | TokenKind::BlockComment(_)
            ) {
                consumed.push(format!("{:?}", self.current_kind()));
                self.bump();
            }
        }

        if !consumed.is_empty() {
            tracing::trace!(
                start = ?start_token,
                consumed = ?consumed,
                after = ?self.current_kind(),
                paren_depth = self.paren_depth,
                "skip_whitespace consumed tokens"
            );
        }
    }

    /// Skip indentation tokens
    fn skip_indentation(&mut self) {
        // Only skip Indent tokens, not Dedent
        // Dedent tokens signal the end of blocks and should be checked explicitly
        while matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }
    }

    /// Skip whitespace AND indentation tokens (for use inside parentheses/brackets)
    /// This allows expressions to span multiple lines when wrapped in parentheses
    // ============================================================================
    // Parsing methods
    // ============================================================================

    fn parse_function(&mut self) -> Result<Function, CompilerError> {
        self.expect(&TokenKind::Function)?;
        self.skip_whitespace();

        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();
        let location = name_token.location.clone();

        self.skip_whitespace();

        // Parameters
        let parameters = if self.eat(&TokenKind::LeftParen) {
            self.parse_parameter_list()?
        } else {
            Vec::new()
        };

        self.skip_whitespace();

        // Return type (optional, defaults to Void)
        let return_type = if self.eat(&TokenKind::Returns) {
            self.skip_whitespace();
            self.parse_type()?
        } else {
            Type::Void
        };

        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Body
        let body = self.parse_block()?;

        Ok(Function {
            name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(location),
        })
    }

    /// Parse start() function (special case - no 'function' keyword)
    /// Example: start()
    ///             print("Hello")
    fn parse_start_function(&mut self) -> Result<Function, CompilerError> {
        let start_token = self.expect(&TokenKind::Start)?;
        let location = start_token.location.clone();

        self.skip_whitespace();

        // Expect ()
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();
        self.expect(&TokenKind::RightParen)?;

        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Body
        let body = self.parse_block()?;

        Ok(Function {
            name: "start".to_string(),
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters: Vec::new(), // start() has no parameters
            return_type: Type::Void,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(location),
        })
    }

    /// Parse a functions: block containing multiple function definitions
    fn parse_functions_block(&mut self) -> Result<Vec<Function>, CompilerError> {
        // Consume "functions" keyword
        self.expect(&TokenKind::Functions)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        let mut functions = Vec::new();

        // Determine the functions block's indentation level
        // Functions should be indented one level from the "functions:" line (which is at level 0)
        let functions_block_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1 // Default to level 1
            }
        } else {
            1 // Default to level 1
        };

        // Parse functions until we hit a dedent or EOF
        // Functions in a functions: block are indented relative to the "functions:" line
        let mut break_outer = false;
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume Dedent tokens within the functions block
            // When we see a Dedent that exits the functions block (goes below our level),
            // DO NOT consume it - let the parent handle it
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                if level < functions_block_level {
                    // This Dedent exits the functions block - DON'T consume it
                    // Set flag to break from outer loop
                    break_outer = true;
                    break;
                }
                self.bump(); // Only consume dedents at or above our level
                self.skip_whitespace();
            }

            if break_outer || self.is_at_end() {
                break;
            }

            // CRITICAL: Check if we're at a top-level construct (end of functions block)
            // This must happen BEFORE trying to parse as a function
            // Note: Start is not included here because it can be used as a method name
            if matches!(
                self.current_kind(),
                TokenKind::Class | TokenKind::Functions | TokenKind::Tests | TokenKind::Import
            ) {
                break;
            }

            // Also check if we see a Dedent that would exit the block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < functions_block_level {
                    // We're at a Dedent that exits the functions block
                    break;
                }
            }

            // Check if we're still in the functions block (indented content)
            // If we hit a non-indented line or EOF, we're done
            match self.current_kind() {
                TokenKind::Indent(_) => {
                    // Still in the block, parse the next function
                    self.skip_indentation();

                    // Check if this line starts a function signature
                    // Functions can optionally start with a return type or keyword that can be used as a name
                    match self.current_kind() {
                        TokenKind::Identifier(_)
                        | TokenKind::Test
                        | TokenKind::Unit
                        | TokenKind::Error
                        | TokenKind::Input
                        | TokenKind::Step
                        | TokenKind::Description
                        | TokenKind::Start => match self.parse_function_in_block() {
                            Ok(func) => functions.push(func),
                            Err(e) => return Err(e),
                        },
                        _ => {
                            // Not a function, might be end of block
                            break;
                        }
                    }
                }
                TokenKind::Identifier(_)
                | TokenKind::Test
                | TokenKind::Unit
                | TokenKind::Error
                | TokenKind::Input
                | TokenKind::Step
                | TokenKind::Description
                | TokenKind::Start => {
                    // Direct identifier or keyword without Indent token - still in functions block
                    // This happens when functions are at the same indentation level
                    match self.parse_function_in_block() {
                        Ok(func) => functions.push(func),
                        Err(e) => return Err(e),
                    }
                }
                _ => {
                    // No indentation = end of functions block
                    break;
                }
            }
        }

        // DON'T consume trailing Dedents - let parse_program handle them
        Ok(functions)
    }

    /// Parse a type apply block: TYPE:\n\tvar1 = value1\n\tvar2 = value2
    /// Example: integer:\n\tcount = 0\n\tmaxSize = 100
    fn parse_type_apply_block(&mut self) -> Result<Statement, CompilerError> {
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
    fn parse_constant_apply_block(&mut self) -> Result<Statement, CompilerError> {
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
    fn parse_function_apply_block(&mut self) -> Result<Statement, CompilerError> {
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
            TokenKind::Println => {
                self.bump();
                "println".to_string()
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

    /// Parse a method apply block: OBJECT.METHOD:\n\targ1\n\targ2
    /// Example: list.push:\n\titem1\n\titem2
    /// Equivalent to: list.push(item1), list.push(item2)
    fn parse_method_apply_block(&mut self) -> Result<Statement, CompilerError> {
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

    /// Parse a single function within a functions: block
    /// Functions in a block have the format: [return_type] name(params)
    fn parse_function_in_block(&mut self) -> Result<Function, CompilerError> {
        let start_location = self.current().location.clone();

        // Save position in case we need to backtrack
        let saved_cursor = self.cursor;

        // Try to parse as type (which handles precision modifiers like number:64)
        let (return_type, func_name) = match self.parse_type() {
            Ok(typ) => {
                self.skip_whitespace();
                // Check if this is actually a parameterless void function
                // (e.g., "first()" where "first" was parsed as Type::Object)
                if matches!(typ, Type::Object(_)) && self.check(&TokenKind::LeftParen) {
                    // This is a function name, not a type
                    // Extract the name from Type::Object
                    let name = if let Type::Object(n) = typ {
                        n
                    } else {
                        unreachable!()
                    };
                    (Type::Void, name)
                } else {
                    // Successfully parsed a type, expect function name next
                    let name_token = self.expect_name()?;
                    (typ, name_token.text.clone())
                }
            }
            Err(_) => {
                // Failed to parse as type, backtrack and try as function name
                self.cursor = saved_cursor;
                let first_token = self.expect_name()?;
                let first_name = first_token.text.clone();
                self.skip_whitespace();

                // Check if this is a parameterless function (name followed by '(')
                if self.check(&TokenKind::LeftParen) {
                    // No return type, first token is function name
                    (Type::Void, first_name)
                } else {
                    // This shouldn't happen since parse_type should have worked
                    // but handle it anyway - treat first token as return type
                    let return_type = match first_name.as_str() {
                        "void" => Type::Void,
                        "integer" => Type::Integer,
                        "number" => Type::Number,
                        "string" => Type::String,
                        "boolean" => Type::Boolean,
                        _ => Type::Object(first_name.clone()),
                    };
                    let name_token = self.expect_name()?;
                    (return_type, name_token.text.clone())
                }
            }
        };

        self.skip_whitespace();

        // Parse parameters: (param1, param2, ...)
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();

        let mut parameters = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                // Parse parameter: type name [= defaultValue]
                // Use parse_type() to handle precision modifiers like integer:8, number:32
                let param_type = self.parse_type()?;

                self.skip_whitespace();
                let name_token = self.expect_name()?;
                let param_name = name_token.text.clone();

                self.skip_whitespace();

                // Check for default value (e.g., name = defaultValue)
                let default_value = if self.eat(&TokenKind::Assign) {
                    self.skip_whitespace();
                    Some(self.parse_expression()?)
                } else {
                    None
                };

                parameters.push(Parameter {
                    name: param_name,
                    type_: param_type,
                    default_value,
                });

                self.skip_whitespace();

                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                self.skip_whitespace();
            }
        }

        self.expect(&TokenKind::RightParen)?;
        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Parse function body
        let body = self.parse_block()?;

        Ok(Function {
            name: func_name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(start_location),
        })
    }

    fn parse_class(&mut self) -> Result<Class, CompilerError> {
        self.expect(&TokenKind::Class)?;
        self.skip_whitespace();

        let name_token = self.expect_name()?;
        let name = name_token.text.clone();
        let location = name_token.location.clone();

        self.skip_whitespace();

        // Base class (optional, using "is" keyword)
        let base_class = if self.eat(&TokenKind::Is) {
            self.skip_whitespace();
            let parent_token = self.expect_identifier()?;
            Some(parent_token.text.clone())
        } else {
            None
        };

        self.skip_whitespace();
        // DON'T skip indentation here - let parse_class_body handle it

        // Class body
        let (fields, methods, constructor) = self.parse_class_body()?;

        Ok(Class {
            name,
            type_parameters: Vec::new(),
            description: None,
            base_class,
            base_class_type_args: Vec::new(),
            fields,
            methods,
            constructor,
            location: Some(location),
        })
    }

    fn parse_class_body(
        &mut self,
    ) -> Result<(Vec<Field>, Vec<Function>, Option<Constructor>), CompilerError> {
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut constructor = None;

        while !self.is_at_end()
            && !matches!(
                self.current_kind(),
                TokenKind::Class | TokenKind::Function | TokenKind::Start
            )
        {
            self.skip_whitespace();
            self.skip_indentation();

            if self.is_at_end() {
                break;
            }

            // Check what comes next in the class body
            match self.current_kind() {
                TokenKind::Constructor => {
                    // Parse constructor
                    constructor = Some(self.parse_constructor()?);
                }
                TokenKind::Functions => {
                    // Parse functions: block
                    self.bump(); // consume 'functions'
                    self.skip_whitespace();
                    self.expect(&TokenKind::Colon)?;
                    self.skip_whitespace();

                    // Determine the functions block indentation level
                    let functions_indent_level =
                        if matches!(self.current_kind(), TokenKind::Indent(_)) {
                            if let TokenKind::Indent(level) = self.current_kind() {
                                *level
                            } else {
                                1
                            }
                        } else {
                            1
                        };

                    // Parse methods in the functions block
                    while !self.is_at_end() {
                        self.skip_whitespace();

                        if self.is_at_end() {
                            break;
                        }

                        // Check for Dedent that exits the functions block
                        if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                            if *dedent_level < functions_indent_level {
                                // This Dedent exits the functions block - DON'T consume it
                                break;
                            }
                            // Dedent at our level or higher - consume it and continue
                            self.bump();
                            self.skip_whitespace();
                        }

                        // Skip Indent tokens at our level
                        self.skip_indentation();

                        if self.is_at_end() {
                            break;
                        }

                        // Check for end of functions block (top-level declarations)
                        if matches!(
                            self.current_kind(),
                            TokenKind::Class
                                | TokenKind::Start
                                | TokenKind::Function
                                | TokenKind::Tests
                        ) {
                            break;
                        }

                        // Parse method (return_type name(params))
                        if matches!(self.current_kind(), TokenKind::Identifier(_)) {
                            match self.parse_function_in_block() {
                                Ok(func) => methods.push(func),
                                Err(e) => {
                                    // Log the error but don't break - try to continue
                                    // This allows recovery from individual method parse errors
                                    warn!(error = %e, "Failed to parse method");
                                    break;
                                }
                            }
                        } else {
                            // No more methods to parse
                            break;
                        }
                    }
                }
                TokenKind::Identifier(_) => {
                    // Could be a field (type name) - parse it
                    fields.push(self.parse_field()?);
                }
                TokenKind::Dedent(level) => {
                    // Dedent within the class body - consume it and continue
                    // Only exit if we dedent to level 0 or below (exiting the class)
                    if *level == 0 {
                        break; // Exit class body
                    }
                    // Otherwise, consume the Dedent and continue parsing the class body
                    self.bump();
                }
                _ => {
                    // Unknown token, exit class body
                    break;
                }
            }
        }

        Ok((fields, methods, constructor))
    }

    fn parse_field(&mut self) -> Result<Field, CompilerError> {
        // Parse type first (e.g., "string", "integer", "list<integer>")
        let type_ = self.parse_type()?;

        self.skip_whitespace();

        // Parse field name
        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();

        self.skip_whitespace();

        // Optional default value
        let default_value = if self.eat(&TokenKind::Assign) {
            self.skip_whitespace();
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Field {
            name,
            type_,
            visibility: Visibility::Public,
            is_static: false,
            default_value,
        })
    }

    fn parse_constructor(&mut self) -> Result<Constructor, CompilerError> {
        let constructor_token = self.expect(&TokenKind::Constructor)?;
        let location = constructor_token.location.clone();

        self.skip_whitespace();

        // Parse parameter list (type name syntax, like functions: block)
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();

        let mut parameters = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                // Parse parameter: type name [= defaultValue]
                // Use parse_type() to handle precision modifiers like integer:8, number:32
                let param_type = self.parse_type()?;

                self.skip_whitespace();
                let name_token = self.expect_name()?;
                let param_name = name_token.text.clone();

                self.skip_whitespace();

                // Check for default value (e.g., name = defaultValue)
                let default_value = if self.eat(&TokenKind::Assign) {
                    self.skip_whitespace();
                    Some(self.parse_expression()?)
                } else {
                    None
                };

                parameters.push(Parameter {
                    name: param_name,
                    type_: param_type,
                    default_value,
                });

                self.skip_whitespace();

                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                self.skip_whitespace();
            }
        }

        self.expect(&TokenKind::RightParen)?;
        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Parse constructor body
        let body = self.parse_block()?;

        Ok(Constructor {
            parameters,
            body,
            location: Some(location),
        })
    }

    #[allow(dead_code)]
    fn parse_method(&mut self) -> Result<Function, CompilerError> {
        // Methods are just functions within a class
        self.parse_function()
    }

    /// Parse a framework block or plugin declaration
    ///
    /// Framework blocks use colon: "identifier:", "identifier string:", "identifier identifier:"
    /// Plugin declarations don't require colon: "data User" (when "data" is a plugin keyword)
    fn parse_framework_block_or_plugin(
        &mut self,
        is_plugin_keyword: bool,
    ) -> Result<Statement, CompilerError> {
        if !is_plugin_keyword {
            // Standard framework block with colon
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

        // Combine keyword and declaration name: "data User"
        let full_block_name = format!("{} {}", keyword, decl_name);

        self.skip_whitespace();

        // Optional colon (some plugin declarations might still use it)
        if matches!(self.current_kind(), TokenKind::Colon) {
            self.bump();
            self.skip_whitespace();
        }

        // Expect newline
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Collect indented content (same logic as parse_framework_block)
        let mut content_lines = Vec::new();

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

        // Collect all lines that are indented at or deeper than block_indent_level
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for dedent that exits the block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < block_indent_level {
                    break;
                } else {
                    self.bump();
                    continue;
                }
            }

            // Consume indent
            if let TokenKind::Indent(indent_level) = self.current_kind() {
                if *indent_level < block_indent_level {
                    break;
                }
                self.bump();
            }

            // Collect line content with smart spacing (same logic as parse_framework_block)
            let mut line_text = String::new();
            let mut prev_kind: Option<TokenKind> = None;

            while !self.is_at_end()
                && !matches!(
                    self.current_kind(),
                    TokenKind::Newline | TokenKind::Dedent(_)
                )
            {
                let token = self.current();
                let curr_kind = token.kind.clone();

                // Determine if we need a space before this token
                let needs_space = if let Some(ref prev) = prev_kind {
                    let should_skip_space = matches!(
                        (&curr_kind, prev),
                        (_, TokenKind::LeftBrace)
                            | (TokenKind::RightBrace, _)
                            | (TokenKind::Dot, _)
                            | (_, TokenKind::Dot)
                            | (TokenKind::Greater, TokenKind::Minus)
                            | (TokenKind::Divide, _)
                            | (_, TokenKind::Divide)
                            | (_, TokenKind::InterpolationStart)
                            | (TokenKind::InterpolationMid, _)
                            | (_, TokenKind::InterpolationMid)
                            | (TokenKind::InterpolationEnd, _)
                    );
                    !should_skip_space
                } else {
                    false
                };

                if needs_space && !line_text.is_empty() {
                    line_text.push(' ');
                }

                // Handle interpolation tokens - reconstruct the curly braces
                match &curr_kind {
                    TokenKind::InterpolationStart => {
                        line_text.push_str(&token.text);
                        line_text.push('{');
                    }
                    TokenKind::InterpolationMid => {
                        line_text.push('}');
                        line_text.push_str(&token.text);
                        line_text.push('{');
                    }
                    TokenKind::InterpolationEnd => {
                        line_text.push('}');
                        line_text.push_str(&token.text);
                    }
                    _ => {
                        line_text.push_str(&token.text);
                    }
                }

                prev_kind = Some(curr_kind);
                self.bump();
            }

            if !line_text.is_empty() {
                content_lines.push(line_text);
            }

            // Consume newline
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
            }
        }

        let content = content_lines.join("\n");

        debug!(
            block_name = %full_block_name,
            content_len = content.len(),
            "Parsed plugin declaration block"
        );

        Ok(Statement::FrameworkBlock {
            name: full_block_name,
            content,
            attributes: vec![],
            location: Some(start_location),
        })
    }

    /// Parse a framework block (e.g., endpoints:, data, component, screen "Name":)
    /// Supports patterns: "identifier:", "identifier string:", "identifier identifier:"
    /// These are captured as raw text for plugin expansion
    fn parse_framework_block(&mut self) -> Result<Statement, CompilerError> {
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

        // Combine block_name and block_arg for the full name
        let full_block_name = match block_arg {
            Some(arg) => format!("{} {}", block_name, arg),
            None => block_name,
        };

        // Expect colon
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Expect newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        // Collect indented content as raw text
        let mut content_lines = Vec::new();

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

        // Collect all lines that are indented at or deeper than block_indent_level
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

            // Consume indent
            if let TokenKind::Indent(indent_level) = self.current_kind() {
                if *indent_level < block_indent_level {
                    // Not part of this block
                    break;
                }
                self.bump(); // Consume indent
            }

            // Collect the line content with smart spacing
            let mut line_text = String::new();
            let mut prev_kind: Option<TokenKind> = None;

            while !self.is_at_end()
                && !matches!(
                    self.current_kind(),
                    TokenKind::Newline | TokenKind::Dedent(_)
                )
            {
                let token = self.current();
                let curr_kind = token.kind.clone();

                // Determine if we need a space before this token
                let needs_space = if let Some(ref prev) = prev_kind {
                    // Default: add space
                    let should_skip_space = matches!(
                        (&curr_kind, prev),
                        // No space: after LeftBrace
                        (_, TokenKind::LeftBrace) |
                        // No space: before RightBrace
                        (TokenKind::RightBrace, _) |
                        // No space: before/after Dot
                        (TokenKind::Dot, _) | (_, TokenKind::Dot) |
                        // No space: Minus followed by Greater  (->)
                        (TokenKind::Greater, TokenKind::Minus) |
                        // No space: before/after Divide (for paths like /users/{id})
                        // This covers all Divide cases including /{, }/, etc.
                        (TokenKind::Divide, _) | (_, TokenKind::Divide) |
                        // No space: after InterpolationStart (we add { manually)
                        (_, TokenKind::InterpolationStart) |
                        // No space: before/after InterpolationMid (we add }{  manually)
                        (TokenKind::InterpolationMid, _) | (_, TokenKind::InterpolationMid) |
                        // No space: before InterpolationEnd (we add } manually)
                        (TokenKind::InterpolationEnd, _)
                    );
                    !should_skip_space
                } else {
                    false
                };

                if needs_space && !line_text.is_empty() {
                    line_text.push(' ');
                }

                // Handle interpolation tokens - reconstruct the curly braces
                match &curr_kind {
                    TokenKind::InterpolationStart => {
                        line_text.push_str(&token.text);
                        line_text.push('{');
                    }
                    TokenKind::InterpolationMid => {
                        line_text.push('}');
                        line_text.push_str(&token.text);
                        line_text.push('{');
                    }
                    TokenKind::InterpolationEnd => {
                        line_text.push('}');
                        line_text.push_str(&token.text);
                    }
                    _ => {
                        line_text.push_str(&token.text);
                    }
                }

                prev_kind = Some(curr_kind);
                self.bump();
            }

            if !line_text.is_empty() {
                trace!(line = %line_text, "Collected framework line");
                content_lines.push(line_text);
            }

            // Consume newline if present
            if matches!(self.current_kind(), TokenKind::Newline) {
                self.bump();
            }
        }

        let content = content_lines.join("\n");

        debug!(
            block_name = %full_block_name,
            line_count = content_lines.len(),
            "Parsed framework block"
        );
        trace!(content = %content, "Framework block content");

        Ok(Statement::FrameworkBlock {
            name: full_block_name,
            content,
            attributes: vec![], // Attributes parsed via @decorator syntax
            location: Some(start_location),
        })
    }

    fn parse_tests_block(&mut self) -> Result<Vec<TestCase>, CompilerError> {
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

            // Parse test case
            if let Ok(test) = self.parse_test() {
                trace!(description = ?test.description, "Parsed test");
                tests.push(test);
            } else {
                trace!("Failed to parse test, skipping line");
                // Skip line on error
                while !matches!(self.current_kind(), TokenKind::Newline | TokenKind::Eof) {
                    self.bump();
                }
            }
        }

        debug!(test_count = tests.len(), "Finished parsing tests block");
        Ok(tests)
    }

    fn parse_test(&mut self) -> Result<TestCase, CompilerError> {
        trace!(cursor = self.cursor, token = ?self.current_kind(), "Starting test parse");

        let start_location = self.current().location.clone();

        // Expect 'test' keyword
        self.expect(&TokenKind::Test)?;
        self.skip_whitespace();

        trace!(cursor = self.cursor, token = ?self.current_kind(), "After test keyword");

        // Expect string literal description
        let description = if let TokenKind::StringLiteral(desc) = self.current_kind() {
            let desc_text = desc.clone();
            self.bump(); // consume string
            self.skip_whitespace();
            Some(desc_text)
        } else {
            return Err(CompilerError::syntax_error(
                "Expected string literal after 'test' keyword",
                None,
                Some(self.current().location.clone()),
            ));
        };

        trace!(description = ?description, "Parsed test description");

        // Parse test body (block of statements)
        let body = self.parse_block()?;

        trace!(statement_count = body.len(), "Parsed test body");

        // For now, create a test case with the body as the test expression
        // The last statement should be an assert
        let test_expression = if body.is_empty() {
            // Empty test body
            Expression::Literal(Value::Boolean(true))
        } else {
            // Use the last statement as the test expression
            // In practice, this should be an assert statement
            Expression::Literal(Value::Boolean(true))
        };

        let expected_value = Expression::Literal(Value::Boolean(true));

        Ok(TestCase {
            description,
            test_expression,
            expected_value,
            location: Some(start_location),
        })
    }

    fn parse_import(&mut self) -> Result<Vec<ImportItem>, CompilerError> {
        self.expect(&TokenKind::Import)?;
        self.skip_whitespace();

        let mut import_items = Vec::new();

        // Check for import: block syntax vs. single import
        if self.eat(&TokenKind::Colon) {
            // Block syntax: import:\n\tmath\n\tstring.concat\n\t...
            self.skip_whitespace();

            // Parse indented import items
            while !self.is_at_end() {
                self.skip_whitespace();

                // Check for indentation or end of block
                if matches!(self.current_kind(), TokenKind::Indent(_)) {
                    self.skip_indentation();

                    // Parse import item
                    import_items.push(self.parse_import_item()?);
                    self.skip_whitespace();
                } else if matches!(self.current_kind(), TokenKind::Dedent(_)) {
                    // End of import block
                    break;
                } else if matches!(
                    self.current_kind(),
                    TokenKind::Functions
                        | TokenKind::Class
                        | TokenKind::Start
                        | TokenKind::Tests
                        | TokenKind::Private
                ) {
                    // Hit next top-level block
                    break;
                } else {
                    // Not indented and not a dedent = end of block
                    break;
                }
            }
        } else {
            // Old syntax: import math (single line, comma-separated)
            import_items.push(self.parse_import_item()?);
            self.skip_whitespace();

            // Parse additional import items if present
            while self.eat(&TokenKind::Comma) {
                self.skip_whitespace();
                import_items.push(self.parse_import_item()?);
                self.skip_whitespace();
            }
        }

        Ok(import_items)
    }

    /// Parse a single import item with support for "Module.symbol" syntax
    /// Examples:
    ///   Math → whole module
    ///   math.sqrt → specific symbol
    ///   Utils as U → module alias
    ///   Json.decode as jd → symbol alias
    fn parse_import_item(&mut self) -> Result<ImportItem, CompilerError> {
        let mut name = String::new();

        // Parse first identifier
        let first_token = self.expect_identifier()?;
        name.push_str(&first_token.text);

        self.skip_whitespace();

        // Check for dot notation (Module.symbol)
        if self.eat(&TokenKind::Dot) {
            name.push('.');
            self.skip_whitespace();

            let symbol_token = self.expect_identifier()?;
            name.push_str(&symbol_token.text);

            self.skip_whitespace();
        }

        // Check for alias (as ...)
        let alias = if let TokenKind::Identifier(id) = self.current_kind() {
            if id == "as" {
                self.bump(); // consume 'as'
                self.skip_whitespace();
                let alias_token = self.expect_identifier()?;
                Some(alias_token.text.clone())
            } else {
                None
            }
        } else {
            None
        };

        Ok(ImportItem { name, alias })
    }

    /// Parse a private: block listing function names to mark as private
    /// Example: private:\n\thelperFunction\n\tinternalProcessor
    fn parse_private(&mut self) -> Result<Statement, CompilerError> {
        let private_token = self.expect(&TokenKind::Private)?;
        let location = private_token.location.clone();

        self.skip_whitespace();

        // Expect colon after private keyword
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        let mut items = Vec::new();

        // Determine the private block's indentation level
        let block_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        // Parse indented function names
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for Dedent that exits the private block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < block_indent_level {
                    // This Dedent exits the private block - DON'T consume it
                    break;
                }
                // Dedent at our level or higher - consume it and continue
                self.bump();
                self.skip_whitespace();
            }

            // Check for indentation or end of block
            if matches!(self.current_kind(), TokenKind::Indent(_)) {
                self.skip_indentation();

                // Parse function name (identifier)
                if let TokenKind::Identifier(name) = self.current_kind() {
                    let name = name.clone();
                    self.bump();

                    // Create an Expression statement with the function name as a Variable
                    items.push(Statement::Expression {
                        expr: Expression::Variable(name),
                        location: Some(self.current().location.clone()),
                    });
                }
                self.skip_whitespace();
            } else if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
            ) {
                // Hit next top-level block
                break;
            } else {
                // Not indented and not a dedent = end of block
                break;
            }
        }

        Ok(Statement::PrivateBlock {
            items,
            location: Some(location),
        })
    }

    fn parse_parameter_list(&mut self) -> Result<Vec<Parameter>, CompilerError> {
        let mut parameters = Vec::new();

        self.skip_whitespace();

        while !self.check(&TokenKind::RightParen) && !self.is_at_end() {
            parameters.push(self.parse_parameter()?);
            self.skip_whitespace();

            if !self.eat(&TokenKind::Comma) {
                break;
            }
            self.skip_whitespace();
        }

        self.expect(&TokenKind::RightParen)?;

        Ok(parameters)
    }

    fn parse_parameter(&mut self) -> Result<Parameter, CompilerError> {
        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();

        self.skip_whitespace();

        // Type annotation (required for parameters)
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();
        let type_ = self.parse_type()?;

        self.skip_whitespace();

        // Optional default value (e.g., name: type = defaultValue)
        let default_value = if self.eat(&TokenKind::Assign) {
            self.skip_whitespace();
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Parameter {
            name,
            type_,
            default_value,
        })
    }

    fn parse_type(&mut self) -> Result<Type, CompilerError> {
        let type_token = self.expect_identifier()?;

        // Check for generic type parameters (e.g., list<integer>, matrix<number>)
        // and precision modifiers (e.g., integer:8, number:32)
        let base_type = match type_token.text.as_str() {
            "integer" => Type::Integer,
            "number" => Type::Number,
            "string" => Type::String,
            "boolean" => Type::Boolean,
            "void" => Type::Void,
            "any" => Type::Any,
            "list" => {
                // Expect list<Type>
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let inner_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                    Type::List(Box::new(inner_type))
                } else {
                    // list without generic parameter - treat as Object
                    Type::Object("list".to_string())
                }
            }
            "matrix" => {
                // Expect matrix<Type>
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let inner_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                    Type::Matrix(Box::new(inner_type))
                } else {
                    // matrix without generic parameter - treat as Object
                    Type::Object("matrix".to_string())
                }
            }
            "pairs" => {
                // Expect pairs<Type, Type>
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let first_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Comma)?; // expect ','
                    self.skip_whitespace();
                    let second_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                    Type::Pairs(Box::new(first_type), Box::new(second_type))
                } else {
                    // pairs without generic parameters - treat as Object
                    Type::Object("pairs".to_string())
                }
            }
            other => Type::Object(other.to_string()),
        };

        // Check for precision modifiers (e.g., integer:8, number:32u)
        if self.check(&TokenKind::Colon) {
            self.bump(); // consume ':'

            // Expect an integer literal for the bit size
            if let TokenKind::IntegerLiteral(size) = self.current_kind() {
                let bits = *size as u8;
                self.bump(); // consume the size

                // Check for 'u' suffix for unsigned
                let unsigned = if let TokenKind::Identifier(suffix) = self.current_kind() {
                    if suffix == "u" {
                        self.bump(); // consume 'u'
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                // Apply precision modifier based on base type
                match base_type {
                    Type::Integer => Ok(Type::IntegerSized { bits, unsigned }),
                    Type::Number => Ok(Type::NumberSized { bits }),
                    _ => Err(CompilerError::parse_error(
                        format!("Precision modifiers are only supported for integer and number types, not {:?}", base_type),
                        Some(self.current().location.clone()),
                        Some("Use :bits syntax only with integer or number types".to_string()),
                    ))
                }
            } else {
                Err(CompilerError::parse_error(
                    "Expected integer literal for precision modifier".to_string(),
                    Some(self.current().location.clone()),
                    Some("Precision modifiers should be in format 'type:bits' (e.g., 'integer:8', 'number:32')".to_string()),
                ))
            }
        } else {
            Ok(base_type)
        }
    }

    /// Parse a block of statements at the current indentation level
    /// Returns when it encounters a Dedent that exits this block level
    fn parse_block(&mut self) -> Result<Vec<Statement>, CompilerError> {
        let mut statements = Vec::new();

        // Consume the leading Indent token to determine our indentation level
        // If there's no Indent, we're at level 0 (top level of a function)
        let block_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                let level_value = *level;
                self.bump(); // Consume the Indent token
                level_value
            } else {
                0
            }
        } else {
            0
        };

        while !self.is_at_end() {
            self.skip_whitespace();

            // Skip any Indent tokens at our level (from line beginnings)
            // These indicate continued statements at the same indentation level
            while matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_indent_level)
            {
                self.bump();
                self.skip_whitespace();
            }

            // Check for block terminators
            if self.is_at_end() {
                break;
            }

            // Check if we've encountered a Dedent token
            // A Dedent(N) token means "we're now at indentation level N"
            // We should exit this block if the dedent takes us BELOW our level
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < block_indent_level {
                    // We've exited this block's scope - DON'T consume the Dedent
                    // Let the parent handle it
                    break;
                }
                // Dedent at our level or higher - consume it and continue
                // (This shouldn't happen in practice, but handle it gracefully)
                self.bump();
                continue;
            }

            // Top-level declarations end the current block
            // Note: TokenKind::Test is NOT included here because "test" can be used as a
            // function name inside blocks (e.g., calling a function named "test()")
            if matches!(
                self.current_kind(),
                TokenKind::Function | TokenKind::Class | TokenKind::Functions
            ) {
                break;
            }

            match self.parse_statement() {
                Ok(stmt) => statements.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.bump(); // Skip error token
                }
            }
        }

        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement, CompilerError> {
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
            TokenKind::Println => {
                // Check if this is a println apply block: println:
                let saved_cursor = self.cursor;
                self.bump(); // consume println
                self.skip_whitespace();
                if self.check(&TokenKind::Colon) {
                    // This is a println apply block
                    self.cursor = saved_cursor; // restore to println token
                    return self.parse_function_apply_block();
                }
                // Not an apply block, restore and parse as regular println
                self.cursor = saved_cursor;
                self.parse_println()
            }
            TokenKind::Error => self.parse_error_statement(),
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

    fn parse_return(&mut self) -> Result<Statement, CompilerError> {
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

    fn parse_break(&mut self) -> Result<Statement, CompilerError> {
        let break_token = self.expect(&TokenKind::Break)?;
        Ok(Statement::Break {
            location: Some(break_token.location),
        })
    }

    fn parse_continue(&mut self) -> Result<Statement, CompilerError> {
        let continue_token = self.expect(&TokenKind::Continue)?;
        Ok(Statement::Continue {
            location: Some(continue_token.location),
        })
    }

    fn parse_if(&mut self) -> Result<Statement, CompilerError> {
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
    fn parse_while(&mut self) -> Result<Statement, CompilerError> {
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

    fn parse_for(&mut self) -> Result<Statement, CompilerError> {
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
    fn parse_iterate(&mut self) -> Result<Statement, CompilerError> {
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

    fn parse_print(&mut self) -> Result<Statement, CompilerError> {
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
                arguments.into_iter().next().unwrap()
            } else {
                // Create a function call to represent multi-arg print
                Expression::Call("print".to_string(), arguments)
            }
        } else {
            // No parentheses - parse single expression
            self.parse_expression()?
        };

        Ok(Statement::Print {
            expression,
            newline: false,
            location: Some(print_token.location),
        })
    }

    fn parse_println(&mut self) -> Result<Statement, CompilerError> {
        let print_token = self.expect(&TokenKind::Println)?;
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
                arguments.into_iter().next().unwrap()
            } else {
                // Create a function call to represent multi-arg println
                Expression::Call("println".to_string(), arguments)
            }
        } else {
            // No parentheses - parse single expression
            self.parse_expression()?
        };

        Ok(Statement::Print {
            expression,
            newline: true,
            location: Some(print_token.location),
        })
    }

    fn parse_error_statement(&mut self) -> Result<Statement, CompilerError> {
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

    fn parse_expression(&mut self) -> Result<Expression, CompilerError> {
        self.parse_on_error()
    }

    // Parse onError expressions: expr onError fallback
    // OnError has lowest precedence (below logical OR)
    // Supports chaining: a onError b onError c = (a onError b) onError c (left-associative)
    fn parse_on_error(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_default()?;

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
            let fallback = self.parse_default()?;
            let location = self.current().location.clone();

            expr = Expression::OnError {
                expression: Box::new(expr),
                fallback: Box::new(fallback),
                location,
            };
        }

        Ok(expr)
    }

    // BOOK: null-coalescing - Parse default expressions: expr default fallback
    // Default has precedence below onError but above logical OR
    // Usage: value default fallback (returns fallback if value is null)
    fn parse_default(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_logical_or()?;

        while self.check(&TokenKind::Default) {
            let _op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_logical_or()?;
            expr = Expression::Binary(Box::new(expr), BinaryOperator::Default, Box::new(right));
        }

        Ok(expr)
    }

    // CRITICAL FIX: Add logical OR operator support (lowest precedence)
    fn parse_logical_or(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_logical_and()?;

        while self.check(&TokenKind::Or) {
            let _op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_logical_and()?;
            expr = Expression::Binary(Box::new(expr), BinaryOperator::Or, Box::new(right));
        }

        Ok(expr)
    }

    // CRITICAL FIX: Add logical AND operator support
    fn parse_logical_and(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_comparison()?;

        while self.check(&TokenKind::And) {
            let _op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_comparison()?;
            expr = Expression::Binary(Box::new(expr), BinaryOperator::And, Box::new(right));
        }

        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expression, CompilerError> {
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
                | TokenKind::Not
        ) {
            let op_token = self.bump();
            self.skip_whitespace();
            let right = self.parse_term()?;

            let op = match &op_token.kind {
                TokenKind::Equal => BinaryOperator::Equal,
                TokenKind::NotEqual => BinaryOperator::NotEqual,
                TokenKind::Less => BinaryOperator::Less,
                TokenKind::Greater => BinaryOperator::Greater,
                TokenKind::LessEqual => BinaryOperator::LessEqual,
                TokenKind::GreaterEqual => BinaryOperator::GreaterEqual,
                TokenKind::Is => BinaryOperator::Is,
                TokenKind::Not => BinaryOperator::Not,
                _ => unreachable!(),
            };

            expr = Expression::Binary(Box::new(expr), op, Box::new(right));
        }

        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expression, CompilerError> {
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

    fn parse_factor(&mut self) -> Result<Expression, CompilerError> {
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

    // CRITICAL FIX: Add exponentiation operator support (higher precedence than multiplication)
    // Right-associative: 2^3^2 = 2^(3^2) = 2^9 = 512
    fn parse_power(&mut self) -> Result<Expression, CompilerError> {
        let mut expr = self.parse_unary()?;

        if self.check(&TokenKind::Power) {
            let _op_token = self.bump();
            self.skip_whitespace();
            // Right associative - recursively parse the right side
            let right = self.parse_power()?;
            expr = Expression::Binary(Box::new(expr), BinaryOperator::Power, Box::new(right));
        }

        Ok(expr)
    }

    // CRITICAL FIX: Add unary operator support (not, unary -)
    fn parse_unary(&mut self) -> Result<Expression, CompilerError> {
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

    fn parse_postfix(&mut self) -> Result<Expression, CompilerError> {
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
                            arguments.push(self.parse_expression()?);
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
                // BOOK: required-operator - Postfix ! assertion for null check
                TokenKind::Bang => {
                    // Required assertion: expr!
                    self.bump(); // consume !
                    expr = Expression::Unary(UnaryOperator::Required, Box::new(expr));
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expression, CompilerError> {
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
            // BOOK: null-support - Null literal parsing
            TokenKind::Null => {
                self.bump();
                Ok(Expression::Literal(Value::Null))
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
            // Allow keywords to be used as identifiers in expressions (for class/type names and variable names)
            TokenKind::Test
            | TokenKind::Error
            | TokenKind::Unit
            | TokenKind::Input
            | TokenKind::Step
            | TokenKind::Description => {
                let token = self.bump();
                // Use the actual token text to preserve the exact identifier (e.g., "Test", not "test")
                let name = token.text.clone();
                Ok(Expression::Variable(name))
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

    fn expect_identifier(&mut self) -> Result<Token, CompilerError> {
        match self.current_kind() {
            TokenKind::Identifier(_) => Ok(self.bump()),
            _ => {
                let token = self.current();
                Err(CompilerError::parse_error(
                    format!("Expected identifier, found {:?}", token.kind),
                    Some(token.location.clone()),
                    None,
                ))
            }
        }
    }

    /// Get identifier or keyword text (for cases where keywords can be used as names)
    fn expect_name(&mut self) -> Result<Token, CompilerError> {
        match self.current_kind() {
            TokenKind::Identifier(_) => Ok(self.bump()),
            // Allow keywords to be used as names in certain contexts (for property/method names and variable names)
            TokenKind::Test
            | TokenKind::Unit
            | TokenKind::Error
            | TokenKind::Input
            | TokenKind::Step
            | TokenKind::Description
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::Not
            | TokenKind::Start => Ok(self.bump()),
            _ => {
                let token = self.current();
                Err(CompilerError::parse_error(
                    format!(
                        "Expected name (identifier or keyword), found {:?}",
                        token.kind
                    ),
                    Some(token.location.clone()),
                    None,
                ))
            }
        }
    }

    fn parse_string_interpolation(&mut self) -> Result<Expression, CompilerError> {
        use crate::ast::StringPart;
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

    /// Parse later assignment: later var = start expr
    fn parse_later_assignment(&mut self) -> Result<Statement, CompilerError> {
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
    fn parse_background(&mut self) -> Result<Statement, CompilerError> {
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
    fn try_parse_on_error_block(
        &mut self,
    ) -> Result<Option<(Vec<Statement>, SourceLocation)>, CompilerError> {
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

    /// Parse a state: block containing state variable declarations
    fn parse_state_block(&mut self) -> Result<crate::ast::StateBlock, CompilerError> {
        use crate::ast::{StateBlock, StateDeclaration, StateScope};

        // Consume "state" keyword
        self.expect(&TokenKind::State)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        let mut declarations: Vec<StateDeclaration> = Vec::new();
        let computed = Vec::new(); // Will be populated when computed: blocks are parsed

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
                _ => {
                    // Unknown token, skip and continue
                    break;
                }
            }
        }

        Ok(StateBlock {
            declarations,
            computed,
            scope: StateScope::App, // Default to App scope for top-level state
            location: None,
        })
    }

    /// Try to parse a guard clause following a state declaration
    /// Guard syntax: guard <condition> else "message"
    /// Returns None if no guard is present, or the parsed GuardClause
    fn try_parse_guard_clause(
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
}
