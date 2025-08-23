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
                    Rule::start_function => {
                        // Parse start function using the existing parser_impl
                        let function = parser_impl::parse_start_function(inner_pair)?;
                        println!(
                            "DEBUG PROGRAM_PARSER: Found start function '{}'",
                            function.name
                        );
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

            println!(
                "DEBUG PROGRAM_PARSER: Creating program with {} functions, start_function = {:?}",
                functions.len(),
                start_function.as_ref().map(|f| &f.name)
            );

            // Create and return the Program
            Ok(Program {
                imports: Vec::new(),
                statements: Vec::new(),
                functions,
                classes: Vec::new(),
                start_function,
                tests: Vec::new(),
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
