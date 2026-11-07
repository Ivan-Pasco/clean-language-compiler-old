//! Error reporting system with colored terminal output and source code highlighting
//!
//! This module provides comprehensive error reporting with:
//! - Colored terminal output
//! - Source code highlighting
//! - Helpful suggestions and fixes
//! - Multi-error reporting

use super::*;
use std::io::{self, IsTerminal, Write};

/// Color codes for terminal output
#[derive(Debug, Clone)]
pub struct Colors {
    pub reset: &'static str,
    pub bold: &'static str,
    pub red: &'static str,
    pub yellow: &'static str,
    pub green: &'static str,
    pub blue: &'static str,
    pub cyan: &'static str,
    pub magenta: &'static str,
    pub bright_red: &'static str,
    pub bright_yellow: &'static str,
    pub bright_blue: &'static str,
    pub dim: &'static str,
}

impl Colors {
    /// Colors for stderr: only when `use_colors` is set, stderr is a
    /// terminal and NO_COLOR is not set.
    pub fn new(use_colors: bool) -> Self {
        if use_colors && std::env::var_os("NO_COLOR").is_none() && io::stderr().is_terminal() {
            Self {
                reset: "\x1b[0m",
                bold: "\x1b[1m",
                red: "\x1b[31m",
                yellow: "\x1b[33m",
                green: "\x1b[32m",
                blue: "\x1b[34m",
                cyan: "\x1b[36m",
                magenta: "\x1b[35m",
                bright_red: "\x1b[91m",
                bright_yellow: "\x1b[93m",
                bright_blue: "\x1b[94m",
                dim: "\x1b[2m",
            }
        } else {
            Self {
                reset: "",
                bold: "",
                red: "",
                yellow: "",
                green: "",
                blue: "",
                cyan: "",
                magenta: "",
                bright_red: "",
                bright_yellow: "",
                bright_blue: "",
                dim: "",
            }
        }
    }
}

/// How one kind of error is rendered.
struct ErrorKind {
    /// Header used when the error is shown without a code.
    title: &'static str,
    /// Code shown when the error carries none (`None`: show the title).
    default_code: Option<&'static str>,
    /// Show `error[CODE]: message` when a code is available, instead of the
    /// title followed by the message.
    prefer_code: bool,
    /// Show the source lines around the error location.
    show_source: bool,
}

/// Error reporter with comprehensive formatting
pub struct ErrorReporter {
    colors: Colors,
    max_suggestions: usize,
    indent: String,
}

impl ErrorReporter {
    pub fn new(use_colors: bool) -> Self {
        Self {
            colors: Colors::new(use_colors),
            max_suggestions: 5,
            indent: "  ".to_string(),
        }
    }

    /// Report a single error with full formatting
    pub fn report_error(
        &self,
        error: &CompilerError,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error(&mut io::stderr(), error, source_code)
    }

    fn write_error(
        &self,
        writer: &mut dyn Write,
        error: &CompilerError,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        let c = &self.colors;
        let (context, kind, color) = match error {
            CompilerError::Syntax { context } => (
                context,
                ErrorKind {
                    title: "Syntax Error",
                    default_code: Some("SYN001"),
                    prefer_code: true,
                    show_source: true,
                },
                c.bright_red,
            ),
            CompilerError::Type { context } => (
                context,
                ErrorKind {
                    title: "Type Error",
                    default_code: Some("TYP001"),
                    prefer_code: true,
                    show_source: true,
                },
                c.red,
            ),
            CompilerError::Validation { context } => (
                context,
                ErrorKind {
                    title: "Validation Error",
                    default_code: None,
                    prefer_code: true,
                    show_source: true,
                },
                c.yellow,
            ),
            CompilerError::Memory { context } => (
                context,
                ErrorKind {
                    title: "Memory Error",
                    default_code: None,
                    prefer_code: false,
                    show_source: true,
                },
                c.magenta,
            ),
            CompilerError::Codegen { context } => (
                context,
                ErrorKind {
                    title: "Code Generation Error",
                    default_code: None,
                    prefer_code: false,
                    show_source: false,
                },
                c.yellow,
            ),
            CompilerError::IO { context } => (
                context,
                ErrorKind {
                    title: "I/O Error",
                    default_code: None,
                    prefer_code: false,
                    show_source: false,
                },
                c.cyan,
            ),
            CompilerError::Runtime { context } => (
                context,
                ErrorKind {
                    title: "Runtime Error",
                    default_code: None,
                    prefer_code: false,
                    show_source: false,
                },
                c.bright_red,
            ),
            CompilerError::Module { context } => (
                context,
                ErrorKind {
                    title: "Module Error",
                    default_code: None,
                    prefer_code: false,
                    show_source: false,
                },
                c.blue,
            ),
            CompilerError::LexError(lex_error) => {
                return writeln!(writer, "Lexical error: {}", lex_error);
            }
            CompilerError::PluginError { message, location } => {
                writeln!(writer, "Plugin error: {}", message)?;
                if let Some(loc) = location {
                    writeln!(writer, "  at {}:{}:{}", loc.file, loc.line, loc.column)?;
                }
                return Ok(());
            }
        };
        self.write_context(writer, context, source_code, &kind, color)
    }

    /// Report multiple errors with summary
    pub fn report_errors(
        &self,
        errors: &[CompilerError],
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        let mut stderr = io::stderr();

        if errors.is_empty() {
            return Ok(());
        }

        // Report each error
        for (i, error) in errors.iter().enumerate() {
            if i > 0 {
                writeln!(stderr)?; // Separator between errors
            }
            self.write_error(&mut stderr, error, source_code)?;
        }

        // Summary
        writeln!(stderr)?;
        self.write_error_summary(&mut stderr, errors)?;

        Ok(())
    }

    fn write_context(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        source_code: Option<&str>,
        kind: &ErrorKind,
        color: &str,
    ) -> Result<(), io::Error> {
        let code = context.error_code.as_deref().or(kind.default_code);
        match code {
            Some(code) if kind.prefer_code => {
                self.write_error_header_with_code(writer, &context.message, code, color)?;
            }
            _ => {
                self.write_error_header(writer, kind.title, color)?;
                self.write_error_message(writer, &context.message)?;
            }
        }
        self.write_error_location(writer, context)?;

        if kind.show_source {
            self.write_source_context(writer, context, source_code)?;
        }

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        if !context.suggestions.is_empty() {
            self.write_suggestions(writer, &context.suggestions)?;
        }

        Ok(())
    }

    fn write_error_header(
        &self,
        writer: &mut dyn Write,
        error_type: &str,
        color: &str,
    ) -> Result<(), io::Error> {
        writeln!(
            writer,
            "{}{}error{}: {}",
            self.colors.bold, color, self.colors.reset, error_type
        )?;
        Ok(())
    }

    fn write_error_header_with_code(
        &self,
        writer: &mut dyn Write,
        error_type: &str,
        error_code: &str,
        color: &str,
    ) -> Result<(), io::Error> {
        writeln!(
            writer,
            "{}{}error[{}]{}: {}",
            self.colors.bold, color, error_code, self.colors.reset, error_type
        )?;
        Ok(())
    }

    fn write_error_message(&self, writer: &mut dyn Write, message: &str) -> Result<(), io::Error> {
        writeln!(
            writer,
            "{}{}{}",
            self.colors.bold, message, self.colors.reset
        )?;
        Ok(())
    }

    fn write_error_location(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
    ) -> Result<(), io::Error> {
        if let Some(location) = &context.location {
            writeln!(
                writer,
                "{}{}-->{} {}:{}:{}",
                self.indent,
                self.colors.blue,
                self.colors.reset,
                location.file,
                location.line,
                location.column
            )?;
        }
        Ok(())
    }

    fn write_source_context(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        if let (Some(location), Some(source)) = (&context.location, source_code) {
            let lines: Vec<&str> = source.lines().collect();
            let line_idx = location.line.saturating_sub(1);

            if line_idx < lines.len() {
                // Show context lines
                let start = line_idx.saturating_sub(2);
                let end = std::cmp::min(line_idx + 3, lines.len());

                // Calculate line number width for consistent alignment
                let line_num_width = format!("{}", end).len().max(4);

                writeln!(writer, "{}   |{}", self.colors.blue, self.colors.reset)?;

                for (i, line) in lines[start..end].iter().enumerate() {
                    let line_num = start + i + 1;
                    let is_error_line = line_num == location.line;

                    if is_error_line {
                        writeln!(
                            writer,
                            "{}{:>width$} |{} {}",
                            self.colors.blue,
                            line_num,
                            self.colors.reset,
                            line,
                            width = line_num_width
                        )?;

                        // Calculate underline length
                        // If we have span info, use it; otherwise estimate from message or use default
                        let col = location.column.saturating_sub(1);
                        let underline_len = self.calculate_underline_length(line, col, context);

                        // Show error indicator with ^^^^ underline
                        // Keep tabs from the source line so the caret lines up
                        // under tab-indented code.
                        let spaces: String = line
                            .chars()
                            .take(col)
                            .map(|c| if c == '\t' { '\t' } else { ' ' })
                            .collect();
                        let underline = "^".repeat(underline_len);
                        writeln!(
                            writer,
                            "{}{:>width$} |{} {}{}{}{}",
                            self.colors.blue,
                            "",
                            self.colors.reset,
                            spaces,
                            self.colors.bright_red,
                            underline,
                            self.colors.reset,
                            width = line_num_width
                        )?;

                        // Show inline error message
                        if !context.message.is_empty() {
                            // Truncate long messages by characters, not bytes.
                            let inline_msg = super::truncate_chars(&context.message, 50);
                            writeln!(
                                writer,
                                "{}{:>width$} |{} {}{}{}{}",
                                self.colors.blue,
                                "",
                                self.colors.reset,
                                spaces,
                                self.colors.bright_red,
                                inline_msg,
                                self.colors.reset,
                                width = line_num_width
                            )?;
                        }
                    } else {
                        writeln!(
                            writer,
                            "{}{:>width$} |{} {}",
                            self.colors.blue,
                            line_num,
                            self.colors.reset,
                            line,
                            width = line_num_width
                        )?;
                    }
                }

                writeln!(writer, "{}   |{}", self.colors.blue, self.colors.reset)?;
            }
        }
        Ok(())
    }

    /// Calculate the length of the underline based on context.
    ///
    /// `col` is a 0-based *character* column (source locations count
    /// characters, not bytes), so all indexing here goes through `chars()`.
    fn calculate_underline_length(&self, line: &str, col: usize, context: &ErrorContext) -> usize {
        let chars: Vec<char> = line.chars().collect();
        if col >= chars.len() {
            return 1;
        }
        let remaining: String = chars[col..].iter().collect();

        // If the message names an identifier that sits at the column, underline it.
        if let Some(identifier) = self.extract_identifier_from_message(&context.message) {
            if remaining.starts_with(&identifier) {
                return identifier.chars().count().max(1);
            }
        }

        // Otherwise extend to the end of the token at the column position.
        let first_char = chars[col];
        if first_char.is_alphanumeric() || first_char == '_' {
            return chars[col..]
                .iter()
                .take_while(|c| c.is_alphanumeric() || **c == '_')
                .count();
        }
        if !first_char.is_whitespace() {
            // Operator or symbol - take 1-2 characters
            if let Some(next) = chars.get(col + 1) {
                if !next.is_alphanumeric() && !next.is_whitespace() {
                    return 2;
                }
            }
            return 1;
        }

        // Default: single caret
        1
    }

    /// Extract identifier from error message if present
    fn extract_identifier_from_message(&self, message: &str) -> Option<String> {
        // Look for patterns like "identifier 'foo'" or "'foo'" or "\"foo\""
        let patterns = [
            (": \"", "\""),        // : "foo"
            (": '", "'"),          // : 'foo'
            ("identifier '", "'"), // identifier 'foo'
            ("'", "'"),            // 'foo'
            ("\"", "\""),          // "foo"
        ];

        for (start, end) in patterns {
            if let Some(start_idx) = message.find(start) {
                let after_start = &message[start_idx + start.len()..];
                if let Some(end_idx) = after_start.find(end) {
                    let identifier = &after_start[..end_idx];
                    if !identifier.is_empty() && identifier.len() < 50 {
                        return Some(identifier.to_string());
                    }
                }
            }
        }

        None
    }

    fn write_help_message(&self, writer: &mut dyn Write, help: &str) -> Result<(), io::Error> {
        writeln!(
            writer,
            "{}{}help:{} {}",
            self.indent, self.colors.green, self.colors.reset, help
        )?;
        Ok(())
    }

    fn write_suggestions(
        &self,
        writer: &mut dyn Write,
        suggestions: &[String],
    ) -> Result<(), io::Error> {
        let limit = std::cmp::min(suggestions.len(), self.max_suggestions);

        if limit > 0 {
            writeln!(
                writer,
                "{}{}suggestions:{}",
                self.indent, self.colors.yellow, self.colors.reset
            )?;

            for suggestion in suggestions.iter().take(limit) {
                writeln!(
                    writer,
                    "{}  {} {}",
                    self.indent, self.colors.yellow, suggestion
                )?;
            }

            if suggestions.len() > limit {
                writeln!(
                    writer,
                    "{}  {} ... and {} more",
                    self.indent,
                    self.colors.dim,
                    suggestions.len() - limit
                )?;
            }
        }
        Ok(())
    }

    fn write_error_summary(
        &self,
        writer: &mut dyn Write,
        errors: &[CompilerError],
    ) -> Result<(), io::Error> {
        let error_count = errors.len();
        let mut type_counts = std::collections::HashMap::new();

        for error in errors {
            let error_type = match error {
                CompilerError::Syntax { .. } => "syntax",
                CompilerError::Type { .. } => "type",
                CompilerError::Memory { .. } => "memory",
                CompilerError::Codegen { .. } => "codegen",
                CompilerError::IO { .. } => "io",
                CompilerError::Runtime { .. } => "runtime",
                CompilerError::Validation { .. } => "validation",
                CompilerError::Module { .. } => "module",
                CompilerError::LexError(_) => "lexical",
                CompilerError::PluginError { .. } => "plugin",
            };

            *type_counts.entry(error_type).or_insert(0) += 1;
        }

        writeln!(
            writer,
            "{}{}Summary:{} {} error{} found",
            self.colors.bold,
            self.colors.red,
            self.colors.reset,
            error_count,
            if error_count == 1 { "" } else { "s" }
        )?;

        for (error_type, count) in type_counts {
            writeln!(writer, "{}  {}: {}", self.indent, error_type, count)?;
        }

        Ok(())
    }
}

impl Default for ErrorReporter {
    fn default() -> Self {
        Self::new(true) // Use colors by default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::SourceLocation;

    fn context_at(message: &str, line: usize, column: usize) -> ErrorContext {
        let mut ctx = ErrorContext::new(message, None, ErrorType::Syntax, None);
        ctx.location = Some(SourceLocation {
            line,
            column,
            file: "test.cln".to_string(),
            byte_start: None,
            byte_end: None,
        });
        ctx
    }

    #[test]
    fn underline_uses_character_columns_after_multibyte_text() {
        // The `bad` identifier starts at character column 19 (0-based) but at
        // byte offset 21, so treating the column as a byte index sliced into
        // the middle of the line and could land inside a multi-byte `é`.
        let line = "\tstring s = \"éé\" + bad";
        let reporter = ErrorReporter::new(false);
        let ctx = context_at("Undefined variable 'bad'", 1, 20);
        assert_eq!(reporter.calculate_underline_length(line, 19, &ctx), 3);
        for col in 0..line.chars().count() + 2 {
            reporter.calculate_underline_length(line, col, &ctx);
        }
    }

    #[test]
    fn source_context_renders_multibyte_lines() {
        let source = "start:\n\tprint(\"ñandú — ok\" + missing)\n";
        let ctx = context_at("Undefined variable 'missing'", 2, 23);
        let reporter = ErrorReporter::new(false);
        let mut out = Vec::new();
        reporter
            .write_source_context(&mut out, &ctx, Some(source))
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("^^^^^^^"), "caret line missing: {text}");
        // The caret prefix keeps the tab so it lines up under the code.
        assert!(text.contains("| \t"), "tab not preserved: {text:?}");
    }
}
