use crate::ast::Program;
use crate::error::CompilerError;
use crate::lexer::specification_token::TokenStream;
use super::parser_impl::ErrorRecoveringParser;

/// Specification-compliant parser that uses tokens from the lexer
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
    
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        // For now, reconstruct source from tokens and use ErrorRecoveringParser
        // This is a temporary bridge until we have a proper token-based parser
        let source = self.reconstruct_source_from_tokens();
        eprintln!("DEBUG SPECIFICATION PARSER: Reconstructed source:");
        eprintln!("{:?}", source);
        let mut parser = ErrorRecoveringParser::new(&source, &self.file_path);
        
        match parser.parse_program() {
            Ok(program) => {
                if parser.errors.is_empty() {
                    Ok(program)
                } else {
                    // Return the first error for now
                    Err(parser.errors.into_iter().next().unwrap())
                }
            }
            Err(error) => {
                // Return the error directly
                Err(error)
            }
        }
    }
    
    /// Temporary method to reconstruct source from tokens
    /// This is not ideal but provides a bridge to the existing parser
    fn reconstruct_source_from_tokens(&self) -> String {
        // Use the source map to reconstruct the original source properly
        if let Some(source_content) = self.get_original_source() {
            return source_content;
        }
        
        // Fallback: reconstruct with proper spacing
        let mut result = String::new();
        let mut current_line = 1;
        
        for (i, token) in self.token_stream.tokens.iter().enumerate() {
            match &token.kind {
                crate::lexer::specification_token::TokenKind::Newline => {
                    result.push('\n');
                    current_line += 1;
                }
                crate::lexer::specification_token::TokenKind::Indent(level) => {
                    for _ in 0..*level {
                        result.push('\t');
                    }
                }
                crate::lexer::specification_token::TokenKind::Dedent(_) => {
                    // Dedent is handled by reducing indentation, no text needed
                }
                crate::lexer::specification_token::TokenKind::Eof => {
                    // End of file, no text needed
                }
                _ => {
                    // Add space before token if needed (except at start of line)
                    if i > 0 && !matches!(self.token_stream.tokens[i-1].kind, 
                        crate::lexer::specification_token::TokenKind::Newline | 
                        crate::lexer::specification_token::TokenKind::Indent(_)) {
                        result.push(' ');
                    }
                    result.push_str(&token.text);
                }
            }
        }
        
        result
    }
    
    /// Try to get the original source content from the source map
    fn get_original_source(&self) -> Option<String> {
        // For now, return None - in a complete implementation, we would
        // store the original source in the TokenStream or read it from the file
        None
    }
}