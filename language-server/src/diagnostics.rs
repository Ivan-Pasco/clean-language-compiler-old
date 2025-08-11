/*
 * Clean Language Diagnostics Provider
 * Created by Ivan Pasco
 */

use tower_lsp::lsp_types::*;
use crate::parser::CleanASTNode;

pub struct DiagnosticsProvider;

impl DiagnosticsProvider {
    pub fn new() -> Self {
        Self
    }

    pub async fn analyze(&self, ast: &CleanASTNode, text: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        
        match ast {
            CleanASTNode::Program { functions, classes, start_function } => {
                // Check for required start function
                if start_function.is_none() && functions.is_empty() && classes.is_empty() {
                    diagnostics.push(Diagnostic {
                        range: Range {
                            start: Position { line: 0, character: 0 },
                            end: Position { line: 0, character: 0 },
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: "Clean programs must have at least a start() function".to_string(),
                        source: Some("clean-analyzer".to_string()),
                        code: Some(NumberOrString::String("CL001".to_string())),
                        ..Default::default()
                    });
                }
                
                // Validate functions
                for function in functions {
                    self.validate_function(function, &mut diagnostics);
                }
                
                // Validate classes
                for class in classes {
                    self.validate_class(class, &mut diagnostics);
                }
                
                // Validate start function if present
                if let Some(start) = start_function {
                    self.validate_start_function(start.as_ref(), &mut diagnostics);
                }
            }
            _ => {
                // Single node validation
                self.validate_node(ast, &mut diagnostics);
            }
        }
        
        // Check for indentation issues
        self.check_indentation_issues(text, &mut diagnostics);
        
        diagnostics
    }

    fn validate_function(&self, function: &CleanASTNode, diagnostics: &mut Vec<Diagnostic>) {
        if let CleanASTNode::Function { name, parameters, return_type: _, body, range } = function {
            // Check function naming conventions
            if name.is_empty() {
                diagnostics.push(Diagnostic {
                    range: *range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "Function name cannot be empty".to_string(),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL002".to_string())),
                    ..Default::default()
                });
            }
            
            // Check for camelCase naming convention
            if !self.is_camel_case(name) && !name.starts_with('_') {
                diagnostics.push(Diagnostic {
                    range: *range,
                    severity: Some(DiagnosticSeverity::INFORMATION),
                    message: format!("Function '{}' should use camelCase naming", name),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL003".to_string())),
                    ..Default::default()
                });
            }
            
            // Check for empty function body
            if body.is_empty() {
                diagnostics.push(Diagnostic {
                    range: *range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!("Function '{}' has empty body", name),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL004".to_string())),
                    ..Default::default()
                });
            }
            
            // Validate function body
            for stmt in body {
                self.validate_node(stmt, diagnostics);
            }
        }
    }

    fn validate_class(&self, class: &CleanASTNode, diagnostics: &mut Vec<Diagnostic>) {
        if let CleanASTNode::Class { name, extends: _, fields, constructor: _, methods, range } = class {
            // Check class naming (should be PascalCase)
            if !self.is_pascal_case(name) {
                diagnostics.push(Diagnostic {
                    range: *range,
                    severity: Some(DiagnosticSeverity::INFORMATION),
                    message: format!("Class '{}' should use PascalCase naming", name),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL005".to_string())),
                    ..Default::default()
                });
            }
            
            // Check for empty class
            if fields.is_empty() && methods.is_empty() {
                diagnostics.push(Diagnostic {
                    range: *range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!("Class '{}' is empty", name),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL006".to_string())),
                    ..Default::default()
                });
            }
            
            // Validate methods
            for method in methods {
                self.validate_function(method, diagnostics);
            }
        }
    }

    fn validate_start_function(&self, start: &CleanASTNode, diagnostics: &mut Vec<Diagnostic>) {
        if let CleanASTNode::Function { name, parameters, return_type: _, body: _, range } = start {
            // start() function should have no parameters
            if !parameters.is_empty() {
                diagnostics.push(Diagnostic {
                    range: *range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "start() function should not have parameters".to_string(),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL007".to_string())),
                    ..Default::default()
                });
            }
        }
    }

    fn validate_node(&self, node: &CleanASTNode, diagnostics: &mut Vec<Diagnostic>) {
        match node {
            CleanASTNode::VariableDeclaration { var_type: _, name, value: _, range } => {
                if name.is_empty() {
                    diagnostics.push(Diagnostic {
                        range: *range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: "Variable name cannot be empty".to_string(),
                        source: Some("clean-analyzer".to_string()),
                        code: Some(NumberOrString::String("CL008".to_string())),
                        ..Default::default()
                    });
                }
            }
            CleanASTNode::StringInterpolation { parts: _, range } => {
                // Could add validation for string interpolation syntax
            }
            CleanASTNode::BinaryOperation { left: _, operator, right: _, range } => {
                // Validate operator usage
                if !self.is_valid_operator(operator) {
                    diagnostics.push(Diagnostic {
                        range: *range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: format!("Invalid operator: {}", operator),
                        source: Some("clean-analyzer".to_string()),
                        code: Some(NumberOrString::String("CL009".to_string())),
                        ..Default::default()
                    });
                }
            }
            CleanASTNode::ImportStatement { module, items: _, alias: _, range } => {
                if module.is_empty() {
                    diagnostics.push(Diagnostic {
                        range: *range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: "Import module name cannot be empty".to_string(),
                        source: Some("clean-analyzer".to_string()),
                        code: Some(NumberOrString::String("CL010".to_string())),
                        ..Default::default()
                    });
                }
            }
            _ => {
                // Additional validation for other node types
            }
        }
    }

    fn check_indentation_issues(&self, text: &str, diagnostics: &mut Vec<Diagnostic>) {
        let lines: Vec<&str> = text.lines().collect();
        
        for (line_idx, line) in lines.iter().enumerate() {
            // Check for mixed tabs and spaces
            if line.starts_with(' ') && line.contains('\t') {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position { line: line_idx as u32, character: 0 },
                        end: Position { line: line_idx as u32, character: line.len() as u32 },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "Clean Language requires tab-based indentation only. Do not mix tabs and spaces".to_string(),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL011".to_string())),
                    ..Default::default()
                });
            }
            
            // Check for space-only indentation (should be tabs)
            if line.starts_with("    ") && !line.trim().is_empty() {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position { line: line_idx as u32, character: 0 },
                        end: Position { line: line_idx as u32, character: 4 },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "Use tabs for indentation instead of spaces".to_string(),
                    source: Some("clean-analyzer".to_string()),
                    code: Some(NumberOrString::String("CL012".to_string())),
                    ..Default::default()
                });
            }
        }
    }

    fn is_camel_case(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        
        let first_char = name.chars().next().unwrap();
        first_char.is_ascii_lowercase() && 
        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') &&
        !name.contains("__")
    }

    fn is_pascal_case(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        
        let first_char = name.chars().next().unwrap();
        first_char.is_ascii_uppercase() && 
        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') &&
        !name.contains("__")
    }

    fn is_valid_operator(&self, operator: &str) -> bool {
        matches!(operator, 
            "+" | "-" | "*" | "/" | "%" | "^" |
            "==" | "!=" | "<" | ">" | "<=" | ">=" |
            "and" | "or" | "not" | "is" |
            "=" | "+=" | "-=" | "*=" | "/="
        )
    }
}