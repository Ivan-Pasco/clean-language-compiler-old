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

/// Parser error recovery
#[derive(Debug, Clone)]
pub struct ParserRecovery {
    /// Synchronization tokens for recovery
    sync_tokens: Vec<String>,
}

impl ParserRecovery {
    pub fn new() -> Self {
        Self {
            sync_tokens: vec![
                ";".to_string(),
                "{".to_string(),
                "}".to_string(),
                "function".to_string(),
                "class".to_string(),
                "start".to_string(),
            ],
        }
    }

    pub fn recover(&mut self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();

        if error_msg.contains("Expected") {
            self.recover_expected_token(error)
        } else if error_msg.contains("Unexpected") {
            self.recover_unexpected_token(error)
        } else if error_msg.contains("Missing") {
            self.recover_missing_construct(error)
        } else {
            RecoveryResult::no_recovery()
        }
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

/// Semantic analysis error recovery
#[derive(Debug, Clone)]
pub struct SemanticRecovery {
    /// Default types for recovery
    default_types: HashMap<String, String>,
}

impl SemanticRecovery {
    pub fn new() -> Self {
        let mut default_types = HashMap::new();
        default_types.insert("integer".to_string(), "0".to_string());
        default_types.insert("number".to_string(), "0.0".to_string());
        default_types.insert("string".to_string(), "\"\"".to_string());
        default_types.insert("boolean".to_string(), "false".to_string());

        Self { default_types }
    }

    pub fn recover(&mut self, error: &CompilerError) -> RecoveryResult {
        let error_msg = error.to_string();

        if error_msg.contains("Type mismatch") {
            self.recover_type_mismatch(error)
        } else if error_msg.contains("Undefined") {
            self.recover_undefined_symbol(error)
        } else if error_msg.contains("Incompatible") {
            self.recover_incompatible_types(error)
        } else {
            RecoveryResult::no_recovery()
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
