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
 */

use std::sync::Arc;

use clean_language_compiler::compile_with_file;
use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{debug, info, warn};

mod completion;
mod diagnostics;
mod formatting;
mod hover;

use completion::CompletionProvider;
use diagnostics::DiagnosticsProvider;
use formatting::FormattingProvider;
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
                                        SemanticTokenType::KEYWORD,
                                        SemanticTokenType::TYPE,
                                        SemanticTokenType::FUNCTION,
                                        SemanticTokenType::VARIABLE,
                                        SemanticTokenType::STRING,
                                        SemanticTokenType::NUMBER,
                                        SemanticTokenType::COMMENT,
                                        SemanticTokenType::OPERATOR,
                                        SemanticTokenType::CLASS,
                                        SemanticTokenType::PROPERTY,
                                    ],
                                    token_modifiers: vec![
                                        SemanticTokenModifier::DECLARATION,
                                        SemanticTokenModifier::DEFINITION,
                                        SemanticTokenModifier::READONLY,
                                        SemanticTokenModifier::STATIC,
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
                version: Some("0.7.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Clean Language Server initialized successfully");

        self.client
            .log_message(
                MessageType::INFO,
                "Clean Language Server ready - integrated with compiler v0.8.2",
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
        Self {
            client,
            documents: Arc::new(DashMap::new()),
            completion_provider: Arc::new(CompletionProvider::new()),
            diagnostics_provider: Arc::new(DiagnosticsProvider::new()),
            hover_provider: Arc::new(HoverProvider::new()),
            formatting_provider: Arc::new(FormattingProvider::new()),
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
        let keywords = [
            "functions",
            "class",
            "start",
            "if",
            "else",
            "while",
            "for",
            "return",
            "integer",
            "number",
            "string",
            "boolean",
            "void",
            "any",
            "constants",
            "types",
            "extends",
            "base",
            "onError",
            "try",
            "catch",
        ];

        let mut char_offset = 0u32;

        for word in line.split_whitespace() {
            let word_start = line.find(word).unwrap_or(0) as u32;

            if keywords.contains(&word) {
                data.push(SemanticToken {
                    delta_line: if data.is_empty() { line_number } else { 0 },
                    delta_start: word_start - char_offset,
                    length: word.len() as u32,
                    token_type: 0, // KEYWORD
                    token_modifiers_bitset: 0,
                });
            }

            char_offset = word_start + word.len() as u32;
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
    // Initialize logging with environment filter support
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("Starting Clean Language Server v0.8.2");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
