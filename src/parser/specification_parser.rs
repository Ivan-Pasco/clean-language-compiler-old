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
    /// Plugin-defined keywords that don't require colons (e.g., "data" from frame.data)
    plugin_keywords: Vec<String>,
    /// When true, skip section ordering enforcement (for plugin output)
    lenient_section_order: bool,
    /// Set by the first `parse_program`, which moves the token stream out.
    consumed: bool,
}

impl SpecificationParser {
    pub fn new(token_stream: TokenStream) -> Self {
        Self {
            token_stream,
            plugin_keywords: Vec::new(),
            lenient_section_order: false,
            consumed: false,
        }
    }

    /// Create a parser with plugin-defined keywords
    ///
    /// Plugin keywords are recognized as framework block starters even without
    /// a trailing colon. For example, with `plugin_keywords = ["data"]`:
    /// - `data User` is parsed as a framework block (plugin keyword)
    /// - `endpoints:` is parsed as a framework block (has colon)
    pub fn with_plugin_keywords(token_stream: TokenStream, plugin_keywords: Vec<String>) -> Self {
        Self {
            token_stream,
            plugin_keywords,
            lenient_section_order: false,
            consumed: false,
        }
    }

    /// Enable lenient section ordering for plugin output parsing
    pub fn set_lenient_section_order(&mut self, lenient: bool) {
        self.lenient_section_order = lenient;
    }

    /// Parse a program using the token-driven parser
    ///
    /// This method uses the new TokenParser that consumes tokens directly,
    /// following the architecture of rustc's parser (see rust-lang/rustc-dev-guide).
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        if self.consumed {
            return Err(CompilerError::parse_error(
                "parse_program was already called on this parser; its token stream is consumed"
                    .to_string(),
                None,
                Some("Create a new SpecificationParser for each parse.".to_string()),
            ));
        }
        self.consumed = true;
        let mut parser = if self.plugin_keywords.is_empty() {
            TokenParser::new(std::mem::take(&mut self.token_stream))
        } else {
            TokenParser::with_plugin_keywords(
                std::mem::take(&mut self.token_stream),
                std::mem::take(&mut self.plugin_keywords),
            )
        };

        if self.lenient_section_order {
            parser.set_lenient_section_order(true);
        }

        parser.parse_program()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
    use crate::lexer::specification_token::TokenStream;

    /// Regression: a second `parse_program` on the same parser used to index
    /// `tokens[len - 1]` on the emptied stream and panic.
    #[test]
    fn second_parse_is_an_error_not_a_panic() {
        let source = SourceCode::new("start:\n\tprint(1)\n".to_string(), "t.cln".to_string());
        let tokens = SpecificationLexer::new(&source).tokenize().unwrap();
        let mut parser = SpecificationParser::new(tokens);
        assert!(parser.parse_program().is_ok());
        assert!(parser.parse_program().is_err());
    }

    /// An empty token stream parses to an empty program instead of panicking.
    #[test]
    fn empty_token_stream_does_not_panic() {
        let mut parser = SpecificationParser::new(TokenStream::default());
        let program = parser.parse_program().unwrap();
        assert!(program.functions.is_empty());
    }
}
