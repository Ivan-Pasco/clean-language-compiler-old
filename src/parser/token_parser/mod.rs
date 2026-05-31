//! Token-driven parser for Clean Language
//!
//! This parser consumes tokens directly from the lexer without source reconstruction,
//! following the architecture of rustc's parser (see rust-lang/rustc-dev-guide).
//!
//! Architecture:
//! - Maintains a cursor into the token stream
//! - Uses utility methods: bump(), check(), eat(), expect(), look_ahead()
//! - Recursive descent parsing
//! - Error recovery with diagnostic generation
//!
//! The implementation is split across submodules for maintainability:
//! - `declarations` — functions, classes, imports, parameters, types, blocks
//! - `statements`   — control flow, assignments, print, error, async stmts
//! - `expressions`  — full operator-precedence expression chain
//! - `blocks`       — framework blocks, tests, state, watch, plugins, external

mod blocks;
mod declarations;
mod expressions;
mod statements;

use crate::ast::{Class, Program};
use crate::error::CompilerError;
use crate::lexer::specification_token::{Token, TokenKind, TokenStream};
use std::collections::HashSet;
use tracing::{debug, trace};

/// Returns the ordering index for the 5 core sections, or None for non-core sections.
/// Order: import: (1) → start: (2) → state: (3) → class (4) → functions: (5)
///
/// Enforces the ordering rule documented in
/// `documentation/Clean_Language_Specification.md:1114-1134` and formalised in
/// `foundation/spec/grammar.ebnf:544`.
fn core_section_order(kind: &TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Import => Some(1),
        TokenKind::Start => Some(2),
        TokenKind::State => Some(3),
        TokenKind::Class => Some(4),
        TokenKind::Functions => Some(5),
        _ => None,
    }
}

/// Returns the human-readable name for a core section given its order index.
fn core_section_name(order: u8) -> &'static str {
    match order {
        1 => "import:",
        2 => "start:",
        3 => "state:",
        4 => "class",
        5 => "functions:",
        _ => "unknown",
    }
}

/// Token-driven parser for Clean Language
pub struct TokenParser {
    pub(super) tokens: Vec<Token>,
    pub(super) cursor: usize,
    pub(super) errors: Vec<CompilerError>,
    pub(super) paren_depth: usize, // Track parenthesis depth for multiline expression support
    pub(super) brace_depth: usize, // Track brace depth for multiline object literal support
    /// Plugin-defined keywords that don't require colons (e.g., "data" from frame.data)
    pub(super) plugin_keywords: HashSet<String>,
    /// Original source text for raw content extraction in framework blocks
    pub(super) source_content: String,
    /// When true, skip section ordering enforcement (used for plugin output parsing
    /// where generated sections may appear in any order)
    pub(super) lenient_section_order: bool,
    /// Synthetic classes generated from plugin sub-section blocks (e.g. `inputs:` → `Inputs`).
    /// Populated by `parse_class_body` and drained into `classes` after each `parse_class` call.
    pub(super) pending_synthetic_classes: Vec<Class>,
}

impl TokenParser {
    pub fn new(token_stream: TokenStream, _file_path: String) -> Self {
        Self {
            source_content: token_stream.source_content,
            tokens: token_stream.tokens,
            cursor: 0,
            errors: Vec::new(),
            paren_depth: 0,
            brace_depth: 0,
            plugin_keywords: HashSet::new(),
            lenient_section_order: false,
            pending_synthetic_classes: Vec::new(),
        }
    }

    /// Create a parser with plugin-defined keywords
    ///
    /// Plugin keywords are recognized as framework block starters even without
    /// a trailing colon. For example, with `plugin_keywords = ["data"]`:
    /// - `data User` is parsed as a framework block (plugin keyword)
    /// - `endpoints:` is parsed as a framework block (has colon)
    pub fn with_plugin_keywords(
        token_stream: TokenStream,
        _file_path: String,
        plugin_keywords: Vec<String>,
    ) -> Self {
        Self {
            source_content: token_stream.source_content,
            tokens: token_stream.tokens,
            cursor: 0,
            errors: Vec::new(),
            paren_depth: 0,
            brace_depth: 0,
            plugin_keywords: plugin_keywords.into_iter().collect(),
            lenient_section_order: false,
            pending_synthetic_classes: Vec::new(),
        }
    }

    /// Enable lenient section ordering (for plugin output parsing).
    /// When enabled, the 5 core sections can appear in any order.
    pub fn set_lenient_section_order(&mut self, lenient: bool) {
        self.lenient_section_order = lenient;
    }

    /// Try to extract a variable name from the current token.
    /// Accepts identifiers and contextual keywords that can be used as variable
    /// names (e.g., `rules`, `computed`, `state`, `guard`, `watch`, `reset`,
    /// `screen`, `source`, `build`, `spec`, `intent`).
    pub(super) fn try_extract_var_name(&self) -> Option<String> {
        match self.current_kind() {
            TokenKind::Identifier(name) => Some(name.clone()),
            TokenKind::Rules
            | TokenKind::Computed
            | TokenKind::State
            | TokenKind::Guard
            | TokenKind::Watch
            | TokenKind::Reset
            | TokenKind::Screen
            | TokenKind::Source
            | TokenKind::Build
            | TokenKind::Spec
            | TokenKind::Intent
            | TokenKind::Description
            | TokenKind::Input
            | TokenKind::Unit
            | TokenKind::Step
            | TokenKind::Test
            | TokenKind::Error => Some(self.current().text.clone()),
            _ => None,
        }
    }

    /// Parse a complete program
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        self.skip_whitespace();

        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut tests = Vec::new();
        let mut imports = Vec::new();
        let mut plugins = Vec::new();
        let mut statements = Vec::new();
        let screens = Vec::new(); // Legacy field; screen blocks are stored in screen_blocks below
        let mut screen_blocks_vec: Vec<crate::ast::Statement> = Vec::new();
        let mut state: Option<crate::ast::StateBlock> = None;
        let mut watch_blocks: Vec<crate::ast::WatchBlock> = Vec::new();
        let mut externals: Vec<crate::ast::ExternalFunction> = Vec::new();
        let mut source_block: Option<crate::ast::Statement> = None;

        // Track section ordering for the 5 core sections:
        // import: (1) → start: (2) → state: (3) → class (4) → functions: (5).
        // Per documentation/Clean_Language_Specification.md:1114 and
        // foundation/spec/grammar.ebnf:544, these must appear in order when present.
        let mut last_core_section: u8 = 0;

        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Skip any Dedent tokens at top level (they mark the end of blocks)
            if matches!(self.current_kind(), TokenKind::Dedent(_)) {
                trace!(cursor = self.cursor, "Skipping dedent token at top level");
                self.bump();
                trace!(cursor = self.cursor, token = ?self.current_kind(), "After bump");
                continue;
            }

            // Enforce section ordering for the 5 core sections (SYN007).
            // Skipped in lenient mode, used for plugin output parsing.
            if !self.lenient_section_order {
                if let Some(current_order) = core_section_order(self.current_kind()) {
                    if current_order < last_core_section {
                        let current_name = core_section_name(current_order);
                        let last_name = core_section_name(last_core_section);
                        let token = self.current();
                        use crate::error::{ErrorContext, ErrorType};
                        self.errors.push(CompilerError::Syntax {
                            context: Box::new(
                                ErrorContext::new(
                                    format!(
                                        "'{}' section is out of order — it must appear before '{}'",
                                        current_name, last_name
                                    ),
                                    Some(
                                        "Expected section order: import:, start:, state:, class, functions:"
                                            .to_string(),
                                    ),
                                    ErrorType::Syntax,
                                    Some(token.location.clone()),
                                )
                                .with_error_code("SYN007"),
                            ),
                        });
                    }
                    last_core_section = last_core_section.max(current_order);
                }
            }

            // Parse top-level declarations
            match self.current_kind() {
                TokenKind::Function => match self.parse_function() {
                    Ok(func) => functions.push(func),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Class => match self.parse_class() {
                    Ok(class) => {
                        classes.push(class);
                        // Drain any synthetic classes created from plugin sub-section blocks
                        // (e.g. `inputs:` inside a class body generates a synthetic `Inputs` class)
                        classes.append(&mut self.pending_synthetic_classes);
                    }
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Tests => {
                    debug!("Parsing tests: block");
                    match self.parse_tests_block() {
                        Ok(test_cases) => {
                            debug!(test_count = test_cases.len(), "Parsed tests block");
                            tests.extend(test_cases)
                        }
                        Err(e) => {
                            debug!(error = ?e, "Failed to parse tests block");
                            self.errors.push(e)
                        }
                    }
                }
                TokenKind::Import => match self.parse_import() {
                    Ok(mut import_items) => imports.append(&mut import_items),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Private => match self.parse_private() {
                    Ok(private_stmt) => statements.push(private_stmt),
                    Err(e) => self.errors.push(e),
                },
                TokenKind::Start => {
                    // Parse start() function (special case - no 'function' keyword)
                    match self.parse_start_function() {
                        Ok(func) => functions.push(func),
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Functions => {
                    // Parse functions: block
                    debug!("Parsing functions: block");
                    match self.parse_functions_block() {
                        Ok(mut block_functions) => {
                            debug!(
                                function_count = block_functions.len(),
                                next_token = ?self.current_kind(),
                                "Parsed functions block"
                            );
                            functions.append(&mut block_functions)
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::State => {
                    // Parse state: block
                    debug!("Parsing state: block");
                    match self.parse_state_block() {
                        Ok(state_block) => {
                            debug!(
                                declaration_count = state_block.declarations.len(),
                                "Parsed state block"
                            );
                            state = Some(state_block);
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Watch => {
                    // Parse watch: block (state change observer)
                    debug!("Parsing watch: block");
                    match self.parse_watch_block() {
                        Ok(watch_block) => {
                            debug!(
                                target_count = watch_block.targets.len(),
                                "Parsed watch block"
                            );
                            watch_blocks.push(watch_block);
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Screen => {
                    // Parse screen Name: block (UI screen with its own state scope)
                    debug!("Parsing screen: block");
                    match self.parse_screen_block() {
                        Ok(screen_stmt) => screen_blocks_vec.push(screen_stmt),
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Build => {
                    // Parse build: block (compiler configuration)
                    debug!("Parsing build: block");
                    match self.parse_build_block() {
                        Ok(build_stmt) => statements.push(build_stmt),
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Source => {
                    // Parse source: block (AI metadata)
                    debug!("Parsing source: block");
                    match self.parse_source_block() {
                        Ok(sb) => {
                            source_block = Some(sb);
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Plugins => {
                    // Parse plugins: block
                    debug!("Parsing plugins: block");
                    match self.parse_plugins_block() {
                        Ok(mut plugin_names) => {
                            debug!(plugin_count = plugin_names.len(), "Parsed plugins block");
                            plugins.append(&mut plugin_names);
                        }
                        Err(e) => self.errors.push(e),
                    }
                }
                TokenKind::Identifier(name) => {
                    // Check for special top-level blocks first
                    // "external:" or "external \"module\":" is a WASM import block
                    if name == "external" {
                        debug!("Parsing external: block for WASM imports");
                        match self.parse_external_block() {
                            Ok(mut external_fns) => externals.append(&mut external_fns),
                            Err(e) => self.errors.push(e),
                        }
                        continue;
                    }

                    // "validate schemaName:" is a named validation schema declaration.
                    // Pattern: Identifier("validate") ~ Identifier(name) ~ Colon ~ NEWLINE ~ fields
                    if name == "validate" {
                        debug!("Parsing validate: block");
                        match self.parse_validate_block() {
                            Ok(stmt) => statements.push(stmt),
                            Err(e) => self.errors.push(e),
                        }
                        continue;
                    }

                    // Check if this is a plugin keyword (e.g., "data" from frame.data plugin)
                    let is_plugin_keyword = self.plugin_keywords.contains(name);

                    // Check if this is a framework block
                    // Patterns:
                    // - "identifier:" (colon-based framework block)
                    // - "identifier string:" or "identifier identifier:" (with colon)
                    // - "plugin_keyword identifier" (plugin keyword without colon)
                    let is_framework_block = match self.peek_kind() {
                        Some(&TokenKind::Colon) => true,
                        Some(&TokenKind::StringLiteral(_)) | Some(&TokenKind::Identifier(_)) => {
                            // Plugin keyword followed by identifier: "data User"
                            if is_plugin_keyword {
                                true
                            } else {
                                // Check if second token is followed by colon
                                matches!(self.look_ahead(2).kind, TokenKind::Colon)
                            }
                        }
                        _ => false,
                    };

                    if is_framework_block {
                        debug!(block_name = %name, is_plugin_keyword, "Found framework block");
                        match self.parse_framework_block_or_plugin(is_plugin_keyword) {
                            Ok(stmt) => statements.push(stmt),
                            Err(e) => self.errors.push(e),
                        }
                    } else {
                        // Not a framework block, unexpected token
                        let token = self.current();
                        self.errors.push(CompilerError::parse_error(
                            format!("Unexpected identifier at top level: {:?}", name),
                            Some(token.location.clone()),
                            Some("Expected 'functions:', 'class', 'start:', or framework block (e.g., 'endpoints:', 'screen \"Name\":')".to_string()),
                        ));
                        self.bump();
                    }
                }
                _ => {
                    let token = self.current();
                    self.errors.push(CompilerError::parse_error(
                        format!("Unexpected token at top level: {:?}", token.kind),
                        Some(token.location.clone()),
                        None,
                    ));
                    self.bump(); // Skip unexpected token
                }
            }

            self.skip_whitespace();
        }

        if !self.errors.is_empty() {
            // Return first error with all subsequent errors as related_errors
            // This allows IDE tools to access all parser errors for better diagnostics
            let mut first_error = self.errors[0].clone();

            // Add remaining errors as related messages
            if self.errors.len() > 1 {
                // Extract the context and add related errors
                match &mut first_error {
                    CompilerError::Syntax { context }
                    | CompilerError::Type { context }
                    | CompilerError::Memory { context }
                    | CompilerError::Codegen { context }
                    | CompilerError::IO { context }
                    | CompilerError::Runtime { context }
                    | CompilerError::Validation { context }
                    | CompilerError::Module { context }
                    | CompilerError::Testing { context } => {
                        for error in &self.errors[1..] {
                            context.related_errors.push(error.to_string());
                        }
                    }
                    CompilerError::LexError(_) => {
                        // LexError doesn't have ErrorContext, skip related errors
                    }
                    CompilerError::PluginError { .. } => {
                        // PluginError doesn't have ErrorContext, skip related errors
                    }
                }
            }

            return Err(first_error);
        }

        let start_function = functions.iter().find(|f| f.name == "start").cloned();

        debug!(
            function_count = functions.len(),
            has_start = start_function.is_some(),
            test_count = tests.len(),
            class_count = classes.len(),
            "Program parsed successfully"
        );

        Ok(Program {
            imports,
            plugins,
            statements,
            functions,
            classes,
            start_function,
            tests,
            screens,
            state,
            watch_blocks,
            screen_blocks: screen_blocks_vec,
            externals,
            source_block,
            location: None,
        })
    }

    // ============================================================================
    // Token cursor utilities (following rustc pattern)
    // ============================================================================

    /// Get the current token without consuming it
    pub(super) fn current(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }

    /// Get the kind of the current token
    pub(super) fn current_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    /// Check if we're at EOF
    pub(super) fn is_at_end(&self) -> bool {
        matches!(self.current_kind(), TokenKind::Eof)
    }

    /// Peek at the next token kind without consuming
    pub(super) fn peek_kind(&self) -> Option<&TokenKind> {
        if self.cursor + 1 < self.tokens.len() {
            Some(&self.tokens[self.cursor + 1].kind)
        } else {
            None
        }
    }

    /// Consume the current token and advance (rustc: bump())
    pub(super) fn bump(&mut self) -> Token {
        let token = self.tokens[self.cursor].clone();
        if self.cursor < self.tokens.len() - 1 {
            self.cursor += 1;
        }
        token
    }

    /// Check if the current token matches without consuming (rustc: check())
    pub(super) fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(kind)
    }

    /// Consume the token if it matches, return whether it matched (rustc: eat())
    pub(super) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect a specific token, error if not found (rustc: expect())
    pub(super) fn expect(&mut self, kind: &TokenKind) -> Result<Token, CompilerError> {
        if self.check(kind) {
            Ok(self.bump())
        } else {
            let current = self.current();
            Err(CompilerError::parse_error(
                format!("Expected {:?}, found {:?}", kind, current.kind),
                Some(current.location.clone()),
                None,
            ))
        }
    }

    /// Look ahead at token N positions forward (rustc: look_ahead())
    pub(super) fn look_ahead(&self, n: usize) -> &Token {
        let pos = (self.cursor + n).min(self.tokens.len() - 1);
        &self.tokens[pos]
    }

    /// Get the current block's indentation level by looking at recent Indent/Dedent tokens
    /// Returns 0 if no Indent/Dedent token found (top-level)
    pub(super) fn get_current_indent_level(&self) -> usize {
        // Look backwards from current position to find the most recent Indent or Dedent token
        // A Dedent(N) token means we're now at level N, so return that
        for i in (0..self.cursor).rev() {
            if let TokenKind::Indent(level) = &self.tokens[i].kind {
                return *level;
            }
            // A Dedent(level) means we've transitioned TO that level
            if let TokenKind::Dedent(level) = &self.tokens[i].kind {
                return *level;
            }
        }
        0 // Default to level 0 if no Indent/Dedent found
    }

    /// Skip whitespace tokens (newlines, comments)
    /// When inside parentheses or braces, also skip Indent/Dedent for multiline expressions
    pub(super) fn skip_whitespace(&mut self) {
        let start_token = self.current_kind().clone();
        let mut consumed = vec![];

        // When inside parentheses or braces, skip indent/dedent tokens too (for multiline expressions)
        if self.paren_depth > 0 || self.brace_depth > 0 {
            while matches!(
                self.current_kind(),
                TokenKind::Newline
                    | TokenKind::Comment(_)
                    | TokenKind::BlockComment(_)
                    | TokenKind::Indent(_)
                    | TokenKind::Dedent(_)
            ) {
                consumed.push(format!("{:?}", self.current_kind()));
                self.bump();
            }
        } else {
            while matches!(
                self.current_kind(),
                TokenKind::Newline | TokenKind::Comment(_) | TokenKind::BlockComment(_)
            ) {
                consumed.push(format!("{:?}", self.current_kind()));
                self.bump();
            }
        }

        if !consumed.is_empty() {
            tracing::trace!(
                start = ?start_token,
                consumed = ?consumed,
                after = ?self.current_kind(),
                paren_depth = self.paren_depth,
                "skip_whitespace consumed tokens"
            );
        }
    }

    /// Skip indentation tokens
    pub(super) fn skip_indentation(&mut self) {
        // Only skip Indent tokens, not Dedent
        // Dedent tokens signal the end of blocks and should be checked explicitly
        while matches!(self.current_kind(), TokenKind::Indent(_)) {
            self.bump();
        }
    }
}
