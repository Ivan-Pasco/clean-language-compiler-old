//! Clean Language Parser Module
//!
//! This module provides specification-compliant parsing capabilities only.
//! All parsing goes through the 7-stage compiler pipeline.

// Include all parser modules
pub mod class_parser;
pub mod expression_parser;
pub mod grammar;
pub mod parser_impl;
pub mod preprocessor;
pub mod property_tests;
pub mod specification_parser;
pub mod statement_parser;
pub mod token_parser; // Token-driven parser (rustc-style)
pub mod type_parser;

// Re-export main components
pub use specification_parser::SpecificationParser;

// Re-export pest parser components for internal use
pub use grammar::*;
pub use parser_impl::ErrorRecoveringParser;

// Re-export location types
pub use crate::ast::SourceLocation;

pub fn get_location(pair: &pest::iterators::Pair<Rule>) -> SourceLocation {
    let span = pair.as_span();
    let start = span.start_pos().line_col();
    SourceLocation {
        line: start.0,
        column: start.1,
        file: String::new(), // Will be set by the parser context
        byte_start: None,
        byte_end: None,
    }
}
