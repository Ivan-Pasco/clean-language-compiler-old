//! Enhanced error handling architecture with structured hierarchy
//!
//! This module provides a comprehensive, hierarchical error handling system that improves
//! error categorization, contextual information, fix suggestions, and recovery mechanisms.

use super::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Enhanced error hierarchy with structured categorization
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    /// Syntax and parsing errors
    Syntax(SyntaxErrorKind),
    /// Type system and semantic errors
    Semantic(SemanticErrorKind),
    /// Code generation and compilation errors
    Compilation(CompilationErrorKind),
    /// Runtime and execution errors
    Runtime(RuntimeErrorKind),
    /// System and infrastructure errors
    System(SystemErrorKind),
    /// User-facing errors (configuration, CLI, etc.)
    User(UserErrorKind),
}

/// Fine-grained syntax error classification
#[derive(Debug, Clone, PartialEq)]
pub enum SyntaxErrorKind {
    /// Invalid token or character
    InvalidToken,
    /// Unexpected token in context
    UnexpectedToken {
        expected: Vec<String>,
        found: String,
    },
    /// Missing required syntax element
    MissingElement { element: String },
    /// Unterminated construct (string, comment, block)
    Unterminated { construct: String },
    /// Malformed construct
    Malformed { construct: String, issue: String },
    /// Invalid indentation or structure
    IndentationError,
}

/// Semantic and type system error classification
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticErrorKind {
    /// Type mismatch or incompatibility
    TypeMismatch {
        expected: String,
        found: String,
        context: String,
    },
    /// Undefined symbol or identifier
    UndefinedSymbol {
        symbol: String,
        suggestions: Vec<String>,
    },
    /// Symbol redefinition or duplicate declaration
    SymbolRedefinition {
        symbol: String,
        original_location: Option<SourceLocation>,
    },
    /// Invalid operation on type
    InvalidOperation {
        operation: String,
        type_name: String,
    },
    /// Visibility or access control violation
    AccessViolation {
        symbol: String,
        required_visibility: String,
    },
    /// Inheritance or polymorphism error
    InheritanceError { issue: String, class: String },
    /// Inheritance cycle detected
    InheritanceCycle,
    /// Invalid type specification
    InvalidType,
    /// Generic type or constraint error
    GenericError { issue: String, context: String },
}

/// Code generation and compilation error classification
#[derive(Debug, Clone, PartialEq)]
pub enum CompilationErrorKind {
    /// WebAssembly generation error
    WasmGeneration { phase: String, details: String },
    /// Optimization error
    OptimizationError { pass: String, details: String },
    /// Memory layout or allocation error
    MemoryLayout { issue: String },
    /// Import/export resolution error
    ModuleResolution { module: String, symbol: String },
    /// Target-specific compilation error
    TargetError { target: String, issue: String },
    /// Function not found during compilation
    FunctionNotFound,
}

/// Runtime and execution error classification
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeErrorKind {
    /// Memory access violation
    MemoryViolation { address: usize, operation: String },
    /// Stack overflow or underflow
    StackError { operation: String },
    /// Division by zero or arithmetic error
    ArithmeticError { operation: String },
    /// Null pointer or invalid reference
    ReferenceError { details: String },
    /// Assertion failure
    AssertionFailure { condition: String, context: String },
}

/// System and infrastructure error classification
#[derive(Debug, Clone, PartialEq)]
pub enum SystemErrorKind {
    /// File system error
    FileSystem { path: String, operation: String },
    /// Network or HTTP error
    Network { url: String, status: Option<u16> },
    /// External tool or dependency error
    ExternalTool {
        tool: String,
        exit_code: Option<i32>,
    },
    /// Resource exhaustion (memory, disk, etc.)
    ResourceExhaustion { resource: String },
}

/// User-facing error classification
#[derive(Debug, Clone, PartialEq)]
pub enum UserErrorKind {
    /// Invalid command-line arguments
    InvalidArguments { argument: String, issue: String },
    /// Configuration file error
    Configuration { file: String, issue: String },
    /// Missing required input
    MissingInput { required: String },
    /// Invalid project structure
    ProjectStructure { issue: String },
}

/// Enhanced error context with rich diagnostic information
#[derive(Debug, Clone)]
pub struct EnhancedErrorContext {
    /// Primary error message
    pub message: String,
    /// Detailed explanation
    pub explanation: Option<String>,
    /// Contextual help message
    pub help: Option<String>,
    /// Error category and classification
    pub category: ErrorCategory,
    /// Source location information
    pub location: Option<SourceLocation>,
    /// Related source locations
    pub related_locations: Vec<(SourceLocation, String)>,
    /// Code suggestions and fixes
    pub suggestions: Vec<CodeSuggestion>,
    /// Source code snippet
    pub source_snippet: Option<SourceSnippet>,
    /// Call stack trace
    pub stack_trace: Vec<StackFrame>,
    /// Error severity level
    pub severity: ErrorSeverity,
    /// Unique error code
    pub error_code: String,
    /// Related or caused errors
    pub related_errors: Vec<Arc<EnhancedCompilerError>>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Code suggestion with fix information
#[derive(Debug, Clone)]
pub struct CodeSuggestion {
    /// Description of the suggestion
    pub description: String,
    /// Suggested code replacement
    pub replacement: Option<String>,
    /// Location where fix should be applied
    pub apply_location: Option<SourceLocation>,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f32,
    /// Whether this fix can be applied automatically
    pub auto_fixable: bool,
}

/// Enhanced source snippet with context
#[derive(Debug, Clone)]
pub struct SourceSnippet {
    /// File path
    pub file: String,
    /// Starting line number
    pub start_line: usize,
    /// Ending line number
    pub end_line: usize,
    /// Source code lines
    pub lines: Vec<String>,
    /// Line highlighting information
    pub highlights: Vec<LineHighlight>,
}

/// Line highlighting for error display
#[derive(Debug, Clone)]
pub struct LineHighlight {
    /// Line number
    pub line: usize,
    /// Starting column
    pub start_col: usize,
    /// Ending column
    pub end_col: usize,
    /// Highlight style
    pub style: HighlightStyle,
    /// Additional message
    pub message: Option<String>,
}

/// Highlighting style for different error elements
#[derive(Debug, Clone)]
pub enum HighlightStyle {
    Error,
    Warning,
    Info,
    Hint,
    Suggestion,
}

/// Enhanced compiler error with structured hierarchy
#[derive(Debug, Clone)]
pub struct EnhancedCompilerError {
    /// Enhanced error context
    pub context: EnhancedErrorContext,
    /// Error creation timestamp
    pub timestamp: std::time::SystemTime,
    /// Error chain (cause hierarchy)
    pub cause_chain: Vec<Arc<EnhancedCompilerError>>,
}

impl EnhancedCompilerError {
    /// Create a new enhanced error
    pub fn new(message: String, category: ErrorCategory, location: Option<SourceLocation>) -> Self {
        let error_code = Self::generate_error_code(&category);

        Self {
            context: EnhancedErrorContext {
                message,
                explanation: None,
                help: None,
                category,
                location,
                related_locations: Vec::new(),
                suggestions: Vec::new(),
                source_snippet: None,
                stack_trace: Vec::new(),
                severity: ErrorSeverity::Error,
                error_code,
                related_errors: Vec::new(),
                metadata: HashMap::new(),
            },
            timestamp: std::time::SystemTime::now(),
            cause_chain: Vec::new(),
        }
    }

    /// Generate structured error code based on category
    fn generate_error_code(category: &ErrorCategory) -> String {
        match category {
            ErrorCategory::Syntax(kind) => match kind {
                SyntaxErrorKind::InvalidToken => "SYN001".to_string(),
                SyntaxErrorKind::UnexpectedToken { .. } => "SYN002".to_string(),
                SyntaxErrorKind::MissingElement { .. } => "SYN003".to_string(),
                SyntaxErrorKind::Unterminated { .. } => "SYN004".to_string(),
                SyntaxErrorKind::Malformed { .. } => "SYN005".to_string(),
                SyntaxErrorKind::IndentationError => "SYN006".to_string(),
            },
            ErrorCategory::Semantic(kind) => match kind {
                SemanticErrorKind::TypeMismatch { .. } => "SEM001".to_string(),
                SemanticErrorKind::UndefinedSymbol { .. } => "SEM002".to_string(),
                SemanticErrorKind::SymbolRedefinition { .. } => "SEM003".to_string(),
                SemanticErrorKind::InvalidOperation { .. } => "SEM004".to_string(),
                SemanticErrorKind::AccessViolation { .. } => "SEM005".to_string(),
                SemanticErrorKind::InheritanceError { .. } => "SEM006".to_string(),
                SemanticErrorKind::GenericError { .. } => "SEM007".to_string(),
                SemanticErrorKind::InheritanceCycle => "SEM008".to_string(),
                SemanticErrorKind::InvalidType => "SEM009".to_string(),
            },
            ErrorCategory::Compilation(kind) => match kind {
                CompilationErrorKind::WasmGeneration { .. } => "COM001".to_string(),
                CompilationErrorKind::OptimizationError { .. } => "COM002".to_string(),
                CompilationErrorKind::MemoryLayout { .. } => "COM003".to_string(),
                CompilationErrorKind::ModuleResolution { .. } => "COM004".to_string(),
                CompilationErrorKind::TargetError { .. } => "COM005".to_string(),
                CompilationErrorKind::FunctionNotFound => "COM006".to_string(),
            },
            ErrorCategory::Runtime(kind) => match kind {
                RuntimeErrorKind::MemoryViolation { .. } => "RUN001".to_string(),
                RuntimeErrorKind::StackError { .. } => "RUN002".to_string(),
                RuntimeErrorKind::ArithmeticError { .. } => "RUN003".to_string(),
                RuntimeErrorKind::ReferenceError { .. } => "RUN004".to_string(),
                RuntimeErrorKind::AssertionFailure { .. } => "RUN005".to_string(),
            },
            ErrorCategory::System(kind) => match kind {
                SystemErrorKind::FileSystem { .. } => "SYS001".to_string(),
                SystemErrorKind::Network { .. } => "SYS002".to_string(),
                SystemErrorKind::ExternalTool { .. } => "SYS003".to_string(),
                SystemErrorKind::ResourceExhaustion { .. } => "SYS004".to_string(),
            },
            ErrorCategory::User(kind) => match kind {
                UserErrorKind::InvalidArguments { .. } => "USR001".to_string(),
                UserErrorKind::Configuration { .. } => "USR002".to_string(),
                UserErrorKind::MissingInput { .. } => "USR003".to_string(),
                UserErrorKind::ProjectStructure { .. } => "USR004".to_string(),
            },
        }
    }

    /// Convert to compiler error
    pub fn into_compiler_error(self) -> CompilerError {
        match self.context.category {
            ErrorCategory::Syntax(_) => CompilerError::syntax_error(
                self.context.message,
                self.context.help,
                self.context.location,
            ),
            ErrorCategory::Semantic(_) => CompilerError::type_error(
                self.context.message,
                self.context.help,
                self.context.location,
            ),
            ErrorCategory::Compilation(_) => CompilerError::codegen_error(
                self.context.message,
                self.context.help,
                self.context.location,
            ),
            ErrorCategory::Runtime(_) => CompilerError::runtime_error(
                self.context.message,
                self.context.help,
                self.context.location,
            ),
            ErrorCategory::System(_) => CompilerError::io_error(
                self.context.message,
                self.context.help,
                self.context.location,
            ),
            ErrorCategory::User(_) => CompilerError::validation_error(
                self.context.message,
                self.context.location.unwrap_or_default(),
            ),
        }
    }

    /// Add explanation to error
    pub fn with_explanation<T: Into<String>>(mut self, explanation: T) -> Self {
        self.context.explanation = Some(explanation.into());
        self
    }

    /// Add help message
    pub fn with_help<T: Into<String>>(mut self, help: T) -> Self {
        self.context.help = Some(help.into());
        self
    }

    /// Add code suggestion
    pub fn with_suggestion(mut self, suggestion: CodeSuggestion) -> Self {
        self.context.suggestions.push(suggestion);
        self
    }

    /// Add related location
    pub fn with_related_location(mut self, location: SourceLocation, description: String) -> Self {
        self.context.related_locations.push((location, description));
        self
    }

    /// Set severity level
    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.context.severity = severity;
        self
    }

    /// Add source snippet
    pub fn with_source_snippet(mut self, snippet: SourceSnippet) -> Self {
        self.context.source_snippet = Some(snippet);
        self
    }

    /// Add to cause chain
    pub fn caused_by(mut self, cause: EnhancedCompilerError) -> Self {
        self.cause_chain.push(Arc::new(cause));
        self
    }

    /// Add metadata
    pub fn with_metadata<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.context.metadata.insert(key.into(), value.into());
        self
    }

    /// Get root cause of error chain
    pub fn root_cause(&self) -> &EnhancedCompilerError {
        let mut current = self;
        while let Some(cause) = current.cause_chain.first() {
            current = cause;
        }
        current
    }

    /// Get all errors in chain
    pub fn error_chain(&self) -> Vec<&EnhancedCompilerError> {
        let mut chain = vec![self];
        let mut current = self;

        while let Some(cause) = current.cause_chain.first() {
            chain.push(cause);
            current = cause;
        }

        chain
    }
}

/// Enhanced error builder for fluent error creation
pub struct EnhancedErrorBuilder {
    error: EnhancedCompilerError,
}

impl EnhancedErrorBuilder {
    /// Create new error builder from existing error
    pub fn new(error: EnhancedCompilerError) -> Self {
        Self { error }
    }

    /// Create new error builder with message and category
    pub fn new_with_message<T: Into<String>>(message: T, category: ErrorCategory) -> Self {
        Self {
            error: EnhancedCompilerError::new(message.into(), category, None),
        }
    }

    /// Set location
    pub fn at(mut self, location: SourceLocation) -> Self {
        self.error.context.location = Some(location);
        self
    }

    /// Add explanation
    pub fn explain<T: Into<String>>(mut self, explanation: T) -> Self {
        self.error.context.explanation = Some(explanation.into());
        self
    }

    /// Add help
    pub fn with_help<T: Into<String>>(mut self, help: T) -> Self {
        self.error.context.help = Some(help.into());
        self
    }

    /// Add help (alias)
    pub fn help<T: Into<String>>(mut self, help: T) -> Self {
        self.error.context.help = Some(help.into());
        self
    }

    /// Add suggestion
    pub fn with_suggestion<T: Into<String>>(mut self, suggestion: T) -> Self {
        self.error.context.suggestions.push(CodeSuggestion {
            description: suggestion.into(),
            replacement: None,
            apply_location: None,
            confidence: 0.7,
            auto_fixable: false,
        });
        self
    }

    /// Add suggestion
    pub fn suggest(mut self, suggestion: CodeSuggestion) -> Self {
        self.error.context.suggestions.push(suggestion);
        self
    }

    /// Set severity
    pub fn severity(mut self, severity: ErrorSeverity) -> Self {
        self.error.context.severity = severity;
        self
    }

    /// Build the enhanced error
    pub fn build(self) -> EnhancedCompilerError {
        self.error
    }
}

/// Enhanced error collector and processor
pub struct EnhancedErrorCollector {
    /// Collected errors
    errors: Vec<EnhancedCompilerError>,
    /// Error correlation map
    correlations: HashMap<String, Vec<usize>>,
    /// Error statistics
    stats: ErrorStatistics,
}

impl Default for EnhancedErrorCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhancedErrorCollector {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            correlations: HashMap::new(),
            stats: ErrorStatistics::new(),
        }
    }

    /// Create semantic error builder
    pub fn create_semantic_error(
        &self,
        kind: SemanticErrorKind,
        message: String,
        location: Option<SourceLocation>,
    ) -> EnhancedErrorBuilder {
        EnhancedErrorBuilder::new(EnhancedCompilerError::new(
            message,
            ErrorCategory::Semantic(kind),
            location,
        ))
    }

    /// Create compilation error builder
    pub fn create_compilation_error(
        &self,
        kind: CompilationErrorKind,
        message: String,
        location: Option<SourceLocation>,
    ) -> EnhancedErrorBuilder {
        EnhancedErrorBuilder::new(EnhancedCompilerError::new(
            message,
            ErrorCategory::Compilation(kind),
            location,
        ))
    }

    /// Add error to collection
    pub fn add_error(&mut self, error: EnhancedCompilerError) {
        self.correlations
            .entry(error.context.error_code.clone())
            .or_default()
            .push(self.errors.len());

        self.errors.push(error);
        self.update_stats();
    }

    /// Get all errors
    pub fn get_errors(&self) -> &[EnhancedCompilerError] {
        &self.errors
    }

    /// Get errors by category
    pub fn get_errors_by_category(&self, category: &ErrorCategory) -> Vec<&EnhancedCompilerError> {
        self.errors
            .iter()
            .filter(|e| {
                std::mem::discriminant(&e.context.category) == std::mem::discriminant(category)
            })
            .collect()
    }

    /// Get error statistics
    pub fn get_statistics(&self) -> &ErrorStatistics {
        &self.stats
    }

    /// Clear all errors
    pub fn clear(&mut self) {
        self.errors.clear();
        self.correlations.clear();
        self.stats = ErrorStatistics::new();
    }

    /// Update error statistics
    fn update_stats(&mut self) {
        // This would be implemented to update various statistics
        // For now, it's a placeholder
    }
}

/// Convenience macros for creating enhanced errors
#[macro_export]
macro_rules! syntax_error {
    ($msg:expr, $kind:expr) => {
        EnhancedCompilerError::new($msg.to_string(), ErrorCategory::Syntax($kind), None)
    };
    ($msg:expr, $kind:expr, $location:expr) => {
        EnhancedCompilerError::new(
            $msg.to_string(),
            ErrorCategory::Syntax($kind),
            Some($location),
        )
    };
}

#[macro_export]
macro_rules! semantic_error {
    ($msg:expr, $kind:expr) => {
        EnhancedCompilerError::new($msg.to_string(), ErrorCategory::Semantic($kind), None)
    };
    ($msg:expr, $kind:expr, $location:expr) => {
        EnhancedCompilerError::new(
            $msg.to_string(),
            ErrorCategory::Semantic($kind),
            Some($location),
        )
    };
}

#[macro_export]
macro_rules! compilation_error {
    ($msg:expr, $kind:expr) => {
        EnhancedCompilerError::new($msg.to_string(), ErrorCategory::Compilation($kind), None)
    };
}

/// Gap 5: Map a `CompilerError` to its canonical spec error code string.
///
/// This is a pure mapping layer over the existing error hierarchy. It does not
/// refactor the error system — it simply provides a lookup function that
/// translates the internal error code (e.g. "E002") or error type into the
/// spec-defined code (e.g. "SEM001").
///
/// Error code ranges defined by the spec:
///
/// | Prefix  | Range       | Meaning                        |
/// |---------|-------------|-------------------------------|
/// | SEM     | 001–009     | Semantic / type system         |
/// | FUNC    | 001–005     | Function-level rules           |
/// | CLASS   | 001–005     | Class-level rules              |
/// | STATE   | 001–003     | State block rules              |
/// | IDX     | 001–004     | Index / bracket access         |
/// | SCOPE   | 001–003     | Scope / shadowing              |
/// | IMPORT  | 001–003     | Import / module rules          |
/// | PLUGIN  | 001–003     | Plugin system rules            |
/// | COM     | 001–006     | Code generation                |
/// | RUN     | 001–005     | Runtime / execution            |
/// | SYN     | 001–006     | Syntax / parsing               |
/// | SYS     | 001–004     | System / infrastructure        |
/// | USR     | 001–004     | User-facing CLI / config       |
pub fn spec_error_code(error: &super::CompilerError) -> Option<&'static str> {
    use super::{CompilerError, ErrorType};

    // First check if the error already carries an explicit spec-style code
    // (e.g. FUNC004, IDX001) set via `with_error_code`.
    let raw_code: Option<&str> = match error {
        CompilerError::Syntax { context } => context.error_code.as_deref(),
        CompilerError::Type { context } => context.error_code.as_deref(),
        CompilerError::Memory { context } => context.error_code.as_deref(),
        CompilerError::Codegen { context } => context.error_code.as_deref(),
        CompilerError::IO { context } => context.error_code.as_deref(),
        CompilerError::Runtime { context } => context.error_code.as_deref(),
        CompilerError::Validation { context } => context.error_code.as_deref(),
        CompilerError::Module { context } => context.error_code.as_deref(),
        CompilerError::Testing { context } => context.error_code.as_deref(),
        CompilerError::LexError(_) => None,
        CompilerError::PluginError { .. } => None,
    };

    // If the code already uses a known spec prefix, return it directly.
    if let Some(code) = raw_code {
        let is_spec_code = matches!(
            code.get(..3),
            Some(
                "SEM"
                    | "FUN"
                    | "CLA"
                    | "STA"
                    | "IDX"
                    | "SCO"
                    | "IMP"
                    | "PLU"
                    | "COM"
                    | "RUN"
                    | "SYN"
                    | "SYS"
                    | "USR"
            )
        );
        if is_spec_code {
            // Return the static string corresponding to known codes.
            return match code {
                // Semantic
                "SEM001" => Some("SEM001"),
                "SEM002" => Some("SEM002"),
                "SEM003" => Some("SEM003"),
                "SEM004" => Some("SEM004"),
                "SEM005" => Some("SEM005"),
                "SEM006" => Some("SEM006"),
                "SEM007" => Some("SEM007"),
                "SEM008" => Some("SEM008"),
                "SEM009" => Some("SEM009"),
                // Function
                "FUNC001" => Some("FUNC001"),
                "FUNC002" => Some("FUNC002"),
                "FUNC003" => Some("FUNC003"),
                "FUNC004" => Some("FUNC004"),
                "FUNC005" => Some("FUNC005"),
                // Class
                "CLASS001" => Some("CLASS001"),
                "CLASS002" => Some("CLASS002"),
                "CLASS003" => Some("CLASS003"),
                "CLASS004" => Some("CLASS004"),
                "CLASS005" => Some("CLASS005"),
                // State
                "STATE001" => Some("STATE001"),
                "STATE002" => Some("STATE002"),
                "STATE003" => Some("STATE003"),
                // Index
                "IDX001" => Some("IDX001"),
                "IDX002" => Some("IDX002"),
                "IDX003" => Some("IDX003"),
                "IDX004" => Some("IDX004"),
                // Scope
                "SCOPE001" => Some("SCOPE001"),
                "SCOPE002" => Some("SCOPE002"),
                "SCOPE003" => Some("SCOPE003"),
                // Import
                "IMPORT001" => Some("IMPORT001"),
                "IMPORT002" => Some("IMPORT002"),
                "IMPORT003" => Some("IMPORT003"),
                // Plugin
                "PLUGIN001" => Some("PLUGIN001"),
                "PLUGIN002" => Some("PLUGIN002"),
                "PLUGIN003" => Some("PLUGIN003"),
                // Compilation
                "COM001" => Some("COM001"),
                "COM002" => Some("COM002"),
                "COM003" => Some("COM003"),
                "COM004" => Some("COM004"),
                "COM005" => Some("COM005"),
                "COM006" => Some("COM006"),
                // Runtime
                "RUN001" => Some("RUN001"),
                "RUN002" => Some("RUN002"),
                "RUN003" => Some("RUN003"),
                "RUN004" => Some("RUN004"),
                "RUN005" => Some("RUN005"),
                // Syntax
                "SYN001" => Some("SYN001"),
                "SYN002" => Some("SYN002"),
                "SYN003" => Some("SYN003"),
                "SYN004" => Some("SYN004"),
                "SYN005" => Some("SYN005"),
                "SYN006" => Some("SYN006"),
                _ => None,
            };
        }
    }

    // Fall back: derive spec code from the error's ErrorType / variant.
    match error {
        CompilerError::Syntax { context } => match context.error_type {
            ErrorType::Syntax => Some("SYN001"),
            _ => None,
        },
        CompilerError::Type { context } => match context.error_type {
            ErrorType::Type => Some("SEM001"),
            ErrorType::Semantic => Some("SEM001"),
            _ => None,
        },
        CompilerError::Memory { .. } => Some("COM003"),
        CompilerError::Codegen { .. } => Some("COM001"),
        CompilerError::IO { .. } => Some("SYS001"),
        CompilerError::Runtime { .. } => Some("RUN001"),
        CompilerError::Validation { context } => match context.error_type {
            ErrorType::Validation => Some("SEM007"),
            ErrorType::Semantic => Some("SEM007"),
            _ => None,
        },
        CompilerError::Module { .. } => Some("IMPORT001"),
        CompilerError::Testing { .. } => None,
        CompilerError::LexError(_) => Some("SYN001"),
        CompilerError::PluginError { .. } => Some("PLUGIN001"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enhanced_error_creation() {
        let error = EnhancedCompilerError::new(
            "Unexpected token".to_string(),
            ErrorCategory::Syntax(SyntaxErrorKind::UnexpectedToken {
                expected: vec!["identifier".to_string()],
                found: "number".to_string(),
            }),
            Some(SourceLocation {
                line: 10,
                column: 5,
                file: "test.cln".to_string(),
                byte_start: None,
                byte_end: None,
            }),
        );

        assert_eq!(error.context.error_code, "SYN002");
        assert_eq!(error.context.message, "Unexpected token");
        assert!(error.context.location.is_some());
    }

    #[test]
    fn test_error_builder() {
        let error = EnhancedErrorBuilder::new_with_message(
            "Type mismatch",
            ErrorCategory::Semantic(SemanticErrorKind::TypeMismatch {
                expected: "integer".to_string(),
                found: "string".to_string(),
                context: "assignment".to_string(),
            }),
        )
        .explain("The variable was declared as integer but assigned a string value")
        .help("Consider converting the string to integer using parseInt()")
        .severity(ErrorSeverity::Error)
        .build();

        assert_eq!(error.context.error_code, "SEM001");
        assert!(error.context.explanation.is_some());
        assert!(error.context.help.is_some());
        assert_eq!(error.context.severity, ErrorSeverity::Error);
    }

    #[test]
    fn test_error_collector() {
        let mut collector = EnhancedErrorCollector::new();

        let error1 = EnhancedCompilerError::new(
            "Parse error".to_string(),
            ErrorCategory::Syntax(SyntaxErrorKind::InvalidToken),
            None,
        );

        let error2 = EnhancedCompilerError::new(
            "Type error".to_string(),
            ErrorCategory::Semantic(SemanticErrorKind::TypeMismatch {
                expected: "boolean".to_string(),
                found: "integer".to_string(),
                context: "condition".to_string(),
            }),
            None,
        );

        collector.add_error(error1);
        collector.add_error(error2);

        assert_eq!(collector.get_errors().len(), 2);

        let syntax_errors =
            collector.get_errors_by_category(&ErrorCategory::Syntax(SyntaxErrorKind::InvalidToken));
        assert_eq!(syntax_errors.len(), 1);
    }
}
