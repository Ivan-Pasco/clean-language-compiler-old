use std::error::Error;
use std::fmt;

use crate::ast::SourceLocation;

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub location: Option<SourceLocation>,
}

impl StackFrame {
    pub fn new<T: Into<String>>(function_name: T, location: Option<SourceLocation>) -> Self {
        Self {
            function_name: function_name.into(),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub message: String,
    pub help: Option<String>,
    pub error_type: ErrorType,
    pub location: Option<SourceLocation>,
    pub suggestions: Vec<String>,
    pub source_snippet: Option<String>,
    pub stack_trace: Vec<StackFrame>,
    pub severity: ErrorSeverity,
    pub error_code: Option<String>,
    pub related_errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorType {
    Syntax,
    Type,
    Memory,
    Codegen,
    IO,
    Runtime,
    Validation,
    Module,
    Import,
    Export,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WarningType {
    UnusedVariable,
    UnusedFunction,
    UnusedImport,
    DeadCode,
    TypeInference,
    Performance,
    Style,
    Deprecation,
    Shadowing,
    UnreachableCode,
}

impl ErrorContext {
    pub fn new<T: Into<String>>(
        message: T,
        help: Option<String>,
        error_type: ErrorType,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            message: message.into(),
            help,
            error_type,
            location,
            suggestions: Vec::new(),
            source_snippet: None,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: None,
            related_errors: Vec::new(),
        }
    }

    pub fn with_help<T: Into<String>>(mut self, help: T) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_location(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub fn with_location_option(mut self, location: Option<SourceLocation>) -> Self {
        if let Some(loc) = location {
            self.location = Some(loc);
        }
        self
    }

    pub fn with_help_option(mut self, help: Option<String>) -> Self {
        if let Some(h) = help {
            self.help = Some(h);
        }
        self
    }

    pub fn with_suggestion<T: Into<String>>(mut self, suggestion: T) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions.extend(suggestions);
        self
    }

    pub fn with_source_snippet<T: Into<String>>(mut self, snippet: T) -> Self {
        self.source_snippet = Some(snippet.into());
        self
    }

    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_error_code<T: Into<String>>(mut self, code: T) -> Self {
        self.error_code = Some(code.into());
        self
    }

    pub fn with_related_error<T: Into<String>>(mut self, related: T) -> Self {
        self.related_errors.push(related.into());
        self
    }

    pub fn add_stack_frame(&mut self, frame: StackFrame) {
        self.stack_trace.push(frame);
    }

    pub fn with_stack_frame(mut self, frame: StackFrame) -> Self {
        self.stack_trace.push(frame);
        self
    }

    /// Create an enhanced syntax error with context and suggestions
    pub fn enhanced_syntax_error<T: Into<String>>(
        message: T,
        location: Option<SourceLocation>,
        source_snippet: Option<String>,
        suggestions: Vec<String>,
    ) -> Self {
        Self {
            message: message.into(),
            help: None,
            error_type: ErrorType::Syntax,
            location,
            suggestions,
            source_snippet,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: Some("E001".to_string()),
            related_errors: Vec::new(),
        }
    }

    /// Create an enhanced type error with detailed type information
    pub fn enhanced_type_error<T: Into<String>>(
        message: T,
        expected_type: Option<String>,
        actual_type: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        let mut help_text = String::new();
        if let (Some(expected), Some(actual)) = (expected_type, actual_type) {
            help_text = format!("Expected type '{expected}', but found '{actual}'");
        }

        Self {
            message: message.into(),
            help: if help_text.is_empty() {
                None
            } else {
                Some(help_text)
            },
            error_type: ErrorType::Type,
            location,
            suggestions: Vec::new(),
            source_snippet: None,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: Some("E002".to_string()),
            related_errors: Vec::new(),
        }
    }

    /// Create a method suggestion error for when traditional function syntax is used
    pub fn method_suggestion_error<T: Into<String>>(
        function_name: T,
        location: Option<SourceLocation>,
        source_snippet: Option<String>,
    ) -> Self {
        let func_name = function_name.into();
        let message = format!("Function '{func_name}' is only available as a method");
        let suggestion = match func_name.as_str() {
            "length" => "Use 'object.length()' instead of 'length(object)'".to_string(),
            "isEmpty" => "Use 'object.isEmpty()' instead of 'isEmpty(object)'".to_string(),
            "isNotEmpty" => "Use 'object.isNotEmpty()' instead of 'isNotEmpty(object)'".to_string(),
            "isDefined" => "Use 'object.isDefined()' instead of 'isDefined(object)'".to_string(),
            "isNotDefined" => {
                "Use 'object.isNotDefined()' instead of 'isNotDefined(object)'".to_string()
            }
            "keepBetween" => {
                "Use 'value.keepBetween(min, max)' instead of 'keepBetween(value, min, max)'"
                    .to_string()
            }
            _ => format!("Use 'object.{func_name}()' method syntax instead"),
        };

        Self {
            message,
            help: Some("Clean Language uses method-style syntax for object operations".to_string()),
            error_type: ErrorType::Syntax,
            location,
            suggestions: vec![suggestion],
            source_snippet,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: Some("E003".to_string()),
            related_errors: Vec::new(),
        }
    }

    /// Create an indentation error with helpful suggestions
    pub fn indentation_error<T: Into<String>>(
        message: T,
        location: Option<SourceLocation>,
        expected_indent: usize,
        actual_indent: usize,
    ) -> Self {
        let help = format!(
            "Expected {expected_indent} spaces of indentation, but found {actual_indent}. Clean Language uses consistent indentation to define code blocks."
        );

        Self {
            message: message.into(),
            help: Some(help),
            error_type: ErrorType::Syntax,
            location,
            suggestions: vec![
                "Use tabs or consistent spaces for indentation".to_string(),
                "Make sure all statements in a block have the same indentation level".to_string(),
            ],
            source_snippet: None,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: Some("E004".to_string()),
            related_errors: Vec::new(),
        }
    }

    /// Create a missing block error with indentation help
    pub fn missing_block_error<T: Into<String>>(
        block_type: T,
        location: Option<SourceLocation>,
    ) -> Self {
        let block = block_type.into();
        Self {
            message: format!("Expected {block} block"),
            help: Some(
                "Clean Language uses indentation to define code blocks. Use tabs for indentation"
                    .to_string(),
            ),
            error_type: ErrorType::Syntax,
            location,
            suggestions: vec![
                format!("Add an indented block after the {block} declaration"),
                "Make sure to use consistent indentation (tabs or spaces)".to_string(),
            ],
            source_snippet: None,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: Some("E005".to_string()),
            related_errors: Vec::new(),
        }
    }
}

impl From<ErrorContext> for String {
    fn from(error: ErrorContext) -> String {
        let mut result = String::new();

        // Error header with severity and code
        let severity_str = match error.severity {
            ErrorSeverity::Error => "Error",
            ErrorSeverity::Warning => "Warning",
            ErrorSeverity::Info => "Info",
            ErrorSeverity::Hint => "Hint",
        };

        if let Some(code) = &error.error_code {
            result.push_str(&format!("{} [{}]: {}\n", severity_str, code, error.message));
        } else {
            result.push_str(&format!("{}: {}\n", severity_str, error.message));
        }

        // Location information
        if let Some(location) = &error.location {
            result.push_str(&format!(
                "  --> {}:{}:{}\n",
                location.file, location.line, location.column
            ));
        }

        // Source snippet with highlighting
        if let Some(snippet) = &error.source_snippet {
            result.push_str("   |\n");
            for (i, line) in snippet.lines().enumerate() {
                result.push_str(&format!("{:3} | {}\n", i + 1, line));
            }
            result.push_str("   |\n");
        }

        // Help text
        if let Some(help) = &error.help {
            result.push_str(&format!("  = help: {help}\n"));
        }

        // Suggestions
        if !error.suggestions.is_empty() {
            result.push_str("  = suggestions:\n");
            for suggestion in &error.suggestions {
                result.push_str(&format!("    - {suggestion}\n"));
            }
        }

        // Related errors
        if !error.related_errors.is_empty() {
            result.push_str("  = related:\n");
            for related in &error.related_errors {
                result.push_str(&format!("    - {related}\n"));
            }
        }

        result
    }
}

impl From<String> for ErrorContext {
    fn from(message: String) -> Self {
        Self::new(message, None, ErrorType::Syntax, None)
    }
}

impl From<&str> for ErrorContext {
    fn from(message: &str) -> Self {
        Self::new(message.to_string(), None, ErrorType::Syntax, None)
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted: String = self.clone().into();
        write!(f, "{formatted}")
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self {
            message: String::new(),
            help: None,
            error_type: ErrorType::Syntax,
            location: None,
            suggestions: Vec::new(),
            source_snippet: None,
            stack_trace: Vec::new(),
            severity: ErrorSeverity::Error,
            error_code: None,
            related_errors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CompilerError {
    Syntax { context: Box<ErrorContext> },
    Type { context: Box<ErrorContext> },
    Memory { context: Box<ErrorContext> },
    Codegen { context: Box<ErrorContext> },
    IO { context: Box<ErrorContext> },
    Runtime { context: Box<ErrorContext> },
    Validation { context: Box<ErrorContext> },
    Module { context: Box<ErrorContext> },
}

impl CompilerError {
    pub fn syntax_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Syntax, location)
                    .with_error_code("E001"),
            ),
        }
    }

    pub fn type_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Type {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Type, location).with_error_code("E002"),
            ),
        }
    }

    pub fn memory_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Memory {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Memory, location)
                    .with_error_code("E006"),
            ),
        }
    }

    pub fn codegen_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Codegen {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Codegen, location)
                    .with_error_code("E007"),
            ),
        }
    }

    pub fn io_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::IO {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::IO, location).with_error_code("E008"),
            ),
        }
    }

    pub fn runtime_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Runtime {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Runtime, location)
                    .with_error_code("E009"),
            ),
        }
    }

    pub fn validation_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Validation {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Validation, location)
                    .with_error_code("E010"),
            ),
        }
    }

    pub fn parse_error<T: Into<String>>(
        message: T,
        location: Option<SourceLocation>,
        help: Option<String>,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Syntax, location)
                    .with_error_code("E001"),
            ),
        }
    }

    /// Enhanced syntax error with source snippet and suggestions
    pub fn enhanced_syntax_error<T: Into<String>>(
        message: T,
        location: Option<SourceLocation>,
        source_snippet: Option<String>,
        suggestions: Vec<String>,
        help: Option<String>,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(
                ErrorContext::enhanced_syntax_error(message, location, source_snippet, suggestions)
                    .with_help_option(help),
            ),
        }
    }

    /// Enhanced type error with detailed type information
    pub fn enhanced_type_error<T: Into<String>>(
        message: T,
        expected_type: Option<String>,
        actual_type: Option<String>,
        location: Option<SourceLocation>,
        suggestions: Vec<String>,
    ) -> Self {
        CompilerError::Type {
            context: Box::new(
                ErrorContext::enhanced_type_error(message, expected_type, actual_type, location)
                    .with_suggestions(suggestions),
            ),
        }
    }

    /// Parse error with suggestions and source snippet
    pub fn parse_error_with_suggestions<T: Into<String>>(
        message: T,
        location: Option<SourceLocation>,
        suggestions: Vec<String>,
        source_snippet: Option<String>,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(ErrorContext::enhanced_syntax_error(
                message,
                location,
                source_snippet,
                suggestions,
            )),
        }
    }

    /// Unexpected token error with helpful suggestions
    pub fn unexpected_token_error(
        found: &str,
        expected: Vec<&str>,
        location: Option<SourceLocation>,
        source_snippet: Option<String>,
    ) -> Self {
        let message = if expected.len() == 1 {
            format!("Expected {}, found '{}'", expected[0], found)
        } else if expected.len() == 2 {
            format!(
                "Expected {} or {}, found '{}'",
                expected[0], expected[1], found
            )
        } else {
            format!(
                "Expected one of: {}, found '{}'",
                expected.join(", "),
                found
            )
        };

        let suggestions = vec![
            format!("Replace '{found}' with one of the expected tokens"),
            "Check the Clean Language syntax guide for proper formatting".to_string(),
        ];

        CompilerError::Syntax {
            context: Box::new(
                ErrorContext::enhanced_syntax_error(message, location, source_snippet, suggestions)
                    .with_help("Refer to the Clean Language syntax guide for proper formatting"),
            ),
        }
    }

    /// Missing element error (like missing function name, missing block, etc.)
    pub fn missing_element_error<T: Into<String>>(
        element_type: T,
        location: Option<SourceLocation>,
        suggestions: Vec<String>,
    ) -> Self {
        let element = element_type.into();
        let message = format!("Missing {element}");
        let help = match element.as_str() {
            "function name" => {
                "Function declarations require a valid function name after the 'function' keyword"
            }
            "indented block" => {
                "Clean Language uses indentation to define code blocks. Use tabs for indentation"
            }
            _ => "Check the syntax requirements for this element",
        };

        CompilerError::Syntax {
            context: Box::new(
                ErrorContext::enhanced_syntax_error(message, location, None, suggestions)
                    .with_help(help)
                    .with_error_code("E005"),
            ),
        }
    }

    /// Method suggestion error for traditional function syntax
    pub fn method_suggestion_error<T: Into<String>>(
        function_name: T,
        location: Option<SourceLocation>,
        source_snippet: Option<String>,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(ErrorContext::method_suggestion_error(
                function_name,
                location,
                source_snippet,
            )),
        }
    }

    /// Function not found error with intelligent suggestions
    pub fn function_not_found_error<T: Into<String>>(
        name: T,
        available_functions: &[&str],
        location: SourceLocation,
    ) -> Self {
        let func_name = name.into();

        // Check if this is a method-style function
        let is_method_function = matches!(
            func_name.as_str(),
            "length" | "isEmpty" | "isNotEmpty" | "isDefined" | "isNotDefined" | "keepBetween"
        );

        if is_method_function {
            return Self::method_suggestion_error(func_name, Some(location), None);
        }

        // Find similar function names
        let suggestions = ErrorUtils::suggest_similar_names(&func_name, available_functions, 3);
        let mut error_suggestions = vec![format!(
            "Check if '{func_name}' is defined and spelled correctly"
        )];

        if !suggestions.is_empty() {
            error_suggestions.push(format!("Did you mean: {}?", suggestions.join(", ")));
        }

        CompilerError::Type {
            context: Box::new(
                ErrorContext::new(
                    format!("Function '{func_name}' not found"),
                    Some(
                        "Check if the function name is correct and the function is defined"
                            .to_string(),
                    ),
                    ErrorType::Type,
                    Some(location),
                )
                .with_suggestions(error_suggestions)
                .with_error_code("E011"),
            ),
        }
    }

    /// Variable not found error with suggestions
    pub fn variable_not_found_error<T: Into<String>>(
        name: T,
        available_variables: &[&str],
        location: SourceLocation,
    ) -> Self {
        let var_name = name.into();
        let suggestions = ErrorUtils::suggest_similar_names(&var_name, available_variables, 3);
        let mut error_suggestions = vec![format!(
            "Check if '{var_name}' is declared and spelled correctly"
        )];

        if !suggestions.is_empty() {
            error_suggestions.push(format!("Did you mean: {}?", suggestions.join(", ")));
        }

        CompilerError::Type {
            context: Box::new(
                ErrorContext::new(
                    format!("Variable '{var_name}' not found"),
                    Some("Variables must be declared before use".to_string()),
                    ErrorType::Type,
                    Some(location),
                )
                .with_suggestions(error_suggestions)
                .with_error_code("E012"),
            ),
        }
    }

    /// Indentation error with helpful guidance
    pub fn indentation_error<T: Into<String>>(
        message: T,
        location: Option<SourceLocation>,
        expected_indent: usize,
        actual_indent: usize,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(ErrorContext::indentation_error(
                message,
                location,
                expected_indent,
                actual_indent,
            )),
        }
    }

    /// Missing block error (like missing function body)
    pub fn missing_block_error<T: Into<String>>(
        block_type: T,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Syntax {
            context: Box::new(ErrorContext::missing_block_error(block_type, location)),
        }
    }

    /// Module-related error
    pub fn module_error<T: Into<String>>(
        message: T,
        help: Option<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        CompilerError::Module {
            context: Box::new(
                ErrorContext::new(message, help, ErrorType::Module, location)
                    .with_error_code("E013"),
            ),
        }
    }

    /// Import resolution error
    pub fn import_error<T: Into<String>>(
        message: T,
        import_name: &str,
        location: Option<SourceLocation>,
    ) -> Self {
        let detailed_message = format!("Import '{import_name}': {}", message.into());
        let help = Some(format!(
            "Check if the module '{import_name}' exists and is accessible"
        ));
        CompilerError::Module {
            context: Box::new(
                ErrorContext::new(detailed_message, help, ErrorType::Import, location)
                    .with_error_code("E014"),
            ),
        }
    }

    /// Symbol resolution error
    pub fn symbol_error<T: Into<String>>(
        message: T,
        symbol_name: &str,
        module_name: Option<&str>,
    ) -> Self {
        let detailed_message = match module_name {
            Some(module) => format!(
                "Symbol '{}' in module '{}': {}",
                symbol_name,
                module,
                message.into()
            ),
            None => format!("Symbol '{symbol_name}': {}", message.into()),
        };
        let help = Some(format!(
            "Check if the symbol '{symbol_name}' is properly exported and accessible"
        ));
        CompilerError::Module {
            context: Box::new(
                ErrorContext::new(detailed_message, help, ErrorType::Module, None)
                    .with_error_code("E015"),
            ),
        }
    }

    /// Memory allocation error
    pub fn memory_allocation_error(
        message: impl AsRef<str>,
        size_requested: usize,
        available_memory: Option<usize>,
        location: Option<SourceLocation>,
    ) -> Self {
        let mut full_message = format!(
            "{}\nRequested allocation size: {} bytes",
            message.as_ref(),
            size_requested
        );

        if let Some(available) = available_memory {
            full_message.push_str(&format!("\nAvailable memory: {available} bytes"));
        }

        let help = Some(
            "Consider reducing the size of data structures or optimizing memory usage. \
            Large allocations may require increasing the memory limit."
                .to_string(),
        );

        CompilerError::Memory {
            context: Box::new(
                ErrorContext::new(full_message, help, ErrorType::Memory, location)
                    .with_error_code("E016"),
            ),
        }
    }

    /// Detailed type error with debug information
    pub fn detailed_type_error(
        message: impl AsRef<str>,
        expected_type: impl std::fmt::Debug,
        actual_type: impl std::fmt::Debug,
        location: Option<SourceLocation>,
        help: Option<String>,
    ) -> Self {
        let full_message = format!(
            "{}\nExpected type: {:?}\nActual type: {:?}",
            message.as_ref(),
            expected_type,
            actual_type
        );

        CompilerError::Type {
            context: Box::new(
                ErrorContext::new(full_message, help, ErrorType::Type, location)
                    .with_error_code("E017"),
            ),
        }
    }

    /// Division by zero error
    pub fn division_by_zero_error(location: Option<SourceLocation>) -> Self {
        CompilerError::Runtime {
            context: Box::new(
                ErrorContext::new(
                    "Division by zero",
                    Some("Check divisor values to ensure they are not zero.".to_string()),
                    ErrorType::Runtime,
                    location,
                )
                .with_error_code("E018"),
            ),
        }
    }

    /// Create a serialization error
    pub fn serialization_error<T: Into<String>>(message: T) -> Self {
        CompilerError::io_error(
            format!("Serialization error: {}", message.into()),
            Some("Check data format and serialization process".to_string()),
            None,
        )
    }
}

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_len = s1.chars().count();
    let s2_len = s2.chars().count();

    if s1_len == 0 {
        return s2_len;
    }
    if s2_len == 0 {
        return s1_len;
    }

    let mut matrix = vec![vec![0; s2_len + 1]; s1_len + 1];

    for (i, row) in matrix.iter_mut().enumerate().take(s1_len + 1) {
        row[0] = i;
    }

    for j in 0..=s2_len {
        matrix[0][j] = j;
    }

    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();

    for i in 1..=s1_len {
        for j in 1..=s2_len {
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                0
            } else {
                1
            };

            matrix[i][j] = std::cmp::min(
                matrix[i - 1][j] + 1,
                std::cmp::min(matrix[i][j - 1] + 1, matrix[i - 1][j - 1] + cost),
            );
        }
    }

    matrix[s1_len][s2_len]
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let context = match self {
            CompilerError::Syntax { context } => {
                write!(f, "Syntax error: {}", context.message)?;
                context
            }
            CompilerError::Type { context } => {
                write!(f, "Type error: {}", context.message)?;
                context
            }
            CompilerError::Memory { context } => {
                write!(f, "Memory error: {}", context.message)?;
                context
            }
            CompilerError::Codegen { context } => {
                write!(f, "Code generation error: {}", context.message)?;
                context
            }
            CompilerError::IO { context } => {
                write!(f, "IO error: {}", context.message)?;
                context
            }
            CompilerError::Runtime { context } => {
                write!(f, "Runtime error: {}", context.message)?;
                context
            }
            CompilerError::Validation { context } => {
                write!(f, "Validation error: {}", context.message)?;
                context
            }
            CompilerError::Module { context } => {
                write!(f, "Module error: {}", context.message)?;
                context
            }
        };

        if let Some(location) = &context.location {
            if !location.file.is_empty() && location.file != "<unknown>" {
                write!(
                    f,
                    "\n  at {}:{}:{}",
                    location.file, location.line, location.column
                )?;
            }
        }

        if let Some(help) = &context.help {
            write!(f, "\n  Help: {help}")?;
        }

        Ok(())
    }
}

impl Error for CompilerError {}

impl From<wasmtime::MemoryAccessError> for CompilerError {
    fn from(error: wasmtime::MemoryAccessError) -> Self {
        CompilerError::memory_error(
            format!("Memory access error: {error}"),
            Some("Check memory bounds and access patterns".to_string()),
            None,
        )
    }
}

pub type CompilerResult<T> = Result<T, CompilerError>;

#[derive(Debug, Clone)]
pub struct CompilerWarning {
    pub message: String,
    pub warning_type: WarningType,
    pub location: Option<SourceLocation>,
    pub help: Option<String>,
    pub suggestion: Option<String>,
}

impl CompilerWarning {
    pub fn new<T: Into<String>>(
        message: T,
        warning_type: WarningType,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            message: message.into(),
            warning_type,
            location,
            help: None,
            suggestion: None,
        }
    }

    pub fn with_help<T: Into<String>>(mut self, help: T) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_suggestion<T: Into<String>>(mut self, suggestion: T) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn unused_variable(name: &str, location: Option<SourceLocation>) -> Self {
        Self::new(
            format!("Variable '{name}' is declared but never used"),
            WarningType::UnusedVariable,
            location,
        )
        .with_help("Consider removing the variable or using it in your code")
        .with_suggestion(format!("Remove 'let {name}' or use the variable"))
    }

    pub fn unused_function(name: &str, location: Option<SourceLocation>) -> Self {
        Self::new(
            format!("Function '{name}' is defined but never called"),
            WarningType::UnusedFunction,
            location,
        )
        .with_help("Consider removing the function or calling it")
        .with_suggestion(format!("Remove function '{name}' or add a call to it"))
    }

    pub fn dead_code(location: Option<SourceLocation>) -> Self {
        Self::new("This code is unreachable", WarningType::DeadCode, location)
            .with_help(
                "Code after a return statement or in an impossible branch will never execute",
            )
            .with_suggestion("Remove the unreachable code or fix the control flow")
    }

    pub fn type_inference_warning(inferred_type: &str, location: Option<SourceLocation>) -> Self {
        Self::new(
            format!(
                "Type inferred as '{inferred_type}' - consider adding explicit type annotation"
            ),
            WarningType::TypeInference,
            location,
        )
        .with_help("Explicit type annotations improve code readability and catch type errors early")
        .with_suggestion(format!(
            "Add ': {inferred_type}' to specify the type explicitly"
        ))
    }
}

impl fmt::Display for CompilerWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Warning: {}", self.message)?;

        if let Some(location) = &self.location {
            writeln!(f)?;
            write!(
                f,
                "  --> {}:{}:{}",
                location.file, location.line, location.column
            )?;
        }

        if let Some(help) = &self.help {
            writeln!(f)?;
            write!(f, "  = help: {help}")?;
        }

        if let Some(suggestion) = &self.suggestion {
            writeln!(f)?;
            write!(f, "  = suggestion: {suggestion}")?;
        }

        Ok(())
    }
}

/// Utility functions for enhanced error reporting
pub struct ErrorUtils;

impl ErrorUtils {
    /// Extract a source snippet around the error location
    pub fn extract_source_snippet(
        source: &str,
        location: &SourceLocation,
        context_lines: usize,
    ) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let error_line = location.line.saturating_sub(1); // Convert to 0-based indexing

        let start_line = error_line.saturating_sub(context_lines);
        let end_line = std::cmp::min(error_line + context_lines + 1, lines.len());

        let mut snippet = String::new();

        for (i, line_content) in lines[start_line..end_line].iter().enumerate() {
            let line_num = start_line + i + 1; // Convert back to 1-based
            let is_error_line = line_num == location.line;

            if is_error_line {
                snippet.push_str(&format!(" --> {line_num}: {line_content}\n"));

                // Add pointer to the specific column
                let pointer_line = format!(
                    "     {}{}",
                    " ".repeat(location.column.saturating_sub(1)),
                    "^".repeat(std::cmp::max(1, 1))
                );
                snippet.push_str(&format!("{pointer_line}\n"));
            } else {
                snippet.push_str(&format!("     {line_num}: {line_content}\n"));
            }
        }

        snippet
    }

    /// Generate suggestions for similar identifiers using Levenshtein distance
    pub fn suggest_similar_names(
        target: &str,
        available: &[&str],
        max_suggestions: usize,
    ) -> Vec<String> {
        let mut suggestions: Vec<(usize, &str)> = available
            .iter()
            .map(|name| (levenshtein_distance(target, name), *name))
            .filter(|(distance, _)| *distance <= 3) // Only suggest if distance is reasonable
            .collect();

        suggestions.sort_by_key(|(distance, _)| *distance);
        suggestions
            .into_iter()
            .take(max_suggestions)
            .map(|(_, name)| format!("Did you mean '{name}'?"))
            .collect()
    }

    /// Create suggestions for common syntax mistakes
    pub fn suggest_syntax_fixes(error_context: &str) -> Vec<String> {
        let mut suggestions = Vec::new();

        if error_context.contains("expected identifier") {
            suggestions
                .push("Check if you're using a reserved keyword as an identifier".to_string());
            suggestions
                .push("Ensure the identifier starts with a letter or underscore".to_string());
        }

        if error_context.contains("expected \")\"") {
            suggestions.push("Check for missing closing parenthesis".to_string());
            suggestions.push("Verify parentheses are properly balanced".to_string());
        }

        if error_context.contains("expected \"}\"") {
            suggestions.push("Check for missing closing brace".to_string());
            suggestions.push("Verify braces are properly balanced".to_string());
        }

        if error_context.contains("expected \"]\"") {
            suggestions.push("Check for missing closing bracket".to_string());
            suggestions.push("Verify brackets are properly balanced".to_string());
        }

        if error_context.contains("unexpected token") {
            suggestions.push("Check the syntax documentation for valid tokens".to_string());
            suggestions.push("Verify you're using the correct Clean Language syntax".to_string());
        }

        suggestions
    }

    /// Convert a Pest parsing error to an enhanced CompilerError with detailed context
    pub fn from_pest_error(
        pest_error: pest::error::Error<crate::parser::Rule>,
        source: &str,
        file_path: &str,
    ) -> CompilerError {
        let (location, error_span) = match pest_error.location {
            pest::error::InputLocation::Pos(pos) => {
                let (line, col) = ErrorUtils::calculate_line_column(source, pos);
                let location = SourceLocation {
                    line,
                    column: col,
                    file: file_path.to_string(),
                };
                (Some(location), (pos, pos))
            }
            pest::error::InputLocation::Span((start_pos, end_pos)) => {
                let (line, col) = ErrorUtils::calculate_line_column(source, start_pos);
                let location = SourceLocation {
                    line,
                    column: col,
                    file: file_path.to_string(),
                };
                (Some(location), (start_pos, end_pos))
            }
        };

        let source_snippet = location
            .as_ref()
            .map(|loc| ErrorUtils::extract_enhanced_source_snippet(source, error_span, loc));

        let (enhanced_message, suggestions, help) =
            ErrorUtils::enhance_pest_error_message(&pest_error, source, error_span);

        CompilerError::enhanced_syntax_error(
            enhanced_message,
            location,
            source_snippet,
            suggestions,
            help,
        )
    }

    /// Calculate accurate line and column numbers from position
    fn calculate_line_column(source: &str, pos: usize) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;

        for (i, ch) in source.char_indices() {
            if i >= pos {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }

        (line, col)
    }

    /// Extract enhanced source snippet with error highlighting
    fn extract_enhanced_source_snippet(
        source: &str,
        error_span: (usize, usize),
        location: &SourceLocation,
    ) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let error_line_idx = location.line.saturating_sub(1);

        if error_line_idx >= lines.len() {
            return "Source line not available".to_string();
        }

        let start_line = error_line_idx.saturating_sub(2);
        let end_line = std::cmp::min(error_line_idx + 3, lines.len());

        let mut snippet = String::new();

        for (line_idx, line_content) in lines.iter().enumerate().take(end_line).skip(start_line) {
            let line_num = line_idx + 1;

            if line_idx == error_line_idx {
                // This is the error line - add highlighting
                snippet.push_str(&format!("{line_num:4} | {line_content}\n"));

                // Add error pointer
                let pointer_offset = location.column.saturating_sub(1);
                let pointer_line = format!("     | {}^", " ".repeat(pointer_offset));

                // If we have a span, show the full error range
                if error_span.1 > error_span.0 {
                    let span_length = std::cmp::min(
                        error_span.1 - error_span.0,
                        line_content.len() - pointer_offset,
                    );
                    if span_length > 1 {
                        snippet.push_str(&format!(
                            "{}{}",
                            pointer_line,
                            "~".repeat(span_length.saturating_sub(1))
                        ));
                    } else {
                        snippet.push_str(&pointer_line);
                    }
                } else {
                    snippet.push_str(&pointer_line);
                }
                snippet.push('\n');
            } else {
                // Context line
                snippet.push_str(&format!("{line_num:4} | {line_content}\n"));
            }
        }

        snippet
    }

    /// Enhance Pest error messages with context-specific information
    fn enhance_pest_error_message(
        pest_error: &pest::error::Error<crate::parser::Rule>,
        source: &str,
        error_span: (usize, usize),
    ) -> (String, Vec<String>, Option<String>) {
        use pest::error::ErrorVariant;

        match &pest_error.variant {
            ErrorVariant::ParsingError {
                positives,
                negatives,
            } => {
                let mut message = String::new();
                let mut suggestions = Vec::new();

                if !positives.is_empty() {
                    let expected: Vec<String> = positives
                        .iter()
                        .map(ErrorUtils::rule_to_friendly_name)
                        .collect();

                    message = if expected.len() == 1 {
                        format!("Expected {}", expected[0])
                    } else {
                        format!("Expected one of: {}", expected.join(", "))
                    };

                    // Add context-specific suggestions
                    suggestions.extend(ErrorUtils::get_context_specific_suggestions(
                        &expected, source, error_span,
                    ));
                }

                if !negatives.is_empty() {
                    let unexpected: Vec<String> = negatives
                        .iter()
                        .map(ErrorUtils::rule_to_friendly_name)
                        .collect();

                    if !message.is_empty() {
                        message.push_str(&format!(", but found {}", unexpected.join(" or ")));
                    } else {
                        message = format!("Unexpected {}", unexpected.join(" or "));
                    }
                }

                // Add helpful context
                let help = Some(ErrorUtils::get_contextual_help(
                    &message, source, error_span,
                ));

                (message, suggestions, help)
            }
            ErrorVariant::CustomError { message } => {
                let suggestions = ErrorUtils::suggest_syntax_fixes(message);
                let help = Some("Check the Clean Language syntax documentation".to_string());
                (message.clone(), suggestions, help)
            }
        }
    }

    /// Convert Pest rule names to user-friendly descriptions
    fn rule_to_friendly_name(rule: &crate::parser::Rule) -> String {
        use crate::parser::Rule;

        match rule {
            Rule::identifier => "identifier".to_string(),
            Rule::number => "number".to_string(),
            Rule::string => "string".to_string(),
            Rule::boolean => "boolean value".to_string(),
            Rule::function_call => "function call".to_string(),
            Rule::method_call => "method call".to_string(),
            Rule::variable_decl => "variable declaration".to_string(),
            Rule::assignment => "assignment".to_string(),
            Rule::if_stmt => "if statement".to_string(),
            Rule::return_stmt => "return statement".to_string(),
            Rule::expression => "expression".to_string(),
            Rule::statement => "statement".to_string(),
            Rule::type_ => "type annotation".to_string(),
            Rule::parameter => "parameter".to_string(),
            Rule::function_body => "function body".to_string(),
            Rule::indented_block => "indented block".to_string(),
            Rule::NEWLINE => "newline".to_string(),
            Rule::INDENT => "indentation".to_string(),
            Rule::EOI => "end of input".to_string(),
            _ => format!("{rule:?}").to_lowercase(),
        }
    }

    /// Get context-specific suggestions based on expected rules and surrounding code
    fn get_context_specific_suggestions(
        expected: &[String],
        source: &str,
        error_span: (usize, usize),
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Get the context around the error
        let context = if error_span.0 > 0 && error_span.0 <= source.len() {
            &source[error_span.0.saturating_sub(50)..std::cmp::min(error_span.1 + 50, source.len())]
        } else {
            ""
        };

        for exp in expected {
            match exp.as_str() {
                "identifier" => {
                    suggestions.push("Use a valid identifier (letters, numbers, underscore, starting with letter)".to_string());
                    if context.contains("function") {
                        suggestions.push("Function names must be valid identifiers".to_string());
                    }
                }
                "type annotation" => {
                    suggestions.push(
                        "Add a type annotation (e.g., 'integer', 'string', 'boolean')".to_string(),
                    );
                    suggestions.push(
                        "Check if you're missing a type before the variable name".to_string(),
                    );
                }
                "expression" => {
                    suggestions.push(
                        "Add a valid expression (variable, literal, or function call)".to_string(),
                    );
                    if context.contains("=") {
                        suggestions
                            .push("Assignment requires a value after the '=' sign".to_string());
                    }
                }
                "indented block" => {
                    suggestions.push("Add proper indentation using tabs".to_string());
                    suggestions.push(
                        "Ensure the block is indented more than the parent statement".to_string(),
                    );
                }
                "newline" => {
                    suggestions.push("Add a newline to separate statements".to_string());
                }
                "end of input" => {
                    suggestions
                        .push("The file may be incomplete or have unclosed constructs".to_string());
                }
                _ => {}
            }
        }

        // Add general suggestions if no specific ones were found
        if suggestions.is_empty() {
            suggestions.push("Check the Clean Language syntax documentation".to_string());
            suggestions.push("Verify proper indentation and statement structure".to_string());
        }

        suggestions
    }

    /// Get contextual help based on the error message and surrounding code
    fn get_contextual_help(message: &str, source: &str, error_span: (usize, usize)) -> String {
        let error_context = &source
            [error_span.0.saturating_sub(20)..std::cmp::min(error_span.1 + 20, source.len())];

        if error_context.contains("function") {
            "Try checking the function declaration syntax".to_string()
        } else if error_context.contains("=") {
            "Check if you have a complete assignment statement".to_string()
        } else if error_context.contains("if") {
            "Verify the if statement has proper condition and body".to_string()
        } else if error_context.contains("print") {
            "Check the print statement syntax and arguments".to_string()
        } else {
            format!("Error in: '{message}'")
        }
    }

    /// Enhanced error message generation with context-aware suggestions
    pub fn from_pest_error_with_recovery(
        pest_error: pest::error::Error<crate::parser::Rule>,
        source: &str,
        file_path: &str,
        recovery_suggestions: Vec<String>,
    ) -> CompilerError {
        let error_span = match pest_error.location {
            pest::error::InputLocation::Pos(pos) => (pos, pos),
            pest::error::InputLocation::Span((start, end)) => (start, end),
        };

        let (line, column) = ErrorUtils::calculate_line_column(source, error_span.0);
        let location = SourceLocation {
            line,
            column,
            file: file_path.to_string(),
        };

        let source_snippet =
            ErrorUtils::extract_enhanced_source_snippet(source, error_span, &location);
        let (enhanced_message, mut suggestions, help) =
            ErrorUtils::enhance_pest_error_message(&pest_error, source, error_span);

        // Add recovery-specific suggestions
        suggestions.extend(recovery_suggestions);

        // Add context-specific suggestions based on error type
        suggestions.extend(ErrorUtils::get_recovery_suggestions(
            &pest_error,
            source,
            error_span,
        ));

        CompilerError::enhanced_syntax_error(
            enhanced_message,
            Some(location),
            Some(source_snippet),
            suggestions,
            help,
        )
    }

    /// Generate recovery-specific suggestions based on the error context
    pub fn get_recovery_suggestions(
        _pest_error: &pest::error::Error<crate::parser::Rule>,
        source: &str,
        error_span: (usize, usize),
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Get the context around the error
        let error_context = if error_span.1 < source.len() {
            &source[error_span.0.saturating_sub(50)..std::cmp::min(error_span.1 + 50, source.len())]
        } else {
            &source[error_span.0.saturating_sub(50)..]
        };

        // Add syntax-specific recovery suggestions based on context
        if error_context.contains("func ") {
            suggestions
                .push("Use 'function' instead of 'func' for function declarations".to_string());
        }

        if error_context.contains("->") {
            suggestions.push(
                "Clean Language uses 'function returnType name()' syntax, not '->' arrows"
                    .to_string(),
            );
        }

        if error_context.contains("let ") {
            suggestions.push(
                "Use explicit types instead of 'let': 'integer x = 5' not 'let x = 5'".to_string(),
            );
        }

        if error_context.contains(" + ") || error_context.contains(" - ") {
            suggestions.push(
                "Check if you have a complete expression on both sides of the operator".to_string(),
            );
        }

        if error_context.contains("= ") {
            suggestions.push("Make sure the assignment has a value after the '=' sign".to_string());
        }

        if error_context.contains("(") && !error_context.contains(")") {
            suggestions.push("Missing closing parenthesis ')'".to_string());
        }

        if error_context.contains("[") && !error_context.contains("]") {
            suggestions.push("Missing closing bracket ']'".to_string());
        }

        if error_context.contains("\"") && error_context.matches("\"").count() % 2 == 1 {
            suggestions.push("Missing closing quote '\"' for string".to_string());
        }

        suggestions
    }

    /// Generate detailed error analysis for multiple errors
    pub fn analyze_multiple_errors(errors: &[CompilerError]) -> Vec<String> {
        let mut analysis = Vec::new();

        if errors.is_empty() {
            return analysis;
        }

        analysis.push(format!(
            "\n📊 Error Analysis ({} errors found):",
            errors.len()
        ));
        analysis.push("═".repeat(50));

        // Categorize errors by type
        let mut syntax_errors = 0;
        let mut type_errors = 0;
        let mut other_errors = 0;
        let mut error_locations = Vec::new();

        for error in errors {
            match error {
                CompilerError::Syntax { context } => {
                    syntax_errors += 1;
                    if let Some(loc) = &context.location {
                        error_locations.push((loc.line, "Syntax"));
                    }
                }
                CompilerError::Type { context } => {
                    type_errors += 1;
                    if let Some(loc) = &context.location {
                        error_locations.push((loc.line, "Type"));
                    }
                }
                _ => {
                    other_errors += 1;
                }
            }
        }

        // Summary
        if syntax_errors > 0 {
            analysis.push(format!("🔸 Syntax Errors: {syntax_errors}"));
        }
        if type_errors > 0 {
            analysis.push(format!("🔸 Type Errors: {type_errors}"));
        }
        if other_errors > 0 {
            analysis.push(format!("🔸 Other Errors: {other_errors}"));
        }

        // Error distribution by line
        if !error_locations.is_empty() {
            analysis.push("\n📍 Error Locations:".to_string());
            error_locations.sort_by_key(|(line, _)| *line);
            for (line, error_type) in error_locations {
                analysis.push(format!("   Line {line}: {error_type}"));
            }
        }

        // Common patterns and suggestions
        analysis.push("\n💡 Recovery Suggestions:".to_string());

        if syntax_errors > type_errors {
            analysis.push(
                "• Focus on syntax errors first - they often cause cascading issues".to_string(),
            );
            analysis.push("• Check for missing brackets, parentheses, or quotes".to_string());
            analysis.push("• Verify proper indentation with tabs".to_string());
        }

        if errors.len() > 10 {
            analysis.push(
                "• Large number of errors detected - consider fixing the first few and re-parsing"
                    .to_string(),
            );
        }

        // Specific pattern detection
        let all_messages: String = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        if all_messages.contains("expected primary") {
            analysis.push(
                "• Multiple 'expected primary' errors suggest incomplete expressions".to_string(),
            );
        }

        if all_messages.contains("missing") {
            analysis
                .push("• Missing elements detected - check for incomplete statements".to_string());
        }

        analysis
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation_and_display() {
        let error = CompilerError::syntax_error("test error", None, None);
        assert!(error.to_string().contains("test error"));

        let error = CompilerError::type_error("type error", None, None);
        assert!(error.to_string().contains("type error"));

        let error = CompilerError::memory_error("memory error", None, None);
        assert!(error.to_string().contains("memory error"));
    }

    #[test]
    fn test_error_conversion() {
        let context = ErrorContext::new("test error", None, ErrorType::Syntax, None);
        let error = CompilerError::syntax_error("test error", None, None);
        assert!(error.to_string().contains("test error"));

        let string: String = context.into();
        assert!(string.contains("test error"));
    }
}
