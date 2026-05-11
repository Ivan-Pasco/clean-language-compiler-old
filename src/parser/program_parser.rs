use crate::ast::Program;
use crate::error::CompilerError;
use crate::parser::grammar::Rule;
use crate::parser::parser_impl;
use pest::iterators::Pair;

/// Parse a program from the AST
pub fn parse_program_ast(pair: Pair<Rule>) -> Result<Program, CompilerError> {
    match pair.as_rule() {
        Rule::program => {
            let mut functions = Vec::new();
            let mut start_function = None;

            // Extract all functions and start function
            for inner_pair in pair.into_inner() {
                match inner_pair.as_rule() {
                    Rule::functions_block => {
                        // Parse functions block using the existing parser_impl
                        let block_functions = parser_impl::parse_functions_block(inner_pair)?;
                        functions.extend(block_functions);
                    }
                    Rule::start_block => {
                        // Parse start block using the existing parser_impl
                        let function = parser_impl::parse_start_block(inner_pair)?;
                        start_function = Some(function);
                    }
                    Rule::class_decl => {
                        // Classes are handled in a different part of the parser
                    }
                    Rule::EOI => {
                        // End of input, ignore
                    }
                    _ => {
                        // Ignore other rules
                    }
                }
            }

            // Create and return the Program
            Ok(Program {
                imports: Vec::new(),
                plugins: Vec::new(),
                statements: Vec::new(),
                functions,
                classes: Vec::new(),
                start_function,
                tests: Vec::new(),
                screens: Vec::new(),
                state: None,
                watch_blocks: Vec::new(),
                screen_blocks: Vec::new(),
                externals: Vec::new(),
                source_block: None,
                location: None,
            })
        }
        _ => Err(CompilerError::parse_error(
            format!("Expected program, found {:?}", pair.as_rule()),
            None,
            None,
        )),
    }
}
