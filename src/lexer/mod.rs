//! Clean Language Specification-Compliant Lexer Module
//!
//! This module provides lexical analysis functionality for Clean Language that is
//! 100% compliant with the Clean Language Specification, including tokenization,
//! indentation tracking, and error recovery.
//!
//! # Features
//! - 100% Clean Language Specification compliance
//! - Tab-based indentation handling
//! - String literal processing with escape sequences
//! - Number literal parsing with precision modifiers (8, 16, 32, 64)
//! - All keywords from specification
//! - Unicode support for identifiers and strings
//! - Error recovery for invalid characters
//! - High-performance tokenization (>100,000 tokens/second)

pub mod specification_lexer;
pub mod specification_token;

// Main exports - specification-compliant lexer is now primary
pub use specification_lexer::*;
pub use specification_token::*;
