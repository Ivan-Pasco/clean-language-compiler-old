//! Specification-compliant lexer implementation for Clean Language
//!
//! This lexer provides 100% compliance with the AST Specification, implementing
//! all language constructs exactly as defined in the Clean Language Specification.

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

        Ok(TokenStream::new(
            tokens,
            self.source_map.clone(),
            self.source_content.to_string(),
        ))
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

        self.next_token_after_indentation()
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

        // NOTE: Reject spaces at the beginning of lines (after tabs)
        // Clean Language mandates tabs-only for indentation
        if let Some(&' ') = self.peek() {
            // Found a space at line start - this is an error
            return Err(LexError::SpacesInIndentation {
                location: start_location,
            });
        }

        // A CRLF line ending (grammar.ebnf `NEWLINE = LF | CR , LF`): drop the
        // CR so a blank CRLF line is an `empty_line` like a blank LF line and
        // does not close the enclosing block.
        if self.peek_chars(2).is_some_and(|c| c == ['\r', '\n']) {
            self.advance();
        }

        // Check if this is an empty line or comment-only line
        if let Some(&'\n') = self.peek() {
            // Skip empty lines for indentation purposes - advance to next line and continue
            // Note: advance() already increments self.line when reading '\n'
            self.advance();
            // DON'T increment line again - advance() already did it
            self.at_line_start = true;
            // Return newline token instead of recursing
            return Ok(Token::simple(TokenKind::Newline, start_location));
        }

        // A comment-only line (`// ...`, or a `/* ... */` with nothing after
        // it) is not a statement line: it must not open or close a block.
        if self.at_comment_only_line() {
            self.at_line_start = false;
            return self.handle_slash();
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

    /// True when the rest of the current line is only a comment: a `//`
    /// comment, or a `/* ... */` block comment (possibly spanning lines)
    /// followed by nothing but whitespace or a `//` comment. Nothing is
    /// consumed.
    fn at_comment_only_line(&self) -> bool {
        let rest = &self.source_content[self.current_pos..];
        if rest.starts_with("//") {
            return true;
        }
        let Some(body) = rest.strip_prefix("/*") else {
            return false;
        };
        let Some(close) = body.find("*/") else {
            // Unterminated: let the comment reader report it.
            return true;
        };
        let after = &body[close + 2..];
        let line_rest = after.split('\n').next().unwrap_or("");
        let trimmed = line_rest.trim_start_matches([' ', '\t', '\r']);
        trimmed.is_empty() || trimmed.starts_with("//")
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

                // Framework attribute prefix (spec/grammar.ebnf framework_attribute)
                '@' => {
                    let loc = self.current_location();
                    self.advance();
                    Ok(Token::simple(TokenKind::At, loc))
                }

                // Nullable-type suffix — spec 04-type-system.md §"Nullable Types".
                // Byte-range extraction for framework blocks reads raw bytes, so
                // emitting a real Question token here does not affect html:/component:
                // block content capture.
                '?' => {
                    let loc = start_location.clone();
                    self.advance();
                    Ok(Token::simple(TokenKind::Question, loc))
                }

                // Characters that are not valid Clean Language syntax outside of
                // framework blocks (html:, component:, etc.) but MUST flow through the
                // token stream so plugin-owned blocks can capture them via byte-range
                // raw extraction. The parser's extract_block_content_raw() reads bytes
                // directly from the source — the token kind for these characters is
                // irrelevant, only their presence (so the lexer doesn't error) and their
                // byte position matter.
                //
                // Pass-through set:
                //   '\''         — html attribute values like alt='Photo'
                //   '&'          — html entities like &rarr;, &middot;
                //   '\\'         — escaped braces `\{` `\}` in html: text (CSS rules)
                //   '#'          — html fragment ids like href="#top" and the
                //                  "#{var}" pattern that precedes an interpolation
                //   non-ASCII    — raw text content like →, ©, emoji, multibyte chars
                //
                // If any of these appear outside a framework block, the parser will
                // catch them later as an unexpected-token error in the right context.
                // Reproduces LEX001 (fingerprint 41310e98b853...) and SYN001
                // (fingerprint 9bc6a62dab24...) when removed.
                '\'' | '&' | '#' | '\\' => {
                    let loc = start_location.clone();
                    let c = self.advance().unwrap();
                    Ok(Token::new(
                        TokenKind::Identifier(c.to_string()),
                        loc,
                        c.to_string(),
                    ))
                }

                c if !c.is_ascii() => {
                    let loc = start_location.clone();
                    let ch = self.advance().unwrap();
                    Ok(Token::new(
                        TokenKind::Identifier(ch.to_string()),
                        loc,
                        ch.to_string(),
                    ))
                }

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
        let mut has_interpolation = false;

        // Save state to restart if we find interpolation
        let saved_pos = self.current_pos;
        let saved_line = self.line;
        let saved_column = self.column;

        while let Some(ch) = self.peek() {
            match *ch {
                '"' => break,
                '\\' => {
                    self.advance(); // Skip backslash
                    self.advance(); // Skip the escaped character
                }
                '{' => {
                    let (is_interpolation, scanned) = self.scan_brace_interpolation();
                    if is_interpolation {
                        has_interpolation = true;
                        break;
                    }
                    // Not interpolation: the braces are literal text. Skip what the
                    // probe already looked at and keep scanning.
                    for _ in 0..scanned {
                        self.advance();
                    }
                }
                '\n' => {
                    return Err(LexError::UnterminatedString {
                        location: start_location.clone(),
                    });
                }
                _ => {
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
            self.read_simple_string(start_location, start_pos)
        } else {
            // Interpolated string - parse and buffer all tokens
            self.read_interpolated_string(start_location, start_pos)
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
            match *ch {
                '"' => {
                    self.advance(); // Skip closing quote
                    let text = self.source_text_range(start_pos, self.current_pos);
                    return Ok(Token::new(
                        TokenKind::StringLiteral(content),
                        start_location,
                        text,
                    ));
                }
                '\\' => {
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
                '\n' => {
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

        while let Some(&ch) = self.peek() {
            match ch {
                '"' => {
                    // End of string
                    parts.push(InterpolationPart::String(current_string));
                    break;
                }
                '\\' => {
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
                '{' if !self.scan_brace_interpolation().0 => {
                    // Literal brace (same decision the pre-scan made)
                    current_string.push('{');
                    self.advance();
                }
                '{' => {
                    // Start of interpolation expression
                    parts.push(InterpolationPart::String(std::mem::take(
                        &mut current_string,
                    )));

                    self.advance(); // Skip '{'

                    // Parse expression tokens until we find matching '}'
                    let expr_tokens = self.read_interpolation_expression()?;
                    parts.push(InterpolationPart::Expression(expr_tokens));
                }
                '\n' => {
                    return Err(LexError::UnterminatedString {
                        location: start_location.clone(),
                    });
                }
                _ => {
                    current_string.push(ch);
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

    /// Decide whether the `{` at the current position opens a string
    /// interpolation. Nothing is consumed. Returns the decision and the number
    /// of characters the probe looked at (from the `{` up to where it stopped),
    /// which the pre-scan skips when the braces are literal.
    ///
    /// A leading expression-start character is the cheap heuristic for "this
    /// might be an interpolation expression". But that alone is not enough — a
    /// string like "color: { family: Inter, weight: 700 }" also starts with a
    /// letter after `{` yet contains characters (':') that never appear inside a
    /// Clean expression. The braced content is walked; a definite
    /// non-expression character or an unclosed brace means `{` is a literal
    /// byte. That way the user does not have to escape every '{' in DSL/config
    /// snippets.
    ///
    /// Expression starts: identifier (alpha / underscore), numeric literal
    /// (digit), parenthesized sub-expression, unary operator, list literal, or
    /// nested string literal. Anything else (':', whitespace-only, closing
    /// brace, etc.) is treated as a literal '{'.
    fn scan_brace_interpolation(&self) -> (bool, usize) {
        let mut ahead = self.input.clone().peekable();
        let mut scanned = 0usize;

        // The '{' itself
        if ahead.next() != Some('{') {
            return (false, 0);
        }
        scanned += 1;

        // Leading whitespace (same set skip_whitespace() skips)
        while let Some(&c) = ahead.peek() {
            if c.is_whitespace() && c != '\n' && c != '\t' {
                ahead.next();
                scanned += 1;
            } else {
                break;
            }
        }

        let is_expression_start = match ahead.peek() {
            Some(&c) => {
                c.is_alphabetic()
                    || c == '_'
                    || c.is_ascii_digit()
                    || matches!(c, '(' | '-' | '+' | '!' | '[' | '"')
            }
            // End of input after '{' - not interpolation
            None => return (false, scanned),
        };
        if !is_expression_start {
            return (false, scanned);
        }

        let mut sub_depth: u32 = 1;
        let mut paren_depth: u32 = 0;
        let mut bracket_depth: u32 = 0;
        // Last non-whitespace character and the identifier/keyword ending
        // there, to tell a nested string literal (`{f("q")}`) from the
        // closing quote of the outer string (`"a {b" + "c}"`).
        let mut prev = '{';
        let mut word = String::new();
        while let Some(&c) = ahead.peek() {
            match c {
                '{' => sub_depth += 1,
                '}' => {
                    sub_depth -= 1;
                    if sub_depth == 0 {
                        return (true, scanned + 1);
                    }
                }
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                // A quote where an operand is expected opens a nested string
                // literal (grammar.ebnf `string_interpolation` holds a full
                // expression): skip it so its quotes and braces are not taken
                // for the outer string's.
                '"' if Self::operand_expected(prev, &word) => {
                    // On failure the outer string ends at this quote, so the
                    // probe reports only what came before it.
                    let quote_at = scanned;
                    ahead.next();
                    scanned += 1;
                    loop {
                        match ahead.next() {
                            Some('"') => break,
                            Some('\\') => {
                                if ahead.next().is_none() {
                                    return (false, quote_at);
                                }
                                scanned += 1;
                            }
                            Some('\n') | None => return (false, quote_at),
                            Some(_) => {}
                        }
                        scanned += 1;
                    }
                    scanned += 1;
                    prev = '"';
                    word.clear();
                    continue;
                }
                // End-of-string or newline before the matching brace — the user
                // did not close the interpolation, treat as literal.
                '"' | '\n' => return (false, scanned),
                // Characters that never appear in a Clean expression. ':' is the
                // block-header marker; ';' / '#' / '@' / '$' / '?' are not in the
                // expression grammar either. Their presence means the braces are
                // not interpolation.
                ':' | ';' | '#' | '@' | '$' | '?' => return (false, scanned),
                // A top-level ',' (outside any nested '(' or '[') cannot appear in
                // a single logical_expression — commas are only legal as
                // separators inside call-arg lists or list/matrix literals. Seeing
                // one at the outer brace level means the braces are a literal
                // DSL/glob pattern like "{a,b}" or "Hero.{design,behavior}.md",
                // not an interpolation.
                ',' if paren_depth == 0 && bracket_depth == 0 => return (false, scanned),
                _ => {}
            }
            if c.is_alphanumeric() || c == '_' {
                if !(prev.is_alphanumeric() || prev == '_') {
                    word.clear();
                }
                word.push(c);
                prev = c;
            } else if !c.is_whitespace() {
                prev = c;
                word.clear();
            }
            ahead.next();
            scanned += 1;
        }

        // Ran off the end of the input: the string itself is unterminated and
        // the interpolated-string reader reports it.
        (true, scanned)
    }

    /// Whether an operand (here: a nested string literal) may follow the
    /// interpolation text seen so far, given its last non-whitespace
    /// character and the word ending there.
    ///
    /// A quote right after the opening `{` is not included: `"{" + x + "}"`
    /// is the common way to build braces, so there the quote ends the outer
    /// string and the brace is literal text.
    fn operand_expected(prev: char, word: &str) -> bool {
        matches!(
            prev,
            '(' | '[' | ',' | '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '^'
        ) || matches!(
            word,
            "and" | "or" | "not" | "is" | "default" | "onError" | "then" | "else"
        )
    }

    /// Read expression tokens inside interpolation {}.
    ///
    /// The content is a full `logical_expression` (grammar.ebnf
    /// `string_interpolation`), so tokens come from the regular tokenizer.
    fn read_interpolation_expression(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        let mut brace_depth = 1; // We already consumed the opening '{'

        loop {
            self.skip_whitespace();
            let start_location = self.current_location();

            match self.peek() {
                Some(&'}') => {
                    self.advance();
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        return Ok(tokens);
                    }
                    tokens.push(Token::simple(TokenKind::RightBrace, start_location));
                }
                Some(&'{') => {
                    self.advance();
                    brace_depth += 1;
                    tokens.push(Token::simple(TokenKind::LeftBrace, start_location));
                }
                None | Some(&'\n') => {
                    return Err(LexError::UnterminatedString {
                        location: start_location,
                    });
                }
                Some(_) => {
                    tokens.push(self.next_token_after_indentation()?);
                    // A nested interpolated string literal buffers its remaining
                    // tokens; they belong to this expression, in order.
                    tokens.append(&mut self.token_buffer);
                }
            }
        }
    }

    /// Emit interpolation tokens to the buffer.
    ///
    /// `parts` alternates `String, Expression, String, ..., String`: the reader
    /// pushes the text before every `{` and the text before the closing quote.
    /// The first text becomes `InterpolationStart`, the last `InterpolationEnd`
    /// and every one in between `InterpolationMid` — even when empty, because
    /// the Mid token is what separates adjacent interpolations like `{a}{b}`.
    fn emit_interpolation_tokens(
        &mut self,
        parts: Vec<InterpolationPart>,
        start_location: SourceLocation,
    ) -> Result<(), LexError> {
        let last = parts.len().saturating_sub(1);
        for (i, part) in parts.into_iter().enumerate() {
            match part {
                InterpolationPart::String(text) => {
                    let token_kind = if i == 0 {
                        TokenKind::InterpolationStart
                    } else if i == last {
                        TokenKind::InterpolationEnd
                    } else {
                        TokenKind::InterpolationMid
                    };
                    self.token_buffer
                        .push(Token::new(token_kind, start_location.clone(), text));
                }
                InterpolationPart::Expression(tokens) => self.token_buffer.extend(tokens),
            }
        }

        Ok(())
    }

    /// Read number literal with optional precision modifier
    fn read_number_literal(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        let start_pos = self.current_pos;

        let mut number_text = String::new();
        let mut is_float = false;

        // Base-prefixed integer detection: 0x..., 0b..., 0o...
        if let Some(token) = self.read_base_prefixed_integer(&start_location, start_pos) {
            return token;
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
        if self.read_exponent(&mut number_text) {
            is_float = true;
        }

        // Check for precision modifier - only if followed by valid digits
        if let Some(&':') = self.peek() {
            // Look ahead to see if this is actually a precision modifier
            if self.is_valid_precision_modifier_ahead() {
                self.advance(); // Skip ':'
                let precision = self.read_precision_modifier(is_float)?;

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
                    let value = Self::decimal_integer(&number_text).ok_or_else(|| {
                        LexError::InvalidNumber {
                            text: text.clone(),
                            location: start_location.clone(),
                        }
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
                let value =
                    Self::decimal_integer(&number_text).ok_or_else(|| LexError::InvalidNumber {
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

    /// Value of a decimal integer literal. `9223372036854775808` (2^63) does
    /// not fit in i64 on its own but is the magnitude of `i64::MIN`, so it
    /// lexes as `i64::MIN`; the parser accepts it only right after a unary
    /// minus and reports it as out of range anywhere else.
    fn decimal_integer(text: &str) -> Option<i64> {
        match text.parse::<i64>() {
            Ok(value) => Some(value),
            Err(_) if text.parse::<u64>() == Ok(1u64 << 63) => Some(i64::MIN),
            Err(_) => None,
        }
    }

    /// Read a base-prefixed integer literal (`0x1F`, `0b101`, `0o17`).
    ///
    /// The prefix is only taken when the character after it is a valid digit of
    /// that base; otherwise `None` is returned and nothing is consumed, so `0`
    /// lexes as a plain integer and the rest as an identifier. That keeps CSS
    /// hex colours such as `#0b5ed7` (captured raw by `style:` blocks) from
    /// being reported as malformed numbers.
    fn read_base_prefixed_integer(
        &mut self,
        start_location: &SourceLocation,
        start_pos: usize,
    ) -> Option<Result<Token, LexError>> {
        let chars = self.peek_chars(3)?;
        let radix = match &chars[..2] {
            ['0', 'x' | 'X'] => 16,
            ['0', 'b' | 'B'] => 2,
            ['0', 'o' | 'O'] => 8,
            _ => return None,
        };
        if !chars[2].is_digit(radix) {
            return None;
        }

        self.advance();
        self.advance();
        let mut digits = String::new();
        while let Some(&ch) = self.peek() {
            if ch.is_digit(radix) {
                digits.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let text = self.source_text_range(start_pos, self.current_pos);
        Some(match i64::from_str_radix(&digits, radix) {
            Ok(value) => Ok(Token::new(
                TokenKind::IntegerLiteral(value),
                start_location.clone(),
                text,
            )),
            Err(_) => Err(LexError::InvalidNumber {
                text,
                location: start_location.clone(),
            }),
        })
    }

    /// Consume a scientific-notation exponent (`e3`, `E-2`, `e+10`) into
    /// `number_text` and return true. The `e`/`E` is only taken when digits
    /// follow (optionally after a sign); otherwise nothing is consumed, so a
    /// CSS unit like `1em` lexes as `1` followed by the identifier `em`.
    fn read_exponent(&mut self, number_text: &mut String) -> bool {
        let mut ahead = self.input.clone();
        if !matches!(ahead.next(), Some('e' | 'E')) {
            return false;
        }
        let has_digits = match ahead.next() {
            Some('+' | '-') => ahead.next().is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        };
        if !has_digits {
            return false;
        }

        // 'e' / 'E' and the optional sign
        if let Some(exp) = self.advance() {
            number_text.push(exp);
        }
        if let Some(&sign @ ('+' | '-')) = self.peek() {
            number_text.push(sign);
            self.advance();
        }
        while let Some(&digit) = self.peek() {
            if digit.is_ascii_digit() {
                number_text.push(digit);
                self.advance();
            } else {
                break;
            }
        }
        true
    }

    /// Read precision modifier (8, 8u, 16, 16u, 32, 64)
    /// Check if what follows the current ':' looks like a valid precision modifier
    fn is_valid_precision_modifier_ahead(&self) -> bool {
        // Collect the potential modifier string and check if it's valid
        let mut modifier = String::new();
        let mut check_pos = 1; // Start after the ':'
        let max_lookahead = 4; // Prevent infinite loops

        while check_pos <= max_lookahead {
            if let Some(ch) = self.peek_at_offset(check_pos) {
                if ch.is_ascii_digit() || ch == 'u' {
                    modifier.push(ch);
                    check_pos += 1;
                } else {
                    break;
                }
            } else {
                // End of input
                break;
            }
        }

        // Check if it matches known precision modifiers
        matches!(modifier.as_str(), "8" | "8u" | "16" | "16u" | "32" | "64")
    }

    fn peek_at_offset(&self, offset: usize) -> Option<char> {
        // Get a slice starting from current position, then get the nth character
        self.source_content
            .get(self.current_pos..)?
            .chars()
            .nth(offset)
    }

    fn read_precision_modifier(&mut self, is_float: bool) -> Result<PrecisionModifier, LexError> {
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
            "32" => {
                if is_float {
                    Ok(PrecisionModifier::Number32)
                } else {
                    Ok(PrecisionModifier::Integer32)
                }
            }
            "64" => {
                if is_float {
                    Ok(PrecisionModifier::Number64)
                } else {
                    Ok(PrecisionModifier::Integer64)
                }
            }
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

    /// Handle exclamation (! or !=)
    // BOOK: required-operator - Standalone ! is the required assertion operator
    fn handle_exclamation(&mut self) -> Result<Token, LexError> {
        let start_location = self.current_location();
        self.advance(); // Skip '!'

        if let Some(&'=') = self.peek() {
            self.advance(); // Skip '='
            Ok(Token::simple(TokenKind::NotEqual, start_location))
        } else {
            // BOOK: required-operator - Standalone ! is the required assertion operator
            Ok(Token::simple(TokenKind::Bang, start_location))
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
                self.read_exponent(&mut number_text);

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
        SourceLocation::with_byte_span(
            self.line,
            self.column,
            &self.source_map.file_path,
            self.current_pos,
        )
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

impl LexError {
    /// Where the error was found. Every lexer error carries the position of
    /// the offending character, so diagnostics never fall back to 1:1.
    pub fn location(&self) -> &SourceLocation {
        match self {
            LexError::InvalidCharacter { location, .. }
            | LexError::UnterminatedString { location }
            | LexError::UnterminatedComment { location }
            | LexError::InvalidNumber { location, .. }
            | LexError::InvalidPrecisionModifier { location, .. }
            | LexError::InvalidIndentation { location, .. }
            | LexError::SpacesInIndentation { location } => location,
        }
    }

    /// The spec error code (foundation/spec/error-codes.md, SYN001-SYN005).
    pub fn error_code(&self) -> &'static str {
        match self {
            LexError::InvalidCharacter { .. } => "SYN001",
            LexError::InvalidIndentation { .. } | LexError::SpacesInIndentation { .. } => "SYN003",
            LexError::UnterminatedString { .. } | LexError::UnterminatedComment { .. } => "SYN004",
            LexError::InvalidNumber { .. } | LexError::InvalidPrecisionModifier { .. } => "SYN005",
        }
    }

    fn help(&self) -> String {
        match self {
            LexError::InvalidCharacter { .. } => {
                "This character is only allowed inside strings, comments and framework blocks."
            }
            LexError::UnterminatedString { .. } => {
                "Close the string literal with the matching quote."
            }
            LexError::UnterminatedComment { .. } => "Close the comment with `*/`.",
            LexError::InvalidNumber { .. } => {
                "Integer literals must fit in 64 bits (-9223372036854775808 to 9223372036854775807)."
            }
            LexError::InvalidPrecisionModifier { .. } => {
                "Valid precision modifiers are :8, :8u, :16, :16u, :32 and :64."
            }
            LexError::InvalidIndentation { .. } => {
                "Dedent to the indentation level of an enclosing block."
            }
            LexError::SpacesInIndentation { .. } => "Indent with tabs only.",
        }
        .to_string()
    }
}

impl From<LexError> for CompilerError {
    /// Every lexer error becomes a located syntax error, so the reporter and
    /// the language server point at the offending character.
    fn from(error: LexError) -> Self {
        CompilerError::syntax_error_with_code(
            error.to_string(),
            Some(error.help()),
            Some(error.location().clone()),
            error.error_code(),
        )
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    fn tokenize(input: &str) -> Result<Vec<Token>, LexError> {
        let source = SourceCode::new(input.to_string(), "test.cln".to_string());
        let mut lexer = SpecificationLexer::new(&source);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    /// Regression: the `?` character must not raise LEX001 (Invalid character).
    /// Framework EBNFs use `T?` for nullable-type suffixes (frame-data.ebnf
    /// `class_name , "?"`), so `?` flows through the lexer as a pass-through
    /// identifier token. Downstream parsing may still reject it until a
    /// nullable-type production lands in core grammar, but that must be a
    /// parser error, not a lex error. Dashboard fingerprint #396dfcb027fc.
    #[test]
    fn question_mark_passthrough_not_lex_error() {
        assert!(
            tokenize("string?").is_ok(),
            "`?` after type name must not be a lex error",
        );
        assert!(
            tokenize("Page?").is_ok(),
            "`?` after class name must not be a lex error",
        );
    }

    /// Regression: a blank line in a CRLF file (`\r\n`) is an `empty_line`
    /// (grammar.ebnf `NEWLINE = LF | CR , LF`). The lexer only recognised a
    /// bare LF as blank, so the CR made it an unindented line that emitted a
    /// Dedent and closed the enclosing block — statements after the blank line
    /// fell out of `start:` and failed to parse.
    #[test]
    fn crlf_blank_line_keeps_block_open() {
        let lf = "start:\n\tinteger a = 1\n\n\tprint(a)\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(kinds(&crlf), kinds(lf));
        let tabbed = "start:\r\n\tinteger a = 1\r\n\t\r\n\tprint(a)\r\n";
        assert_eq!(kinds(tabbed), kinds(&tabbed.replace("\r\n", "\n")));
    }

    fn kinds(input: &str) -> Vec<TokenKind> {
        tokenize(input)
            .unwrap_or_else(|e| panic!("`{}` must lex, got {}", input, e))
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    /// Regression: CSS inside `style:` blocks flows through the lexer before the
    /// plugin captures the raw bytes. A unit suffix starting with `e` (`1em`,
    /// `1.5em`) or a hex colour starting with a base prefix (`#0b5ed7`,
    /// `#0x...`-looking digits) must not be read as a malformed number.
    #[test]
    fn css_units_and_hex_colours_are_not_number_errors() {
        for src in [
            "padding: 1em;",
            "margin: 1.5em;",
            "width: .5em;",
            "color: #2ecc71;",
            "color: #0b5ed7;",
            "color: #0o0;",
            "color: #0xg;",
        ] {
            assert!(
                tokenize(src).is_ok(),
                "`{}` must lex: {:?}",
                src,
                tokenize(src).err()
            );
        }
        assert_eq!(
            kinds("1em")[..2],
            [
                TokenKind::IntegerLiteral(1),
                TokenKind::Identifier("em".to_string())
            ]
        );
        assert_eq!(
            kinds("0b5ed7")[..2],
            [
                TokenKind::IntegerLiteral(0),
                TokenKind::Identifier("b5ed7".to_string())
            ]
        );
    }

    /// Regression: a line holding only a `/* ... */` comment is a comment
    /// line like `// ...`. Unindented, it used to emit a Dedent and close the
    /// enclosing block, so the following statement fell out of `start:`.
    #[test]
    fn block_comment_only_line_keeps_block_open() {
        let plain = "start:\n\tinteger a = 1\n\tprint(a)\n";
        let layout = |src: &str| -> Vec<TokenKind> {
            kinds(src)
                .into_iter()
                .filter(|k| {
                    !matches!(
                        k,
                        TokenKind::Comment(_) | TokenKind::BlockComment(_) | TokenKind::Newline
                    )
                })
                .collect()
        };
        for src in [
            "start:\n\tinteger a = 1\n/* c */\n\tprint(a)\n",
            "start:\n\tinteger a = 1\n\t\t\t/* c */ // d\n\tprint(a)\n",
            "start:\n\tinteger a = 1\n/* one\ntwo */\n\tprint(a)\n",
        ] {
            assert_eq!(layout(src), layout(plain), "{}", src.escape_debug());
        }
        // Code after the comment keeps its indentation.
        assert!(kinds("start:\n\t/* c */ print(1)\n").contains(&TokenKind::Indent(1)));
    }

    /// Regression: a string literal inside an interpolation (`{f("q")}`)
    /// ended the outer string at its first quote.
    #[test]
    fn string_literal_inside_interpolation() {
        let k = kinds("\"v={f(\"q\")} w={a + \"}\"}\"");
        assert!(
            k.contains(&TokenKind::StringLiteral("q".to_string())),
            "{:?}",
            k
        );
        assert!(
            k.contains(&TokenKind::StringLiteral("}".to_string())),
            "{:?}",
            k
        );
        assert_eq!(
            k.iter()
                .filter(|k| matches!(k, TokenKind::InterpolationEnd))
                .count(),
            1,
            "{:?}",
            k
        );
        // A quote right after `{` closes the outer string: building braces
        // by concatenation keeps working.
        let k = kinds("json = \"{\" + body + \"}\"\nx = 1");
        assert_eq!(k[2], TokenKind::StringLiteral("{".to_string()), "{:?}", k);
        assert_eq!(k[6], TokenKind::StringLiteral("}".to_string()), "{:?}", k);
        let k = kinds("r = r.replace(\"{\", \"\\\\{\")\nx = 1");
        assert!(
            k.contains(&TokenKind::StringLiteral("{".to_string())),
            "{:?}",
            k
        );
        // A quote after an operand still closes the outer string: the brace
        // stays literal text.
        let k = kinds("\"a {b\" + \"c}\"");
        assert_eq!(
            k[..3],
            [
                TokenKind::StringLiteral("a {b".to_string()),
                TokenKind::Plus,
                TokenKind::StringLiteral("c}".to_string()),
            ]
        );
    }

    /// Real exponents and base-prefixed integers keep working.
    #[test]
    fn exponents_and_base_prefixes_still_lex() {
        assert_eq!(kinds("1e3")[0], TokenKind::NumberLiteral(1000.0));
        assert_eq!(kinds("2.5E-1")[0], TokenKind::NumberLiteral(0.25));
        assert_eq!(kinds(".5e+1")[0], TokenKind::NumberLiteral(5.0));
        assert_eq!(kinds("0xFF")[0], TokenKind::IntegerLiteral(255));
        assert_eq!(kinds("0b101")[0], TokenKind::IntegerLiteral(5));
        assert_eq!(kinds("0o17")[0], TokenKind::IntegerLiteral(15));
    }

    /// Regression: an interpolation holds a full `logical_expression`
    /// (grammar.ebnf `string_interpolation`), so every binary operator must
    /// lex inside `{...}`, and adjacent interpolations need a separator token.
    #[test]
    fn interpolation_accepts_all_operators_and_adjacent_parts() {
        for src in [
            "\"r {a > 2}\"",
            "\"r {a == 7}\"",
            "\"r {a != 7}\"",
            "\"r {a % 2}\"",
            "\"r {a <= b}\"",
            "\"r {a ^ 2}\"",
            "\"r {a > 2 and b < 3}\"",
        ] {
            assert!(
                tokenize(src).is_ok(),
                "`{}` must lex: {:?}",
                src,
                tokenize(src).err()
            );
        }
        let k = kinds("\"r {a}{b}\"");
        assert_eq!(
            k.iter()
                .filter(|k| matches!(k, TokenKind::InterpolationMid))
                .count(),
            1,
            "adjacent interpolations need an InterpolationMid separator: {:?}",
            k
        );
    }
}
