//! Specification-compliant lexer implementation for Clean Language
//!
//! This lexer provides 100% compliance with the AST Specification, implementing
//! all language constructs exactly as defined in the Clean Language Specification.

#![allow(dead_code)]

use super::specification_token::*;
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use std::iter::Peekable;
use std::str::Chars;

/// Source code input structure
#[derive(Debug, Clone)]
pub struct SourceCode {
    pub content: String,
    pub file_path: String,
    pub encoding: SourceEncoding,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceEncoding {
    Utf8,
}

impl SourceCode {
    pub fn new(content: String, file_path: String) -> Self {
        Self {
            content,
            file_path,
            encoding: SourceEncoding::Utf8,
        }
    }
}

/// Parts of an interpolated string
#[derive(Debug, Clone)]
enum InterpolationPart {
    String(String),
    Expression(Vec<Token>),
}

/// Clean Language lexer with 100% specification compliance
pub struct SpecificationLexer<'a> {
    input: Peekable<Chars<'a>>,
    source_content: &'a str,
    source_map: SourceMap,
    current_pos: usize,
    line: usize,
    column: usize,
    indentation_stack: Vec<usize>,
    at_line_start: bool,
    token_buffer: Vec<Token>, // Buffer for emitting multiple tokens from string interpolation
    in_interpolation: bool,   // Track if we're inside an interpolated string
    interpolation_depth: usize, // Track brace nesting depth during interpolation
    suppress_indent_after_block_comment: bool, // Suppress indentation handling after multi-line block comment
}

impl<'a> SpecificationLexer<'a> {
    /// Create new lexer from source code
    pub fn new(source: &'a SourceCode) -> Self {
        let source_map = SourceMap::new(source.file_path.clone(), &source.content);

        Self {
            input: source.content.chars().peekable(),
            source_content: &source.content,
            source_map,
            current_pos: 0,
            line: 1,
            column: 1,
            indentation_stack: vec![0], // Start with zero indentation
            at_line_start: true,
            token_buffer: Vec::new(),
            in_interpolation: false,
            interpolation_depth: 0,
            suppress_indent_after_block_comment: false,
        }
    }

    /// Tokenize the entire source into a token stream
    pub fn tokenize(&mut self) -> Result<TokenStream, LexError> {
        let mut tokens = Vec::new();

        loop {
            match self.next_token()? {
                token if token.kind == TokenKind::Eof => {
                    // Add final dedents to match indentation stack
                    while self.indentation_stack.len() > 1 {
                        self.indentation_stack.pop();
                        let location = self.current_location();
                        // Emit the indentation level we're at, not the stack length
                        let current_level = *self.indentation_stack.last().unwrap_or(&0);
                        tokens.push(Token::simple(TokenKind::Dedent(current_level), location));
                    }

                    tokens.push(token);
                    break;
                }
                token => tokens.push(token),
            }
        }

        Ok(TokenStream::new(tokens, self.source_map.clone()))
    }

    /// Get the source map for this lexer
    pub fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    /// Get next token from input
    fn next_token(&mut self) -> Result<Token, LexError> {
        // Check if we have buffered tokens from string interpolation
        if !self.token_buffer.is_empty() {
            return Ok(self.token_buffer.remove(0));
        }

        // Handle indentation at line start
        if self.at_line_start {
            return self.handle_indentation();
        }

        // Skip whitespace (but preserve newlines and tabs)
        self.skip_whitespace();

        let start_location = self.current_location();

        match self.peek() {
            None => Ok(Token::simple(TokenKind::Eof, start_location)),
            Some(&ch) => match ch {
                // Newlines are significant
                '\n' => {
                    self.advance();
                    self.at_line_start = true;
                    Ok(Token::simple(TokenKind::Newline, start_location))
                }

                // String literals
                '"' => self.read_string_literal(),

                // Numbers (including precision modifiers)
                c if c.is_ascii_digit() => self.read_number_literal(),

                // Identifiers and keywords
                c if c.is_alphabetic() || c == '_' => self.read_identifier_or_keyword(),

                // Single-character operators and punctuation
                '+' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Plus, start_location))
                }
                '-' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Minus, start_location))
                }
                '*' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Multiply, start_location))
                }
                '/' => self.handle_slash(), // Could be division or comment
                '%' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Modulo, start_location))
                }
                '^' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Power, start_location))
                }
                '(' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::LeftParen, start_location))
                }
                ')' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::RightParen, start_location))
                }
                '[' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::LeftBracket, start_location))
                }
                ']' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::RightBracket, start_location))
                }
                '{' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::LeftBrace, start_location))
                }
                '}' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::RightBrace, start_location))
                }
                ',' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Comma, start_location))
                }
                ';' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Semicolon, start_location))
                }
                ':' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Colon, start_location))
                }

                // Multi-character operators
                '=' => self.handle_equals(),
                '!' => self.handle_exclamation(),
                '<' => self.handle_less_than(),
                '>' => self.handle_greater_than(),
                '.' => self.handle_dot(),

                // Hash-style comments
                '#' => self.handle_hash_comment(),

                // Invalid character
                _ => {
                    let invalid_char = self.advance().unwrap();
                    Err(LexError::InvalidCharacter {
                        char: invalid_char,
                        location: start_location,
                    })
                }
            },
        }
    }

    /// Handle indentation at the beginning of a line
    fn handle_indentation(&mut self) -> Result<Token, LexError> {
        let mut indent_level = 0;
        let start_location = self.current_location();

        // Skip indentation handling if we just exited a multi-line block comment
        if self.suppress_indent_after_block_comment {
            self.suppress_indent_after_block_comment = false;
            self.at_line_start = false;
            return self.next_token_after_indentation();
        }

        // Count tab characters for indentation
        while let Some(&'\t') = self.peek() {
            self.advance();
            indent_level += 1;
        }

        // CRITICAL FIX: Reject spaces at the beginning of lines (after tabs)
        // Clean Language mandates tabs-only for indentation
        if let Some(&' ') = self.peek() {
            // Found a space at line start - this is an error
            return Err(LexError::SpacesInIndentation {
                location: start_location,
            });
        }

        // Check if this is an empty line or comment-only line
        if let Some(&'\n') = self.peek() {
            // Skip empty lines for indentation purposes - advance to next line and continue
            self.advance();
            self.line += 1;
            self.column = 1;
            self.at_line_start = true;
            // Return newline token instead of recursing
            return Ok(Token::simple(TokenKind::Newline, start_location));
        }

        if let Some(&'/') = self.peek() {
            if let Some(chars) = self.peek_chars(2) {
                if chars[1] == '/' {
                    // Comment line - handle the comment normally without recursion
                    self.at_line_start = false;
                    return self.handle_slash();
                }
            }
        }

        if let Some(&'#') = self.peek() {
            // Hash-style comment line - handle the comment normally without recursion
            self.at_line_start = false;
            return self.handle_hash_comment();
        }

        self.at_line_start = false;

        let current_indent = *self.indentation_stack.last().unwrap();

        if indent_level > current_indent {
            // Increased indentation - push new level
            self.indentation_stack.push(indent_level);
            Ok(Token::simple(
                TokenKind::Indent(indent_level),
                start_location,
            ))
        } else if indent_level < current_indent {
            // Decreased indentation - emit multiple dedents (one for each level popped)
            // This follows Python's approach: one DEDENT token per level decrease
            let mut dedent_tokens = Vec::new();

            while let Some(&stack_level) = self.indentation_stack.last() {
                if stack_level <= indent_level {
                    break;
                }
                self.indentation_stack.pop();
                // Emit a DEDENT token with the indentation level we're at (not the stack length)
                // After popping, the last element in the stack is the level we've returned to
                let current_level = *self.indentation_stack.last().unwrap_or(&0);
                dedent_tokens.push(Token::simple(
                    TokenKind::Dedent(current_level),
                    start_location.clone(),
                ));
            }

            // Verify indentation matches a previous level
            if self.indentation_stack.last() != Some(&indent_level) {
                return Err(LexError::InvalidIndentation {
                    expected_levels: self.indentation_stack.clone(),
                    found_level: indent_level,
                    location: start_location,
                });
            }

            // Buffer remaining dedent tokens for later emission
            if dedent_tokens.len() > 1 {
                for token in dedent_tokens.iter().skip(1) {
                    self.token_buffer.push(token.clone());
                }
            }

            // Return the first dedent token
            Ok(dedent_tokens.into_iter().next().unwrap())
        } else {
            // Same indentation level
            if indent_level > 0 {
                // Emit Indent token for continuation lines at the same indentation level
                Ok(Token::simple(
                    TokenKind::Indent(indent_level),
                    start_location,
                ))
            } else {
                // Level 0 (no indentation) - continue parsing
                self.next_token_after_indentation()
            }
        }
    }

    /// Get next token after handling indentation (prevents recursion)
    fn next_token_after_indentation(&mut self) -> Result<Token, LexError> {
        // Skip whitespace (but preserve newlines and tabs)
        self.skip_whitespace();

        let start_location = self.current_location();

        match self.peek() {
            None => Ok(Token::simple(TokenKind::Eof, start_location)),
            Some(&ch) => match ch {
                // Newlines are significant
                '\n' => {
                    self.advance();
                    self.at_line_start = true;
                    Ok(Token::simple(TokenKind::Newline, start_location))
                }

                // String literals
                '"' => self.read_string_literal(),

                // Numbers (including precision modifiers)
                c if c.is_ascii_digit() => self.read_number_literal(),

                // Identifiers and keywords
                c if c.is_alphabetic() || c == '_' => self.read_identifier_or_keyword(),

                // Single-character operators and punctuation
                '+' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Plus, start_location))
                }
                '-' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Minus, start_location))
                }
                '*' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Multiply, start_location))
                }
                '/' => self.handle_slash(), // Could be division or comment
                '%' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Modulo, start_location))
                }
                '^' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Power, start_location))
                }
                '(' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::LeftParen, start_location))
                }
                ')' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::RightParen, start_location))
                }
                '[' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::LeftBracket, start_location))
                }
                ']' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::RightBracket, start_location))
                }
                '{' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::LeftBrace, start_location))
                }
                '}' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::RightBrace, start_location))
                }
                ',' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Comma, start_location))
                }
                ';' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Semicolon, start_location))
                }
                ':' => {
                    self.advance();
                    Ok(Token::simple(TokenKind::Colon, start_location))
                }
                '.' => self.handle_dot(),
                '=' => self.handle_equals(),
                '<' => self.handle_less_than(),
                '>' => self.handle_greater_than(),
                '!' => self.handle_exclamation(),

                // Hash-style comments
                '#' => self.handle_hash_comment(),

                // Invalid character
                _ => {
                    let invalid_char = self.advance().unwrap();
                    Err(LexError::InvalidCharacter {
                        char: invalid_char,
                        location: start_location,
                    })
                }
            },
        }
    }

    /// Read string literal with interpolation support
    fn read_string_literal(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;

        self.advance(); // Skip opening quote

        // First, scan the entire string to check if it has interpolation
        let mut content = String::new();
        let mut has_interpolation = false;

        // Save state to restart if we find interpolation
        let saved_pos = self.current_pos;
        let saved_line = self.line;
        let saved_column = self.column;

        while let Some(ch) = self.peek() {
            match ch {
                &'"' => break,
                &'\\' => {
                    self.advance(); // Skip backslash
                    if let Some(escaped) = self.advance() {
                        if escaped == '{' || escaped == '}' {
                            content.push(escaped);
                        }
                    }
                }
                &'{' => {
                    has_interpolation = true;
                    break;
                }
                &'\n' => {
                    return Err(LexError::UnterminatedString {
                        location: start_location.clone(),
                    });
                }
                _ => {
                    content.push(*ch);
                    self.advance();
                }
            }
        }

        // Restore state and parse properly
        self.current_pos = saved_pos;
        self.line = saved_line;
        self.column = saved_column;
        self.input = self.source_content[saved_pos..].chars().peekable();

        if !has_interpolation {
            // Simple string without interpolation
            return self.read_simple_string(start_location, start_pos);
        } else {
            // Interpolated string - parse and buffer all tokens
            return self.read_interpolated_string(start_location, start_pos);
        }
    }

    /// Read a simple string literal (no interpolation)
    fn read_simple_string(
        &mut self,
        start_location: SourceLocation,
        start_pos: usize,
    ) -> Result<Token, LexError> {
        let mut content = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                &'"' => {
                    self.advance(); // Skip closing quote
                    let text = self.source_text_range(start_pos, self.current_pos);
                    return Ok(Token::new(
                        TokenKind::StringLiteral(content),
                        start_location,
                        text,
                    ));
                }
                &'\\' => {
                    self.advance(); // Skip backslash
                    match self.advance() {
                        Some('n') => content.push('\n'),
                        Some('t') => content.push('\t'),
                        Some('r') => content.push('\r'),
                        Some('\\') => content.push('\\'),
                        Some('"') => content.push('"'),
                        Some('{') => content.push('{'),
                        Some('}') => content.push('}'),
                        Some('0') => content.push('\0'),
                        Some(other) => {
                            content.push('\\');
                            content.push(other);
                        }
                        None => {
                            return Err(LexError::UnterminatedString {
                                location: start_location.clone(),
                            });
                        }
                    }
                }
                &'\n' => {
                    return Err(LexError::UnterminatedString {
                        location: start_location,
                    });
                }
                _ => {
                    content.push(*ch);
                    self.advance();
                }
            }
        }

        Err(LexError::UnterminatedString {
            location: start_location,
        })
    }

    /// Read an interpolated string and buffer all tokens
    fn read_interpolated_string(
        &mut self,
        start_location: SourceLocation,
        start_pos: usize,
    ) -> Result<Token, LexError> {
        // Collect all parts: string segments and expressions
        let mut parts: Vec<InterpolationPart> = Vec::new();
        let mut current_string = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                &'"' => {
                    // End of string
                    parts.push(InterpolationPart::String(current_string));
                    break;
                }
                &'\\' => {
                    self.advance(); // Skip backslash
                    match self.advance() {
                        Some('n') => current_string.push('\n'),
                        Some('t') => current_string.push('\t'),
                        Some('r') => current_string.push('\r'),
                        Some('\\') => current_string.push('\\'),
                        Some('"') => current_string.push('"'),
                        Some('{') => current_string.push('{'), // Escaped brace
                        Some('}') => current_string.push('}'), // Escaped brace
                        Some('0') => current_string.push('\0'),
                        Some(other) => {
                            current_string.push('\\');
                            current_string.push(other);
                        }
                        None => {
                            return Err(LexError::UnterminatedString {
                                location: start_location.clone(),
                            });
                        }
                    }
                }
                &'{' => {
                    // Start of interpolation expression
                    parts.push(InterpolationPart::String(current_string.clone()));
                    current_string.clear();

                    self.advance(); // Skip '{'

                    // Parse expression tokens until we find matching '}'
                    let expr_tokens = self.read_interpolation_expression()?;
                    parts.push(InterpolationPart::Expression(expr_tokens));
                }
                &'\n' => {
                    return Err(LexError::UnterminatedString {
                        location: start_location.clone(),
                    });
                }
                _ => {
                    current_string.push(*ch);
                    self.advance();
                }
            }
        }

        // Consume closing quote
        if self.peek() == Some(&'"') {
            self.advance();
        } else {
            return Err(LexError::UnterminatedString {
                location: start_location.clone(),
            });
        }

        // Now emit tokens based on parts
        self.emit_interpolation_tokens(parts, start_location.clone())?;

        // Return the first buffered token
        if !self.token_buffer.is_empty() {
            Ok(self.token_buffer.remove(0))
        } else {
            // Empty interpolated string - emit empty StringLiteral
            Ok(Token::new(
                TokenKind::StringLiteral(String::new()),
                start_location,
                self.source_text_range(start_pos, self.current_pos),
            ))
        }
    }

    /// Read expression tokens inside interpolation {}
    fn read_interpolation_expression(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        let mut brace_depth = 1; // We already consumed the opening '{'

        while brace_depth > 0 && !self.is_at_end() {
            self.skip_whitespace();
            let start_location = self.current_location();

            match self.peek() {
                Some(&'}') => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        self.advance(); // Consume closing '}'
                        break;
                    } else {
                        // Nested closing brace
                        self.advance();
                        tokens.push(Token::simple(TokenKind::RightBrace, start_location));
                    }
                }
                Some(&'{') => {
                    // Nested opening brace
                    self.advance();
                    brace_depth += 1;
                    tokens.push(Token::simple(TokenKind::LeftBrace, start_location));
                }
                Some(&ch) if ch.is_alphabetic() || ch == '_' => {
                    tokens.push(self.read_identifier_or_keyword()?);
                }
                Some(&ch) if ch.is_ascii_digit() => {
                    tokens.push(self.read_number_literal()?);
                }
                Some(&'.') => {
                    tokens.push(self.handle_dot()?);
                }
                Some(&'(') => {
                    self.advance();
                    tokens.push(Token::simple(TokenKind::LeftParen, start_location));
                }
                Some(&')') => {
                    self.advance();
                    tokens.push(Token::simple(TokenKind::RightParen, start_location));
                }
                Some(&'[') => {
                    self.advance();
                    tokens.push(Token::simple(TokenKind::LeftBracket, start_location));
                }
                Some(&']') => {
                    self.advance();
                    tokens.push(Token::simple(TokenKind::RightBracket, start_location));
                }
                Some(_) => {
                    // Try to get next token normally
                    let token = self.next_token_in_interpolation()?;
                    tokens.push(token);
                }
                None => {
                    return Err(LexError::UnterminatedString {
                        location: start_location,
                    });
                }
            }
        }

        Ok(tokens)
    }

    /// Get next token while inside interpolation (helper)
    fn next_token_in_interpolation(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();

        match self.peek() {
            Some(&'+') => {
                self.advance();
                Ok(Token::simple(TokenKind::Plus, start_location))
            }
            Some(&'-') => {
                self.advance();
                Ok(Token::simple(TokenKind::Minus, start_location))
            }
            Some(&'*') => {
                self.advance();
                Ok(Token::simple(TokenKind::Multiply, start_location))
            }
            Some(&'/') => {
                self.advance();
                Ok(Token::simple(TokenKind::Divide, start_location))
            }
            Some(&',') => {
                self.advance();
                Ok(Token::simple(TokenKind::Comma, start_location))
            }
            _ => Err(LexError::InvalidCharacter {
                char: self.advance().unwrap_or('\0'),
                location: start_location,
            }),
        }
    }

    /// Emit interpolation tokens to the buffer
    fn emit_interpolation_tokens(
        &mut self,
        parts: Vec<InterpolationPart>,
        start_location: SourceLocation,
    ) -> Result<(), LexError> {
        if parts.is_empty() {
            return Ok(());
        }

        let mut is_first = true;
        let mut last_was_expr = false;

        for (i, part) in parts.iter().enumerate() {
            match part {
                InterpolationPart::String(s) => {
                    if s.is_empty() && !is_first {
                        continue; // Skip empty string parts in the middle
                    }

                    let token_kind = if is_first {
                        is_first = false;
                        last_was_expr = false;
                        TokenKind::InterpolationStart
                    } else if i == parts.len() - 1 {
                        TokenKind::InterpolationEnd
                    } else {
                        last_was_expr = false;
                        TokenKind::InterpolationMid
                    };

                    self.token_buffer.push(Token::new(
                        token_kind,
                        start_location.clone(),
                        s.clone(),
                    ));
                }
                InterpolationPart::Expression(tokens) => {
                    // Add all expression tokens to buffer
                    for token in tokens {
                        self.token_buffer.push(token.clone());
                    }
                    last_was_expr = true;
                    is_first = false;
                }
            }
        }

        // If the last part was an expression, we need to add an empty InterpolationEnd
        if last_was_expr {
            self.token_buffer.push(Token::new(
                TokenKind::InterpolationEnd,
                start_location,
                String::new(),
            ));
        }

        Ok(())
    }

    fn is_at_end(&self) -> bool {
        self.current_pos >= self.source_content.len()
    }

    /// Read number literal with optional precision modifier
    fn read_number_literal(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;

        let mut number_text = String::new();
        let mut is_float = false;

        // Base-prefixed integer detection: 0x..., 0b..., 0o...
        if let Some(&'0') = self.peek() {
            if let Some(chars) = self.peek_chars(2) {
                match chars.as_slice() {
                    ['0', 'x'] | ['0', 'X'] => {
                        // consume 0x
                        self.advance();
                        self.advance();
                        let mut digits = String::new();
                        while let Some(&ch) = self.peek() {
                            if ch.is_ascii_hexdigit() {
                                digits.push(ch);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let text = self.source_text_range(start_pos, self.current_pos);
                        let value = i64::from_str_radix(&digits, 16).map_err(|_| {
                            LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            }
                        })?;
                        return Ok(Token::new(
                            TokenKind::IntegerLiteral(value),
                            start_location,
                            text,
                        ));
                    }
                    ['0', 'b'] | ['0', 'B'] => {
                        self.advance();
                        self.advance();
                        let mut digits = String::new();
                        while let Some(&ch) = self.peek() {
                            if ch == '0' || ch == '1' {
                                digits.push(ch);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let text = self.source_text_range(start_pos, self.current_pos);
                        let value = i64::from_str_radix(&digits, 2).map_err(|_| {
                            LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            }
                        })?;
                        return Ok(Token::new(
                            TokenKind::IntegerLiteral(value),
                            start_location,
                            text,
                        ));
                    }
                    ['0', 'o'] | ['0', 'O'] => {
                        self.advance();
                        self.advance();
                        let mut digits = String::new();
                        while let Some(&ch) = self.peek() {
                            if ch >= '0' && ch <= '7' {
                                digits.push(ch);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let text = self.source_text_range(start_pos, self.current_pos);
                        let value = i64::from_str_radix(&digits, 8).map_err(|_| {
                            LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            }
                        })?;
                        return Ok(Token::new(
                            TokenKind::IntegerLiteral(value),
                            start_location,
                            text,
                        ));
                    }
                    _ => {}
                }
            }
        }

        // Read integer part
        while let Some(&ch) = self.peek() {
            if ch.is_ascii_digit() {
                number_text.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Check for decimal point
        if let Some(&'.') = self.peek() {
            // Look ahead to ensure it's not a range operator (..)
            if let Some(chars) = self.peek_chars(2) {
                if chars[1] != '.' && chars[1].is_ascii_digit() {
                    is_float = true;
                    number_text.push('.');
                    self.advance();

                    // Read fractional part
                    while let Some(&ch) = self.peek() {
                        if ch.is_ascii_digit() {
                            number_text.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // Check for scientific notation
        if let Some(&ch) = self.peek() {
            if ch == 'e' || ch == 'E' {
                is_float = true;
                number_text.push(ch);
                self.advance();

                // Optional sign
                if let Some(&sign) = self.peek() {
                    if sign == '+' || sign == '-' {
                        number_text.push(sign);
                        self.advance();
                    }
                }

                // Exponent digits
                while let Some(&ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        number_text.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        // Check for precision modifier - only if followed by valid digits
        if let Some(&':') = self.peek() {
            // Look ahead to see if this is actually a precision modifier
            if self.is_valid_precision_modifier_ahead() {
                self.advance(); // Skip ':'
                let precision = self.read_precision_modifier()?;

                let text = self.source_text_range(start_pos, self.current_pos);

                // Create precision-specific token
                if is_float {
                    match precision {
                        PrecisionModifier::Number32 => {
                            let value: f32 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Number32Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Number64 => {
                            let value: f64 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Number64Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        _ => Err(LexError::InvalidPrecisionModifier {
                            modifier: format!("{:?}", precision),
                            location: start_location,
                        }),
                    }
                } else {
                    match precision {
                        PrecisionModifier::Integer8 => {
                            let value: i8 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Integer8Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Integer8u => {
                            let value: u8 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Integer8uLiteral(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Integer16 => {
                            let value: i16 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Integer16Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Integer16u => {
                            let value: u16 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Integer16uLiteral(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Integer32 => {
                            let value: i32 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Integer32Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Integer64 => {
                            let value: i64 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Integer64Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Number32 => {
                            let value: f32 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Number32Literal(value),
                                start_location,
                                text,
                            ))
                        }
                        PrecisionModifier::Number64 => {
                            let value: f64 =
                                number_text.parse().map_err(|_| LexError::InvalidNumber {
                                    text: text.clone(),
                                    location: start_location.clone(),
                                })?;
                            Ok(Token::new(
                                TokenKind::Number64Literal(value),
                                start_location,
                                text,
                            ))
                        }
                    }
                }
            } else {
                // Colon found but not a precision modifier - treat as default number
                let text = self.source_text_range(start_pos, self.current_pos);

                if is_float {
                    let value: f64 = number_text.parse().map_err(|_| LexError::InvalidNumber {
                        text: text.clone(),
                        location: start_location.clone(),
                    })?;
                    Ok(Token::new(
                        TokenKind::NumberLiteral(value),
                        start_location,
                        text,
                    ))
                } else {
                    let value: i64 = number_text.parse().map_err(|_| LexError::InvalidNumber {
                        text: text.clone(),
                        location: start_location.clone(),
                    })?;
                    Ok(Token::new(
                        TokenKind::IntegerLiteral(value),
                        start_location,
                        text,
                    ))
                }
            }
        } else {
            // Default precision
            let text = self.source_text_range(start_pos, self.current_pos);

            if is_float {
                let value: f64 = number_text.parse().map_err(|_| LexError::InvalidNumber {
                    text: text.clone(),
                    location: start_location.clone(),
                })?;
                Ok(Token::new(
                    TokenKind::NumberLiteral(value),
                    start_location,
                    text,
                ))
            } else {
                let value: i64 = number_text.parse().map_err(|_| LexError::InvalidNumber {
                    text: text.clone(),
                    location: start_location.clone(),
                })?;
                Ok(Token::new(
                    TokenKind::IntegerLiteral(value),
                    start_location,
                    text,
                ))
            }
        }
    }

    /// Read precision modifier (8, 8u, 16, 16u, 32, 64)
    /// Check if what follows the current ':' looks like a valid precision modifier
    fn is_valid_precision_modifier_ahead(&self) -> bool {
        let pos = 1; // Start after the ':'
        let mut has_digits = false;

        // Look ahead to see if we have a valid precision modifier pattern
        while let Some(ch) = self.peek_at_offset(pos) {
            if ch.is_ascii_digit() {
                has_digits = true;
                let _ = pos + 1; // Validation only, don't advance
            } else if ch == 'u' && has_digits {
                break; // 'u' can only be at the end
            } else {
                break;
            }
        }

        if !has_digits {
            return false;
        }

        // Collect the potential modifier string and check if it's valid
        let mut modifier = String::new();
        let mut check_pos = 1;
        while let Some(ch) = self.peek_at_offset(check_pos) {
            if ch.is_ascii_digit() || ch == 'u' {
                modifier.push(ch);
                check_pos += 1;
            } else {
                break;
            }
        }

        // Check if it matches known precision modifiers
        matches!(modifier.as_str(), "8" | "8u" | "16" | "16u" | "32" | "64")
    }

    fn peek_at_offset(&self, offset: usize) -> Option<char> {
        if self.current_pos + offset < self.source_content.len() {
            self.source_content.chars().nth(self.current_pos + offset)
        } else {
            None
        }
    }

    fn read_precision_modifier(&mut self) -> Result<PrecisionModifier, LexError> {
        let mut modifier = String::new();

        while let Some(&ch) = self.peek() {
            if ch.is_ascii_digit() || ch == 'u' {
                modifier.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        match modifier.as_str() {
            "8" => Ok(PrecisionModifier::Integer8),
            "8u" => Ok(PrecisionModifier::Integer8u),
            "16" => Ok(PrecisionModifier::Integer16),
            "16u" => Ok(PrecisionModifier::Integer16u),
            "32" => Ok(PrecisionModifier::Integer32),
            "64" => Ok(PrecisionModifier::Integer64),
            _ => Err(LexError::InvalidPrecisionModifier {
                modifier,
                location: self.current_location(),
            }),
        }
    }

    /// Read identifier or keyword
    fn read_identifier_or_keyword(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;

        let mut identifier = String::new();

        // First character (already validated)
        if let Some(ch) = self.advance() {
            identifier.push(ch);
        }

        // Rest of identifier
        while let Some(&ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                identifier.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let text = self.source_text_range(start_pos, self.current_pos);

        // Check if it's a keyword
        if let Some(keyword_token) = Keywords::lookup(&identifier) {
            Ok(Token::new(keyword_token, start_location, text))
        } else {
            Ok(Token::new(
                TokenKind::Identifier(identifier),
                start_location,
                text,
            ))
        }
    }

    /// Handle slash (could be division or comment)
    fn handle_slash(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '/'

        match self.peek() {
            Some(&'/') => {
                // Single-line comment
                self.advance(); // Skip second '/'
                let mut comment = String::new();

                while let Some(&ch) = self.peek() {
                    if ch == '\n' {
                        break;
                    }
                    comment.push(ch);
                    self.advance();
                }

                Ok(Token::new(
                    TokenKind::Comment(comment.clone()),
                    start_location,
                    format!("//{}", comment),
                ))
            }
            Some(&'*') => {
                // Block comment
                self.advance(); // Skip '*'
                let mut comment = String::new();
                let mut has_newline = false;

                loop {
                    match self.advance() {
                        Some('*') if self.peek() == Some(&'/') => {
                            self.advance(); // Skip '/'
                            break;
                        }
                        Some('\n') => {
                            // Track that we've seen a newline in this block comment
                            has_newline = true;
                            comment.push('\n');
                        }
                        Some(ch) => comment.push(ch),
                        None => {
                            return Err(LexError::UnterminatedComment {
                                location: start_location.clone(),
                            });
                        }
                    }
                }

                // If the block comment contained newlines and we're at line start,
                // suppress indentation handling on the next line
                if has_newline && self.column == 1 {
                    self.suppress_indent_after_block_comment = true;
                    self.at_line_start = true;
                }

                Ok(Token::new(
                    TokenKind::BlockComment(comment.clone()),
                    start_location,
                    format!("/*{}*/", comment),
                ))
            }
            _ => {
                // Division operator
                Ok(Token::simple(TokenKind::Divide, start_location))
            }
        }
    }

    /// Handle hash-style comment (#)
    fn handle_hash_comment(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '#'

        let mut comment = String::new();

        while let Some(&ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            comment.push(ch);
            self.advance();
        }

        Ok(Token::new(
            TokenKind::Comment(comment.clone()),
            start_location,
            format!("#{}", comment),
        ))
    }

    /// Handle equals (= or ==)
    fn handle_equals(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '='

        if let Some(&'=') = self.peek() {
            self.advance(); // Skip second '='
            Ok(Token::simple(TokenKind::Equal, start_location))
        } else {
            Ok(Token::simple(TokenKind::Assign, start_location))
        }
    }

    /// Handle exclamation (!=)
    fn handle_exclamation(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '!'

        if let Some(&'=') = self.peek() {
            self.advance(); // Skip '='
            Ok(Token::simple(TokenKind::NotEqual, start_location))
        } else {
            Err(LexError::InvalidCharacter {
                char: '!',
                location: start_location,
            })
        }
    }

    /// Handle less than (< or <=)
    fn handle_less_than(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '<'

        if let Some(&'=') = self.peek() {
            self.advance(); // Skip '='
            Ok(Token::simple(TokenKind::LessEqual, start_location))
        } else {
            Ok(Token::simple(TokenKind::Less, start_location))
        }
    }

    /// Handle greater than (> or >=)
    fn handle_greater_than(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '>'

        if let Some(&'=') = self.peek() {
            self.advance(); // Skip '='
            Ok(Token::simple(TokenKind::GreaterEqual, start_location))
        } else {
            Ok(Token::simple(TokenKind::Greater, start_location))
        }
    }

    /// Handle dot (. or .. or floating-point literal starting with .)
    fn handle_dot(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;
        self.advance(); // Skip '.'

        // Check if this is a floating-point literal starting with . (e.g., .5, .25)
        if let Some(&ch) = self.peek() {
            if ch.is_ascii_digit() {
                // This is a floating-point literal like .5
                let mut number_text = String::from("0."); // Normalize to 0.5 format

                // Read fractional part
                while let Some(&digit) = self.peek() {
                    if digit.is_ascii_digit() {
                        number_text.push(digit);
                        self.advance();
                    } else {
                        break;
                    }
                }

                // Check for scientific notation
                if let Some(&exp) = self.peek() {
                    if exp == 'e' || exp == 'E' {
                        number_text.push(exp);
                        self.advance();

                        // Optional sign
                        if let Some(&sign) = self.peek() {
                            if sign == '+' || sign == '-' {
                                number_text.push(sign);
                                self.advance();
                            }
                        }

                        // Exponent digits
                        while let Some(&digit) = self.peek() {
                            if digit.is_ascii_digit() {
                                number_text.push(digit);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                }

                let text = self.source_text_range(start_pos, self.current_pos);
                let value: f64 = number_text.parse().map_err(|_| LexError::InvalidNumber {
                    text: text.clone(),
                    location: start_location.clone(),
                })?;

                return Ok(Token::new(
                    TokenKind::NumberLiteral(value),
                    start_location,
                    text,
                ));
            } else if ch == '.' {
                // Range operator (..)
                self.advance(); // Skip second '.'
                if let Some(&'=') = self.peek() {
                    self.advance(); // Skip '='
                    return Ok(Token::simple(TokenKind::RangeInclusive, start_location));
                } else {
                    return Ok(Token::simple(TokenKind::Range, start_location));
                }
            }
        }

        // Just a dot
        Ok(Token::simple(TokenKind::Dot, start_location))
    }

    // Helper methods

    fn peek(&mut self) -> Option<&char> {
        self.input.peek()
    }

    fn peek_chars(&mut self, count: usize) -> Option<Vec<char>> {
        // This is a simplified version - in practice would need proper lookahead
        let mut chars = Vec::new();
        let mut temp_input = self.input.clone();

        for _ in 0..count {
            if let Some(ch) = temp_input.next() {
                chars.push(ch);
            } else {
                return None;
            }
        }

        Some(chars)
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(ch) = self.input.next() {
            self.current_pos += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            Some(ch)
        } else {
            None
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(&ch) = self.peek() {
            if ch.is_whitespace() && ch != '\n' && ch != '\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn current_location(&self) -> SourceLocation {
        SourceLocation::new(self.line, self.column, &self.source_map.file_path)
    }

    fn source_text_range(&self, start: usize, end: usize) -> String {
        // Extract actual text from the source content using byte indices
        // start and end are byte positions tracked by current_pos
        if start <= end && end <= self.source_content.len() {
            self.source_content
                .get(start..end)
                .unwrap_or("")
                .to_string()
        } else {
            // If bounds are invalid, return empty string
            String::new()
        }
    }
}

#[derive(Debug, Clone)]
enum PrecisionModifier {
    Integer8,
    Integer8u,
    Integer16,
    Integer16u,
    Integer32,
    Integer64,
    Number32,
    Number64,
}

// Error types for lexer
#[derive(Debug, Clone, thiserror::Error)]
pub enum LexError {
    #[error("Invalid character '{char}' at {location}")]
    InvalidCharacter {
        char: char,
        location: SourceLocation,
    },

    #[error("Unterminated string literal at {location}")]
    UnterminatedString { location: SourceLocation },

    #[error("Unterminated comment at {location}")]
    UnterminatedComment { location: SourceLocation },

    #[error("Invalid number format '{text}' at {location}")]
    InvalidNumber {
        text: String,
        location: SourceLocation,
    },

    #[error("Invalid precision modifier '{modifier}' at {location}")]
    InvalidPrecisionModifier {
        modifier: String,
        location: SourceLocation,
    },

    #[error("Invalid indentation at {location}")]
    InvalidIndentation {
        expected_levels: Vec<usize>,
        found_level: usize,
        location: SourceLocation,
    },

    #[error("Indentation must use tabs only, not spaces, at {location}")]
    SpacesInIndentation { location: SourceLocation },
}

impl From<LexError> for CompilerError {
    fn from(error: LexError) -> Self {
        CompilerError::LexError(error)
    }
}

// Tests are in tests/specification_lexer_tests.rs
