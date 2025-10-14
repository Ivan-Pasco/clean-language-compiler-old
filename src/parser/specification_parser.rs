use super::token_parser::TokenParser;
use crate::ast::Program;
use crate::error::CompilerError;
use crate::lexer::specification_token::TokenStream;

/// Specification-compliant parser that uses tokens from the lexer
///
/// This parser follows the rustc architecture pattern, consuming tokens directly
/// without source reconstruction. See token_parser.rs for the token-driven implementation.
pub struct SpecificationParser {
    token_stream: TokenStream,
    file_path: String,
}

impl SpecificationParser {
    pub fn new(token_stream: TokenStream, file_path: String) -> Self {
        Self {
            token_stream,
            file_path,
        }
    }

    /// Parse a program using the token-driven parser
    ///
    /// This method uses the new TokenParser that consumes tokens directly,
    /// following the architecture of rustc's parser (see rust-lang/rustc-dev-guide).
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        let mut parser = TokenParser::new(
            std::mem::take(&mut self.token_stream),
            self.file_path.clone(),
        );

        parser.parse_program()
    }
}
