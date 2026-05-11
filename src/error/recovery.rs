//! Error recovery mechanisms for the Clean Language compiler
//!
//! This module provides comprehensive error recovery strategies for each compilation phase.

use super::*;
use std::collections::HashMap;

/// Error recovery strategies for different compilation phases
#[derive(Debug, Clone)]
pub struct ErrorRecovery {
    /// Strategy for lexical analysis errors
    lexer_recovery: LexerRecovery,
    /// Strategy for parser errors
    parser_recovery: ParserRecovery,
    /// Strategy for semantic analysis errors
    semantic_recovery: SemanticRecovery,
    /// Strategy for code generation errors
    codegen_recovery: CodegenRecovery,
}

impl ErrorRecovery {
    pub fn new() -> Self {
        Self {
            lexer_recovery: LexerRecovery::new(),
            parser_recovery: ParserRecovery::new(),
            semantic_recovery: SemanticRecovery::new(),
            codegen_recovery: CodegenRecovery::new(),
        }
    }

    /// Attempt to recover from a compiler error
    pub fn recover_from_error(&mut self, error: &CompilerError) -> RecoveryResult {
        match error {
            CompilerError::Syntax { context } => match context.error_type {
                ErrorType::Syntax => self.lexer_recovery.recover(error),
                _ => self.parser_recovery.recover(error),
            },
            CompilerError::Type { .. } | CompilerError::Validation { .. } => {
                self.semantic_recovery.recover(error)
            }
            CompilerError::Codegen { .. } => self.codegen_recovery.recover(error),
            _ => RecoveryResult::no_recovery(),
        }
    }
}

/// Result of an error recovery attempt
#[derive(Debug, Clone)]
pub struct RecoveryResult {
    /// Whether recovery was successful
    pub recovered: bool,
    /// Suggested actions to continue compilation
    pub actions: Vec<RecoveryAction>,
    /// Additional error context discovered during recovery
    pub additional_errors: Vec<CompilerError>,
    /// Confidence level in the recovery (0.0 to 1.0)
    pub confidence: f64,
}

impl RecoveryResult {
    pub fn success(actions: Vec<RecoveryAction>, confidence: f64) -> Self {
        Self {
            recovered: true,
            actions,
            additional_errors: Vec::new(),
            confidence,
        }
    }

    pub fn partial(
        actions: Vec<RecoveryAction>,
        errors: Vec<CompilerError>,
        confidence: f64,
    ) -> Self {
        Self {
            recovered: true,
            actions,
            additional_errors: errors,
            confidence,
        }
    }

    pub fn no_recovery() -> Self {
        Self {
            recovered: false,
            actions: Vec::new(),
            additional_errors: Vec::new(),
            confidence: 0.0,
        }
    }
}

/// Actions the compiler can take to recover from errors
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Skip to next synchronization point
    SkipToSync(SyncPoint),
    /// Insert missing token
    InsertToken(String),
    /// Replace incorrect token
    ReplaceToken { from: String, to: String },
    /// Assume default value
    AssumeDefault(String),
    /// Continue with reduced functionality
    ContinueReduced,
    /// Suggest user fix
    SuggestFix(String),
}

/// Synchronization points for error recovery
#[derive(Debug, Clone)]
pub enum SyncPoint {
    NextStatement,
    NextFunction,
    NextClass,
    NextBlock,
    EndOfFile,
}

/// Lexer error recovery
#[derive(Debug, Clone)]
pub struct LexerRecovery {
    /// Common character substitutions for recovery
    #[allow(dead_code)] // Populated at construction; recovery not yet invoked from the parser
    char_substitutions: HashMap<char, Vec<char>>,
}

impl LexerRecovery {
    pub fn new() -> Self {
        let mut char_substitutions = HashMap::new();

        // Common typos and their corrections
        char_substitutions.insert('0', vec!['O', 'o']);
        char_substitutions.insert('1', vec!['l', 'I']);
        char_substitutions.insert('5', vec!['S']);
        char_substitutions.insert('8', vec!['B']);

        Self { char_substitutions }
    }

    pub fn recover(&mut self, error: &CompilerError) -> RecoveryResult {
        // Extract error message to understand the issue
        let error_msg = error.to_string();

        if error_msg.contains("Invalid character") {
            self.recover_invalid_character(error)
        } else if error_msg.contains("Unterminated string") {
            self.recover_unterminated_string(error)
        } else if error_msg.contains("Invalid number") {
            self.recover_invalid_number(error)
        } else {
            RecoveryResult::no_recovery()
        }
    }

    fn recover_invalid_character(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SkipToSync(SyncPoint::NextStatement),
                RecoveryAction::SuggestFix("Check for unsupported characters".to_string()),
            ],
            0.7,
        )
    }

    fn recover_unterminated_string(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::InsertToken("\"".to_string()),
                RecoveryAction::SuggestFix("Add closing quote to string literal".to_string()),
            ],
            0.9,
        )
    }

    fn recover_invalid_number(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::AssumeDefault("0".to_string()),
                RecoveryAction::SuggestFix(
                    "Check number format (integers or decimals only)".to_string(),
                ),
            ],
            0.6,
        )
    }
}

/// Parser error recovery with advanced recovery strategies
#[derive(Debug, Clone)]
pub struct ParserRecovery {
    /// Common syntax error patterns and their recovery strategies
    error_patterns: HashMap<String, RecoveryStrategy>,
    /// Track recovery success rates for adaptive learning
    recovery_stats: HashMap<String, RecoveryStats>,
    /// Maximum depth for nested recovery attempts
    max_recovery_depth: usize,
    /// Current recovery depth
    current_depth: usize,
}

#[derive(Debug, Clone)]
struct RecoveryStrategy {
    actions: Vec<RecoveryAction>,
    confidence: f64,
}

#[derive(Debug, Clone)]
struct RecoveryStats {
    attempted: usize,
    successful: usize,
    last_confidence: f64,
}

impl ParserRecovery {
    pub fn new() -> Self {
        let mut error_patterns = HashMap::new();

        // Define recovery strategies for common error patterns
        error_patterns.insert(
            "expected identifier".to_string(),
            RecoveryStrategy {
                actions: vec![
                    RecoveryAction::SuggestFix("Use a valid identifier (start with letter, contain only letters, numbers, underscore)".to_string()),
                    RecoveryAction::SkipToSync(SyncPoint::NextStatement),
                ],
                confidence: 0.8,
            }
        );

        error_patterns.insert(
            "expected \")\"".to_string(),
            RecoveryStrategy {
                actions: vec![
                    RecoveryAction::InsertToken(")".to_string()),
                    RecoveryAction::SuggestFix("Add missing closing parenthesis".to_string()),
                ],
                confidence: 0.9,
            },
        );

        error_patterns.insert(
            "expected indented block".to_string(),
            RecoveryStrategy {
                actions: vec![
                    RecoveryAction::SuggestFix(
                        "Add indented block using tabs or spaces".to_string(),
                    ),
                    RecoveryAction::AssumeDefault("\t// Missing block".to_string()),
                ],
                confidence: 0.7,
            },
        );

        error_patterns.insert(
            "expected function body".to_string(),
            RecoveryStrategy {
                actions: vec![
                    RecoveryAction::AssumeDefault(
                        "\treturn // function body placeholder".to_string(),
                    ),
                    RecoveryAction::SuggestFix(
                        "Add function body with proper indentation".to_string(),
                    ),
                ],
                confidence: 0.6,
            },
        );

        Self {
            error_patterns,
            recovery_stats: HashMap::new(),
            max_recovery_depth: 5,
            current_depth: 0,
        }
    }

    pub fn recover(&mut self, error: &CompilerError) -> RecoveryResult {
        // Prevent infinite recovery loops
        if self.current_depth >= self.max_recovery_depth {
            return RecoveryResult::no_recovery();
        }

        self.current_depth += 1;
        let result = self.recover_internal(error);
        self.current_depth -= 1;

        // Update recovery statistics
        self.update_recovery_stats(error, &result);

        result
    }

    fn recover_internal(&mut self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();

        // Try pattern-based recovery first (most specific)
        for (pattern, strategy) in &self.error_patterns {
            if error_msg.contains(pattern) {
                return self.apply_recovery_strategy(pattern, strategy, error);
            }
        }

        // Fall back to general recovery strategies
        if error_msg.contains("Expected") {
            self.recover_expected_token(error)
        } else if error_msg.contains("Unexpected") {
            self.recover_unexpected_token(error)
        } else if error_msg.contains("Missing") {
            self.recover_missing_construct(error)
        } else if error_msg.contains("Invalid") {
            self.recover_invalid_construct(error)
        } else if error_msg.contains("Unterminated") {
            self.recover_unterminated_construct(error)
        } else {
            // Try contextual recovery based on error location and surrounding code
            self.recover_contextual(error)
        }
    }

    fn apply_recovery_strategy(
        &self,
        pattern: &str,
        strategy: &RecoveryStrategy,
        _error: &CompilerError,
    ) -> RecoveryResult {
        // Adjust confidence based on historical success rate
        let adjusted_confidence = if let Some(stats) = self.recovery_stats.get(pattern) {
            let success_rate = stats.successful as f64 / stats.attempted as f64;
            strategy.confidence * (0.5 + success_rate * 0.5) // Weight current confidence with success rate
        } else {
            strategy.confidence
        };

        RecoveryResult::success(strategy.actions.clone(), adjusted_confidence)
    }

    fn update_recovery_stats(&mut self, error: &CompilerError, result: &RecoveryResult) {
        let error_msg = error.to_string();
        for pattern in self.error_patterns.keys() {
            if error_msg.contains(pattern) {
                let stats = self
                    .recovery_stats
                    .entry(pattern.clone())
                    .or_insert(RecoveryStats {
                        attempted: 0,
                        successful: 0,
                        last_confidence: 0.0,
                    });

                stats.attempted += 1;
                if result.recovered {
                    stats.successful += 1;
                }
                stats.last_confidence = result.confidence;
                break;
            }
        }
    }

    fn recover_invalid_construct(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SkipToSync(SyncPoint::NextStatement),
                RecoveryAction::SuggestFix(
                    "Check syntax - this construct is not valid in Clean Language".to_string(),
                ),
            ],
            0.7,
        )
    }

    fn recover_unterminated_construct(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SuggestFix(
                    "Add missing closing delimiter (quote, bracket, parenthesis)".to_string(),
                ),
                RecoveryAction::SkipToSync(SyncPoint::NextStatement),
            ],
            0.8,
        )
    }

    fn recover_contextual(&self, error: &CompilerError) -> RecoveryResult {
        // Try to recover based on error context and location
        if let CompilerError::Syntax { context } = error {
            if let Some(location) = &context.location {
                // Analyze surrounding context to provide better recovery
                if location.line > 0 {
                    // This is a more sophisticated recovery attempt
                    return RecoveryResult::success(
                        vec![
                            RecoveryAction::SuggestFix(format!(
                                "Syntax error at line {}:{}. Check the Clean Language syntax guide",
                                location.line, location.column
                            )),
                            RecoveryAction::SkipToSync(SyncPoint::NextStatement),
                        ],
                        0.5,
                    );
                }
            }
        }

        RecoveryResult::no_recovery()
    }

    fn recover_expected_token(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SkipToSync(SyncPoint::NextStatement),
                RecoveryAction::SuggestFix("Check syntax around this location".to_string()),
            ],
            0.8,
        )
    }

    fn recover_unexpected_token(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SkipToSync(SyncPoint::NextStatement),
                RecoveryAction::SuggestFix("Remove or replace unexpected token".to_string()),
            ],
            0.7,
        )
    }

    fn recover_missing_construct(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::AssumeDefault("/* missing construct */".to_string()),
                RecoveryAction::SuggestFix("Add missing language construct".to_string()),
            ],
            0.5,
        )
    }
}

/// Semantic analysis error recovery with intelligent type inference
#[derive(Debug, Clone)]
pub struct SemanticRecovery {
    /// Default values for recovery by type
    default_types: HashMap<String, String>,
    /// Symbol table for recovery context
    known_symbols: HashMap<String, String>,
    /// Type compatibility matrix for smart conversions
    type_compatibility: HashMap<(String, String), f64>,
    /// Recovery confidence thresholds
    confidence_thresholds: RecoveryThresholds,
}

#[derive(Debug, Clone)]
struct RecoveryThresholds {
    high_confidence: f64,
    medium_confidence: f64,
    low_confidence: f64,
}

impl SemanticRecovery {
    pub fn new() -> Self {
        let mut default_types = HashMap::new();
        default_types.insert("integer".to_string(), "0".to_string());
        default_types.insert("number".to_string(), "0.0".to_string());
        default_types.insert("string".to_string(), "\"\"".to_string());
        default_types.insert("boolean".to_string(), "false".to_string());
        default_types.insert("list".to_string(), "[]".to_string());
        default_types.insert("array".to_string(), "[]".to_string());

        let mut type_compatibility = HashMap::new();
        // Define type conversion confidence levels
        type_compatibility.insert(("integer".to_string(), "number".to_string()), 0.9);
        type_compatibility.insert(("number".to_string(), "integer".to_string()), 0.7); // May lose precision
        type_compatibility.insert(("integer".to_string(), "string".to_string()), 0.8);
        type_compatibility.insert(("number".to_string(), "string".to_string()), 0.8);
        type_compatibility.insert(("boolean".to_string(), "string".to_string()), 0.8);
        type_compatibility.insert(("string".to_string(), "integer".to_string()), 0.5); // Parse required
        type_compatibility.insert(("string".to_string(), "number".to_string()), 0.5);
        type_compatibility.insert(("string".to_string(), "boolean".to_string()), 0.6);

        Self {
            default_types,
            known_symbols: HashMap::new(),
            type_compatibility,
            confidence_thresholds: RecoveryThresholds {
                high_confidence: 0.8,
                medium_confidence: 0.6,
                low_confidence: 0.4,
            },
        }
    }

    /// Add known symbol for better recovery context
    pub fn add_symbol(&mut self, name: String, type_name: String) {
        self.known_symbols.insert(name, type_name);
    }

    /// Get type compatibility confidence
    pub fn get_type_compatibility(&self, from_type: &str, to_type: &str) -> f64 {
        self.type_compatibility
            .get(&(from_type.to_string(), to_type.to_string()))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn recover(&mut self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();

        if error_msg.contains("Type mismatch") {
            self.recover_type_mismatch_intelligent(error)
        } else if error_msg.contains("Undefined") || error_msg.contains("not found") {
            self.recover_undefined_symbol_intelligent(error)
        } else if error_msg.contains("Incompatible") {
            self.recover_incompatible_types_intelligent(error)
        } else if error_msg.contains("Cannot convert") || error_msg.contains("conversion") {
            self.recover_type_conversion(error)
        } else if error_msg.contains("Expected type") {
            self.recover_expected_type(error)
        } else {
            RecoveryResult::no_recovery()
        }
    }

    fn recover_type_mismatch_intelligent(&self, error: &CompilerError) -> RecoveryResult {
        // Try to extract type information from error message
        let error_msg = error.to_string();
        let (expected_type, actual_type) = self.extract_type_info(&error_msg);

        if let (Some(expected), Some(actual)) = (expected_type, actual_type) {
            let compatibility = self.get_type_compatibility(&actual, &expected);

            let mut actions = vec![];
            let confidence;

            if compatibility >= self.confidence_thresholds.high_confidence {
                // High confidence conversion
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Add explicit conversion: {actual}.to{expected}() or use automatic conversion",
                )));
                confidence = compatibility;
            } else if compatibility >= self.confidence_thresholds.medium_confidence {
                // Medium confidence - suggest explicit conversion
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Types '{actual}' and '{expected}' may be compatible. Consider explicit conversion: {actual}.to{expected}()"
                )));
                confidence = compatibility;
            } else {
                // Low compatibility - suggest redesign
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Types '{actual}' and '{expected}' are incompatible. Consider using a compatible type or redesigning the logic"
                )));
                confidence = self.confidence_thresholds.low_confidence;
            }

            actions.push(RecoveryAction::ContinueReduced);
            return RecoveryResult::success(actions, confidence);
        }

        // Fall back to original recovery
        self.recover_type_mismatch(error)
    }

    fn recover_undefined_symbol_intelligent(&self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();

        // Try to extract symbol name from error
        if let Some(symbol_name) = self.extract_symbol_name(&error_msg) {
            // Check if we have similar known symbols
            let suggestions = self.suggest_similar_symbols(&symbol_name);

            let mut actions = vec![];
            let confidence = if !suggestions.is_empty() {
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Symbol '{symbol_name}' not found. Did you mean: {}?",
                    suggestions.join(", ")
                )));
                0.7
            } else {
                // Try to infer what kind of symbol this might be based on context
                let symbol_type = self.infer_symbol_type(&symbol_name);
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Define the symbol '{symbol_name}' (likely type: {symbol_type}) before use"
                )));
                0.5
            };

            actions.push(RecoveryAction::AssumeDefault(
                self.default_types
                    .get("string")
                    .unwrap_or(&"\"unknown\"".to_string())
                    .clone(),
            ));

            return RecoveryResult::success(actions, confidence);
        }

        // Fall back to original recovery
        self.recover_undefined_symbol(error)
    }

    fn recover_incompatible_types_intelligent(&self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();
        let (type1, type2) = self.extract_type_info(&error_msg);

        if let (Some(t1), Some(t2)) = (type1, type2) {
            let compat1to2 = self.get_type_compatibility(&t1, &t2);
            let compat2to1 = self.get_type_compatibility(&t2, &t1);

            let mut actions = vec![];
            let confidence;

            if compat1to2 >= self.confidence_thresholds.medium_confidence {
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Convert '{t1}' to '{t2}': use {t1}.to{t2}() method"
                )));
                confidence = compat1to2;
            } else if compat2to1 >= self.confidence_thresholds.medium_confidence {
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Convert '{t2}' to '{t1}': use {t2}.to{t1}() method"
                )));
                confidence = compat2to1;
            } else {
                actions.push(RecoveryAction::SuggestFix(format!(
                    "Types '{t1}' and '{t2}' are incompatible. Use a common type or explicit conversion"
                )));
                confidence = self.confidence_thresholds.low_confidence;
            }

            actions.push(RecoveryAction::ContinueReduced);
            return RecoveryResult::success(actions, confidence);
        }

        self.recover_incompatible_types(error)
    }

    fn recover_type_conversion(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SuggestFix(
                    "Use explicit type conversion methods: toInteger(), toString(), toBoolean(), toNumber()".to_string()
                ),
                RecoveryAction::ContinueReduced,
            ],
            0.7,
        )
    }

    fn recover_expected_type(&self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();
        let expected_type = self.extract_expected_type(&error_msg);

        let mut actions = vec![];
        if let Some(expected) = expected_type {
            if let Some(default_value) = self.default_types.get(&expected) {
                actions.push(RecoveryAction::AssumeDefault(default_value.clone()));
            }
            actions.push(RecoveryAction::SuggestFix(format!(
                "Provide a value of type '{expected}' or check variable declarations"
            )));
        } else {
            actions.push(RecoveryAction::SuggestFix(
                "Check type annotations and variable declarations".to_string(),
            ));
        }

        RecoveryResult::success(actions, 0.6)
    }

    /// Extract type information from error messages
    fn extract_type_info(&self, error_msg: &str) -> (Option<String>, Option<String>) {
        // Look for patterns like "Expected 'integer', found 'string'"
        if let Some(start) = error_msg.find("Expected '") {
            let after_expected = &error_msg[start + 10..];
            if let Some(end_expected) = after_expected.find("'") {
                let expected = after_expected[..end_expected].to_string();

                // Look for the "found" part
                if let Some(found_start) = error_msg.find(", found '") {
                    let after_found = &error_msg[found_start + 9..];
                    if let Some(end_found) = after_found.find("'") {
                        let found = after_found[..end_found].to_string();
                        return (Some(expected), Some(found));
                    }
                }
            }
        }

        // Look for patterns like "type 'string' but found 'integer'"
        if let Some(start) = error_msg.find("type '") {
            let after_type = &error_msg[start + 6..];
            if let Some(end_type) = after_type.find("'") {
                let type1 = after_type[..end_type].to_string();

                if let Some(found_start) = error_msg.find("found '") {
                    let after_found = &error_msg[found_start + 7..];
                    if let Some(end_found) = after_found.find("'") {
                        let type2 = after_found[..end_found].to_string();
                        return (Some(type1), Some(type2));
                    }
                }
            }
        }

        (None, None)
    }

    /// Extract symbol name from error message
    fn extract_symbol_name(&self, error_msg: &str) -> Option<String> {
        // Try different patterns for symbol names
        let patterns = ["Symbol '", "Variable '", "Function '"];

        for pattern in &patterns {
            if let Some(start) = error_msg.find(pattern) {
                let after_pattern = &error_msg[start + pattern.len()..];
                if let Some(end) = after_pattern.find("'") {
                    return Some(after_pattern[..end].to_string());
                }
            }
        }
        None
    }

    /// Extract expected type from error message
    fn extract_expected_type(&self, error_msg: &str) -> Option<String> {
        if let Some(start) = error_msg.find("Expected type '") {
            let after_pattern = &error_msg[start + 15..];
            if let Some(end) = after_pattern.find("'") {
                return Some(after_pattern[..end].to_string());
            }
        }
        None
    }

    /// Suggest similar symbols based on known symbols
    fn suggest_similar_symbols(&self, target: &str) -> Vec<String> {
        self.known_symbols
            .keys()
            .filter_map(|symbol| {
                let distance = self.levenshtein_distance(target, symbol);
                if distance <= 2 && distance > 0 {
                    Some(symbol.clone())
                } else {
                    None
                }
            })
            .take(3)
            .collect()
    }

    /// Simple Levenshtein distance calculation
    fn levenshtein_distance(&self, s1: &str, s2: &str) -> usize {
        let s1_chars: Vec<char> = s1.chars().collect();
        let s2_chars: Vec<char> = s2.chars().collect();
        let s1_len = s1_chars.len();
        let s2_len = s2_chars.len();

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

        for i in 1..=s1_len {
            for j in 1..=s2_len {
                let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                    0
                } else {
                    1
                };
                matrix[i][j] = std::cmp::min(
                    matrix[i - 1][j] + 1, // deletion
                    std::cmp::min(
                        matrix[i][j - 1] + 1,        // insertion
                        matrix[i - 1][j - 1] + cost, // substitution
                    ),
                );
            }
        }

        matrix[s1_len][s2_len]
    }

    /// Infer symbol type based on naming patterns
    fn infer_symbol_type(&self, symbol_name: &str) -> String {
        if symbol_name.ends_with("_count")
            || symbol_name.ends_with("_index")
            || symbol_name.starts_with("num_")
            || symbol_name.ends_with("_num")
        {
            "integer".to_string()
        } else if symbol_name.contains("_rate")
            || symbol_name.contains("_percent")
            || symbol_name.contains("_value") && !symbol_name.contains("_string")
        {
            "number".to_string()
        } else if symbol_name.contains("_flag")
            || symbol_name.starts_with("is_")
            || symbol_name.starts_with("has_")
            || symbol_name.starts_with("can_")
        {
            "boolean".to_string()
        } else if symbol_name.contains("_name")
            || symbol_name.contains("_text")
            || symbol_name.contains("_message")
            || symbol_name.ends_with("_str")
        {
            "string".to_string()
        } else if symbol_name.contains("_list")
            || symbol_name.ends_with("_items")
            || symbol_name.ends_with("s") && symbol_name.len() > 3
        {
            "list".to_string()
        } else {
            "unknown".to_string()
        }
    }

    fn recover_type_mismatch(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SuggestFix(
                    "Check type compatibility or add type conversion".to_string(),
                ),
                RecoveryAction::ContinueReduced,
            ],
            0.6,
        )
    }

    fn recover_undefined_symbol(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::AssumeDefault("undefined".to_string()),
                RecoveryAction::SuggestFix("Define the symbol before use".to_string()),
            ],
            0.4,
        )
    }

    fn recover_incompatible_types(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SuggestFix(
                    "Use compatible types or add explicit conversion".to_string(),
                ),
                RecoveryAction::ContinueReduced,
            ],
            0.5,
        )
    }
}

/// Code generation error recovery
#[derive(Debug, Clone)]
pub struct CodegenRecovery;

impl CodegenRecovery {
    pub fn new() -> Self {
        Self
    }

    pub fn recover(&mut self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();

        if error_msg.contains("WebAssembly") {
            self.recover_wasm_error(error)
        } else if error_msg.contains("Instruction") {
            self.recover_instruction_error(error)
        } else {
            RecoveryResult::no_recovery()
        }
    }

    fn recover_wasm_error(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SuggestFix("Check WebAssembly module structure".to_string()),
                RecoveryAction::ContinueReduced,
            ],
            0.3,
        )
    }

    fn recover_instruction_error(&self, _error: &CompilerError) -> RecoveryResult {
        RecoveryResult::success(
            vec![
                RecoveryAction::SuggestFix("Review generated instructions".to_string()),
                RecoveryAction::ContinueReduced,
            ],
            0.4,
        )
    }
}

impl Default for ErrorRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for LexerRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ParserRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SemanticRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for CodegenRecovery {
    fn default() -> Self {
        Self::new()
    }
}
