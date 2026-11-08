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
    Print,       // print (use print() + for newline)
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
    Can, // can — capability declaration/conformance clause (Clean Language Specification §Capabilities, grammar.ebnf §6.4a)
    Returns, // returns
    Description, // description
    Input, // input
    Unit, // unit
    Public, // public — opens a public: sub-section that exposes the
    // contained members; default visibility otherwise is private
    Constant,  // constant
    Functions, // functions
    Plugins,   // plugins - Plugin loading block
    State,     // state - State management block
    Watch,     // watch - State change observer
    Computed,  // computed - Derived state value
    Guard,     // guard - State validation constraint
    Reset,     // reset - Reset state to initial value
    Screen,    // screen - Screen-level state scope
    None,      // none  // null-support - None literal keyword
    Default,   // default - Null-coalescing binary operator (spec grammar.ebnf line 232)
    Break,     // break - Loop control flow
    Continue,  // continue - Loop control flow
    Require,   // require - Contract precondition
    Ensure,    // ensure - Contract postcondition
    Always,    // always - Class invariant block keyword
    Rules,     // rules - State invariants
    Spec,      // spec - AI metadata: links function to specification
    Intent,    // intent - AI metadata: describes function purpose
    Source,    // source - AI metadata: marks file as generated from spec
    Build,     // build - Build configuration block

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
    Bang,         // ! (used in != operator context)
    Question,     // ? (nullable-type suffix: T?)
    At,           // @ (framework attribute prefix)

    // String interpolation (§2.3.2)
    InterpolationStart, // "Hello {
    InterpolationMid,   // } text {
    InterpolationEnd,   // }"

    // Special tokens
    Identifier(String), // Variable names, function names, etc.
    /// A character that is not Clean Language syntax (`'`, `&`, `#`, `\\`,
    /// a non-alphabetic non-ASCII character). It only exists so plugin-owned
    /// blocks (`html:`, `style:`, ...) can capture it by byte range; the
    /// parser rejects it anywhere else.
    RawChar(char),
    Newline,       // \n (significant for Clean Language)
    Indent(usize), // Tab levels for indentation
    Dedent(usize), // Reduced tab levels
    Eof,           // End of file

    // Comments (not in AST but needed for parsing)
    Comment(String),      // // single line comments
    BlockComment(String), // /* block comments */

    // Error recovery
    Invalid {
        text: String,
        message: String,
    },
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Literals
            TokenKind::IntegerLiteral(n) => write!(f, "{}", n),
            TokenKind::NumberLiteral(n) => write!(f, "{}", n),
            TokenKind::StringLiteral(s) => write!(f, "\"{}\"", s),

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
            TokenKind::Can => write!(f, "can"),
            TokenKind::Returns => write!(f, "returns"),
            TokenKind::Description => write!(f, "description"),
            TokenKind::Input => write!(f, "input"),
            TokenKind::Unit => write!(f, "unit"),
            TokenKind::Public => write!(f, "public"),
            TokenKind::Constant => write!(f, "constant"),
            TokenKind::Functions => write!(f, "functions"),
            TokenKind::Plugins => write!(f, "plugins"),
            TokenKind::State => write!(f, "state"),
            TokenKind::Watch => write!(f, "watch"),
            TokenKind::Computed => write!(f, "computed"),
            TokenKind::Guard => write!(f, "guard"),
            TokenKind::Reset => write!(f, "reset"),
            TokenKind::Screen => write!(f, "screen"),
            TokenKind::None => write!(f, "none"),
            TokenKind::Default => write!(f, "default"),
            TokenKind::Break => write!(f, "break"),
            TokenKind::Continue => write!(f, "continue"),
            TokenKind::Require => write!(f, "require"),
            TokenKind::Ensure => write!(f, "ensure"),
            TokenKind::Always => write!(f, "always"),
            TokenKind::Rules => write!(f, "rules"),
            TokenKind::Spec => write!(f, "spec"),
            TokenKind::Intent => write!(f, "intent"),
            TokenKind::Source => write!(f, "source"),
            TokenKind::Build => write!(f, "build"),

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
            TokenKind::Bang => write!(f, "!"),
            TokenKind::Question => write!(f, "?"),
            TokenKind::At => write!(f, "@"),

            // String interpolation
            TokenKind::InterpolationStart => write!(f, "STRING_INTERP_START"),
            TokenKind::InterpolationMid => write!(f, "STRING_INTERP_MID"),
            TokenKind::InterpolationEnd => write!(f, "STRING_INTERP_END"),

            // Special
            TokenKind::Identifier(name) => write!(f, "{}", name),
            TokenKind::RawChar(c) => write!(f, "{}", c),
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

/// Token stream produced by the lexer
#[derive(Debug, Clone, Default)]
pub struct TokenStream {
    pub tokens: Vec<Token>,
    /// Original source text for raw content extraction in framework blocks
    pub source_content: String,
}

impl TokenStream {
    pub fn new(tokens: Vec<Token>, source_content: String) -> Self {
        Self {
            tokens,
            source_content,
        }
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
            "can" => Some(TokenKind::Can),
            "returns" => Some(TokenKind::Returns),
            "description" => Some(TokenKind::Description),
            "input" => Some(TokenKind::Input),
            "unit" => Some(TokenKind::Unit),
            "public" => Some(TokenKind::Public),
            "constant" => Some(TokenKind::Constant),
            "functions" => Some(TokenKind::Functions),
            "plugins" => Some(TokenKind::Plugins),
            "state" => Some(TokenKind::State),
            "watch" => Some(TokenKind::Watch),
            "computed" => Some(TokenKind::Computed),
            "guard" => Some(TokenKind::Guard),
            "reset" => Some(TokenKind::Reset),
            "screen" => Some(TokenKind::Screen),
            "none" | "null" => Some(TokenKind::None), // `none` is the standard spelling, `null` its alias (03-lexical-structure.md)
            "default" => Some(TokenKind::Default), // null-coalescing operator (spec grammar.ebnf line 232)
            "break" => Some(TokenKind::Break),
            "continue" => Some(TokenKind::Continue),
            "require" => Some(TokenKind::Require),
            "ensure" => Some(TokenKind::Ensure),
            "always" => Some(TokenKind::Always),
            "rules" => Some(TokenKind::Rules),
            "spec" => Some(TokenKind::Spec),
            "intent" => Some(TokenKind::Intent),
            "source" => Some(TokenKind::Source),
            "build" => Some(TokenKind::Build),
            _ => None,
        }
    }
}
