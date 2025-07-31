use std::collections::HashMap;

/// Lexical analyzer for identifying function boundaries and indentation structure
#[derive(Debug, Clone)]
pub struct LexicalAnalyzer {
    pub source: String,
    pub lines: Vec<String>,
    pub function_boundaries: Vec<FunctionBoundary>,
    pub indentation_levels: HashMap<usize, usize>, // line_number -> indentation_level
}

/// Represents a function boundary in the source code
#[derive(Debug, Clone)]
pub struct FunctionBoundary {
    pub start_line: usize,
    pub end_line: usize,
    pub function_name: String,
    pub base_indentation: usize,
    pub has_complex_body: bool, // Contains if-statements, loops, etc.
}

impl LexicalAnalyzer {
    pub fn new(source: &str) -> Self {
        let lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
        let mut analyzer = Self {
            source: source.to_string(),
            lines,
            function_boundaries: Vec::new(),
            indentation_levels: HashMap::new(),
        };
        
        analyzer.analyze();
        analyzer
    }
    
    /// Main analysis method that identifies function boundaries
    fn analyze(&mut self) {
        self.analyze_indentation();
        self.identify_function_boundaries();
    }
    
    /// Analyze indentation levels for each line
    fn analyze_indentation(&mut self) {
        for (line_num, line) in self.lines.iter().enumerate() {
            let indentation = self.count_indentation(line);
            self.indentation_levels.insert(line_num, indentation);
        }
    }
    
    /// Count the indentation level of a line (tabs and spaces)
    fn count_indentation(&self, line: &str) -> usize {
        let mut count = 0;
        for ch in line.chars() {
            match ch {
                '\t' => count += 1,
                ' ' => count += 1,
                _ => break,
            }
        }
        count
    }
    
    /// Identify function boundaries within functions: blocks
    fn identify_function_boundaries(&mut self) {
        let mut in_functions_block = false;
        let mut functions_block_indentation = 0;
        let mut current_function: Option<FunctionBoundary> = None;
        
        for (line_num, line) in self.lines.iter().enumerate() {
            let trimmed = line.trim();
            let indentation = self.indentation_levels[&line_num];
            
            // Check for functions: block start
            if trimmed == "functions:" {
                in_functions_block = true;
                functions_block_indentation = indentation;
                continue;
            }
            
            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            
            if in_functions_block {
                // Check if we've exited the functions block
                if indentation <= functions_block_indentation && !trimmed.is_empty() {
                    // Finalize current function if any
                    if let Some(mut func) = current_function.take() {
                        func.end_line = line_num.saturating_sub(1);
                        self.function_boundaries.push(func);
                    }
                    in_functions_block = false;
                    continue;
                }
                
                // Check for function declaration
                if self.is_function_declaration(trimmed) {
                    // Finalize previous function if any
                    if let Some(mut func) = current_function.take() {
                        func.end_line = line_num.saturating_sub(1);
                        self.function_boundaries.push(func);
                    }
                    
                    // Start new function
                    let function_name = self.extract_function_name(trimmed);
                    current_function = Some(FunctionBoundary {
                        start_line: line_num,
                        end_line: line_num, // Will be updated when function ends
                        function_name,
                        base_indentation: indentation,
                        has_complex_body: false,
                    });
                } else if let Some(ref mut func) = current_function {
                    // Check for complex constructs within function body
                    if trimmed.starts_with("if ") || 
                       trimmed.starts_with("iterate ") ||
                       trimmed.starts_with("test ") {
                        func.has_complex_body = true;
                    }
                }
            }
        }
        
        // Finalize last function if any
        if let Some(mut func) = current_function.take() {
            func.end_line = self.lines.len().saturating_sub(1);
            self.function_boundaries.push(func);
        }
    }
    
    /// Check if a line contains a function declaration
    fn is_function_declaration(&self, line: &str) -> bool {
        // Pattern: [type] identifier(parameters)
        if line.contains('(') && line.contains(')') {
            let before_paren = line.split('(').next().unwrap_or("");
            let parts: Vec<&str> = before_paren.split_whitespace().collect();
            
            // Should have at least identifier, optionally with return type
            match parts.len() {
                1 => {
                    // Just identifier(params) - like factorial(integer n)
                    self.is_valid_identifier(parts[0])
                },
                2 => {
                    // type identifier(params) - like integer factorial(integer n)
                    self.is_valid_type(parts[0]) && self.is_valid_identifier(parts[1])
                },
                _ => false
            }
        } else {
            false
        }
    }
    
    /// Extract function name from declaration line
    fn extract_function_name(&self, line: &str) -> String {
        if let Some(paren_pos) = line.find('(') {
            let before_paren = &line[..paren_pos];
            let parts: Vec<&str> = before_paren.split_whitespace().collect();
            
            match parts.len() {
                1 => parts[0].to_string(),
                2 => parts[1].to_string(),
                _ => "unknown".to_string()
            }
        } else {
            "unknown".to_string()
        }
    }
    
    /// Check if a string is a valid identifier
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
    
    /// Check if a string is a valid type
    fn is_valid_type(&self, s: &str) -> bool {
        matches!(s, "integer" | "number" | "string" | "boolean" | "void" | "any") ||
        s.starts_with("list<") ||
        s.starts_with("matrix<") ||
        s.starts_with("pairs<") ||
        self.is_valid_identifier(s) // Could be a custom type
    }
    
    /// Get function boundary for a specific line
    pub fn get_function_boundary_for_line(&self, line_num: usize) -> Option<&FunctionBoundary> {
        self.function_boundaries.iter()
            .find(|boundary| line_num >= boundary.start_line && line_num <= boundary.end_line)
    }
    
    /// Split source into function segments
    pub fn split_into_function_segments(&self) -> Vec<FunctionSegment> {
        let mut segments = Vec::new();
        
        for boundary in &self.function_boundaries {
            let function_lines: Vec<String> = self.lines[boundary.start_line..=boundary.end_line]
                .iter()
                .cloned()
                .collect();
            
            segments.push(FunctionSegment {
                boundary: boundary.clone(),
                source: function_lines.join("\n"),
                line_offset: boundary.start_line,
            });
        }
        
        segments
    }
}

/// Represents a segment of source code containing a single function
#[derive(Debug, Clone)]
pub struct FunctionSegment {
    pub boundary: FunctionBoundary,
    pub source: String,
    pub line_offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_function_detection() {
        let source = r#"functions:
	integer factorial(integer n)
		if n <= 1
			return 1
		return n * factorial(n - 1)
	
	number divide(number a, number b)
		return a / b"#;
        
        let analyzer = LexicalAnalyzer::new(source);
        assert_eq!(analyzer.function_boundaries.len(), 2);
        
        let first_func = &analyzer.function_boundaries[0];
        assert_eq!(first_func.function_name, "factorial");
        assert!(first_func.has_complex_body);
        
        let second_func = &analyzer.function_boundaries[1];
        assert_eq!(second_func.function_name, "divide");
        assert!(!second_func.has_complex_body);
    }
    
    #[test]
    fn test_function_without_return_type() {
        let source = r#"functions:
	factorial(integer n)
		return n * 2"#;
        
        let analyzer = LexicalAnalyzer::new(source);
        assert_eq!(analyzer.function_boundaries.len(), 1);
        assert_eq!(analyzer.function_boundaries[0].function_name, "factorial");
    }
}