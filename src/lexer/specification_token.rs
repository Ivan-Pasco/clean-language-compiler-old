//! Token definitions for Clean Language lexer - 100% AST Specification compliant
//!
//! This module defines tokens that exactly match the AST Specification requirements.
//! Every token type corresponds directly to language constructs defined in the
//! Clean Language Specification.

use crate::ast::SourceLocation;
use std::fmt;

/// Token types exactly matching Clean Language Specification
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals (§2.3 - exactly matching AST specification Value enum)
    IntegerLiteral(i64),   // 42, -17 (default)
    NumberLiteral(f64),    // 3.14, 6.02e23 (default)
    StringLiteral(String), // "Hello, World!"
    BooleanLiteral(bool),  // true, false

    // Precision Literals (§3.2 - AST specification precision modifiers)
    Integer8Literal(i8),    // integer:8
    Integer8uLiteral(u8),   // integer:8u
    Integer16Literal(i16),  // integer:16
    Integer16uLiteral(u16), // integer:16u
    Integer32Literal(i32),  // integer:32
    Integer64Literal(i64),  // integer:64
    Number32Literal(f32),   // number:32
    Number64Literal(f64),   // number:64

    // Keywords (all from Clean Language Specification - complete list)
    And,         // and
    Background,  // background
    Class,       // class
    Constructor, // constructor
    Else,        // else
    Error,       // error
    False,       // false
    For,         // for
    From,        // from
    Function,    // function
    If,          // if
    Import,      // import
    In,          // in
    Iterate,     // iterate
    Later,       // later
    Not,         // not
    OnError,     // onError
    Or,          // or
    Print,       // print
    Println,     // println
    Return,      // return
    Start,       // start
    Step,        // step
    Test,        // test
    Tests,       // tests
    This,        // this
    To,          // to
    True,        // true
    While,       // while
    Is,          // is
    Returns,     // returns
    Description, // description
    Input,       // input
    Unit,        // unit
    Private,     // private
    Constant,    // constant
    Functions,   // functions

    // Operators (§5.1 - Binary and Unary operators from specification)
    Plus,     // +
    Minus,    // -
    Multiply, // *
    Divide,   // /
    Modulo,   // %
    Power,    // ^

    // Comparison operators
    Equal,        // ==
    NotEqual,     // !=
    Less,         // <
    Greater,      // >
    LessEqual,    // <=
    GreaterEqual, // >=

    // Assignment
    Assign, // =

    // Punctuation and delimiters
    LeftParen,    // (
    RightParen,   // )
    LeftBracket,  // [
    RightBracket, // ]
    LeftBrace,    // {
    RightBrace,   // }
    Comma,        // ,
    Dot,          // .
    Colon,        // :
    Semicolon,    // ;

    // Range operators (§9.2 - Range expressions)
    Range,          // .. for ranges like 1..10
    RangeInclusive, // ..= for inclusive ranges (if supported)

    // String interpolation (§2.3.2)
    InterpolationStart, // "Hello {
    InterpolationMid,   // } text {
    InterpolationEnd,   // }"

    // Special tokens
    Identifier(String), // Variable names, function names, etc.
    Newline,            // \n (significant for Clean Language)
    Indent(usize),      // Tab levels for indentation
    Dedent(usize),      // Reduced tab levels
    Eof,                // End of file

    // Comments (not in AST but needed for parsing)
    Comment(String),      // // single line comments
    BlockComment(String), // /* block comments */

    // Error recovery
    Invalid { text: String, message: String },
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Literals
            TokenKind::IntegerLiteral(n) => write!(f, "{}", n),
            TokenKind::NumberLiteral(n) => write!(f, "{}", n),
            TokenKind::StringLiteral(s) => write!(f, "\"{}\"", s),
            TokenKind::BooleanLiteral(b) => write!(f, "{}", b),

            // Precision literals
            TokenKind::Integer8Literal(n) => write!(f, "{}:8", n),
            TokenKind::Integer8uLiteral(n) => write!(f, "{}:8u", n),
            TokenKind::Integer16Literal(n) => write!(f, "{}:16", n),
            TokenKind::Integer16uLiteral(n) => write!(f, "{}:16u", n),
            TokenKind::Integer32Literal(n) => write!(f, "{}:32", n),
            TokenKind::Integer64Literal(n) => write!(f, "{}:64", n),
            TokenKind::Number32Literal(n) => write!(f, "{}:32", n),
            TokenKind::Number64Literal(n) => write!(f, "{}:64", n),

            // Keywords
            TokenKind::And => write!(f, "and"),
            TokenKind::Background => write!(f, "background"),
            TokenKind::Class => write!(f, "class"),
            TokenKind::Constructor => write!(f, "constructor"),
            TokenKind::Else => write!(f, "else"),
            TokenKind::Error => write!(f, "error"),
            TokenKind::False => write!(f, "false"),
            TokenKind::For => write!(f, "for"),
            TokenKind::From => write!(f, "from"),
            TokenKind::Function => write!(f, "function"),
            TokenKind::If => write!(f, "if"),
            TokenKind::Import => write!(f, "import"),
            TokenKind::In => write!(f, "in"),
            TokenKind::Iterate => write!(f, "iterate"),
            TokenKind::Later => write!(f, "later"),
            TokenKind::Not => write!(f, "not"),
            TokenKind::OnError => write!(f, "onError"),
            TokenKind::Or => write!(f, "or"),
            TokenKind::Print => write!(f, "print"),
            TokenKind::Println => write!(f, "println"),
            TokenKind::Return => write!(f, "return"),
            TokenKind::Start => write!(f, "start"),
            TokenKind::Step => write!(f, "step"),
            TokenKind::Test => write!(f, "test"),
            TokenKind::Tests => write!(f, "tests"),
            TokenKind::This => write!(f, "this"),
            TokenKind::To => write!(f, "to"),
            TokenKind::True => write!(f, "true"),
            TokenKind::While => write!(f, "while"),
            TokenKind::Is => write!(f, "is"),
            TokenKind::Returns => write!(f, "returns"),
            TokenKind::Description => write!(f, "description"),
            TokenKind::Input => write!(f, "input"),
            TokenKind::Unit => write!(f, "unit"),
            TokenKind::Private => write!(f, "private"),
            TokenKind::Constant => write!(f, "constant"),
            TokenKind::Functions => write!(f, "functions"),

            // Operators
            TokenKind::Plus => write!(f, "+"),
            TokenKind::Minus => write!(f, "-"),
            TokenKind::Multiply => write!(f, "*"),
            TokenKind::Divide => write!(f, "/"),
            TokenKind::Modulo => write!(f, "%"),
            TokenKind::Power => write!(f, "^"),
            TokenKind::Equal => write!(f, "=="),
            TokenKind::NotEqual => write!(f, "!="),
            TokenKind::Less => write!(f, "<"),
            TokenKind::Greater => write!(f, ">"),
            TokenKind::LessEqual => write!(f, "<="),
            TokenKind::GreaterEqual => write!(f, ">="),
            TokenKind::Assign => write!(f, "="),

            // Punctuation
            TokenKind::LeftParen => write!(f, "("),
            TokenKind::RightParen => write!(f, ")"),
            TokenKind::LeftBracket => write!(f, "["),
            TokenKind::RightBracket => write!(f, "]"),
            TokenKind::LeftBrace => write!(f, "{{"),
            TokenKind::RightBrace => write!(f, "}}"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Dot => write!(f, "."),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Semicolon => write!(f, ";"),
            TokenKind::Range => write!(f, ".."),
            TokenKind::RangeInclusive => write!(f, "..="),

            // String interpolation
            TokenKind::InterpolationStart => write!(f, "STRING_INTERP_START"),
            TokenKind::InterpolationMid => write!(f, "STRING_INTERP_MID"),
            TokenKind::InterpolationEnd => write!(f, "STRING_INTERP_END"),

            // Special
            TokenKind::Identifier(name) => write!(f, "{}", name),
            TokenKind::Newline => write!(f, "\\n"),
            TokenKind::Indent(level) => write!(f, "INDENT({})", level),
            TokenKind::Dedent(level) => write!(f, "DEDENT({})", level),
            TokenKind::Eof => write!(f, "EOF"),

            // Comments
            TokenKind::Comment(text) => write!(f, "//{}", text),
            TokenKind::BlockComment(text) => write!(f, "/*{}*/", text),

            // Error
            TokenKind::Invalid { text, message } => write!(f, "INVALID('{}': {})", text, message),
        }
    }
}

/// Token with complete source location information
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub location: SourceLocation,
    pub text: String, // Original text from source for error reporting
}

impl Token {
    pub fn new(kind: TokenKind, location: SourceLocation, text: String) -> Self {
        Self {
            kind,
            location,
            text,
        }
    }

    /// Create token with default text representation
    pub fn simple(kind: TokenKind, location: SourceLocation) -> Self {
        let text = format!("{}", kind);
        Self::new(kind, location, text)
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}:{}",
            self.kind, self.location.line, self.location.column
        )
    }
}

/// Token stream with source mapping information
#[derive(Debug, Clone)]
pub struct TokenStream {
    pub tokens: Vec<Token>,
    pub source_map: SourceMap,
}

impl TokenStream {
    pub fn new(tokens: Vec<Token>, source_map: SourceMap) -> Self {
        Self { tokens, source_map }
    }
}

impl Default for TokenStream {
    fn default() -> Self {
        Self {
            tokens: Vec::new(),
            source_map: SourceMap {
                file_path: String::new(),
                line_starts: vec![0],
                total_bytes: 0,
            },
        }
    }
}

/// Source map for tracking original source positions
#[derive(Debug, Clone)]
pub struct SourceMap {
    pub file_path: String,
    pub line_starts: Vec<usize>, // Byte positions where each line starts
    pub total_bytes: usize,
}

impl SourceMap {
    pub fn new(file_path: String, source_content: &str) -> Self {
        let mut line_starts = vec![0];
        for (byte_pos, ch) in source_content.char_indices() {
            if ch == '\n' {
                line_starts.push(byte_pos + ch.len_utf8());
            }
        }

        Self {
            file_path,
            line_starts,
            total_bytes: source_content.len(),
        }
    }

    /// Get source location from byte offset
    pub fn location_from_offset(&self, offset: usize) -> SourceLocation {
        // Binary search to find line number
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line + 1,
            Err(line) => line,
        };

        let line_start = self
            .line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let column = offset - line_start + 1;

        SourceLocation::new(line, column, &self.file_path)
    }
}

/// Keyword recognition table
pub struct Keywords;

impl Keywords {
    /// Check if identifier is a keyword, return appropriate token kind
    pub fn lookup(identifier: &str) -> Option<TokenKind> {
        match identifier {
            // All keywords from Clean Language Specification
            "and" => Some(TokenKind::And),
            "background" => Some(TokenKind::Background),
            "class" => Some(TokenKind::Class),
            "constructor" => Some(TokenKind::Constructor),
            "else" => Some(TokenKind::Else),
            "error" => Some(TokenKind::Error),
            "false" => Some(TokenKind::False),
            "for" => Some(TokenKind::For),
            "from" => Some(TokenKind::From),
            "function" => Some(TokenKind::Function),
            "if" => Some(TokenKind::If),
            "import" => Some(TokenKind::Import),
            "in" => Some(TokenKind::In),
            "iterate" => Some(TokenKind::Iterate),
            "later" => Some(TokenKind::Later),
            "not" => Some(TokenKind::Not),
            "onError" => Some(TokenKind::OnError),
            "or" => Some(TokenKind::Or),
            "print" => Some(TokenKind::Print),
            "println" => Some(TokenKind::Println),
            "return" => Some(TokenKind::Return),
            "start" => Some(TokenKind::Start),
            "step" => Some(TokenKind::Step),
            "test" => Some(TokenKind::Test),
            "tests" => Some(TokenKind::Tests),
            "this" => Some(TokenKind::This),
            "to" => Some(TokenKind::To),
            "true" => Some(TokenKind::True),
            "while" => Some(TokenKind::While),
            "is" => Some(TokenKind::Is),
            "returns" => Some(TokenKind::Returns),
            "description" => Some(TokenKind::Description),
            "input" => Some(TokenKind::Input),
            "unit" => Some(TokenKind::Unit),
            "private" => Some(TokenKind::Private),
            "constant" => Some(TokenKind::Constant),
            "functions" => Some(TokenKind::Functions),
            _ => None,
        }
    }

    /// Get boolean literal value
    pub fn boolean_value(identifier: &str) -> Option<bool> {
        match identifier {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

/// Impl block for TokenKind
impl TokenKind {
    /// Convert token back to source string representation
    /// Used for capturing raw content in framework blocks
    pub fn to_source_string(&self) -> String {
        match self {
            TokenKind::IntegerLiteral(n) => n.to_string(),
            TokenKind::NumberLiteral(n) => n.to_string(),
            TokenKind::StringLiteral(s) => format!("\"{}\"", s),
            TokenKind::BooleanLiteral(b) => b.to_string(),
            TokenKind::Identifier(s) => s.clone(),
            TokenKind::And => "and".to_string(),
            TokenKind::Background => "background".to_string(),
            TokenKind::Class => "class".to_string(),
            TokenKind::Constructor => "constructor".to_string(),
            TokenKind::Else => "else".to_string(),
            TokenKind::Error => "error".to_string(),
            TokenKind::Functions => "functions".to_string(),
            TokenKind::If => "if".to_string(),
            TokenKind::Import => "import".to_string(),
            TokenKind::In => "in".to_string(),
            TokenKind::Is => "is".to_string(),
            TokenKind::Iterate => "iterate".to_string(),
            TokenKind::Later => "later".to_string(),
            TokenKind::Not => "not".to_string(),
            TokenKind::OnError => "onError".to_string(),
            TokenKind::Or => "or".to_string(),
            TokenKind::Private => "private".to_string(),
            TokenKind::Return => "return".to_string(),
            TokenKind::Start => "start".to_string(),
            TokenKind::Tests => "tests".to_string(),
            TokenKind::To => "to".to_string(),
            TokenKind::Step => "step".to_string(),
            TokenKind::Plus => "+".to_string(),
            TokenKind::Minus => "-".to_string(),
            TokenKind::Multiply => "*".to_string(),
            TokenKind::Divide => "/".to_string(),
            TokenKind::Modulo => "%".to_string(),
            TokenKind::Power => "^".to_string(),
            TokenKind::Equal => "==".to_string(),
            TokenKind::NotEqual => "!=".to_string(),
            TokenKind::Less => "<".to_string(),
            TokenKind::LessEqual => "<=".to_string(),
            TokenKind::Greater => ">".to_string(),
            TokenKind::GreaterEqual => ">=".to_string(),
            TokenKind::Assign => "=".to_string(),
            TokenKind::Comma => ",".to_string(),
            TokenKind::Colon => ":".to_string(),
            TokenKind::LeftParen => "(".to_string(),
            TokenKind::RightParen => ")".to_string(),
            TokenKind::LeftBracket => "[".to_string(),
            TokenKind::RightBracket => "]".to_string(),
            TokenKind::Dot => ".".to_string(),
            TokenKind::LeftBrace => "{".to_string(),
            TokenKind::RightBrace => "}".to_string(),
            _ => String::new(),
        }
    }
}
