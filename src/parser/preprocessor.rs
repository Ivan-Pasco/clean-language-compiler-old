/// Preprocessing solution for handling multiple functions in Clean Language
/// 
/// This approach completely isolates individual functions before parsing,
/// preventing PEG parser boundary issues that cause statement fallback to expression parsing.

use pest::Parser;
use crate::ast::Function;
use crate::error::CompilerError;
use super::{CleanParser, Rule};

/// Preprocessor that handles functions block parsing by isolating individual functions
pub struct FunctionPreprocessor {
    source: String,
}

impl FunctionPreprocessor {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
        }
    }
    
    /// Main preprocessing method: isolate and parse functions individually
    pub fn process_functions_block(&self, functions_block_source: &str) -> Result<Vec<Function>, CompilerError> {
        // Extract individual function text segments
        let function_segments = self.extract_function_segments(functions_block_source)?;
        
        if function_segments.is_empty() {
            return Ok(Vec::new());
        }
        
        let mut functions = Vec::new();
        
        // Parse each function in complete isolation
        for (_index, segment) in function_segments.iter().enumerate() {
            // Create a complete, standalone functions block with just this one function
            let isolated_source = format!("functions:\n{}", segment);
            
            // Parse this isolated function
            match self.parse_isolated_function(&isolated_source) {
                Ok(function) => functions.push(function),
                Err(error) => {
                    // For now, just return the error without location adjustment
                    // In a full implementation, we'd adjust the error location
                    return Err(error);
                }
            }
        }
        
        Ok(functions)
    }
    
    /// Extract individual function text segments from functions block
    fn extract_function_segments(&self, source: &str) -> Result<Vec<String>, CompilerError> {
        let lines: Vec<&str> = source.lines().collect();
        let mut segments = Vec::new();
        let mut current_function_lines = Vec::new();
        let mut in_functions_block = false;
        let mut base_indentation = 0;
        
        for line in lines {
            let trimmed = line.trim();
            
            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") {
                if !current_function_lines.is_empty() {
                    current_function_lines.push(line.to_string());
                }
                continue;
            }
            
            // Check for functions: block start
            if trimmed == "functions:" {
                in_functions_block = true;
                continue;
            }
            
            if !in_functions_block {
                continue;
            }
            
            let indentation = self.count_indentation(line);
            
            // Check if this is a function declaration (has parentheses and proper indentation)
            if self.is_function_declaration_line(line) {
                // Save previous function if exists
                if !current_function_lines.is_empty() {
                    segments.push(current_function_lines.join("\n"));
                    current_function_lines.clear();
                }
                
                // Start new function
                base_indentation = indentation;
                current_function_lines.push(line.to_string());
            } else if indentation > base_indentation && !current_function_lines.is_empty() {
                // This line belongs to the current function body
                current_function_lines.push(line.to_string());
            } else if indentation <= base_indentation && !current_function_lines.is_empty() {
                // This line starts a new function or ends the functions block
                if self.is_function_declaration_line(line) {
                    // Save current function and start new one
                    segments.push(current_function_lines.join("\n"));
                    current_function_lines.clear();
                    base_indentation = indentation;
                    current_function_lines.push(line.to_string());
                } else {
                    // End of functions block
                    break;
                }
            }
        }
        
        // Save the last function
        if !current_function_lines.is_empty() {
            segments.push(current_function_lines.join("\n"));
        }
        
        Ok(segments)
    }
    
    /// Check if a line is a function declaration
    fn is_function_declaration_line(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        // Must contain parentheses
        if !trimmed.contains('(') || !trimmed.contains(')') {
            return false;
        }
        
        // Extract the part before the opening parenthesis
        if let Some(paren_pos) = trimmed.find('(') {
            let before_paren = &trimmed[..paren_pos];
            let parts: Vec<&str> = before_paren.split_whitespace().collect();
            
            // Should be either "identifier(" or "type identifier("
            match parts.len() {
                1 => self.is_valid_identifier(parts[0]),
                2 => self.is_valid_type(parts[0]) && self.is_valid_identifier(parts[1]),
                _ => false,
            }
        } else {
            false
        }
    }
    
    /// Count indentation level (tabs and spaces)
    fn count_indentation(&self, line: &str) -> usize {
        line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
    }
    
    /// Check if string is a valid identifier
    fn is_valid_identifier(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        
        let first_char = s.chars().next().unwrap();
        if !first_char.is_ascii_alphabetic() && first_char != '_' {
            return false;
        }
        
        s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
    
    /// Check if string is a valid type
    fn is_valid_type(&self, s: &str) -> bool {
        matches!(s, "integer" | "number" | "string" | "boolean" | "void" | "any") ||
        s.starts_with("list<") ||
        s.starts_with("matrix<") ||
        s.starts_with("pairs<") ||
        self.is_valid_identifier(s)
    }
    
    /// Parse an isolated function from a complete functions block
    fn parse_isolated_function(&self, source: &str) -> Result<Function, CompilerError> {
        // Parse the complete source
        let parse_result = CleanParser::parse(Rule::functions_block, source)
            .map_err(|e| CompilerError::syntax_error(
                &format!("Failed to parse isolated function: {}", e),
                None,
                None
            ))?;
            
        // Extract the single function
        for pair in parse_result {
            if pair.as_rule() == Rule::functions_block {
                for inner_pair in pair.into_inner() {
                    if inner_pair.as_rule() == Rule::indented_functions_block {
                        for func_pair in inner_pair.into_inner() {
                            if func_pair.as_rule() == Rule::function_in_block {
                                return super::parser_impl::parse_function_in_block(func_pair);
                            }
                        }
                    }
                }
            }
        }
        
        Err(CompilerError::syntax_error(
            "No function found in isolated source",
            None,
            None
        ))
    }
    
    /// Get line offset for a function index (for error reporting)
    fn get_function_line_offset(&self, _function_index: usize) -> usize {
        // For now, return 0. In a full implementation, we'd track line numbers
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_function_segment_extraction() {
        let source = r#"functions:
	integer factorial(integer n)
		if n <= 1
			return 1
		return n * factorial(n - 1)
	
	number divide(number a, number b)
		return a / b"#;
        
        let preprocessor = FunctionPreprocessor::new(source);
        let segments = preprocessor.extract_function_segments(source).unwrap();
        
        assert_eq!(segments.len(), 2);
        assert!(segments[0].contains("factorial"));
        assert!(segments[1].contains("divide"));
    }
    
    #[test]
    fn test_simple_functions() {
        let source = r#"functions:
	integer a(integer n)
		return n
	integer b(integer m)
		return m"#;
        
        let preprocessor = FunctionPreprocessor::new(source);
        let segments = preprocessor.extract_function_segments(source).unwrap();
        
        assert_eq!(segments.len(), 2);
    }
}