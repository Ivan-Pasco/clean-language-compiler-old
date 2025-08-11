//! Token definitions for Clean Language lexer

use super::Span;
use std::fmt;

/// Token types in Clean Language
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Integer(i64),
    Number(f64),
    String(String),
    Boolean(bool),
    
    // Identifiers and Keywords
    Identifier(String),
    Let,
    Function,
    Functions,
    Class,
    Constructor,
    If,
    Else,
    While,
    For,
    Iterate,
    From,
    To,
    Step,
    In,
    Return,
    Returns,
    Import,
    Export,
    Async,
    Await,
    Start,
    Later,
    Background,
    OnError,
    Error,
    Base,
    This,
    True,
    False,
    Print,
    Println,
    Input,
    Test,
    Tests,
    Description,
    Unit,
    Private,
    Constant,
    Is,
    And,
    Or,
    Not,
    
    // Types
    IntegerType,
    NumberType,
    StringType,
    BooleanType,
    VoidType,
    List,
    Matrix,
    Any,
    Pairs,
    
    // Operators
    Plus,
    Minus,
    Multiply,
    Divide,
    Power,
    Modulo,
    Assign,
    PlusAssign,
    MinusAssign,
    MultiplyAssign,
    DivideAssign,
    
    // Comparison
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    
    // Punctuation
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    LeftAngle,
    RightAngle,
    Comma,
    Dot,
    DotDot,         // ..
    DotDotEqual,    // ..=
    Colon,
    Semicolon,
    Question,
    Arrow,          // ->
    FatArrow,       // =>
    
    // Special
    Tab,
    Newline,
    Eof,
    
    // Comments
    Comment(String),
    
    // Method-style operators
    DoubleColon, // ::
    
    // String interpolation
    StringStart,    // String with interpolation start
    StringMiddle,   // String interpolation middle
    StringEnd,      // String interpolation end
    InterpolationStart, // {
    InterpolationEnd,   // }
    
    // Error recovery
    Invalid(String),
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Integer(n) => write!(f, "{}", n),
            TokenKind::Number(n) => write!(f, "{}", n),
            TokenKind::String(s) => write!(f, "\"{}\"", s),
            TokenKind::Boolean(b) => write!(f, "{}", b),
            TokenKind::Identifier(name) => write!(f, "{}", name),
            TokenKind::Comment(text) => write!(f, "//{}", text),
            TokenKind::Invalid(text) => write!(f, "INVALID({})", text),
            _ => write!(f, "{:?}", self),
        }
    }
}

/// Token with location information
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.kind, self.span)
    }
}