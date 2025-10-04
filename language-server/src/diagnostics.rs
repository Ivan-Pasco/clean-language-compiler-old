/*
 * Clean Language Server - Diagnostics Provider
 *
 * Converts compiler errors to LSP diagnostics with accurate positions and helpful messages
 */

use clean_language_compiler::error::CompilerError;
use tower_lsp::lsp_types::*;

pub struct DiagnosticsProvider;

impl DiagnosticsProvider {
    pub fn new() -> Self {
        Self
    }

    pub async fn convert_compiler_errors(&self, errors: &[CompilerError], _source: &str) -> Vec<Diagnostic> {
        errors.iter().map(|error| self.convert_error(error)).collect()
    }

    fn convert_error(&self, error: &CompilerError) -> Diagnostic {
        let (severity, message, range) = self.analyze_error(error);

        Diagnostic {
            range,
            severity: Some(severity),
            code: None,
            code_description: None,
            source: Some("clean-compiler".to_string()),
            message,
            related_information: None,
            tags: None,
            data: None,
        }
    }

    fn analyze_error(&self, error: &CompilerError) -> (DiagnosticSeverity, String, Range) {
        let default_range = Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 0 },
        };

        match error {
            CompilerError::Syntax { context } => {
                let range = context
                    .location
                    .as_ref()
                    .map(|loc| Range {
                        start: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: loc.column as u32,
                        },
                        end: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: (loc.column + 10) as u32, // Approximate end
                        },
                    })
                    .unwrap_or(default_range);

                (DiagnosticSeverity::ERROR, self.enhance_syntax_message(&context.message), range)
            }
            CompilerError::Type { context } => {
                let range = context
                    .location
                    .as_ref()
                    .map(|loc| Range {
                        start: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: loc.column as u32,
                        },
                        end: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: (loc.column + 20) as u32, // Approximate end
                        },
                    })
                    .unwrap_or(default_range);

                (DiagnosticSeverity::ERROR, self.enhance_type_message(&context.message), range)
            }
            CompilerError::Validation { context } => {
                let range = context
                    .location
                    .as_ref()
                    .map(|loc| Range {
                        start: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: loc.column as u32,
                        },
                        end: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: (loc.column + 15) as u32, // Approximate end
                        },
                    })
                    .unwrap_or(default_range);

                (DiagnosticSeverity::ERROR, self.enhance_semantic_message(&context.message), range)
            }
            CompilerError::Module { context } => {
                let range = context
                    .location
                    .as_ref()
                    .map(|loc| Range {
                        start: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: loc.column as u32,
                        },
                        end: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: (loc.column + 12) as u32, // Approximate end
                        },
                    })
                    .unwrap_or(default_range);

                (DiagnosticSeverity::ERROR, self.enhance_resolver_message(&context.message), range)
            }
            CompilerError::Codegen { context } => {
                (DiagnosticSeverity::ERROR, format!("Code generation error: {}", context.message), default_range)
            }
            CompilerError::IO { context } => {
                (DiagnosticSeverity::ERROR, format!("I/O error: {}", context.message), default_range)
            }
            CompilerError::Runtime { context } => {
                (DiagnosticSeverity::ERROR, format!("Runtime error: {}", context.message), default_range)
            }
            CompilerError::Memory { context } => {
                (DiagnosticSeverity::ERROR, format!("Memory error: {}", context.message), default_range)
            }
            CompilerError::Testing { context } => {
                (DiagnosticSeverity::WARNING, format!("Testing: {}", context.message), default_range)
            }
            CompilerError::LexError(_) => {
                (DiagnosticSeverity::ERROR, "Lexical analysis error".to_string(), default_range)
            }
        }
    }

    fn enhance_syntax_message(&self, msg: &str) -> String {
        if msg.contains("expected") {
            format!("{}\n\n💡 Tip: Check for missing colons (:) after keywords like 'functions', 'class', 'if', etc.", msg)
        } else if msg.contains("unexpected") {
            format!("{}\n\n💡 Tip: Ensure proper Clean Language syntax is used", msg)
        } else if msg.contains("function") {
            format!("{}\n\n💡 Tip: Functions should be inside a 'functions:' block or be a standalone 'start()' function", msg)
        } else if msg.contains("indentation") || msg.contains("tab") {
            format!("{}\n\n💡 Tip: Clean Language uses tab-based indentation", msg)
        } else {
            msg.to_string()
        }
    }

    fn enhance_semantic_message(&self, msg: &str) -> String {
        if msg.contains("undefined") {
            format!("{}\n\n💡 Tip: Make sure variables and functions are declared before use", msg)
        } else if msg.contains("scope") {
            format!("{}\n\n💡 Tip: Check variable and function visibility scopes", msg)
        } else if msg.contains("return") {
            format!("{}\n\n💡 Tip: Functions with return types must have return statements", msg)
        } else {
            msg.to_string()
        }
    }

    fn enhance_type_message(&self, msg: &str) -> String {
        if msg.contains("mismatch") {
            format!("{}\n\n💡 Tip: Check that variable and function types match their usage", msg)
        } else if msg.contains("inference") {
            format!("{}\n\n💡 Tip: Consider adding explicit type annotations", msg)
        } else if msg.contains("constraint") {
            format!("{}\n\n💡 Tip: Review type relationships and generic constraints", msg)
        } else {
            msg.to_string()
        }
    }

    fn enhance_resolver_message(&self, msg: &str) -> String {
        if msg.contains("not found") {
            format!("{}\n\n💡 Tip: Check spelling and ensure the symbol is imported or defined", msg)
        } else if msg.contains("ambiguous") {
            format!("{}\n\n💡 Tip: Use fully qualified names to resolve ambiguity", msg)
        } else if msg.contains("circular") {
            format!("{}\n\n💡 Tip: Break circular dependencies between modules or types", msg)
        } else {
            msg.to_string()
        }
    }
}

impl Default for DiagnosticsProvider {
    fn default() -> Self {
        Self::new()
    }
}