//! Advanced error diagnostics and analysis system
//!
//! This module provides sophisticated error analysis, pattern detection,
//! and automated fix suggestions for Clean Language compilation errors.

use super::*;
use std::collections::HashMap;

/// Comprehensive diagnostic system for error analysis
pub struct DiagnosticSystem {
    /// Error pattern database
    error_patterns: ErrorPatternDatabase,
    /// Fix suggestion engine
    fix_engine: FixSuggestionEngine,
    /// Error correlation analyzer
    correlator: ErrorCorrelator,
    /// Statistics collector
    stats: ErrorStatistics,
}

impl DiagnosticSystem {
    pub fn new() -> Self {
        Self {
            error_patterns: ErrorPatternDatabase::new(),
            fix_engine: FixSuggestionEngine::new(),
            correlator: ErrorCorrelator::new(),
            stats: ErrorStatistics::new(),
        }
    }

    /// Perform comprehensive analysis of compilation errors
    pub fn analyze_errors(&mut self, errors: &[CompilerError]) -> DiagnosticReport {
        // Update statistics
        self.stats.update(errors);

        // Detect error patterns
        let patterns = self.error_patterns.detect_patterns(errors);

        // Correlate related errors
        let correlations = self.correlator.analyze_correlations(errors);

        // Generate fix suggestions
        let suggestions = self.fix_engine.generate_suggestions(errors, &patterns);

        // Assess error severity and impact
        let severity_analysis = self.analyze_severity(errors);

        DiagnosticReport {
            total_errors: errors.len(),
            patterns,
            correlations,
            suggestions,
            severity_analysis,
            statistics: self.stats.clone(),
        }
    }

    fn analyze_severity(&self, errors: &[CompilerError]) -> SeverityAnalysis {
        let mut critical_count = 0;
        let mut warning_count = 0;
        let mut blocking_errors = Vec::new();

        for error in errors {
            match error {
                CompilerError::Syntax { context } => {
                    if context.severity == ErrorSeverity::Error {
                        critical_count += 1;
                        if self.is_blocking_error(error) {
                            blocking_errors.push(error.clone());
                        }
                    }
                }
                CompilerError::Type { .. } => {
                    critical_count += 1;
                    if self.is_blocking_error(error) {
                        blocking_errors.push(error.clone());
                    }
                }
                _ => warning_count += 1,
            }
        }

        SeverityAnalysis {
            critical_errors: critical_count,
            warnings: warning_count,
            compilation_viable: blocking_errors.is_empty(),
            blocking_errors,
        }
    }

    fn is_blocking_error(&self, error: &CompilerError) -> bool {
        let error_msg = error.to_string();

        // Syntax errors that prevent further parsing
        if error_msg.contains("Unexpected end of file")
            || error_msg.contains("Invalid syntax")
            || error_msg.contains("Parse error")
        {
            return true;
        }

        // Type errors that prevent code generation
        if error_msg.contains("Undefined function")
            || error_msg.contains("Type not found")
            || error_msg.contains("Incompatible types")
        {
            return true;
        }

        false
    }
}

/// Database of common error patterns and their characteristics
pub struct ErrorPatternDatabase {
    patterns: Vec<ErrorPattern>,
}

impl ErrorPatternDatabase {
    pub fn new() -> Self {
        let patterns = vec![
            ErrorPattern {
                id: "missing_semicolon".to_string(),
                description: "Missing semicolon at end of statement".to_string(),
                signature: vec!["Expected ';'".to_string()],
                frequency: 0.15, // 15% of syntax errors
                fix_complexity: FixComplexity::Trivial,
                auto_fixable: true,
            },
            ErrorPattern {
                id: "unmatched_braces".to_string(),
                description: "Unmatched braces or brackets".to_string(),
                signature: vec!["Expected '}'".to_string(), "Unmatched '{'".to_string()],
                frequency: 0.12,
                fix_complexity: FixComplexity::Simple,
                auto_fixable: true,
            },
            ErrorPattern {
                id: "undefined_variable".to_string(),
                description: "Use of undefined variable".to_string(),
                signature: vec![
                    "Variable not found".to_string(),
                    "Undefined identifier".to_string(),
                ],
                frequency: 0.20,
                fix_complexity: FixComplexity::Moderate,
                auto_fixable: false,
            },
            ErrorPattern {
                id: "type_mismatch".to_string(),
                description: "Type mismatch in assignment or expression".to_string(),
                signature: vec![
                    "Type mismatch".to_string(),
                    "Incompatible types".to_string(),
                ],
                frequency: 0.18,
                fix_complexity: FixComplexity::Complex,
                auto_fixable: false,
            },
            ErrorPattern {
                id: "missing_function_body".to_string(),
                description: "Function declared without body".to_string(),
                signature: vec!["Expected function body".to_string()],
                frequency: 0.08,
                fix_complexity: FixComplexity::Moderate,
                auto_fixable: false,
            },
        ];

        Self { patterns }
    }

    pub fn detect_patterns(&self, errors: &[CompilerError]) -> Vec<DetectedPattern> {
        let mut detected = Vec::new();

        for pattern in &self.patterns {
            let matches = self.find_pattern_matches(pattern, errors);
            if !matches.is_empty() {
                let confidence = self.calculate_confidence(&pattern, &matches);
                detected.push(DetectedPattern {
                    pattern: pattern.clone(),
                    instances: matches,
                    confidence,
                });
            }
        }

        // Sort by confidence and frequency
        detected.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        detected
    }

    fn find_pattern_matches(&self, pattern: &ErrorPattern, errors: &[CompilerError]) -> Vec<usize> {
        let mut matches = Vec::new();

        for (i, error) in errors.iter().enumerate() {
            let error_text = error.to_string();
            for signature in &pattern.signature {
                if error_text.contains(signature) {
                    matches.push(i);
                    break;
                }
            }
        }

        matches
    }

    fn calculate_confidence(&self, pattern: &ErrorPattern, matches: &[usize]) -> f64 {
        if matches.is_empty() {
            return 0.0;
        }

        // Base confidence on pattern frequency and match count
        let base_confidence = pattern.frequency;
        let match_factor = (matches.len() as f64).min(5.0) / 5.0; // Cap at 5 matches

        (base_confidence + match_factor) / 2.0
    }
}

/// Engine for generating automated fix suggestions
pub struct FixSuggestionEngine {
    fix_templates: HashMap<String, Vec<FixTemplate>>,
}

impl FixSuggestionEngine {
    pub fn new() -> Self {
        let mut fix_templates = HashMap::new();

        // Semicolon fixes
        fix_templates.insert(
            "missing_semicolon".to_string(),
            vec![FixTemplate {
                description: "Add missing semicolon".to_string(),
                action: FixAction::Insert {
                    position: "end_of_statement".to_string(),
                    text: ";".to_string(),
                },
                confidence: 0.95,
                auto_apply: true,
            }],
        );

        // Brace fixes
        fix_templates.insert(
            "unmatched_braces".to_string(),
            vec![FixTemplate {
                description: "Add missing closing brace".to_string(),
                action: FixAction::Insert {
                    position: "end_of_block".to_string(),
                    text: "}".to_string(),
                },
                confidence: 0.85,
                auto_apply: true,
            }],
        );

        // Variable definition fixes
        fix_templates.insert(
            "undefined_variable".to_string(),
            vec![
                FixTemplate {
                    description: "Declare variable before use".to_string(),
                    action: FixAction::InsertBefore {
                        line: "current".to_string(),
                        text: "let VARIABLE_NAME = /* initial value */;".to_string(),
                    },
                    confidence: 0.60,
                    auto_apply: false,
                },
                FixTemplate {
                    description: "Check for typos in variable name".to_string(),
                    action: FixAction::Suggest {
                        message: "Review similar variable names in scope".to_string(),
                    },
                    confidence: 0.40,
                    auto_apply: false,
                },
            ],
        );

        Self { fix_templates }
    }

    pub fn generate_suggestions(
        &self,
        errors: &[CompilerError],
        patterns: &[DetectedPattern],
    ) -> Vec<FixSuggestion> {
        let mut suggestions = Vec::new();

        for pattern in patterns {
            if let Some(templates) = self.fix_templates.get(&pattern.pattern.id) {
                for template in templates {
                    for &error_idx in &pattern.instances {
                        if let Some(error) = errors.get(error_idx) {
                            suggestions.push(FixSuggestion {
                                error_index: error_idx,
                                template: template.clone(),
                                context: self.extract_context(error),
                            });
                        }
                    }
                }
            }
        }

        suggestions.sort_by(|a, b| {
            b.template
                .confidence
                .partial_cmp(&a.template.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        suggestions
    }

    fn extract_context(&self, error: &CompilerError) -> FixContext {
        let context = match error {
            CompilerError::Syntax { context } => context,
            CompilerError::Type { context } => context,
            CompilerError::Memory { context } => context,
            CompilerError::Codegen { context } => context,
            CompilerError::IO { context } => context,
            CompilerError::Runtime { context } => context,
            CompilerError::Validation { context } => context,
            CompilerError::Module { context } => context,
            CompilerError::Testing { context } => context,
            CompilerError::LexError(_) => {
                // LexError doesn't have ErrorContext, return minimal context
                return FixContext {
                    location: None,
                    surrounding_code: None,
                    variable_scope: Vec::new(),
                };
            }
        };

        FixContext {
            location: context.location.clone(),
            surrounding_code: None,     // TODO: Extract from source
            variable_scope: Vec::new(), // TODO: Extract from semantic analysis
        }
    }
}

/// Analyzer for correlating related errors
pub struct ErrorCorrelator;

impl ErrorCorrelator {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze_correlations(&self, errors: &[CompilerError]) -> Vec<ErrorCorrelation> {
        let mut correlations = Vec::new();

        // Find cascading errors (errors caused by other errors)
        correlations.extend(self.find_cascading_errors(errors));

        // Find grouped errors (errors in same function/class)
        correlations.extend(self.find_grouped_errors(errors));

        // Find pattern-based correlations
        correlations.extend(self.find_pattern_correlations(errors));

        correlations
    }

    fn find_cascading_errors(&self, errors: &[CompilerError]) -> Vec<ErrorCorrelation> {
        let mut correlations = Vec::new();

        for (i, error) in errors.iter().enumerate() {
            if let Some(source_location) = self.get_error_location(error) {
                for (j, other_error) in errors.iter().enumerate() {
                    if i != j {
                        if let Some(other_location) = self.get_error_location(other_error) {
                            // Check if errors are on adjacent lines (potential cascade)
                            if source_location.file == other_location.file
                                && (source_location.line as i32 - other_location.line as i32).abs()
                                    <= 2
                            {
                                correlations.push(ErrorCorrelation {
                                    correlation_type: CorrelationType::Cascade,
                                    primary_error: i,
                                    related_errors: vec![j],
                                    confidence: 0.7,
                                    description: "Errors may be related due to proximity"
                                        .to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        correlations
    }

    fn find_grouped_errors(&self, errors: &[CompilerError]) -> Vec<ErrorCorrelation> {
        let mut file_groups: HashMap<String, Vec<usize>> = HashMap::new();

        // Group errors by file
        for (i, error) in errors.iter().enumerate() {
            if let Some(location) = self.get_error_location(error) {
                let file = &location.file;
                file_groups.entry(file.clone()).or_default().push(i);
            }
        }

        let mut correlations = Vec::new();
        for (file, indices) in file_groups {
            if indices.len() > 1 {
                correlations.push(ErrorCorrelation {
                    correlation_type: CorrelationType::Grouped,
                    primary_error: indices[0],
                    related_errors: indices[1..].to_vec(),
                    confidence: 0.5,
                    description: format!("Multiple errors in file: {}", file),
                });
            }
        }

        correlations
    }

    fn find_pattern_correlations(&self, _errors: &[CompilerError]) -> Vec<ErrorCorrelation> {
        // TODO: Implement pattern-based correlation analysis
        Vec::new()
    }

    fn get_error_location<'a>(&self, error: &'a CompilerError) -> Option<&'a SourceLocation> {
        let context = match error {
            CompilerError::Syntax { context } => context,
            CompilerError::Type { context } => context,
            CompilerError::Memory { context } => context,
            CompilerError::Codegen { context } => context,
            CompilerError::IO { context } => context,
            CompilerError::Runtime { context } => context,
            CompilerError::Validation { context } => context,
            CompilerError::Module { context } => context,
            CompilerError::Testing { context } => context,
            CompilerError::LexError(_) => return None,
        };

        context.location.as_ref()
    }
}

/// Statistics collector for error patterns and trends
#[derive(Debug, Clone)]
pub struct ErrorStatistics {
    pub total_errors: usize,
    pub error_types: HashMap<String, usize>,
    pub error_locations: HashMap<String, usize>,
    pub most_common_patterns: Vec<(String, usize)>,
}

impl ErrorStatistics {
    pub fn new() -> Self {
        Self {
            total_errors: 0,
            error_types: HashMap::new(),
            error_locations: HashMap::new(),
            most_common_patterns: Vec::new(),
        }
    }

    pub fn update(&mut self, errors: &[CompilerError]) {
        self.total_errors += errors.len();

        for error in errors {
            // Count error types
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
            };

            *self.error_types.entry(error_type.to_string()).or_insert(0) += 1;

            // Count error locations
            if let Some(location) = self.get_error_location(error) {
                let file = &location.file;
                *self.error_locations.entry(file.clone()).or_insert(0) += 1;
            }
        }
    }

    fn get_error_location<'a>(&self, error: &'a CompilerError) -> Option<&'a SourceLocation> {
        let context = match error {
            CompilerError::Syntax { context } => context,
            CompilerError::Type { context } => context,
            CompilerError::Memory { context } => context,
            CompilerError::Codegen { context } => context,
            CompilerError::IO { context } => context,
            CompilerError::Runtime { context } => context,
            CompilerError::Validation { context } => context,
            CompilerError::Module { context } => context,
            CompilerError::Testing { context } => context,
            CompilerError::LexError(_) => return None,
        };

        context.location.as_ref()
    }
}

// Supporting data structures

#[derive(Debug, Clone)]
pub struct ErrorPattern {
    pub id: String,
    pub description: String,
    pub signature: Vec<String>,
    pub frequency: f64, // 0.0 to 1.0
    pub fix_complexity: FixComplexity,
    pub auto_fixable: bool,
}

#[derive(Debug, Clone)]
pub enum FixComplexity {
    Trivial,  // Single character/token fix
    Simple,   // Single line fix
    Moderate, // Multiple lines, single concept
    Complex,  // Requires understanding of broader context
}

#[derive(Debug, Clone)]
pub struct DetectedPattern {
    pub pattern: ErrorPattern,
    pub instances: Vec<usize>, // Error indices
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct FixTemplate {
    pub description: String,
    pub action: FixAction,
    pub confidence: f64,
    pub auto_apply: bool,
}

#[derive(Debug, Clone)]
pub enum FixAction {
    Insert { position: String, text: String },
    Replace { from: String, to: String },
    Delete { target: String },
    InsertBefore { line: String, text: String },
    InsertAfter { line: String, text: String },
    Suggest { message: String },
}

#[derive(Debug, Clone)]
pub struct FixSuggestion {
    pub error_index: usize,
    pub template: FixTemplate,
    pub context: FixContext,
}

/// Actionable fix suggestion with precise location and replacement text
/// This can be used by IDEs/editors to apply quick fixes
#[derive(Debug, Clone)]
pub struct ActionableFix {
    /// Human-readable description of the fix
    pub message: String,
    /// The exact text to replace (None means insert)
    pub old_text: Option<String>,
    /// The replacement text to insert
    pub new_text: String,
    /// Start line (1-indexed)
    pub start_line: usize,
    /// Start column (1-indexed)
    pub start_column: usize,
    /// End line (1-indexed, inclusive)
    pub end_line: usize,
    /// End column (1-indexed, inclusive)
    pub end_column: usize,
    /// Confidence score (0.0 to 1.0) - higher means more likely to be correct
    pub confidence: f64,
    /// Whether this fix can be auto-applied safely
    pub auto_applicable: bool,
}

impl ActionableFix {
    /// Create an insertion fix (adds text at a position)
    pub fn insert(
        message: impl Into<String>,
        new_text: impl Into<String>,
        line: usize,
        column: usize,
        confidence: f64,
    ) -> Self {
        Self {
            message: message.into(),
            old_text: None,
            new_text: new_text.into(),
            start_line: line,
            start_column: column,
            end_line: line,
            end_column: column,
            confidence,
            auto_applicable: confidence >= 0.9,
        }
    }

    /// Create a replacement fix (replaces text at a range)
    pub fn replace(
        message: impl Into<String>,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
        confidence: f64,
    ) -> Self {
        Self {
            message: message.into(),
            old_text: Some(old_text.into()),
            new_text: new_text.into(),
            start_line,
            start_column,
            end_line,
            end_column,
            confidence,
            auto_applicable: confidence >= 0.9,
        }
    }

    /// Create a deletion fix (removes text at a range)
    pub fn delete(
        message: impl Into<String>,
        old_text: impl Into<String>,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
        confidence: f64,
    ) -> Self {
        Self {
            message: message.into(),
            old_text: Some(old_text.into()),
            new_text: String::new(),
            start_line,
            start_column,
            end_line,
            end_column,
            confidence,
            auto_applicable: false, // Deletions are risky, don't auto-apply
        }
    }
}

/// Generator for actionable fixes based on error analysis
pub struct ActionableFixGenerator;

impl ActionableFixGenerator {
    /// Generate actionable fixes for common error patterns
    pub fn generate_fixes(error: &CompilerError, source: &str) -> Vec<ActionableFix> {
        let mut fixes = Vec::new();

        let (context, error_type) = match error {
            CompilerError::Syntax { context } => (context, "syntax"),
            CompilerError::Type { context } => (context, "type"),
            CompilerError::Validation { context } => (context, "validation"),
            _ => return fixes,
        };

        let location = match &context.location {
            Some(loc) => loc,
            None => return fixes,
        };

        let message = &context.message;

        // Generate fixes based on error patterns
        match error_type {
            "syntax" => {
                fixes.extend(Self::generate_syntax_fixes(message, location, source));
            }
            "type" => {
                fixes.extend(Self::generate_type_fixes(
                    message, location, source, context,
                ));
            }
            "validation" => {
                fixes.extend(Self::generate_validation_fixes(message, location, source));
            }
            _ => {}
        }

        // Sort by confidence (highest first)
        fixes.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        fixes
    }

    fn generate_syntax_fixes(
        message: &str,
        location: &SourceLocation,
        source: &str,
    ) -> Vec<ActionableFix> {
        let mut fixes = Vec::new();

        // Missing closing parenthesis
        if message.contains("Expected ')'") || message.contains("missing )") {
            fixes.push(ActionableFix::insert(
                "Add missing closing parenthesis",
                ")",
                location.line,
                Self::get_line_end_column(source, location.line),
                0.85,
            ));
        }

        // Missing closing bracket
        if message.contains("Expected ']'") || message.contains("missing ]") {
            fixes.push(ActionableFix::insert(
                "Add missing closing bracket",
                "]",
                location.line,
                Self::get_line_end_column(source, location.line),
                0.85,
            ));
        }

        // Missing closing brace
        if message.contains("Expected '}'") || message.contains("missing }") {
            fixes.push(ActionableFix::insert(
                "Add missing closing brace",
                "}",
                location.line,
                Self::get_line_end_column(source, location.line),
                0.80,
            ));
        }

        // Traditional function syntax suggestions
        if message.contains("only available as a method") {
            if let Some(func_name) = Self::extract_function_name(message) {
                fixes.push(ActionableFix {
                    message: format!("Convert to method syntax: object.{}()", func_name),
                    old_text: Some(format!("{}(", func_name)),
                    new_text: format!("object.{}(", func_name),
                    start_line: location.line,
                    start_column: location.column,
                    end_line: location.line,
                    end_column: location.column + func_name.len() + 1,
                    confidence: 0.70,
                    auto_applicable: false, // Needs user to specify object
                });
            }
        }

        // 'func' instead of 'function'
        if message.contains("expected 'function'")
            || (message.contains("unexpected") && message.contains("func"))
        {
            if let Some((line_content, col)) = Self::find_in_line(source, location.line, "func ") {
                if !line_content.contains("function") {
                    fixes.push(ActionableFix::replace(
                        "Use 'function' keyword instead of 'func'",
                        "func",
                        "function",
                        location.line,
                        col,
                        location.line,
                        col + 4,
                        0.95,
                    ));
                }
            }
        }

        // 'let' variable declaration
        if message.contains("unexpected 'let'") || message.contains("use explicit types") {
            if let Some((_, col)) = Self::find_in_line(source, location.line, "let ") {
                fixes.push(ActionableFix {
                    message: "Use explicit type instead of 'let' (e.g., 'integer x = 5')"
                        .to_string(),
                    old_text: Some("let".to_string()),
                    new_text: "integer".to_string(), // Default suggestion
                    start_line: location.line,
                    start_column: col,
                    end_line: location.line,
                    end_column: col + 3,
                    confidence: 0.60,
                    auto_applicable: false,
                });
            }
        }

        fixes
    }

    fn generate_type_fixes(
        message: &str,
        location: &SourceLocation,
        _source: &str,
        context: &ErrorContext,
    ) -> Vec<ActionableFix> {
        let mut fixes = Vec::new();

        // Type mismatch fixes
        if message.contains("Type mismatch") || message.contains("Incompatible types") {
            // Check if we have type info in help text
            if let Some(help) = &context.help {
                if help.contains("Expected type") && help.contains("but found") {
                    // Extract types from help message
                    if let (Some(expected), Some(actual)) = (
                        Self::extract_type_from_help(help, "Expected type '", "'"),
                        Self::extract_type_from_help(help, "but found '", "'"),
                    ) {
                        // Suggest type conversion
                        if expected == "integer" && actual == "number" {
                            fixes.push(ActionableFix {
                                message: "Convert number to integer using .toInteger()".to_string(),
                                old_text: None,
                                new_text: ".toInteger()".to_string(),
                                start_line: location.line,
                                start_column: location.column,
                                end_line: location.line,
                                end_column: location.column,
                                confidence: 0.75,
                                auto_applicable: false,
                            });
                        } else if expected == "number" && actual == "integer" {
                            fixes.push(ActionableFix {
                                message: "Convert integer to number using .toNumber()".to_string(),
                                old_text: None,
                                new_text: ".toNumber()".to_string(),
                                start_line: location.line,
                                start_column: location.column,
                                end_line: location.line,
                                end_column: location.column,
                                confidence: 0.75,
                                auto_applicable: false,
                            });
                        } else if expected == "string" {
                            fixes.push(ActionableFix {
                                message: "Convert to string using .toString()".to_string(),
                                old_text: None,
                                new_text: ".toString()".to_string(),
                                start_line: location.line,
                                start_column: location.column,
                                end_line: location.line,
                                end_column: location.column,
                                confidence: 0.70,
                                auto_applicable: false,
                            });
                        }
                    }
                }
            }
        }

        // Variable not found
        if message.contains("Variable") && message.contains("not found") {
            if let Some(var_name) = Self::extract_quoted_name(message) {
                fixes.push(ActionableFix {
                    message: format!("Declare variable '{}' before use", var_name),
                    old_text: None,
                    new_text: format!("integer {} = 0\n", var_name),
                    start_line: location.line,
                    start_column: 1,
                    end_line: location.line,
                    end_column: 1,
                    confidence: 0.50,
                    auto_applicable: false,
                });
            }
        }

        // Function not found - use similar names from suggestions
        if message.contains("Function") && message.contains("not found") {
            for suggestion in &context.suggestions {
                if suggestion.contains("Did you mean") {
                    if let Some(suggested_name) = Self::extract_quoted_name(suggestion) {
                        if let Some(original_name) = Self::extract_quoted_name(message) {
                            fixes.push(ActionableFix::replace(
                                format!("Replace '{}' with '{}'", original_name, suggested_name),
                                original_name.clone(),
                                suggested_name,
                                location.line,
                                location.column,
                                location.line,
                                location.column + original_name.len(),
                                0.80,
                            ));
                        }
                    }
                }
            }
        }

        fixes
    }

    fn generate_validation_fixes(
        message: &str,
        location: &SourceLocation,
        _source: &str,
    ) -> Vec<ActionableFix> {
        let mut fixes = Vec::new();

        // Unused variable warning
        if message.contains("declared but never used") {
            if let Some(var_name) = Self::extract_quoted_name(message) {
                fixes.push(ActionableFix {
                    message: format!(
                        "Prefix with underscore to mark as intentionally unused: _{}",
                        var_name
                    ),
                    old_text: Some(var_name.clone()),
                    new_text: format!("_{}", var_name),
                    start_line: location.line,
                    start_column: location.column,
                    end_line: location.line,
                    end_column: location.column + var_name.len(),
                    confidence: 0.90,
                    auto_applicable: true,
                });
            }
        }

        fixes
    }

    // Helper functions
    fn get_line_end_column(source: &str, line: usize) -> usize {
        source
            .lines()
            .nth(line.saturating_sub(1))
            .map(|l| l.len() + 1)
            .unwrap_or(1)
    }

    fn find_in_line(source: &str, line: usize, pattern: &str) -> Option<(String, usize)> {
        source
            .lines()
            .nth(line.saturating_sub(1))
            .and_then(|line_content| {
                line_content
                    .find(pattern)
                    .map(|pos| (line_content.to_string(), pos + 1))
            })
    }

    fn extract_function_name(message: &str) -> Option<String> {
        // Extract function name from messages like "Function 'length' is only available as a method"
        Self::extract_quoted_name(message)
    }

    fn extract_quoted_name(text: &str) -> Option<String> {
        let start = text.find('\'')?;
        let end = text[start + 1..].find('\'')?;
        Some(text[start + 1..start + 1 + end].to_string())
    }

    fn extract_type_from_help(help: &str, prefix: &str, suffix: &str) -> Option<String> {
        let start = help.find(prefix)? + prefix.len();
        let rest = &help[start..];
        let end = rest.find(suffix)?;
        Some(rest[..end].to_string())
    }
}

#[derive(Debug, Clone)]
pub struct FixContext {
    pub location: Option<SourceLocation>,
    pub surrounding_code: Option<String>,
    pub variable_scope: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ErrorCorrelation {
    pub correlation_type: CorrelationType,
    pub primary_error: usize,
    pub related_errors: Vec<usize>,
    pub confidence: f64,
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum CorrelationType {
    Cascade, // One error causes another
    Grouped, // Errors in same scope/file
    Pattern, // Errors following same pattern
}

#[derive(Debug, Clone)]
pub struct SeverityAnalysis {
    pub critical_errors: usize,
    pub warnings: usize,
    pub blocking_errors: Vec<CompilerError>,
    pub compilation_viable: bool,
}

#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub total_errors: usize,
    pub patterns: Vec<DetectedPattern>,
    pub correlations: Vec<ErrorCorrelation>,
    pub suggestions: Vec<FixSuggestion>,
    pub severity_analysis: SeverityAnalysis,
    pub statistics: ErrorStatistics,
}

impl Default for DiagnosticSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actionable_fix_insert() {
        let fix = ActionableFix::insert("Add missing parenthesis", ")", 10, 25, 0.95);

        assert_eq!(fix.message, "Add missing parenthesis");
        assert_eq!(fix.new_text, ")");
        assert!(fix.old_text.is_none());
        assert_eq!(fix.start_line, 10);
        assert_eq!(fix.start_column, 25);
        assert!(fix.auto_applicable); // confidence >= 0.9
    }

    #[test]
    fn test_actionable_fix_replace() {
        let fix = ActionableFix::replace(
            "Replace func with function",
            "func",
            "function",
            5,
            1,
            5,
            5,
            0.95,
        );

        assert_eq!(fix.message, "Replace func with function");
        assert_eq!(fix.old_text, Some("func".to_string()));
        assert_eq!(fix.new_text, "function");
        assert_eq!(fix.start_line, 5);
        assert_eq!(fix.end_column, 5);
        assert!(fix.auto_applicable);
    }

    #[test]
    fn test_actionable_fix_delete() {
        let fix = ActionableFix::delete("Remove unused import", "import foo", 1, 1, 1, 11, 0.85);

        assert_eq!(fix.message, "Remove unused import");
        assert_eq!(fix.old_text, Some("import foo".to_string()));
        assert!(fix.new_text.is_empty());
        assert!(!fix.auto_applicable); // deletions are never auto-applicable
    }

    #[test]
    fn test_actionable_fix_low_confidence() {
        let fix = ActionableFix::insert("Maybe add this", "?", 1, 1, 0.5);

        assert!(!fix.auto_applicable); // confidence < 0.9
    }

    #[test]
    fn test_extract_quoted_name() {
        let text = "Variable 'myVar' not found";
        let result = ActionableFixGenerator::extract_quoted_name(text);
        assert_eq!(result, Some("myVar".to_string()));

        let text2 = "Function 'calculate' is undefined";
        let result2 = ActionableFixGenerator::extract_quoted_name(text2);
        assert_eq!(result2, Some("calculate".to_string()));

        let text3 = "No quotes here";
        let result3 = ActionableFixGenerator::extract_quoted_name(text3);
        assert!(result3.is_none());
    }

    #[test]
    fn test_generate_syntax_fixes_for_missing_paren() {
        let location = SourceLocation {
            line: 5,
            column: 10,
            file: "test.cln".to_string(),
        };

        let source = "line1\nline2\nline3\nline4\nprint(x\nline6";
        let message = "Expected ')' at end of function call";

        let fixes = ActionableFixGenerator::generate_syntax_fixes(message, &location, source);

        assert!(!fixes.is_empty());
        assert!(fixes[0].message.contains("parenthesis"));
        assert_eq!(fixes[0].new_text, ")");
    }

    #[test]
    fn test_diagnostic_system_creation() {
        let system = DiagnosticSystem::new();
        let report = system.error_patterns.detect_patterns(&[]);
        assert!(report.is_empty());
    }

    #[test]
    fn test_error_pattern_database() {
        let db = ErrorPatternDatabase::new();

        // Verify patterns are loaded
        assert!(!db.patterns.is_empty());

        // Should find at least the common patterns
        let pattern_ids: Vec<&str> = db.patterns.iter().map(|p| p.id.as_str()).collect();
        assert!(pattern_ids.contains(&"missing_semicolon"));
        assert!(pattern_ids.contains(&"unmatched_braces"));
        assert!(pattern_ids.contains(&"undefined_variable"));
        assert!(pattern_ids.contains(&"type_mismatch"));
    }

    #[test]
    fn test_fix_suggestion_engine() {
        let engine = FixSuggestionEngine::new();

        // Verify fix templates are loaded
        assert!(engine.fix_templates.contains_key("missing_semicolon"));
        assert!(engine.fix_templates.contains_key("unmatched_braces"));
        assert!(engine.fix_templates.contains_key("undefined_variable"));
    }

    #[test]
    fn test_error_statistics() {
        let mut stats = ErrorStatistics::new();

        let errors = vec![
            CompilerError::syntax_error("test syntax error", None, None),
            CompilerError::type_error("test type error", None, None),
            CompilerError::syntax_error("another syntax error", None, None),
        ];

        stats.update(&errors);

        assert_eq!(stats.total_errors, 3);
        assert_eq!(stats.error_types.get("syntax"), Some(&2));
        assert_eq!(stats.error_types.get("type"), Some(&1));
    }
}
