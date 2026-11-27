/*
 * Clean Language Server - Diagnostics Provider
 *
 * Converts compiler errors to LSP diagnostics with accurate positions and helpful messages.
 * Also validates plugin keywords and block structure.
 */

use clean_language_compiler::error::CompilerError;
use std::path::Path;
use tower_lsp::lsp_types::*;

pub struct DiagnosticsProvider;

impl DiagnosticsProvider {
    pub fn new() -> Self {
        Self
    }

    /// Convert the result of checking one document into LSP diagnostics.
    ///
    /// `errors` become `ERROR` diagnostics and `warnings` become `WARNING`
    /// diagnostics; each carries the compiler's error code when it has one.
    /// `document_path` is the file the document is; a diagnostic located in
    /// another file (an imported module) cannot be placed in this document, so
    /// it is shown at the top of the document with the file and position in
    /// its message.
    pub async fn convert_compiler_errors(
        &self,
        errors: &[CompilerError],
        warnings: &[CompilerError],
        _source: &str,
        document_path: Option<&Path>,
    ) -> Vec<Diagnostic> {
        let errors = errors
            .iter()
            .map(|error| self.convert_error(error, None, document_path));
        let warnings = warnings.iter().map(|warning| {
            self.convert_error(warning, Some(DiagnosticSeverity::WARNING), document_path)
        });
        errors.chain(warnings).collect()
    }

    fn convert_error(
        &self,
        error: &CompilerError,
        severity_override: Option<DiagnosticSeverity>,
        document_path: Option<&Path>,
    ) -> Diagnostic {
        let (severity, mut message, mut range) = self.analyze_error(error);
        let severity = severity_override.unwrap_or(severity);

        if let Some(location) = error.context().and_then(|c| c.location.as_ref()) {
            if !location.file.is_empty() && !is_same_file(&location.file, document_path) {
                message = format!(
                    "{}:{}:{}: {}",
                    location.file, location.line, location.column, message
                );
                range = Range::default();
            }
        }

        Diagnostic {
            range,
            severity: Some(severity),
            code: error
                .context()
                .and_then(|c| c.error_code.clone())
                .map(NumberOrString::String),
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
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
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

                (
                    DiagnosticSeverity::ERROR,
                    self.enhance_syntax_message(&context.message),
                    range,
                )
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

                (
                    DiagnosticSeverity::ERROR,
                    self.enhance_type_message(&context.message),
                    range,
                )
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

                (
                    DiagnosticSeverity::ERROR,
                    self.enhance_semantic_message(&context.message),
                    range,
                )
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

                (
                    DiagnosticSeverity::ERROR,
                    self.enhance_resolver_message(&context.message),
                    range,
                )
            }
            CompilerError::Codegen { context } => (
                DiagnosticSeverity::ERROR,
                format!("Code generation error: {}", context.message),
                default_range,
            ),
            CompilerError::IO { context } => (
                DiagnosticSeverity::ERROR,
                format!("I/O error: {}", context.message),
                default_range,
            ),
            CompilerError::Runtime { context } => (
                DiagnosticSeverity::ERROR,
                format!("Runtime error: {}", context.message),
                default_range,
            ),
            CompilerError::Memory { context } => (
                DiagnosticSeverity::ERROR,
                format!("Memory error: {}", context.message),
                default_range,
            ),
            CompilerError::LexError(_) => (
                DiagnosticSeverity::ERROR,
                "Lexical analysis error".to_string(),
                default_range,
            ),
            CompilerError::PluginError { message, location } => {
                let range = location
                    .as_ref()
                    .map(|loc| Range {
                        start: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: loc.column as u32,
                        },
                        end: Position {
                            line: loc.line.saturating_sub(1) as u32,
                            character: (loc.column + 15) as u32,
                        },
                    })
                    .unwrap_or(default_range);

                (
                    DiagnosticSeverity::ERROR,
                    format!("Plugin error: {}", message),
                    range,
                )
            }
        }
    }

    fn enhance_syntax_message(&self, msg: &str) -> String {
        if msg.contains("expected") {
            format!("{msg}\n\n💡 Tip: Check for missing colons (:) after keywords like 'functions', 'class', 'if', etc.")
        } else if msg.contains("unexpected") {
            format!("{msg}\n\n💡 Tip: Ensure proper Clean Language syntax is used")
        } else if msg.contains("function") {
            format!("{msg}\n\n💡 Tip: Functions should be inside a 'functions:' block or be a standalone 'start()' function")
        } else if msg.contains("indentation") || msg.contains("tab") {
            format!("{msg}\n\n💡 Tip: Clean Language uses tab-based indentation")
        } else {
            msg.to_string()
        }
    }

    fn enhance_semantic_message(&self, msg: &str) -> String {
        if msg.contains("undefined") {
            format!("{msg}\n\n💡 Tip: Make sure variables and functions are declared before use")
        } else if msg.contains("scope") {
            format!("{msg}\n\n💡 Tip: Check variable and function visibility scopes")
        } else if msg.contains("return") {
            format!("{msg}\n\n💡 Tip: Functions with return types must have return statements")
        } else {
            msg.to_string()
        }
    }

    fn enhance_type_message(&self, msg: &str) -> String {
        if msg.contains("mismatch") {
            format!("{msg}\n\n💡 Tip: Check that variable and function types match their usage")
        } else if msg.contains("inference") {
            format!("{msg}\n\n💡 Tip: Consider adding explicit type annotations")
        } else if msg.contains("constraint") {
            format!("{msg}\n\n💡 Tip: Review type relationships and generic constraints")
        } else {
            msg.to_string()
        }
    }

    fn enhance_resolver_message(&self, msg: &str) -> String {
        if msg.contains("not found") {
            format!("{msg}\n\n💡 Tip: Check spelling and ensure the symbol is imported or defined")
        } else if msg.contains("ambiguous") {
            format!("{msg}\n\n💡 Tip: Use fully qualified names to resolve ambiguity")
        } else if msg.contains("circular") {
            format!("{msg}\n\n💡 Tip: Break circular dependencies between modules or types")
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

/// True when a diagnostic's file name refers to the document being checked.
/// Without a document path (an unsaved buffer with no file) every located
/// diagnostic is taken to be in the document.
fn is_same_file(diagnostic_file: &str, document_path: Option<&Path>) -> bool {
    let Some(document_path) = document_path else {
        return true;
    };
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical(Path::new(diagnostic_file)) == canonical(document_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clean_language_compiler::ast::SourceLocation;

    fn located(message: &str, code: &'static str, file: &str, line: usize) -> CompilerError {
        let location = SourceLocation {
            file: file.to_string(),
            line,
            column: 3,
            ..Default::default()
        };
        CompilerError::syntax_error_with_code(message, None, Some(location), code)
    }

    async fn convert(
        errors: &[CompilerError],
        warnings: &[CompilerError],
        path: Option<&Path>,
    ) -> Vec<Diagnostic> {
        DiagnosticsProvider::new()
            .convert_compiler_errors(errors, warnings, "", path)
            .await
    }

    #[tokio::test]
    async fn errors_carry_their_code_and_error_severity() {
        let errors = [located("boom", "SEM001", "a.cln", 2)];
        let diagnostics = convert(&errors, &[], Some(Path::new("a.cln"))).await;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("SEM001".to_string()))
        );
        assert_eq!(diagnostics[0].range.start.line, 1);
    }

    #[tokio::test]
    async fn warnings_are_sent_as_warnings_with_their_code() {
        let warnings = [located("careful", "FUNC005", "a.cln", 5)];
        let diagnostics = convert(&[], &warnings, Some(Path::new("a.cln"))).await;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("FUNC005".to_string()))
        );
    }

    #[tokio::test]
    async fn a_diagnostic_in_another_file_is_pinned_to_the_top_with_its_origin() {
        let errors = [located("bad import body", "SEM002", "other.cln", 9)];
        let diagnostics = convert(&errors, &[], Some(Path::new("a.cln"))).await;
        assert_eq!(diagnostics[0].range, Range::default());
        assert!(
            diagnostics[0].message.starts_with("other.cln:9:3: "),
            "message: {}",
            diagnostics[0].message
        );
    }
}
