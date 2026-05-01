/*
 * Clean Language Server
 *
 * Language Server Protocol implementation for Clean Language providing:
 * - Syntax highlighting and parsing using main compiler
 * - Error detection and recovery
 * - Autocompletion for built-in functions and language constructs
 * - Type checking and validation
 * - Hover information and documentation
 * - Real-time diagnostics
 * - Plugin-aware completions and hover (dynamic DSL support)
 */

use std::sync::Arc;

use clean_language_compiler::compile_with_file;
use clean_language_compiler::plugins::{LanguageRegistry, PluginDiscovery, PluginRegistry};
use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{debug, info, warn};

mod completion;
mod diagnostics;
mod formatting;
mod grammar_generator;
mod hover;

use completion::CompletionProvider;
use diagnostics::DiagnosticsProvider;
use formatting::FormattingProvider;
pub use grammar_generator::GrammarGenerator;
use hover::HoverProvider;

#[derive(Debug)]
struct TextDocumentItem {
    #[allow(dead_code)]
    uri: Url,
    text: Rope,
    version: i32,
}

struct Backend {
    client: Client,
    documents: Arc<DashMap<Url, TextDocumentItem>>,
    completion_provider: Arc<CompletionProvider>,
    diagnostics_provider: Arc<DiagnosticsProvider>,
    hover_provider: Arc<HoverProvider>,
    formatting_provider: Arc<FormattingProvider>,
    /// Plugin registry for dynamic DSL support (WASM plugins)
    #[allow(dead_code)]
    plugin_registry: Arc<PluginRegistry>,
    /// Language registry for static plugin definitions (plugin.toml)
    #[allow(dead_code)]
    language_registry: Arc<LanguageRegistry>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        info!("Clean Language Server initializing...");

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        ":".to_string(),
                        "(".to_string(),
                        "\t".to_string(),
                    ]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                        SemanticTokensRegistrationOptions {
                            text_document_registration_options: {
                                TextDocumentRegistrationOptions {
                                    document_selector: Some(vec![DocumentFilter {
                                        language: Some("clean".to_string()),
                                        scheme: Some("file".to_string()),
                                        pattern: None,
                                    }]),
                                }
                            },
                            semantic_tokens_options: SemanticTokensOptions {
                                work_done_progress_options: WorkDoneProgressOptions::default(),
                                legend: SemanticTokensLegend {
                                    token_types: vec![
                                        SemanticTokenType::KEYWORD,      // 0 - blue
                                        SemanticTokenType::TYPE,         // 1
                                        SemanticTokenType::FUNCTION,     // 2 - blue (built-in)
                                        SemanticTokenType::VARIABLE,     // 3
                                        SemanticTokenType::STRING,       // 4
                                        SemanticTokenType::NUMBER,       // 5
                                        SemanticTokenType::COMMENT,      // 6
                                        SemanticTokenType::OPERATOR,     // 7
                                        SemanticTokenType::CLASS,        // 8
                                        SemanticTokenType::PROPERTY,     // 9
                                        SemanticTokenType::NAMESPACE,    // 10 - purple (plugin functions)
                                    ],
                                    token_modifiers: vec![
                                        SemanticTokenModifier::DECLARATION,
                                        SemanticTokenModifier::DEFINITION,
                                        SemanticTokenModifier::READONLY,
                                        SemanticTokenModifier::STATIC,
                                        SemanticTokenModifier::DEFAULT_LIBRARY, // For plugin indicator
                                    ],
                                },
                                range: Some(true),
                                full: Some(SemanticTokensFullOptions::Bool(true)),
                            },
                            static_registration_options: StaticRegistrationOptions::default(),
                        },
                    ),
                ),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "Clean Language Server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Clean Language Server initialized successfully");

        self.client
            .log_message(
                MessageType::INFO,
                &format!("Clean Language Server ready - v{}", env!("CARGO_PKG_VERSION")),
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Clean Language Server shutting down");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        debug!("Document opened: {}", params.text_document.uri);

        let document = TextDocumentItem {
            uri: params.text_document.uri.clone(),
            text: Rope::from_str(&params.text_document.text),
            version: params.text_document.version,
        };

        self.documents
            .insert(params.text_document.uri.clone(), document);

        // Parse and analyze the document
        self.analyze_document(&params.text_document.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        debug!("Document changed: {}", params.text_document.uri);

        if let Some(mut document) = self.documents.get_mut(&params.text_document.uri) {
            for change in params.content_changes {
                match change.range {
                    Some(range) => {
                        // Incremental change
                        let start_line = range.start.line as usize;
                        let start_char = range.start.character as usize;
                        let end_line = range.end.line as usize;
                        let end_char = range.end.character as usize;

                        if let (Ok(start_idx), Ok(end_idx)) = (
                            document
                                .text
                                .try_line_to_char(start_line)
                                .map(|line_start| {
                                    line_start
                                        + start_char.min(document.text.line(start_line).len_chars())
                                }),
                            document.text.try_line_to_char(end_line).map(|line_start| {
                                line_start + end_char.min(document.text.line(end_line).len_chars())
                            }),
                        ) {
                            if end_idx >= start_idx && end_idx <= document.text.len_chars() {
                                document.text.remove(start_idx..end_idx);
                                document.text.insert(start_idx, &change.text);
                            } else {
                                warn!("Invalid range for incremental change, falling back to full document update");
                                document.text = Rope::from_str(&change.text);
                            }
                        } else {
                            warn!("Failed to convert line positions, falling back to full document update");
                            document.text = Rope::from_str(&change.text);
                        }
                    }
                    None => {
                        // Full document change
                        document.text = Rope::from_str(&change.text);
                    }
                }
            }
            document.version = params.text_document.version;
        }

        // Re-analyze the document
        self.analyze_document(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        debug!("Document closed: {}", params.text_document.uri);
        self.documents.remove(&params.text_document.uri);

        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        debug!(
            "Completion requested at {:?}",
            params.text_document_position
        );

        if let Some(document) = self
            .documents
            .get(&params.text_document_position.text_document.uri)
        {
            let completions = self
                .completion_provider
                .provide_completions(&document.text, params.text_document_position.position)
                .await;

            Ok(Some(CompletionResponse::Array(completions)))
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        debug!(
            "Hover requested at {:?}",
            params.text_document_position_params
        );

        if let Some(document) = self
            .documents
            .get(&params.text_document_position_params.text_document.uri)
        {
            let hover_info = self
                .hover_provider
                .provide_hover(
                    &document.text,
                    params.text_document_position_params.position,
                )
                .await;

            Ok(hover_info)
        } else {
            Ok(None)
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        debug!("Formatting requested for: {}", params.text_document.uri);

        if let Some(document) = self.documents.get(&params.text_document.uri) {
            let edits = self
                .formatting_provider
                .format_document(&document.text, &params.options)
                .await;

            Ok(edits)
        } else {
            Ok(None)
        }
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        debug!(
            "Range formatting requested for: {}",
            params.text_document.uri
        );

        if let Some(document) = self.documents.get(&params.text_document.uri) {
            let edits = self
                .formatting_provider
                .format_range(&document.text, &params.range, &params.options)
                .await;

            Ok(edits)
        } else {
            Ok(None)
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        debug!("Code action requested for: {}", params.text_document.uri);

        if let Some(document) = self.documents.get(&params.text_document.uri) {
            let actions = self.generate_code_actions(&document.text, &params).await;
            Ok(Some(actions))
        } else {
            Ok(None)
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        debug!(
            "Semantic tokens requested for: {}",
            params.text_document.uri
        );

        if let Some(document) = self.documents.get(&params.text_document.uri) {
            let tokens = self.generate_semantic_tokens(&document.text).await;
            Ok(Some(SemanticTokensResult::Tokens(tokens)))
        } else {
            Ok(None)
        }
    }
}

impl Backend {
    fn new(client: Client) -> Self {
        // Discover plugins from global and project directories
        let discovery = PluginDiscovery::new();
        let manifests = discovery.discover_all().unwrap_or_default();

        // Build the language registry from discovered plugin manifests
        let language_registry = Arc::new(LanguageRegistry::from_manifests(&manifests));

        if !manifests.is_empty() {
            info!(
                "Loaded language definitions from {} plugins: {:?}",
                manifests.len(),
                manifests.keys().collect::<Vec<_>>()
            );
        }

        // Create the plugin registry (can be extended with plugins at runtime)
        let plugin_registry = Arc::new(
            PluginRegistry::builder()
                .build()
                .expect("Failed to build plugin registry"),
        );

        // Create providers with both plugin and language registry support
        let completion_provider = Arc::new(CompletionProvider::with_language_registry(
            Arc::clone(&plugin_registry),
            Arc::clone(&language_registry),
        ));

        let hover_provider = Arc::new(HoverProvider::with_language_registry(
            Arc::clone(&plugin_registry),
            Arc::clone(&language_registry),
        ));

        let diagnostics_provider = Arc::new(DiagnosticsProvider::new());

        Self {
            client,
            documents: Arc::new(DashMap::new()),
            completion_provider,
            diagnostics_provider,
            hover_provider,
            formatting_provider: Arc::new(FormattingProvider::new()),
            plugin_registry,
            language_registry,
        }
    }

    /// Create a backend with specific registries
    #[allow(dead_code)]
    fn with_registries(
        client: Client,
        plugin_registry: Arc<PluginRegistry>,
        language_registry: Arc<LanguageRegistry>,
    ) -> Self {
        let completion_provider = Arc::new(CompletionProvider::with_language_registry(
            Arc::clone(&plugin_registry),
            Arc::clone(&language_registry),
        ));

        let hover_provider = Arc::new(HoverProvider::with_language_registry(
            Arc::clone(&plugin_registry),
            Arc::clone(&language_registry),
        ));

        let diagnostics_provider = Arc::new(DiagnosticsProvider::new());

        Self {
            client,
            documents: Arc::new(DashMap::new()),
            completion_provider,
            diagnostics_provider,
            hover_provider,
            formatting_provider: Arc::new(FormattingProvider::new()),
            plugin_registry,
            language_registry,
        }
    }

    /// Create a backend with a specific plugin registry (legacy method)
    #[allow(dead_code)]
    fn with_plugins(client: Client, registry: Arc<PluginRegistry>) -> Self {
        let language_registry = Arc::new(LanguageRegistry::new());

        let completion_provider = Arc::new(CompletionProvider::with_language_registry(
            Arc::clone(&registry),
            Arc::clone(&language_registry),
        ));
        let hover_provider = Arc::new(HoverProvider::with_language_registry(
            Arc::clone(&registry),
            Arc::clone(&language_registry),
        ));

        Self {
            client,
            documents: Arc::new(DashMap::new()),
            completion_provider,
            diagnostics_provider: Arc::new(DiagnosticsProvider::new()),
            hover_provider,
            formatting_provider: Arc::new(FormattingProvider::new()),
            plugin_registry: registry,
            language_registry,
        }
    }

    async fn analyze_document(&self, uri: &Url) {
        if let Some(document) = self.documents.get(uri) {
            let text = document.text.to_string();

            // Use the main compiler for parsing and analysis
            match compile_with_file(&text, "lsp-temp.cln") {
                Ok(_) => {
                    // Successful compilation - no errors
                    debug!("Document compiled successfully: {}", uri);
                    self.client
                        .publish_diagnostics(uri.clone(), vec![], Some(document.version))
                        .await;
                }
                Err(errors) => {
                    // Convert compiler errors to LSP diagnostics
                    let diagnostics = self
                        .diagnostics_provider
                        .convert_compiler_errors(&errors, &text)
                        .await;

                    debug!(
                        "Found {} diagnostics for document: {}",
                        diagnostics.len(),
                        uri
                    );
                    self.client
                        .publish_diagnostics(uri.clone(), diagnostics, Some(document.version))
                        .await;
                }
            }
        }
    }

    async fn generate_code_actions(
        &self,
        text: &Rope,
        params: &CodeActionParams,
    ) -> CodeActionResponse {
        let mut actions = vec![];

        // Add common code actions for Clean Language

        // Fix indentation action
        if self.needs_indentation_fix(text) {
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Fix indentation (use tabs)".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(params.context.diagnostics.clone()),
                edit: Some(self.create_indentation_fix_edit(text, &params.text_document.uri)),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            }));
        }

        // Add functions block action
        if self.needs_functions_block(text) {
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Wrap functions in functions: block".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(params.context.diagnostics.clone()),
                edit: Some(self.create_functions_block_edit(text, &params.text_document.uri)),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }

        actions
    }

    async fn generate_semantic_tokens(&self, text: &Rope) -> SemanticTokens {
        // Basic semantic tokenization for Clean Language
        let mut data = vec![];

        for (line_idx, line) in text.lines().enumerate() {
            let line_str = line.to_string();

            // Tokenize keywords, types, functions, etc.
            self.tokenize_line(&line_str, line_idx as u32, &mut data);
        }

        SemanticTokens {
            result_id: None,
            data,
        }
    }

    fn tokenize_line(&self, line: &str, line_number: u32, data: &mut Vec<SemanticToken>) {
        // Core language keywords (token_type 0: KEYWORD)
        let keywords: &[&str] = &[
            // Control flow
            "if", "else", "iterate", "in", "to", "step", "repeat", "break", "continue",
            "return", "onError",
            // Declarations
            "functions", "class", "start", "state", "computed", "watch", "rules",
            "constants", "tests", "build", "plugins",
            // Modifiers
            "private", "constant", "extends", "base", "new",
            // Operators
            "and", "or", "not", "is",
            // Async
            "later", "background",
            // AI integration
            "spec", "intent", "source",
            // Error handling
            "error", "assert",
        ];

        // Type keywords (token_type 1: TYPE)
        let type_keywords: &[&str] = &[
            "integer", "number", "string", "boolean", "void", "any",
            "list", "matrix", "pairs",
            // Sized types
            "integer:8", "integer:16", "integer:32", "integer:64",
            "integer:8u", "integer:16u", "integer:32u", "integer:64u",
            "number:32", "number:64",
        ];

        // Built-in functions/namespaces (token_type 2: FUNCTION)
        let builtin_functions: &[&str] = &[
            "print", "printl", "input",
        ];

        // Built-in classes/namespaces (token_type 8: CLASS)
        let builtin_classes: &[&str] = &[
            "Math", "String", "Integer", "List", "File", "Http", "JSON",
        ];

        // Framework block keywords (token_type 10: NAMESPACE for plugin blocks)
        let framework_blocks: &[&str] = &[
            "data", "migrate", "endpoints", "server", "component", "screen", "page",
            "styles", "html", "ui", "auth", "protected", "login", "roles",
            "canvasScene", "draw", "onFrame", "onPointerDown", "onPointerMove", "onKeyDown",
        ];

        // Sub-block keywords (token_type 9: PROPERTY)
        let sub_blocks: &[&str] = &[
            "where", "order", "limit", "offset", "guard", "returns", "cache", "handle",
            "props", "render", "handlers", "session", "jwt",
            "up", "down", "tenant", "theme", "colors", "spacing", "maxAge", "noCache",
        ];

        // HTTP methods (token_type 10: NAMESPACE)
        let http_methods: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];

        // Namespace prefixes for plugin/stdlib calls (token_type 10: NAMESPACE)
        let namespace_prefixes: &[&str] = &[
            "math.", "string.", "list.", "integer.", "file.", "json.",
            "http.", "db.", "crypto.", "request.", "response.",
            "route.", "session.", "auth.", "cache.", "email.", "queue.", "storage.",
            "canvas.", "sprite.", "audio.", "input.", "collision.", "camera.", "ease.",
            "scene.", "tenant.", "Data.", "ctx.",
        ];

        // Also check language registry for plugin-defined functions
        let registry_functions: Vec<&str> = self.language_registry.all_functions();

        // Collect tokens with absolute positions first, then convert to deltas
        let mut tokens: Vec<(u32, u32, u32)> = vec![]; // (pos, len, token_type)

        // Pass 1: Namespace-prefixed calls (e.g., math.sqrt, canvas.clear)
        for prefix in namespace_prefixes {
            let mut search_from = 0;
            while search_from < line.len() {
                if let Some(pos) = line[search_from..].find(prefix) {
                    let abs_pos = search_from + pos;
                    // Verify it's not inside a string or after a non-boundary char
                    if abs_pos > 0 {
                        let prev = line.as_bytes()[abs_pos - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            search_from = abs_pos + 1;
                            continue;
                        }
                    }
                    let rest = &line[abs_pos..];
                    let end = rest.find(|c: char| c == '(' || c.is_whitespace() || c == ')' || c == ',')
                        .unwrap_or(rest.len());
                    if end > prefix.len() {
                        tokens.push((abs_pos as u32, end as u32, 10));
                    }
                    search_from = abs_pos + end;
                } else {
                    break;
                }
            }
        }

        // Pass 2: Words (keywords, types, functions, classes, blocks, sub-blocks)
        let mut word_start = None;
        let bytes = line.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            let is_word_char = b.is_ascii_alphanumeric() || b == b'_';
            if is_word_char && word_start.is_none() {
                word_start = Some(i);
            } else if !is_word_char && word_start.is_some() {
                let start = word_start.unwrap();
                let word = &line[start..i];
                word_start = None;

                // Skip if already covered by a namespace token
                let already_covered = tokens.iter().any(|(pos, len, _)| {
                    let tok_start = *pos as usize;
                    let tok_end = tok_start + *len as usize;
                    start >= tok_start && i <= tok_end
                });
                if already_covered {
                    continue;
                }

                // Classify the word
                if keywords.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 0));
                } else if type_keywords.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 1));
                } else if builtin_functions.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 2));
                } else if builtin_classes.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 8));
                } else if framework_blocks.contains(&word) {
                    // Only classify as block if followed by ':'
                    let rest = line[i..].trim_start();
                    if rest.starts_with(':') || rest.is_empty() {
                        tokens.push((start as u32, word.len() as u32, 10));
                    }
                } else if sub_blocks.contains(&word) {
                    let rest = line[i..].trim_start();
                    if rest.starts_with(':') || rest.starts_with('=') || rest.is_empty() {
                        tokens.push((start as u32, word.len() as u32, 9));
                    }
                } else if http_methods.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 10));
                } else if registry_functions.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 2));
                }
            }
        }
        // Handle word at end of line
        if let Some(start) = word_start {
            let word = &line[start..];
            let already_covered = tokens.iter().any(|(pos, len, _)| {
                let tok_start = *pos as usize;
                let tok_end = tok_start + *len as usize;
                start >= tok_start && line.len() <= tok_end
            });
            if !already_covered {
                if keywords.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 0));
                } else if type_keywords.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 1));
                } else if builtin_functions.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 2));
                } else if builtin_classes.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 8));
                } else if framework_blocks.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 10));
                } else if sub_blocks.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 9));
                } else if http_methods.contains(&word) {
                    tokens.push((start as u32, word.len() as u32, 10));
                }
            }
        }

        // Sort by position and convert to delta encoding
        tokens.sort_by_key(|(pos, _, _)| *pos);

        // Reconstruct absolute line of the last token in data
        let prev_abs_line: u32 = data.iter().map(|tok| tok.delta_line).sum();

        let mut last_pos = 0u32;
        let mut is_first_on_this_line = true;

        for (pos, len, token_type) in tokens {
            let delta_line = line_number - prev_abs_line;
            let delta_start = if !is_first_on_this_line {
                pos - last_pos
            } else {
                pos
            };

            data.push(SemanticToken {
                delta_line: if is_first_on_this_line { delta_line } else { 0 },
                delta_start,
                length: len,
                token_type,
                token_modifiers_bitset: 0,
            });
            last_pos = pos;
            is_first_on_this_line = false;
        }
    }

    fn needs_indentation_fix(&self, text: &Rope) -> bool {
        // Check if document uses spaces instead of tabs for indentation
        text.lines().any(|line| {
            let line_str = line.to_string();
            line_str.starts_with("    ") || line_str.starts_with("  ")
        })
    }

    fn needs_functions_block(&self, text: &Rope) -> bool {
        // Check if there are function definitions not wrapped in functions: block
        let text_str = text.to_string();
        text_str.contains("(")
            && text_str.contains(")")
            && !text_str.contains("functions:")
            && !text_str.starts_with("start()")
    }

    fn create_indentation_fix_edit(&self, text: &Rope, uri: &Url) -> WorkspaceEdit {
        let formatted_text = self.fix_indentation(text);

        let full_range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: text.len_lines() as u32 - 1,
                character: text.line(text.len_lines() - 1).len_chars() as u32,
            },
        };

        let mut changes = std::collections::HashMap::new();
        changes.insert(
            uri.clone(),
            vec![TextEdit {
                range: full_range,
                new_text: formatted_text,
            }],
        );

        WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }
    }

    fn create_functions_block_edit(&self, text: &Rope, uri: &Url) -> WorkspaceEdit {
        let wrapped_text = self.wrap_in_functions_block(text);

        let full_range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: text.len_lines() as u32 - 1,
                character: text.line(text.len_lines() - 1).len_chars() as u32,
            },
        };

        let mut changes = std::collections::HashMap::new();
        changes.insert(
            uri.clone(),
            vec![TextEdit {
                range: full_range,
                new_text: wrapped_text,
            }],
        );

        WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }
    }

    fn fix_indentation(&self, text: &Rope) -> String {
        text.lines()
            .map(|line| {
                let line_str = line.to_string();
                // Convert leading spaces to tabs (4 spaces = 1 tab)
                let leading_spaces = line_str.len() - line_str.trim_start().len();
                let tabs = "\t".repeat(leading_spaces / 4);
                let remaining_spaces = " ".repeat(leading_spaces % 4);
                format!("{}{}{}", tabs, remaining_spaces, line_str.trim_start())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wrap_in_functions_block(&self, text: &Rope) -> String {
        let text_str = text.to_string();

        if text_str.trim().starts_with("start()") {
            // Don't wrap start() functions
            return text_str;
        }

        format!(
            "functions:\n\t{}",
            text_str
                .lines()
                .map(|line| if line.trim().is_empty() {
                    line.to_string()
                } else {
                    format!("\t{line}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

#[tokio::main]
async fn main() {
    // Parse --log-level <level> and --version args; ignore unknown args (e.g. --stdio)
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("clean-language-server {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let log_level = args
        .windows(2)
        .find(|w| w[0] == "--log-level")
        .map(|w| w[1].as_str())
        .unwrap_or("info");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("Starting Clean Language Server v{}", env!("CARGO_PKG_VERSION"));

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
