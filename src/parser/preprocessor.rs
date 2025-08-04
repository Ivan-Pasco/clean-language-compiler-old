use super::{CleanParser, Rule};
use crate::ast::{Field, Function};
use crate::error::CompilerError;
/// Preprocessing solution for handling multiple functions in Clean Language
///
/// This approach completely isolates individual functions before parsing,
/// preventing PEG parser boundary issues that cause statement fallback to expression parsing.
/// Enhanced to maintain class context when processing class methods in functions: blocks.
use pest::Parser;

/// Preprocessor that handles functions block parsing by isolating individual functions
pub struct FunctionPreprocessor {
    source: String,
    /// Class context for when processing class methods
    class_context: Option<ClassContext>,
}

/// Class context information for processing class methods
#[derive(Debug, Clone)]
pub struct ClassContext {
    pub class_name: String,
    pub fields: Vec<Field>,
    pub base_class: Option<String>,
}

impl FunctionPreprocessor {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            class_context: None,
        }
    }

    /// Create a new preprocessor with class context for processing class methods
    pub fn with_class_context(source: &str, class_context: ClassContext) -> Self {
        Self {
            source: source.to_string(),
            class_context: Some(class_context),
        }
    }

    /// Main preprocessing method: isolate and parse functions individually
    pub fn process_functions_block(
        &self,
        functions_block_source: &str,
    ) -> Result<Vec<Function>, CompilerError> {
        // Extract individual function text segments
        let function_segments = self.extract_function_segments(functions_block_source)?;

        if function_segments.is_empty() {
            return Ok(Vec::new());
        }

        let mut functions = Vec::new();

        // Parse each function in complete isolation
        for (_index, segment) in function_segments.iter().enumerate() {
            // Create a complete, standalone functions block with just this one function
            // Normalize indentation to match functions: block expectations
            let normalized_segment = self.normalize_function_indentation(segment);
            let isolated_source = format!("functions:\n{}", normalized_segment);

            // Parse this isolated function
            match self.parse_isolated_function(&isolated_source) {
                Ok(function) => {
                    functions.push(function);
                }
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
                // This line might start a new function or end the functions block
                if self.is_function_declaration_line(line) {
                    // Save current function and start new one
                    segments.push(current_function_lines.join("\n"));
                    current_function_lines.clear();
                    base_indentation = indentation;
                    current_function_lines.push(line.to_string());
                } else if trimmed.is_empty() {
                    // Empty line at base level - might be separating functions, add to current function
                    current_function_lines.push(line.to_string());
                } else {
                    // Non-empty line that's not a function declaration - end of functions block
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

            // Special case: start() is not a function declaration in a functions block
            if parts.len() == 1 && parts[0] == "start" {
                return false;
            }

            // Functions in functions: blocks must have explicit return types: "type identifier("
            match parts.len() {
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
        matches!(
            s,
            "integer" | "number" | "string" | "boolean" | "void" | "any"
        ) || s.starts_with("list<")
            || s.starts_with("matrix<")
            || s.starts_with("pairs<")
    }

    /// Parse an isolated function from a complete functions block
    fn parse_isolated_function(&self, source: &str) -> Result<Function, CompilerError> {
        // Parse the complete source
        let parse_result = <CleanParser as Parser<Rule>>::parse(Rule::functions_block, source)
            .map_err(|e| {
                CompilerError::syntax_error(
                    &format!("Failed to parse isolated function: {e}"),
                    None,
                    None,
                )
            })?;

        // Extract the single function
        for pair in parse_result {
            if pair.as_rule() == Rule::functions_block {
                for inner_pair in pair.into_inner() {
                    if inner_pair.as_rule() == Rule::indented_functions_block {
                        for func_pair in inner_pair.into_inner() {
                            if func_pair.as_rule() == Rule::function_in_block {
                                let mut function =
                                    super::parser_impl::parse_function_in_block(func_pair)?;

                                // If we have class context, mark this as a class method
                                if let Some(ref class_ctx) = self.class_context {
                                    function = self
                                        .enhance_function_with_class_context(function, class_ctx)?;
                                }

                                return Ok(function);
                            }
                        }
                    }
                }
            }
        }

        Err(CompilerError::syntax_error(
            "No function found in isolated source",
            None,
            None,
        ))
    }

    /// Enhance a function with class context information
    /// This allows the semantic analyzer to properly handle class field access
    fn enhance_function_with_class_context(
        &self,
        mut function: Function,
        class_ctx: &ClassContext,
    ) -> Result<Function, CompilerError> {
        // Mark the function as a method by storing class context in a custom attribute
        // The semantic analyzer will use this information to inject class fields into scope

        // For now, we'll use the function's description field to store class context
        // In a production implementation, we'd add a proper class_context field to Function
        let class_info = format!("CLASS_CONTEXT:{}", class_ctx.class_name);

        function.description = Some(match function.description {
            Some(existing) => format!("{}\n{}", existing, class_info),
            None => class_info,
        });

        Ok(function)
    }

    /// Get line offset for a function index (for error reporting)
    fn get_function_line_offset(&self, _function_index: usize) -> usize {
        // For now, return 0. In a full implementation, we'd track line numbers
        0
    }

    /// Normalize function indentation to match functions: block expectations
    fn normalize_function_indentation(&self, segment: &str) -> String {
        let lines: Vec<&str> = segment.lines().collect();
        if lines.is_empty() {
            return segment.to_string();
        }

        // Find the minimum indentation of non-empty lines (this should be the function declaration)
        let mut min_indent = usize::MAX;
        for line in &lines {
            if !line.trim().is_empty() {
                let indent = self.count_indentation(line);
                if indent < min_indent {
                    min_indent = indent;
                }
            }
        }

        // If no indentation found, return as-is
        if min_indent == usize::MAX {
            return segment.to_string();
        }

        // Remove the minimum indentation from all lines and add single tab for functions: block
        let mut normalized_lines = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                normalized_lines.push(String::new()); // Empty lines remain empty
            } else {
                let current_indent = self.count_indentation(line);
                let relative_indent = current_indent.saturating_sub(min_indent);
                let new_line = format!("\t{}{}", "\t".repeat(relative_indent), line.trim_start());
                normalized_lines.push(new_line);
            }
        }

        normalized_lines.join("\n")
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
