/*
 * Clean Language Server
 * Created by Ivan Pasco
 * 
 * Language Server Protocol implementation for Clean Language providing:
 * - Syntax highlighting and parsing
 * - Error detection and recovery
 * - Autocompletion for built-in classes and methods
 * - Type checking and validation
 * - Hover information and documentation
 */

use std::sync::Arc;

use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{debug, info};

mod analyzer;
mod completion;
mod diagnostics;
mod hover;
mod parser;

use analyzer::CleanAnalyzer;
use completion::CompletionProvider;
use diagnostics::DiagnosticsProvider;
use hover::HoverProvider;
use parser::CleanParser;

#[derive(Debug)]
struct TextDocumentItem {
    uri: Url,
    text: Rope,
    version: i32,
}

struct Backend {
    client: Client,
    documents: Arc<DashMap<Url, TextDocumentItem>>,
    parser: Arc<CleanParser>,
    analyzer: Arc<CleanAnalyzer>,
    completion_provider: Arc<CompletionProvider>,
    diagnostics_provider: Arc<DiagnosticsProvider>,
    hover_provider: Arc<HoverProvider>,
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
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
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
                version: Some("0.2.2".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Clean Language Server initialized successfully");
        
        self.client
            .log_message(MessageType::INFO, "Clean Language Server ready")
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

        self.documents.insert(params.text_document.uri.clone(), document);
        
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
                        
                        if let (Some(start_idx), Some(end_idx)) = (
                            document.text.try_line_to_char(start_line).ok()
                                .and_then(|line_start| Some(line_start + start_char)),
                            document.text.try_line_to_char(end_line).ok()
                                .and_then(|line_start| Some(line_start + end_char))
                        ) {
                            document.text.remove(start_idx..end_idx);
                            document.text.insert(start_idx, &change.text);
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
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        debug!("Completion requested at {:?}", params.text_document_position);
        
        if let Some(document) = self.documents.get(&params.text_document_position.text_document.uri) {
            let completions = self.completion_provider.provide_completions(
                &document.text,
                params.text_document_position.position,
            ).await;
            
            Ok(Some(CompletionResponse::Array(completions)))
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        debug!("Hover requested at {:?}", params.text_document_position_params);
        
        if let Some(document) = self.documents.get(&params.text_document_position_params.text_document.uri) {
            let hover_info = self.hover_provider.provide_hover(
                &document.text,
                params.text_document_position_params.position,
            ).await;
            
            Ok(hover_info)
        } else {
            Ok(None)
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        debug!("Formatting requested for: {}", params.text_document.uri);
        
        if let Some(document) = self.documents.get(&params.text_document.uri) {
            // Clean Language uses tab-based indentation
            let formatted_text = self.format_document(&document.text).await;
            
            let full_range = Range {
                start: Position { line: 0, character: 0 },
                end: Position {
                    line: document.text.len_lines() as u32 - 1,
                    character: document.text.line(document.text.len_lines() - 1).len_chars() as u32,
                },
            };
            
            Ok(Some(vec![TextEdit {
                range: full_range,
                new_text: formatted_text,
            }]))
        } else {
            Ok(None)
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        debug!("Code action requested for: {}", params.text_document.uri);
        
        if let Some(document) = self.documents.get(&params.text_document.uri) {
            let actions = self.generate_code_actions(&document.text, &params.range).await;
            Ok(Some(actions))
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
            parser: Arc::new(CleanParser::new()),
            analyzer: Arc::new(CleanAnalyzer::new()),
            completion_provider: Arc::new(CompletionProvider::new()),
            diagnostics_provider: Arc::new(DiagnosticsProvider::new()),
            hover_provider: Arc::new(HoverProvider::new()),
        }
    }

    async fn analyze_document(&self, uri: &Url) {
        if let Some(document) = self.documents.get(uri) {
            let text = document.text.to_string();
            
            // Parse the document
            match self.parser.parse(&text).await {
                Ok(ast) => {
                    // Analyze for diagnostics
                    let diagnostics = self.diagnostics_provider.analyze(&ast, &text).await;
                    
                    // Publish diagnostics
                    self.client
                        .publish_diagnostics(uri.clone(), diagnostics, Some(document.version))
                        .await;
                }
                Err(parse_errors) => {
                    // Convert parse errors to diagnostics
                    let diagnostics: Vec<Diagnostic> = parse_errors
                        .into_iter()
                        .map(|error| Diagnostic {
                            range: error.range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            code: None,
                            code_description: None,
                            source: Some("clean-parser".to_string()),
                            message: error.message,
                            related_information: None,
                            tags: None,
                            data: None,
                        })
                        .collect();
                    
                    self.client
                        .publish_diagnostics(uri.clone(), diagnostics, Some(document.version))
                        .await;
                }
            }
        }
    }

    async fn format_document(&self, text: &Rope) -> String {
        // Basic Clean Language formatting - ensure tab-based indentation
        let lines: Vec<String> = text
            .lines()
            .map(|line| {
                let line_str = line.to_string();
                // Replace leading spaces with tabs (4 spaces = 1 tab)
                let leading_spaces = line_str.len() - line_str.trim_start().len();
                let tabs = "\t".repeat(leading_spaces / 4);
                let remaining_spaces = " ".repeat(leading_spaces % 4);
                format!("{}{}{}", tabs, remaining_spaces, line_str.trim_start())
            })
            .collect();
        
        lines.join("\n")
    }

    async fn generate_code_actions(&self, _text: &Rope, _range: &Range) -> CodeActionResponse {
        // Placeholder for code actions like "Fix indentation", "Add missing functions block", etc.
        CodeActionResponse::new()
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .init();

    info!("Starting Clean Language Server");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend::new(client));
    Server::new(stdin, stdout).serve(service).await;
}