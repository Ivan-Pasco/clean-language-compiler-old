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
