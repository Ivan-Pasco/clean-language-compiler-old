use pest_derive::Parser;
use crate::ast::Program;
use crate::error::CompilerError;
use super::parser_impl::ErrorRecoveringParser;

#[derive(Parser)]
#[grammar = "parser/grammar.pest"]
pub struct CleanParser;

impl CleanParser {
    /// Static method for parsing program from source string
    pub fn parse_program(source: &str) -> Result<Program, CompilerError> {
        let mut parser = ErrorRecoveringParser::new(source, "");
        parser.parse_program()
    }
    
    /// Static method for parsing program with file path for better error reporting
    pub fn parse_program_with_file(source: &str, file_path: &str) -> Result<Program, CompilerError> {
        let mut parser = ErrorRecoveringParser::new(source, file_path);
        parser.parse_program()
    }
}
