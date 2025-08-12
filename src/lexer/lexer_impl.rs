//! Lexer implementation for Clean Language

use super::*;
use crate::error::CompilerError;

/// Implementation of the Clean Language lexer
pub struct CleanLexer {
    input: Vec<char>,
    position: usize,
    line: usize,
    column: usize,
    current_token: Option<Token>,
}

impl CleanLexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            position: 0,
            line: 1,
            column: 1,
            current_token: None,
        }
    }

    fn current_char(&self) -> Option<char> {
        self.input.get(self.position).copied()
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(ch) = self.current_char() {
            self.position += 1;
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

    fn peek(&self) -> Option<char> {
        self.input.get(self.position + 1).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current_char() {
            if ch.is_whitespace() && ch != '\n' && ch != '\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self) -> LexResult<String> {
        let mut string = String::new();
        self.advance(); // Skip opening quote

        while let Some(ch) = self.current_char() {
            if ch == '"' {
                self.advance(); // Skip closing quote
                return Ok(string);
            } else if ch == '\\' {
                self.advance();
                match self.current_char() {
                    Some('n') => string.push('\n'),
                    Some('t') => string.push('\t'),
                    Some('r') => string.push('\r'),
                    Some('\\') => string.push('\\'),
                    Some('"') => string.push('"'),
                    Some('{') => string.push('{'),
                    Some('}') => string.push('}'),
                    Some('0') => string.push('\0'),
                    Some(other) => {
                        // For unknown escape sequences, include both backslash and character
                        string.push('\\');
                        string.push(other);
                    }
                    None => {
                        return Err(CompilerError::syntax_error(
                            "Unterminated string escape",
                            None,
                            None,
                        ))
                    }
                }
                self.advance();
            } else {
                string.push(ch);
                self.advance();
            }
        }

        Err(CompilerError::syntax_error(
            "Unterminated string literal",
            None,
            None,
        ))
    }

    /// Check if a string contains interpolation markers
    #[allow(dead_code)]
    fn has_interpolation(s: &str) -> bool {
        s.contains('{') && s.contains('}')
    }

    fn read_number(&mut self) -> LexResult<TokenKind> {
        // Check for different number bases
        if let Some('0') = self.current_char() {
            if let Some(next) = self.peek() {
                match next {
                    'x' | 'X' => return self.read_hex_number(),
                    'b' | 'B' => return self.read_binary_number(),
                    'o' | 'O' => return self.read_octal_number(),
                    _ => {} // Continue with decimal parsing
                }
            }
        }

        // Decimal number parsing (existing logic enhanced)
        let mut number = String::new();
        let mut is_float = false;
        let mut has_scientific = false;

        while let Some(ch) = self.current_char() {
            if ch.is_ascii_digit() {
                number.push(ch);
                self.advance();
            } else if ch == '.'
                && !is_float
                && !has_scientific
                && self.peek().map_or(false, |c| c.is_ascii_digit())
            {
                is_float = true;
                number.push(ch);
                self.advance();
            } else if (ch == 'e' || ch == 'E') && !has_scientific {
                has_scientific = true;
                is_float = true;
                number.push(ch);
                self.advance();
                // Handle optional +/- after e/E
                if let Some(sign) = self.current_char() {
                    if sign == '+' || sign == '-' {
                        number.push(sign);
                        self.advance();
                    }
                }
            } else {
                break;
            }
        }

        if is_float {
            number
                .parse::<f64>()
                .map(TokenKind::Number)
                .map_err(|_| CompilerError::syntax_error("Invalid number literal", None, None))
        } else {
            number
                .parse::<i64>()
                .map(TokenKind::Integer)
                .map_err(|_| CompilerError::syntax_error("Invalid integer literal", None, None))
        }
    }

    fn read_hex_number(&mut self) -> LexResult<TokenKind> {
        self.advance(); // Skip '0'
        self.advance(); // Skip 'x' or 'X'

        let mut hex_str = String::new();
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_hexdigit() {
                hex_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if hex_str.is_empty() {
            return Err(CompilerError::syntax_error(
                "Invalid hexadecimal literal",
                None,
                None,
            ));
        }

        i64::from_str_radix(&hex_str, 16)
            .map(TokenKind::Integer)
            .map_err(|_| CompilerError::syntax_error("Invalid hexadecimal literal", None, None))
    }

    fn read_binary_number(&mut self) -> LexResult<TokenKind> {
        self.advance(); // Skip '0'
        self.advance(); // Skip 'b' or 'B'

        let mut bin_str = String::new();
        while let Some(ch) = self.current_char() {
            if ch == '0' || ch == '1' {
                bin_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if bin_str.is_empty() {
            return Err(CompilerError::syntax_error(
                "Invalid binary literal",
                None,
                None,
            ));
        }

        i64::from_str_radix(&bin_str, 2)
            .map(TokenKind::Integer)
            .map_err(|_| CompilerError::syntax_error("Invalid binary literal", None, None))
    }

    fn read_octal_number(&mut self) -> LexResult<TokenKind> {
        self.advance(); // Skip '0'
        self.advance(); // Skip 'o' or 'O'

        let mut oct_str = String::new();
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_digit() && ch < '8' {
                oct_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if oct_str.is_empty() {
            return Err(CompilerError::syntax_error(
                "Invalid octal literal",
                None,
                None,
            ));
        }

        i64::from_str_radix(&oct_str, 8)
            .map(TokenKind::Integer)
            .map_err(|_| CompilerError::syntax_error("Invalid octal literal", None, None))
    }

    fn read_identifier(&mut self) -> String {
        let mut identifier = String::new();

        while let Some(ch) = self.current_char() {
            if ch.is_alphanumeric() || ch == '_' {
                identifier.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        identifier
    }

    fn read_line_comment(&mut self) -> LexResult<TokenKind> {
        self.advance(); // Skip first '/'
        self.advance(); // Skip second '/'

        let mut comment = String::new();
        while let Some(ch) = self.current_char() {
            if ch == '\n' {
                break;
            }
            comment.push(ch);
            self.advance();
        }

        Ok(TokenKind::Comment(comment))
    }

    fn read_block_comment(&mut self) -> LexResult<TokenKind> {
        self.advance(); // Skip '/'
        self.advance(); // Skip '*'

        let mut comment = String::new();
        let mut found_end = false;

        while let Some(ch) = self.current_char() {
            if ch == '*' && self.peek() == Some('/') {
                self.advance(); // Skip '*'
                self.advance(); // Skip '/'
                found_end = true;
                break;
            }
            comment.push(ch);
            self.advance();
        }

        if !found_end {
            return Err(CompilerError::syntax_error(
                "Unterminated block comment",
                None,
                None,
            ));
        }

        Ok(TokenKind::Comment(comment))
    }

    fn scan_token(&mut self) -> LexResult<TokenKind> {
        self.skip_whitespace();

        match self.current_char() {
            None => Ok(TokenKind::Eof),
            Some('\n') => {
                self.advance();
                Ok(TokenKind::Newline)
            }
            Some('\t') => {
                self.advance();
                Ok(TokenKind::Tab)
            }
            Some('"') => self.read_string().map(TokenKind::String),
            Some(ch) if ch.is_ascii_digit() => self.read_number(),
            Some(ch) if ch.is_alphabetic() || ch == '_' => {
                let identifier = self.read_identifier();
                let token = match identifier.as_str() {
                    // Core keywords
                    "let" => TokenKind::Let,
                    "function" => TokenKind::Function,
                    "functions" => TokenKind::Functions,
                    "class" => TokenKind::Class,
                    "constructor" => TokenKind::Constructor,
                    "if" => TokenKind::If,
                    "else" => TokenKind::Else,
                    "while" => TokenKind::While,
                    "for" => TokenKind::For,
                    "iterate" => TokenKind::Iterate,
                    "from" => TokenKind::From,
                    "to" => TokenKind::To,
                    "step" => TokenKind::Step,
                    "in" => TokenKind::In,
                    "return" => TokenKind::Return,
                    "returns" => TokenKind::Returns,
                    "import" => TokenKind::Import,
                    "export" => TokenKind::Export,

                    // Async/await
                    "async" => TokenKind::Async,
                    "await" => TokenKind::Await,
                    "start" => TokenKind::Start,
                    "later" => TokenKind::Later,
                    "background" => TokenKind::Background,

                    // Error handling
                    "onError" => TokenKind::OnError,
                    "error" => TokenKind::Error,

                    // Object-oriented
                    "base" => TokenKind::Base,
                    "this" => TokenKind::This,
                    "is" => TokenKind::Is,

                    // Literals
                    "true" => TokenKind::Boolean(true),
                    "false" => TokenKind::Boolean(false),

                    // Types
                    "integer" => TokenKind::IntegerType,
                    "number" => TokenKind::NumberType,
                    "string" => TokenKind::StringType,
                    "boolean" => TokenKind::BooleanType,
                    "void" => TokenKind::VoidType,
                    "list" => TokenKind::List,
                    "matrix" => TokenKind::Matrix,
                    "pairs" => TokenKind::Pairs,
                    "any" => TokenKind::Any,

                    // I/O and testing
                    "print" => TokenKind::Print,
                    "println" => TokenKind::Println,
                    "input" => TokenKind::Input,
                    "test" => TokenKind::Test,
                    "tests" => TokenKind::Tests,

                    // Logical operators (word form)
                    "and" => TokenKind::And,
                    "or" => TokenKind::Or,
                    "not" => TokenKind::Not,

                    // Modifiers
                    "description" => TokenKind::Description,
                    "unit" => TokenKind::Unit,
                    "private" => TokenKind::Private,
                    "constant" => TokenKind::Constant,

                    _ => TokenKind::Identifier(identifier),
                };
                Ok(token)
            }
            Some('+') => {
                self.advance();
                if self.current_char() == Some('=') {
                    self.advance();
                    Ok(TokenKind::PlusAssign)
                } else {
                    Ok(TokenKind::Plus)
                }
            }
            Some('-') => {
                self.advance();
                if self.current_char() == Some('=') {
                    self.advance();
                    Ok(TokenKind::MinusAssign)
                } else if self.current_char() == Some('>') {
                    self.advance();
                    Ok(TokenKind::Arrow)
                } else {
                    Ok(TokenKind::Minus)
                }
            }
            Some('*') => {
                self.advance();
                if self.current_char() == Some('=') {
                    self.advance();
                    Ok(TokenKind::MultiplyAssign)
                } else {
                    Ok(TokenKind::Multiply)
                }
            }
            Some('/') => {
                self.advance();
                match self.current_char() {
                    Some('=') => {
                        self.advance();
                        Ok(TokenKind::DivideAssign)
                    }
                    Some('/') => {
                        self.position -= 1; // Backtrack
                        self.read_line_comment()
                    }
                    Some('*') => {
                        self.position -= 1; // Backtrack
                        self.read_block_comment()
                    }
                    _ => Ok(TokenKind::Divide),
                }
            }
            Some('^') => {
                self.advance();
                Ok(TokenKind::Power)
            }
            Some('%') => {
                self.advance();
                Ok(TokenKind::Modulo)
            }
            Some('=') => {
                self.advance();
                match self.current_char() {
                    Some('=') => {
                        self.advance();
                        Ok(TokenKind::Equal)
                    }
                    Some('>') => {
                        self.advance();
                        Ok(TokenKind::FatArrow)
                    }
                    _ => Ok(TokenKind::Assign),
                }
            }
            Some('!') => {
                self.advance();
                if self.current_char() == Some('=') {
                    self.advance();
                    Ok(TokenKind::NotEqual)
                } else {
                    Ok(TokenKind::Not)
                }
            }
            Some('<') => {
                self.advance();
                if self.current_char() == Some('=') {
                    self.advance();
                    Ok(TokenKind::LessEqual)
                } else {
                    Ok(TokenKind::Less)
                }
            }
            Some('>') => {
                self.advance();
                if self.current_char() == Some('=') {
                    self.advance();
                    Ok(TokenKind::GreaterEqual)
                } else {
                    Ok(TokenKind::Greater)
                }
            }
            Some('(') => {
                self.advance();
                Ok(TokenKind::LeftParen)
            }
            Some(')') => {
                self.advance();
                Ok(TokenKind::RightParen)
            }
            Some('{') => {
                self.advance();
                Ok(TokenKind::LeftBrace)
            }
            Some('}') => {
                self.advance();
                Ok(TokenKind::RightBrace)
            }
            Some('[') => {
                self.advance();
                Ok(TokenKind::LeftBracket)
            }
            Some(']') => {
                self.advance();
                Ok(TokenKind::RightBracket)
            }
            Some(',') => {
                self.advance();
                Ok(TokenKind::Comma)
            }
            Some('.') => {
                self.advance();
                if self.current_char() == Some('.') {
                    self.advance();
                    if self.current_char() == Some('=') {
                        self.advance();
                        Ok(TokenKind::DotDotEqual)
                    } else {
                        Ok(TokenKind::DotDot)
                    }
                } else {
                    Ok(TokenKind::Dot)
                }
            }
            Some(':') => {
                self.advance();
                if self.current_char() == Some(':') {
                    self.advance();
                    Ok(TokenKind::DoubleColon)
                } else {
                    Ok(TokenKind::Colon)
                }
            }
            Some(';') => {
                self.advance();
                Ok(TokenKind::Semicolon)
            }
            Some('?') => {
                self.advance();
                Ok(TokenKind::Question)
            }
            Some(ch) => {
                self.advance();
                Ok(TokenKind::Invalid(ch.to_string()))
            }
        }
    }
}

impl Lexer for CleanLexer {
    fn next_token(&mut self) -> LexResult<Token> {
        let start_pos = Position::new(self.line, self.column, self.position);
        let token_kind = self.scan_token()?;
        let end_pos = Position::new(self.line, self.column, self.position);
        let span = Span::new(start_pos, end_pos);

        let token = Token::new(token_kind, span);
        self.current_token = Some(token.clone());
        Ok(token)
    }

    fn peek_token(&mut self) -> LexResult<&Token> {
        if self.current_token.is_none() {
            self.next_token()?;
        }
        Ok(self.current_token.as_ref().unwrap())
    }

    fn position(&self) -> Position {
        Position::new(self.line, self.column, self.position)
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn get_error_suggestions(&self) -> Vec<String> {
        // Implementation will be added in Task 2.1
        vec![]
    }
}
