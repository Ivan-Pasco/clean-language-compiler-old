/*
 * Clean Language Server - Formatting Provider
 *
 * Provides document and range formatting for Clean Language files
 */

use ropey::Rope;
use tower_lsp::lsp_types::*;

pub struct FormattingProvider;

impl FormattingProvider {
    pub fn new() -> Self {
        Self
    }

    pub async fn format_document(
        &self,
        text: &Rope,
        _options: &FormattingOptions,
    ) -> Option<Vec<TextEdit>> {
        let formatted = self.format_text(text);

        if formatted != *text {
            let full_range = Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: text.len_lines() as u32 - 1,
                    character: text.line(text.len_lines() - 1).len_chars() as u32,
                },
            };

            Some(vec![TextEdit {
                range: full_range,
                new_text: formatted,
            }])
        } else {
            None
        }
    }

    pub async fn format_range(
        &self,
        text: &Rope,
        range: &Range,
        _options: &FormattingOptions,
    ) -> Option<Vec<TextEdit>> {
        let start_line = range.start.line as usize;
        let end_line = range.end.line as usize;

        if start_line >= text.len_lines() || end_line >= text.len_lines() {
            return None;
        }

        // Extract the range text
        let range_text = self.extract_range_text(text, range);
        let formatted_range = self.format_text(&Rope::from_str(&range_text));

        if formatted_range != range_text {
            Some(vec![TextEdit {
                range: *range,
                new_text: formatted_range,
            }])
        } else {
            None
        }
    }

    fn format_text(&self, text: &Rope) -> String {
        let lines: Vec<String> = text
            .lines()
            .map(|line| self.format_line(&line.to_string()))
            .collect();

        lines.join("\n")
    }

    fn format_line(&self, line: &str) -> String {
        // Trim trailing whitespace
        let trimmed = line.trim_end();

        if trimmed.is_empty() {
            return String::new();
        }

        // Convert leading spaces to tabs
        let leading_spaces = line.len() - line.trim_start().len();
        let content = line.trim_start();

        // Clean Language uses tab-based indentation
        let tabs = "\t".repeat(leading_spaces / 4);
        let remaining_spaces = " ".repeat(leading_spaces % 4);

        // Format specific Clean Language constructs
        let formatted_content = self.format_clean_syntax(content);

        format!("{tabs}{remaining_spaces}{formatted_content}")
    }

    fn format_clean_syntax(&self, content: &str) -> String {
        let mut result = content.to_string();

        // Ensure proper spacing around operators
        result = self.format_operators(&result);

        // Format function declarations
        result = self.format_function_declarations(&result);

        // Format block keywords
        result = self.format_block_keywords(&result);

        // Format method calls and property access
        result = self.format_method_calls(&result);

        result
    }

    fn format_operators(&self, content: &str) -> String {
        let mut result = content.to_string();

        // Binary operators (ensure single space on both sides)
        let operators = [
            "=", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%",
        ];

        for op in operators.iter() {
            // Skip if it's part of a string literal
            if self.is_in_string(&result, op) {
                continue;
            }

            // Add proper spacing around operators
            let _pattern = format!(r"\s*{}\s*", regex::escape(op));
            let replacement = format!(" {op} ");

            // Simple string replacement for now (would use regex in production)
            result = result.replace(&format!("{op}="), &replacement);
            result = result.replace(&format!("= {op}"), &replacement);
        }

        result
    }

    fn format_function_declarations(&self, content: &str) -> String {
        let mut result = content.to_string();

        // Ensure proper spacing in function declarations
        if content.contains("(") && content.contains(")") {
            // Format parameter lists - no space after opening paren, no space before closing paren
            result = result.replace("( ", "(");
            result = result.replace(" )", ")");

            // Ensure space after comma in parameter lists
            if !self.is_in_string(&result, ",") {
                result = result.replace(",", ", ");
                result = result.replace(",  ", ", "); // Fix double spaces
            }
        }

        result
    }

    fn format_block_keywords(&self, content: &str) -> String {
        let mut result = content.to_string();

        let block_keywords = [
            "functions:",
            "class",
            "can",
            "constants:",
            "types:",
            "if",
            "else",
            "while",
            "for",
        ];

        for keyword in block_keywords.iter() {
            if content.starts_with(keyword) {
                // Ensure proper spacing after block keywords
                let colon_keyword = keyword.ends_with(':');
                if !colon_keyword
                    && !content
                        .chars()
                        .nth(keyword.len())
                        .unwrap_or(' ')
                        .is_whitespace()
                {
                    result = result.replacen(keyword, &format!("{keyword} "), 1);
                }
            }
        }

        result
    }

    fn format_method_calls(&self, content: &str) -> String {
        let mut result = content.to_string();

        // Format method chaining - ensure proper spacing
        if content.contains(".") {
            // No space before or after dots in method calls
            result = result.replace(" .", ".");
            result = result.replace(". ", ".");
        }

        result
    }

    fn is_in_string(&self, content: &str, _target: &str) -> bool {
        // Simple check - in production would properly parse string literals
        let quote_count = content.chars().filter(|&c| c == '"').count();
        quote_count % 2 == 1 // Odd number means we're inside a string
    }

    fn extract_range_text(&self, text: &Rope, range: &Range) -> String {
        let start_line = range.start.line as usize;
        let start_char = range.start.character as usize;
        let end_line = range.end.line as usize;
        let end_char = range.end.character as usize;

        if start_line == end_line {
            // Single line range
            let line = text.line(start_line);
            let line_str = line.to_string();
            let start = start_char.min(line_str.len());
            let end = end_char.min(line_str.len());
            line_str[start..end].to_string()
        } else {
            // Multi-line range
            let mut result = String::new();

            for line_idx in start_line..=end_line {
                if line_idx >= text.len_lines() {
                    break;
                }

                let line = text.line(line_idx);
                let line_str = line.to_string();

                if line_idx == start_line {
                    // First line - from start_char to end
                    let start = start_char.min(line_str.len());
                    result.push_str(&line_str[start..]);
                } else if line_idx == end_line {
                    // Last line - from beginning to end_char
                    let end = end_char.min(line_str.len());
                    result.push_str(&line_str[..end]);
                } else {
                    // Middle lines - entire line
                    result.push_str(&line_str);
                }

                if line_idx < end_line {
                    result.push('\n');
                }
            }

            result
        }
    }
}

impl Default for FormattingProvider {
    fn default() -> Self {
        Self::new()
    }
}

// Helper module for regex escaping (simplified version)
mod regex {
    pub fn escape(text: &str) -> String {
        text.chars()
            .map(|c| match c {
                '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{'
                | '}' => {
                    format!("\\{c}")
                }
                _ => c.to_string(),
            })
            .collect()
    }
}
