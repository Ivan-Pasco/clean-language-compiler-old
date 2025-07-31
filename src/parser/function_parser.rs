use pest::{Parser, iterators::Pair};
use crate::ast::{Function, FunctionSyntax, Parameter, Type, Statement, Visibility, FunctionModifier, SourceLocation};
use crate::error::CompilerError;
use super::{CleanParser, Rule};
use super::lexical_analyzer::{LexicalAnalyzer, FunctionSegment};
use super::statement_parser::parse_statement;
use super::type_parser::parse_type;

/// Enhanced function parser that uses lexical analysis for proper boundary detection
pub struct FunctionParser {
    lexical_analyzer: LexicalAnalyzer,
}

impl FunctionParser {
    pub fn new(source: &str) -> Self {
        Self {
            lexical_analyzer: LexicalAnalyzer::new(source),
        }
    }
    
    /// Parse functions block using lexical analysis for boundary detection
    pub fn parse_functions_block(&self, pair: Pair<Rule>) -> Result<Vec<Function>, CompilerError> {
        let mut functions = Vec::new();
        
        // Get function segments from lexical analysis
        let segments = self.lexical_analyzer.split_into_function_segments();
        
        if segments.is_empty() {
            // Fall back to traditional parsing if no segments found
            return self.parse_functions_traditional(pair);
        }
        
        // Parse each function segment individually
        for segment in segments {
            match self.parse_function_segment(&segment) {
                Ok(function) => functions.push(function),
                Err(e) => {
                    // Add line offset to error location
                    return Err(e);
                }
            }
        }
        
        Ok(functions)
    }
    
    /// Parse a single function segment
    fn parse_function_segment(&self, segment: &FunctionSegment) -> Result<Function, CompilerError> {
        // Create a minimal functions block wrapper for parsing
        let wrapped_source = format!("functions:\n{}", segment.source);
        
        // Parse with pest
        let parse_result = CleanParser::parse(Rule::functions_block, &wrapped_source);
        let pairs = parse_result.map_err(|e| {
            CompilerError::syntax_error(
                &format!("Failed to parse function {}: {}", segment.boundary.function_name, e),
                None,
                Some(SourceLocation {
                    line: 1,
                    column: 1,
                    file: String::new(),
                })
            )
        })?;
        
        // Extract the function from the parsed result
        for pair in pairs {
            if pair.as_rule() == Rule::functions_block {
                for inner_pair in pair.into_inner() {
                    if inner_pair.as_rule() == Rule::indented_functions_block {
                        // Parse the first (and should be only) function
                        return self.parse_single_function_from_block(inner_pair);
                    }
                }
            }
        }
        
        Err(CompilerError::syntax_error(
            &format!("No function found in segment for {}", segment.boundary.function_name),
            None,
            Some(SourceLocation {
                line: 1,
                column: 1,
                file: String::new(),
            })
        ))
    }
    
    /// Parse a single function from an indented functions block
    fn parse_single_function_from_block(&self, pair: Pair<Rule>) -> Result<Function, CompilerError> {
        for inner_pair in pair.into_inner() {
            match inner_pair.as_rule() {
                Rule::function_in_block => {
                    return self.parse_function_declaration(inner_pair);
                }
                _ => continue,
            }
        }
        
        Err(CompilerError::syntax_error(
            "No function declaration found",
            None,
            Some(SourceLocation {
                line: 1,
                column: 1,
                file: String::new(),
            })
        ))
    }
    
    /// Parse a function declaration pair
    fn parse_function_declaration(&self, pair: Pair<Rule>) -> Result<Function, CompilerError> {
        let mut return_type = Type::Void;
        let mut name = String::new();
        let mut parameters = Vec::new();
        let mut statements = Vec::new();
        let mut syntax = FunctionSyntax::Simple;
        let span = pair.as_span();
        
        for inner_pair in pair.into_inner() {
            match inner_pair.as_rule() {
                Rule::function_type => {
                    return_type = parse_type(inner_pair)?;
                }
                Rule::identifier => {
                    name = inner_pair.as_str().to_string();
                }
                Rule::parameter_list => {
                    parameters = self.parse_parameter_list(inner_pair)?;
                }
                Rule::function_body => {
                    let parsed_body = self.parse_function_body(inner_pair)?;
                    statements = parsed_body.statements;
                    syntax = parsed_body.syntax;
                }
                _ => {}
            }
        }
        
        Ok(Function {
            name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type,
            body: statements,
            description: None,
            syntax,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(SourceLocation {
                line: span.start_pos().line_col().0,
                column: span.start_pos().line_col().1,
                file: String::new(),
            }),
        })
    }
    
    /// Parse parameter list
    fn parse_parameter_list(&self, pair: Pair<Rule>) -> Result<Vec<Parameter>, CompilerError> {
        let mut parameters = Vec::new();
        
        for param_pair in pair.into_inner() {
            if param_pair.as_rule() == Rule::parameter {
                parameters.push(self.parse_parameter(param_pair)?);
            }
        }
        
        Ok(parameters)
    }
    
    /// Parse a single parameter
    fn parse_parameter(&self, pair: Pair<Rule>) -> Result<Parameter, CompilerError> {
        let mut param_type = Type::Any;
        let mut name = String::new();
        let mut default_value = None;
        
        for inner_pair in pair.into_inner() {
            match inner_pair.as_rule() {
                Rule::type_ => {
                    param_type = parse_type(inner_pair)?;
                }
                Rule::identifier => {
                    name = inner_pair.as_str().to_string();
                }
                Rule::expression => {
                    default_value = Some(super::expression_parser::parse_expression(inner_pair)?);
                }
                _ => {}
            }
        }
        
        Ok(Parameter {
            name,
            type_: param_type,
            default_value,
        })
    }
    
    /// Parse function body
    fn parse_function_body(&self, pair: Pair<Rule>) -> Result<FunctionBodyResult, CompilerError> {
        let mut statements = Vec::new();
        let mut syntax = FunctionSyntax::Simple;
        
        for inner_pair in pair.into_inner() {
            match inner_pair.as_rule() {
                Rule::setup_block => {
                    syntax = FunctionSyntax::Detailed;
                    // Skip setup parsing for now - handled elsewhere
                }
                Rule::function_content => {
                    let content_statements = self.parse_function_content(inner_pair)?;
                    statements.extend(content_statements);
                }
                Rule::limited_statements => {
                    let content_statements = self.parse_limited_statements(inner_pair)?;
                    statements.extend(content_statements);
                }
                _ => {}
            }
        }
        
        Ok(FunctionBodyResult { statements, syntax })
    }
    
    /// Parse function content (statements)
    fn parse_function_content(&self, pair: Pair<Rule>) -> Result<Vec<Statement>, CompilerError> {
        let mut statements = Vec::new();
        
        for inner_pair in pair.into_inner() {
            match inner_pair.as_rule() {
                Rule::limited_statement | Rule::statement => {
                    statements.push(parse_statement(inner_pair)?);
                }
                Rule::limited_statements => {
                    let nested_statements = self.parse_limited_statements(inner_pair)?;
                    statements.extend(nested_statements);
                }
                _ => {}
            }
        }
        
        Ok(statements)
    }
    
    /// Parse limited statements
    fn parse_limited_statements(&self, pair: Pair<Rule>) -> Result<Vec<Statement>, CompilerError> {
        let mut statements = Vec::new();
        
        for inner_pair in pair.into_inner() {
            if inner_pair.as_rule() == Rule::limited_statement {
                statements.push(parse_statement(inner_pair)?);
            }
        }
        
        Ok(statements)
    }
    
    /// Fallback to traditional parsing if lexical analysis fails
    fn parse_functions_traditional(&self, pair: Pair<Rule>) -> Result<Vec<Function>, CompilerError> {
        // Use the existing parser implementation as fallback
        super::parser_impl::parse_functions_block_traditional(pair)
    }
}

/// Result of parsing a function body
struct FunctionBodyResult {
    statements: Vec<Statement>,
    syntax: FunctionSyntax,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_function_segment_parsing() {
        let source = r#"functions:
	integer factorial(integer n)
		if n <= 1
			return 1
		return n * factorial(n - 1)
	
	number divide(number a, number b)
		return a / b"#;
        
        let parser = FunctionParser::new(source);
        let segments = parser.lexical_analyzer.split_into_function_segments();
        
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].boundary.function_name, "factorial");
        assert_eq!(segments[1].boundary.function_name, "divide");
    }
}