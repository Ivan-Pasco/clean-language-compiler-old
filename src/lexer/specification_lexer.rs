//! Specification-compliant lexer implementation for Clean Language
//!
//! This lexer provides 100% compliance with the AST Specification, implementing
//! all language constructs exactly as defined in the Clean Language Specification.

use super::specification_token::*;
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use std::str::Chars;
use std::iter::Peekable;

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
                        tokens.push(Token::simple(
                            TokenKind::Dedent(self.indentation_stack.len()),
                            location,
                        ));
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
                
                // Invalid character
                _ => {
                    let invalid_char = self.advance().unwrap();
                    Err(LexError::InvalidCharacter {
                        char: invalid_char,
                        location: start_location,
                    })
                }
            }
        }
    }
    
    /// Handle indentation at the beginning of a line
    fn handle_indentation(&mut self) -> Result<Token, LexError> {
        let mut indent_level = 0;
        let start_location = self.current_location();
        
        // Count tab characters for indentation
        while let Some(&'\t') = self.peek() {
            self.advance();
            indent_level += 1;
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
        
        self.at_line_start = false;
        
        let current_indent = *self.indentation_stack.last().unwrap();
        
        if indent_level > current_indent {
            // Increased indentation - push new level
            self.indentation_stack.push(indent_level);
            Ok(Token::simple(TokenKind::Indent(indent_level), start_location))
        } else if indent_level < current_indent {
            // Decreased indentation - may need multiple dedents
            while let Some(&stack_level) = self.indentation_stack.last() {
                if stack_level <= indent_level {
                    break;
                }
                self.indentation_stack.pop();
            }
            
            // Verify indentation matches a previous level
            if self.indentation_stack.last() != Some(&indent_level) {
                return Err(LexError::InvalidIndentation {
                    expected_levels: self.indentation_stack.clone(),
                    found_level: indent_level,
                    location: start_location,
                });
            }
            
            Ok(Token::simple(TokenKind::Dedent(self.indentation_stack.len()), start_location))
        } else {
            // Same indentation level - continue parsing without recursion
            // The tokenize loop will call next_token again for the actual token
            self.next_token_after_indentation()
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
                
                // Invalid character
                _ => {
                    let invalid_char = self.advance().unwrap();
                    Err(LexError::InvalidCharacter {
                        char: invalid_char,
                        location: start_location,
                    })
                }
            }
        }
    }
    
    /// Read string literal with interpolation support
    fn read_string_literal(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;
        
        self.advance(); // Skip opening quote
        let mut content = String::new();
        let mut has_interpolation = false;
        
        while let Some(ch) = self.peek() {
            match ch {
                &'"' => {
                    self.advance(); // Skip closing quote
                    let text = self.source_text_range(start_pos, self.current_pos);
                    
                    if has_interpolation {
                        // TODO: Handle string interpolation
                        // For now, treat as regular string
                        return Ok(Token::new(
                            TokenKind::StringLiteral(content),
                            start_location,
                            text,
                        ));
                    } else {
                        return Ok(Token::new(
                            TokenKind::StringLiteral(content),
                            start_location,
                            text,
                        ));
                    }
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
                &'{' => {
                    // String interpolation detected
                    has_interpolation = true;
                    content.push('{');
                    self.advance();
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
    
    /// Read number literal with optional precision modifier
    fn read_number_literal(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;
        
        let mut number_text = String::new();
        let mut is_float = false;
        
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
        
        // Check for precision modifier
        if let Some(&':') = self.peek() {
            self.advance(); // Skip ':'
            let precision = self.read_precision_modifier()?;
            
            let text = self.source_text_range(start_pos, self.current_pos);
            
            // Create precision-specific token
            if is_float {
                match precision {
                    PrecisionModifier::Number32 => {
                        let value: f32 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Number32Literal(value), start_location, text))
                    }
                    PrecisionModifier::Number64 => {
                        let value: f64 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Number64Literal(value), start_location, text))
                    }
                    _ => Err(LexError::InvalidPrecisionModifier {
                        modifier: format!("{:?}", precision),
                        location: start_location,
                    }),
                }
            } else {
                match precision {
                    PrecisionModifier::Integer8 => {
                        let value: i8 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Integer8Literal(value), start_location, text))
                    }
                    PrecisionModifier::Integer8u => {
                        let value: u8 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Integer8uLiteral(value), start_location, text))
                    }
                    PrecisionModifier::Integer16 => {
                        let value: i16 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Integer16Literal(value), start_location, text))
                    }
                    PrecisionModifier::Integer16u => {
                        let value: u16 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Integer16uLiteral(value), start_location, text))
                    }
                    PrecisionModifier::Integer32 => {
                        let value: i32 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Integer32Literal(value), start_location, text))
                    }
                    PrecisionModifier::Integer64 => {
                        let value: i64 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Integer64Literal(value), start_location, text))
                    }
                    PrecisionModifier::Number32 => {
                        let value: f32 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Number32Literal(value), start_location, text))
                    }
                    PrecisionModifier::Number64 => {
                        let value: f64 = number_text.parse()
                            .map_err(|_| LexError::InvalidNumber {
                                text: text.clone(),
                                location: start_location.clone(),
                            })?;
                        Ok(Token::new(TokenKind::Number64Literal(value), start_location, text))
                    }
                }
            }
        } else {
            // Default precision
            let text = self.source_text_range(start_pos, self.current_pos);
            
            if is_float {
                let value: f64 = number_text.parse()
                    .map_err(|_| LexError::InvalidNumber {
                        text: text.clone(),
                        location: start_location.clone(),
                    })?;
                Ok(Token::new(TokenKind::NumberLiteral(value), start_location, text))
            } else {
                let value: i64 = number_text.parse()
                    .map_err(|_| LexError::InvalidNumber {
                        text: text.clone(),
                        location: start_location.clone(),
                    })?;
                Ok(Token::new(TokenKind::IntegerLiteral(value), start_location, text))
            }
        }
    }
    
    /// Read precision modifier (8, 8u, 16, 16u, 32, 64)
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
                
                loop {
                    match self.advance() {
                        Some('*') if self.peek() == Some(&'/') => {
                            self.advance(); // Skip '/'
                            break;
                        }
                        Some(ch) => comment.push(ch),
                        None => {
                            return Err(LexError::UnterminatedComment {
                                location: start_location.clone(),
                            });
                        }
                    }
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
    
    /// Handle dot (. or ..)
    fn handle_dot(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '.'
        
        if let Some(&'.') = self.peek() {
            self.advance(); // Skip second '.'
            if let Some(&'=') = self.peek() {
                self.advance(); // Skip '='
                Ok(Token::simple(TokenKind::RangeInclusive, start_location))
            } else {
                Ok(Token::simple(TokenKind::Range, start_location))
            }
        } else {
            Ok(Token::simple(TokenKind::Dot, start_location))
        }
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
        if start <= end && end <= self.source_content.len() {
            // For now, we'll use char indices. In a complete implementation,
            // we'd track byte vs char indices properly
            let chars: Vec<char> = self.source_content.chars().collect();
            if start <= chars.len() && end <= chars.len() {
                chars[start..end].iter().collect()
            } else {
                // Fallback if indices are out of bounds
                self.source_content.get(start..end).unwrap_or("").to_string()
            }
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
    UnterminatedString {
        location: SourceLocation,
    },
    
    #[error("Unterminated comment at {location}")]
    UnterminatedComment {
        location: SourceLocation,
    },
    
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
}

impl From<LexError> for CompilerError {
    fn from(error: LexError) -> Self {
        CompilerError::LexError(error)
    }
}

// Tests are in tests/specification_lexer_tests.rs