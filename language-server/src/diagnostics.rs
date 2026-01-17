/*
 * Clean Language Server - Diagnostics Provider
 *
 * Converts compiler errors to LSP diagnostics with accurate positions and helpful messages.
 * Also validates plugin keywords and block structure.
 */

use clean_language_compiler::error::CompilerError;
use clean_language_compiler::plugins::LanguageRegistry;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

pub struct DiagnosticsProvider {
    /// Language registry for validating plugin keywords
    language_registry: Option<Arc<LanguageRegistry>>,
}

impl DiagnosticsProvider {
    pub fn new() -> Self {
        Self {
            language_registry: None,
        }
    }

    /// Create a diagnostics provider with language registry support
    pub fn with_language_registry(registry: Arc<LanguageRegistry>) -> Self {
        Self {
            language_registry: Some(registry),
        }
    }

    /// Set the language registry (for dynamic updates)
    pub fn set_language_registry(&mut self, registry: Arc<LanguageRegistry>) {
        self.language_registry = Some(registry);
    }

    pub async fn convert_compiler_errors(
        &self,
        errors: &[CompilerError],
        _source: &str,
    ) -> Vec<Diagnostic> {
        errors
            .iter()
            .map(|error| self.convert_error(error))
            .collect()
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
            CompilerError::Testing { context } => (
                DiagnosticSeverity::WARNING,
                format!("Testing: {}", context.message),
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

    /// Validate plugin keywords in the source text
    ///
    /// Returns diagnostics for any unrecognized keywords used in plugin blocks.
    pub fn validate_plugin_keywords(&self, text: &str) -> Vec<Diagnostic> {
        let registry = match &self.language_registry {
            Some(r) => r,
            None => return Vec::new(),
        };

        let mut diagnostics = Vec::new();
        let lines: Vec<&str> = text.lines().collect();

        let mut current_block: Option<String> = None;
        let mut block_indent = 0;

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_indent = line.len() - line.trim_start().len();

            // Detect block start (e.g., "data:", "endpoints:")
            if trimmed.ends_with(':') && !trimmed.starts_with('#') && !trimmed.starts_with("//") {
                let block_name = trimmed.trim_end_matches(':');
                // Check if this is a plugin block
                if registry.has_block(block_name) {
                    current_block = Some(block_name.to_string());
                    block_indent = line_indent;
                } else {
                    // Not a plugin block, reset
                    current_block = None;
                }
                continue;
            }

            // If we've left the block (less indentation), clear it
            if line_indent <= block_indent && current_block.is_some() && !trimmed.is_empty() {
                current_block = None;
            }

            // If in a plugin block, validate keywords
            if current_block.is_some() && !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("//") {
                // Extract first word as potential keyword
                let first_word = trimmed.split_whitespace().next().unwrap_or("");
                let first_word_clean = first_word.trim_end_matches(':');

                // Skip if it's a known keyword or looks like a value/assignment
                if !first_word.contains('=')
                    && !first_word.starts_with('"')
                    && !first_word.starts_with('\'')
                    && !first_word.chars().next().map(|c| c.is_numeric()).unwrap_or(false)
                {
                    // Check if keyword is valid for this context
                    let valid_keywords: Vec<&str> = registry
                        .keywords_for_context("block")
                        .iter()
                        .map(|k| k.name.as_str())
                        .collect();

                    // If it looks like a keyword but isn't recognized, add a warning
                    if first_word.len() > 1
                        && first_word.chars().all(|c| c.is_alphabetic() || c == '_')
                        && !valid_keywords.iter().any(|k| k.eq_ignore_ascii_case(first_word_clean))
                    {
                        // Could be a field name, check if there's a colon
                        if !trimmed.contains(':') && !trimmed.contains('=') {
                            diagnostics.push(Diagnostic {
                                range: Range {
                                    start: Position {
                                        line: line_num as u32,
                                        character: (line_indent) as u32,
                                    },
                                    end: Position {
                                        line: line_num as u32,
                                        character: (line_indent + first_word.len()) as u32,
                                    },
                                },
                                severity: Some(DiagnosticSeverity::INFORMATION),
                                code: Some(NumberOrString::String("plugin-unknown-keyword".to_string())),
                                source: Some("clean-plugins".to_string()),
                                message: format!(
                                    "Unknown keyword '{}'. Did you mean one of: {}?",
                                    first_word,
                                    valid_keywords.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                                ),
                                related_information: None,
                                tags: None,
                                code_description: None,
                                data: None,
                            });
                        }
                    }
                }
            }
        }

        diagnostics
    }

    /// Validate block structure against plugin definitions
    ///
    /// Checks that blocks are properly formatted and contain valid content.
    pub fn validate_block_structure(
        &self,
        block_name: &str,
        content: &str,
    ) -> Vec<Diagnostic> {
        let registry = match &self.language_registry {
            Some(r) => r,
            None => return Vec::new(),
        };

        let mut diagnostics = Vec::new();

        // Check if block is recognized
        if !registry.has_block(block_name) {
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position { line: 0, character: 0 },
                    end: Position { line: 0, character: block_name.len() as u32 },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("unknown-block".to_string())),
                source: Some("clean-plugins".to_string()),
                message: format!("Block '{}' is not recognized by any loaded plugin", block_name),
                related_information: None,
                tags: None,
                code_description: None,
                data: None,
            });
        }

        // Check for empty block
        if content.trim().is_empty() {
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position { line: 0, character: 0 },
                    end: Position { line: 0, character: (block_name.len() + 1) as u32 },
                },
                severity: Some(DiagnosticSeverity::INFORMATION),
                code: Some(NumberOrString::String("empty-block".to_string())),
                source: Some("clean-plugins".to_string()),
                message: format!("Block '{}' is empty", block_name),
                related_information: None,
                tags: None,
                code_description: None,
                data: None,
            });
        }

        diagnostics
    }

    /// Get all diagnostics for a document, including plugin validation
    pub async fn get_all_diagnostics(
        &self,
        compiler_errors: &[CompilerError],
        text: &str,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.convert_compiler_errors(compiler_errors, text).await;

        // Add plugin keyword validation
        diagnostics.extend(self.validate_plugin_keywords(text));

        diagnostics
    }
}

impl Default for DiagnosticsProvider {
    fn default() -> Self {
        Self::new()
    }
}
