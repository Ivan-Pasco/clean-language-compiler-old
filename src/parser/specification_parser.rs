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
    /// Plugin-defined keywords that don't require colons (e.g., "data" from frame.data)
    plugin_keywords: Vec<String>,
    /// When true, skip section ordering enforcement (for plugin output)
    lenient_section_order: bool,
}

impl SpecificationParser {
    pub fn new(token_stream: TokenStream, file_path: String) -> Self {
        Self {
            token_stream,
            file_path,
            plugin_keywords: Vec::new(),
            lenient_section_order: false,
        }
    }

    /// Create a parser with plugin-defined keywords
    ///
    /// Plugin keywords are recognized as framework block starters even without
    /// a trailing colon. For example, with `plugin_keywords = ["data"]`:
    /// - `data User` is parsed as a framework block (plugin keyword)
    /// - `endpoints:` is parsed as a framework block (has colon)
    pub fn with_plugin_keywords(
        token_stream: TokenStream,
        file_path: String,
        plugin_keywords: Vec<String>,
    ) -> Self {
        Self {
            token_stream,
            file_path,
            plugin_keywords,
            lenient_section_order: false,
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
        let mut parser = if self.plugin_keywords.is_empty() {
            TokenParser::new(
                std::mem::take(&mut self.token_stream),
                self.file_path.clone(),
            )
        } else {
            TokenParser::with_plugin_keywords(
                std::mem::take(&mut self.token_stream),
                self.file_path.clone(),
                std::mem::take(&mut self.plugin_keywords),
            )
        };

        if self.lenient_section_order {
            parser.set_lenient_section_order(true);
        }

        parser.parse_program()
    }
}
