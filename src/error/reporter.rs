//! Error reporting system with colored terminal output and source code highlighting
//!
//! This module provides comprehensive error reporting with:
//! - Colored terminal output
//! - Source code highlighting
//! - Helpful suggestions and fixes
//! - Multi-error reporting

use super::*;
use std::io::{self, Write};

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
    pub fn new(use_colors: bool) -> Self {
        if use_colors && atty::is(atty::Stream::Stderr) {
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

/// Error reporter with comprehensive formatting
pub struct ErrorReporter {
    colors: Colors,
    show_source: bool,
    show_suggestions: bool,
    max_suggestions: usize,
    indent: String,
}

impl ErrorReporter {
    pub fn new(use_colors: bool) -> Self {
        Self {
            colors: Colors::new(use_colors),
            show_source: true,
            show_suggestions: true,
            max_suggestions: 5,
            indent: "  ".to_string(),
        }
    }

    /// Configure reporter options
    pub fn configure(&mut self, show_source: bool, show_suggestions: bool, max_suggestions: usize) {
        self.show_source = show_source;
        self.show_suggestions = show_suggestions;
        self.max_suggestions = max_suggestions;
    }

    /// Report a single error with full formatting
    pub fn report_error(
        &self,
        error: &CompilerError,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        let mut stderr = io::stderr();

        match error {
            CompilerError::Syntax { context } => {
                self.report_syntax_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Type { context } => {
                self.report_type_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Memory { context } => {
                self.report_memory_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Codegen { context } => {
                self.report_codegen_error(&mut stderr, context, source_code)?;
            }
            CompilerError::IO { context } => {
                self.report_io_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Runtime { context } => {
                self.report_runtime_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Validation { context } => {
                self.report_validation_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Module { context } => {
                self.report_module_error(&mut stderr, context, source_code)?;
            }
            CompilerError::Testing { context } => {
                self.report_validation_error(&mut stderr, context, source_code)?;
            }
            CompilerError::LexError(lex_error) => {
                // Handle lexical errors
                writeln!(stderr, "Lexical error: {}", lex_error)?;
            }
            CompilerError::PluginError { message, location } => {
                // Handle plugin errors
                writeln!(stderr, "Plugin error: {}", message)?;
                if let Some(loc) = location {
                    writeln!(stderr, "  at {}:{}:{}", loc.file, loc.line, loc.column)?;
                }
            }
        }

        Ok(())
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
            self.report_error(error, source_code)?;
        }

        // Summary
        writeln!(stderr)?;
        self.write_error_summary(&mut stderr, errors)?;

        Ok(())
    }

    fn report_syntax_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        // Use error code if available, otherwise use type-based code
        let error_code = context.error_code.as_deref().unwrap_or("SYN001");
        self.write_error_header_with_code(
            writer,
            &context.message,
            error_code,
            self.colors.bright_red,
        )?;
        self.write_error_location(writer, context)?;

        if self.show_source {
            self.write_source_context(writer, context, source_code)?;
        }

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        if self.show_suggestions && !context.suggestions.is_empty() {
            self.write_suggestions(writer, &context.suggestions)?;
        }

        Ok(())
    }

    fn report_type_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        let error_code = context.error_code.as_deref().unwrap_or("TYP001");
        self.write_error_header_with_code(writer, &context.message, error_code, self.colors.red)?;
        self.write_error_location(writer, context)?;

        if self.show_source {
            self.write_source_context(writer, context, source_code)?;
        }

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        if self.show_suggestions && !context.suggestions.is_empty() {
            self.write_suggestions(writer, &context.suggestions)?;
        }

        Ok(())
    }

    fn report_memory_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error_header(writer, "Memory Error", self.colors.magenta)?;
        self.write_error_message(writer, &context.message)?;
        self.write_error_location(writer, context)?;

        if self.show_source {
            self.write_source_context(writer, context, source_code)?;
        }

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        Ok(())
    }

    fn report_codegen_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        _source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error_header(writer, "Code Generation Error", self.colors.yellow)?;
        self.write_error_message(writer, &context.message)?;
        self.write_error_location(writer, context)?;

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        Ok(())
    }

    fn report_io_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        _source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error_header(writer, "I/O Error", self.colors.cyan)?;
        self.write_error_message(writer, &context.message)?;
        self.write_error_location(writer, context)?;

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        Ok(())
    }

    fn report_runtime_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        _source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error_header(writer, "Runtime Error", self.colors.bright_red)?;
        self.write_error_message(writer, &context.message)?;
        self.write_error_location(writer, context)?;

        // Show stack trace for runtime errors
        if !context.stack_trace.is_empty() {
            self.write_stack_trace(writer, &context.stack_trace)?;
        }

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        Ok(())
    }

    fn report_validation_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error_header(writer, "Validation Error", self.colors.yellow)?;
        self.write_error_message(writer, &context.message)?;
        self.write_error_location(writer, context)?;

        if self.show_source {
            self.write_source_context(writer, context, source_code)?;
        }

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
        }

        Ok(())
    }

    fn report_module_error(
        &self,
        writer: &mut dyn Write,
        context: &ErrorContext,
        _source_code: Option<&str>,
    ) -> Result<(), io::Error> {
        self.write_error_header(writer, "Module Error", self.colors.blue)?;
        self.write_error_message(writer, &context.message)?;
        self.write_error_location(writer, context)?;

        if let Some(help) = &context.help {
            self.write_help_message(writer, help)?;
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
                &location.file,
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
                        let spaces = " ".repeat(col);
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
                            // Truncate message if too long
                            let inline_msg = if context.message.len() > 50 {
                                format!("{}...", &context.message[..47])
                            } else {
                                context.message.clone()
                            };
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

    /// Calculate the length of the underline based on context
    fn calculate_underline_length(&self, line: &str, col: usize, context: &ErrorContext) -> usize {
        // Try to extract a reasonable span from the error context
        // Look for identifiers, keywords, or tokens at the error position

        // If message mentions a specific identifier, try to find it
        if let Some(identifier) = self.extract_identifier_from_message(&context.message) {
            if col < line.len() {
                // Check if the identifier is at the column position
                let remaining = &line[col..];
                if remaining.starts_with(&identifier) {
                    return identifier.len().max(1);
                }
            }
        }

        // Try to find the extent of the current token at column position
        if col < line.len() {
            let chars: Vec<char> = line.chars().collect();
            let mut len = 0;

            // Skip to the character at col
            let mut char_idx = 0;
            let mut byte_idx = 0;
            for (i, c) in chars.iter().enumerate() {
                if byte_idx >= col {
                    char_idx = i;
                    break;
                }
                byte_idx += c.len_utf8();
            }

            // Extend to the end of the current token (identifier, keyword, or symbol)
            let first_char = chars.get(char_idx).copied().unwrap_or(' ');
            if first_char.is_alphanumeric() || first_char == '_' {
                // Identifier or keyword
                for c in chars[char_idx..].iter() {
                    if c.is_alphanumeric() || *c == '_' {
                        len += 1;
                    } else {
                        break;
                    }
                }
            } else if !first_char.is_whitespace() {
                // Operator or symbol - take 1-2 characters
                len = 1;
                if char_idx + 1 < chars.len()
                    && !chars[char_idx + 1].is_alphanumeric()
                    && !chars[char_idx + 1].is_whitespace()
                {
                    len = 2;
                }
            }

            if len > 0 {
                return len;
            }
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

    fn write_stack_trace(
        &self,
        writer: &mut dyn Write,
        stack_trace: &[StackFrame],
    ) -> Result<(), io::Error> {
        writeln!(
            writer,
            "{}{}stack trace:{}",
            self.indent, self.colors.cyan, self.colors.reset
        )?;

        for frame in stack_trace {
            if let Some(location) = &frame.location {
                writeln!(
                    writer,
                    "{}  {} in {} at {}:{}",
                    self.indent,
                    self.colors.cyan,
                    frame.function_name,
                    &location.file,
                    location.line
                )?;
            } else {
                writeln!(
                    writer,
                    "{}  {} in {}",
                    self.indent, self.colors.cyan, frame.function_name
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
                CompilerError::Testing { .. } => "testing",
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

/// Simple function to detect if terminal supports colors
mod atty {
    #[cfg(unix)]
    pub fn is(stream: Stream) -> bool {
        use std::os::unix::io::AsRawFd;
        match stream {
            Stream::Stderr => unsafe { libc::isatty(std::io::stderr().as_raw_fd()) != 0 },
        }
    }

    #[cfg(not(unix))]
    pub fn is(_stream: Stream) -> bool {
        // Conservative approach for non-Unix systems
        std::env::var("NO_COLOR").is_err()
    }

    pub enum Stream {
        Stderr,
    }
}
