# Clean Language Server Documentation

**Version:** 1.0.0  
**Author:** Ivan Pasco  
**Last Updated:** 2025-01-11  
**Status:** Production Ready

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Installation & Setup](#installation--setup)
4. [Core Features](#core-features)
5. [LSP Capabilities](#lsp-capabilities)
6. [Development Guide](#development-guide)
7. [Integration with Compiler](#integration-with-compiler)
8. [Maintenance & Updates](#maintenance--updates)
9. [Performance Optimization](#performance-optimization)
10. [Troubleshooting](#troubleshooting)
11. [API Reference](#api-reference)
12. [Contributing](#contributing)

---

## Overview

The Clean Language Server is a comprehensive Language Server Protocol (LSP) implementation that provides rich IDE features for the Clean programming language. Built with Rust and the `tower-lsp` framework, it delivers high-performance language services including syntax highlighting, error detection, autocompletion, and intelligent code analysis.

### Key Features

- **Real-time Syntax Analysis**: Immediate parsing and error detection
- **Intelligent Autocompletion**: Context-aware suggestions for keywords, methods, and types
- **Error Recovery**: Robust error handling with helpful diagnostic messages
- **Code Formatting**: Automatic Clean Language formatting with tab-based indentation
- **Hover Information**: Rich documentation and type information on hover
- **Incremental Updates**: Efficient document synchronization for optimal performance
- **Multi-workspace Support**: Handle multiple Clean Language projects simultaneously

### Supported IDEs

- **Visual Studio Code** (primary target)
- **Neovim** (via built-in LSP client)
- **Emacs** (via lsp-mode)
- **Vim** (via vim-lsp)
- **Any LSP-compatible editor**

---

## Architecture

### High-Level Architecture

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│                 │    │                  │    │                 │
│   IDE Client    │◄──►│ Language Server  │◄──►│ Clean Compiler  │
│                 │    │                  │    │                 │
└─────────────────┘    └──────────────────┘    └─────────────────┘
                              │
                              ▼
                       ┌──────────────┐
                       │              │
                       │ AST + Parser │
                       │              │
                       └──────────────┘
```

### Component Structure

```rust
Backend
├── Client (tower-lsp::Client)
├── Documents (Arc<DashMap<Url, TextDocumentItem>>)
├── Parser (Arc<CleanParser>)
├── Analyzer (Arc<CleanAnalyzer>)
├── CompletionProvider (Arc<CompletionProvider>)
├── DiagnosticsProvider (Arc<DiagnosticsProvider>)
└── HoverProvider (Arc<HoverProvider>)
```

### Module Organization

```
language-server/
├── src/
│   ├── main.rs              # LSP server entry point & Backend implementation
│   ├── parser.rs            # Clean Language parsing logic
│   ├── analyzer.rs          # Semantic analysis and type checking
│   ├── completion.rs        # Autocompletion provider
│   ├── diagnostics.rs       # Error detection and reporting
│   └── hover.rs             # Hover information provider
├── Cargo.toml              # Dependencies and build configuration
└── README.md               # Basic setup instructions
```

---

## Installation & Setup

### Prerequisites

- **Rust 1.70+** (for building from source)
- **Clean Language Compiler** (integrated with project)
- **Node.js 16+** (for VS Code extension development)

### Building the Language Server

```bash
# Navigate to the language server directory
cd /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/language-server

# Build in release mode for production
cargo build --release

# The binary will be available at:
# ./target/release/clean-language-server
```

### VS Code Extension Setup

1. **Install Dependencies**:
   ```bash
   cd clean-language-extension
   npm install
   ```

2. **Configure Extension**:
   ```json
   // settings.json
   {
     "clean-language.server.path": "./language-server/target/release/clean-language-server",
     "clean-language.server.debug": false
   }
   ```

3. **Package Extension**:
   ```bash
   npx vsce package
   code --install-extension clean-language-*.vsix
   ```

### Manual Installation

For editors with built-in LSP support:

```bash
# Add to PATH or specify full path in editor config
export PATH="$PATH:/path/to/clean-language-compiler/language-server/target/release"

# Test the server
clean-language-server --version
```

---

## Core Features

### 1. Document Management

The language server maintains an in-memory representation of all opened Clean Language documents using the `DashMap` data structure for thread-safe concurrent access.

**Key Components:**

- **TextDocumentItem**: Stores document URI, content (as `Rope`), and version
- **Incremental Synchronization**: Efficient updates using LSP text change events
- **Version Tracking**: Maintains document versions for consistency

```rust
#[derive(Debug)]
struct TextDocumentItem {
    uri: Url,
    text: Rope,        // Efficient text rope for incremental edits
    version: i32,      // LSP document version
}
```

### 2. Parsing & AST Generation

The integrated parser creates Abstract Syntax Trees (ASTs) for Clean Language documents, supporting all language constructs including:

**Supported Language Constructs:**

- **Functions & Classes**: Full support for Clean Language OOP
- **Apply Blocks**: Language-specific `identifier:` syntax
- **Type Annotations**: Strong typing with inference
- **Error Recovery**: Continue parsing after encountering errors

**AST Node Types:**

```rust
pub enum CleanASTNode {
    Program { functions, classes, start_function },
    Function { name, parameters, return_type, body, range },
    Class { name, extends, fields, constructor, methods, range },
    ApplyBlock { target, items, range },
    VariableDeclaration { var_type, name, value, range },
    // ... additional node types
}
```

### 3. Error Detection & Diagnostics

**Multi-layered Error Detection:**

1. **Lexical Errors**: Invalid tokens, malformed literals
2. **Syntax Errors**: Grammar violations, missing constructs
3. **Semantic Errors**: Type mismatches, undefined variables
4. **Style Warnings**: Indentation issues, naming conventions

**Diagnostic Categories:**

```rust
pub enum DiagnosticSeverity {
    Error,      // Compilation failures
    Warning,    // Potential issues
    Information,// Code suggestions
    Hint,       // Style recommendations
}
```

### 4. Intelligent Autocompletion

**Context-Aware Completions:**

- **Dot Notation**: Method and property completions after `.`
- **Apply Blocks**: Suggestions for `identifier:` patterns
- **Type Annotations**: Built-in and user-defined types
- **Keywords**: Language-specific keywords with snippets

**Completion Triggers:**

- `.` (dot notation)
- `:` (apply blocks)  
- Whitespace (keywords and types)
- User-defined triggers

### 5. Code Formatting

**Clean Language Formatting Rules:**

- **Tab-based Indentation**: Enforces language standard
- **Consistent Spacing**: Standardized operator and punctuation spacing
- **Block Structure**: Proper alignment of code blocks
- **Line Length**: Configurable maximum line length

---

## LSP Capabilities

### Currently Implemented

| Capability | Status | Description |
|------------|--------|-------------|
| `textDocument/didOpen` | ✅ | Document opening and parsing |
| `textDocument/didChange` | ✅ | Incremental document updates |
| `textDocument/didClose` | ✅ | Document cleanup |
| `textDocument/completion` | ✅ | Context-aware autocompletion |
| `textDocument/hover` | ✅ | Hover information and documentation |
| `textDocument/formatting` | ✅ | Automatic code formatting |
| `textDocument/codeAction` | ✅ | Code actions and quick fixes |
| `initialize` | ✅ | Server capabilities negotiation |
| `shutdown` | ✅ | Graceful server shutdown |

### Planned Extensions

| Capability | Priority | Target Version |
|------------|----------|----------------|
| `textDocument/definition` | High | 1.1.0 |
| `textDocument/references` | High | 1.1.0 |
| `textDocument/rename` | Medium | 1.2.0 |
| `textDocument/documentSymbol` | Medium | 1.2.0 |
| `workspace/symbol` | Low | 1.3.0 |
| `textDocument/semanticTokens` | Low | 1.3.0 |

### Server Capabilities Configuration

```rust
ServerCapabilities {
    text_document_sync: Some(TextDocumentSyncCapability::Kind(
        TextDocumentSyncKind::INCREMENTAL,
    )),
    hover_provider: Some(HoverProviderCapability::Simple(true)),
    completion_provider: Some(CompletionOptions {
        resolve_provider: Some(false),
        trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
        // ... additional options
    }),
    definition_provider: Some(OneOf::Left(true)),
    references_provider: Some(OneOf::Left(true)),
    document_formatting_provider: Some(OneOf::Left(true)),
    code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
    // ... additional capabilities
}
```

---

## Development Guide

### Development Environment Setup

1. **Clone & Setup**:
   ```bash
   git clone <clean-language-compiler-repo>
   cd clean-language-compiler/language-server
   ```

2. **Development Dependencies**:
   ```bash
   cargo install cargo-watch  # For auto-recompilation
   cargo install cargo-expand # For macro expansion debugging
   ```

3. **IDE Configuration**:
   ```bash
   # VS Code with rust-analyzer
   code .
   
   # Configure rust-analyzer for the language server project
   ```

### Development Workflow

**1. Feature Development Cycle:**

```bash
# Start development with auto-reload
cargo watch -x "build --release"

# Run tests continuously
cargo watch -x test

# Check for issues
cargo clippy -- -D warnings
cargo fmt --check
```

**2. Testing Strategy:**

```bash
# Unit tests
cargo test --lib

# Integration tests  
cargo test --test integration

# Test with actual Clean Language files
cargo test --test clean_file_parsing
```

**3. Debugging:**

```rust
// Enable detailed logging
tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .with_writer(std::io::stderr)
    .init();

// Add debug points in code
debug!("Processing completion at {:?}", position);
```

### Adding New LSP Features

**1. Define the Capability:**

```rust
// In main.rs - ServerCapabilities
my_new_feature_provider: Some(MyNewFeatureProviderCapability::Simple(true)),
```

**2. Implement the Handler:**

```rust
#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn my_new_feature(&self, params: MyFeatureParams) -> Result<Option<MyFeatureResponse>> {
        // Implementation
        Ok(response)
    }
}
```

**3. Create Feature Module:**

```rust
// src/my_feature.rs
pub struct MyFeatureProvider;

impl MyFeatureProvider {
    pub fn new() -> Self { Self }
    
    pub async fn provide_feature(&self, text: &Rope, position: Position) -> MyFeatureResult {
        // Feature logic
    }
}
```

**4. Integrate with Backend:**

```rust
// Add to Backend struct
my_feature_provider: Arc<MyFeatureProvider>,

// Initialize in Backend::new()
my_feature_provider: Arc::new(MyFeatureProvider::new()),
```

### Code Style Guidelines

**Rust Style:**

```rust
// Use explicit types for public APIs
pub async fn provide_completions(&self, text: &Rope, position: Position) -> Vec<CompletionItem>

// Prefer ? operator for error handling
let document = self.documents.get(uri).ok_or_else(|| Error::invalid_request())?;

// Use structured logging
info!("Processing completion request for {}", uri);
debug!("Current position: {:?}", position);
```

**Error Handling Patterns:**

```rust
// Graceful degradation
match self.parse_document(&text).await {
    Ok(ast) => self.analyze_with_ast(ast).await,
    Err(e) => {
        warn!("Parse failed, using text-based analysis: {}", e);
        self.analyze_text_only(&text).await
    }
}
```

---

## Integration with Compiler

### Compiler Components Used

The language server integrates directly with the Clean Language compiler components:

**1. Parser Integration:**

```rust
// Direct integration with compiler parser
use clean_language_compiler::parser::CleanParser as CompilerParser;
use clean_language_compiler::ast::ASTNode;

impl CleanParser {
    async fn parse(&self, text: &str) -> Result<CleanASTNode, Vec<ParseError>> {
        // Use compiler's parser with LSP-specific error recovery
        match CompilerParser::parse_program(text) {
            Ok(compiler_ast) => Ok(self.convert_to_lsp_ast(compiler_ast)),
            Err(compiler_errors) => Err(self.convert_to_lsp_errors(compiler_errors)),
        }
    }
}
```

**2. Semantic Analysis Integration:**

```rust
// Leverage compiler's semantic analyzer
use clean_language_compiler::semantic::SemanticAnalyzer;

impl CleanAnalyzer {
    async fn analyze(&self, ast: &CleanASTNode) -> AnalysisResult {
        let compiler_ast = self.convert_from_lsp_ast(ast);
        let semantic_analyzer = SemanticAnalyzer::new();
        
        match semantic_analyzer.analyze(&compiler_ast) {
            Ok(analyzed) => self.extract_lsp_information(analyzed),
            Err(errors) => self.convert_semantic_errors(errors),
        }
    }
}
```

**3. Type System Integration:**

```rust
// Use compiler's type information
use clean_language_compiler::types::{Type, TypeChecker};

impl CompletionProvider {
    fn get_type_completions(&self, context: &CompletionContext) -> Vec<CompletionItem> {
        let type_checker = TypeChecker::new();
        let available_types = type_checker.get_available_types_in_scope(&context.scope);
        
        available_types
            .into_iter()
            .map(|ty| self.type_to_completion_item(ty))
            .collect()
    }
}
```

### Keeping Compiler Integration Updated

**1. Version Synchronization:**

```toml
# Cargo.toml - Keep compiler dependency updated
[dependencies]
clean-language-compiler = { path = "..", version = "0.2.2" }
```

**2. API Change Handling:**

```rust
// Wrapper for compiler API changes
impl CompilerIntegration {
    #[cfg(feature = "compiler-v0-2")]
    fn parse_with_v0_2(&self, text: &str) -> Result<AST> {
        CompilerParser::new().parse_program(text)
    }
    
    #[cfg(feature = "compiler-v0-3")]
    fn parse_with_v0_3(&self, text: &str) -> Result<AST> {
        CompilerParser::with_config(Default::default()).parse_program(text)
    }
}
```

**3. Feature Flag Management:**

```rust
// Conditional compilation for compiler features
#[cfg(feature = "advanced-diagnostics")]
impl DiagnosticsProvider {
    async fn advanced_analysis(&self, ast: &CleanASTNode) -> Vec<Diagnostic> {
        // Use advanced compiler diagnostics
    }
}
```

---

## Maintenance & Updates

### Update Checklist

**When Compiler Updates:**

1. **Update Dependencies:**
   ```bash
   cd language-server
   cargo update clean-language-compiler
   ```

2. **Test Integration:**
   ```bash
   cargo test --all-features
   cargo test --test compiler_integration
   ```

3. **Update AST Mappings:**
   ```rust
   // Check if compiler AST changes require LSP AST updates
   fn convert_to_lsp_ast(&self, compiler_ast: CompilerAST) -> CleanASTNode {
       match compiler_ast {
           // Handle new AST node types
           CompilerAST::NewNodeType { .. } => {
               // Add corresponding LSP AST node
           }
           // ... existing mappings
       }
   }
   ```

4. **Update Completions:**
   ```rust
   // Add new language features to completions
   fn get_keyword_completions(&self) -> Vec<CompletionItem> {
       vec![
           // Existing keywords
           self.create_keyword_completion("start"),
           self.create_keyword_completion("functions"),
           // New keywords from compiler updates
           self.create_keyword_completion("new_keyword"),
       ]
   }
   ```

### Automated Update Process

**1. CI/CD Pipeline:**

```yaml
# .github/workflows/language-server-update.yml
name: Language Server Update
on:
  push:
    paths: ['src/**', 'language-server/**']

jobs:
  test-integration:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Build compiler
        run: cargo build --release
      - name: Test language server
        run: |
          cd language-server
          cargo test --release
      - name: Integration test
        run: |
          cd language-server
          cargo run --release -- --test-mode
```

**2. Version Management:**

```bash
#!/bin/bash
# scripts/update-language-server.sh

# Update compiler dependency
cd language-server
cargo update clean-language-compiler

# Run tests
cargo test --all-features

# Update version if tests pass
if [ $? -eq 0 ]; then
    # Bump language server version
    sed -i 's/version = "[0-9]\+\.[0-9]\+\.[0-9]\+"/version = "'"$NEW_VERSION"'"/g' Cargo.toml
    
    # Build release
    cargo build --release
    
    echo "Language server updated to $NEW_VERSION"
else
    echo "Tests failed, update aborted"
    exit 1
fi
```

**3. Compatibility Testing:**

```rust
// tests/compiler_compatibility.rs
#[test]
fn test_parser_compatibility() {
    let test_cases = load_clean_test_files();
    
    for test_file in test_cases {
        let lsp_result = language_server_parse(&test_file.content);
        let compiler_result = compiler_parse(&test_file.content);
        
        // Ensure LSP parsing is compatible with compiler
        assert_compatible(lsp_result, compiler_result);
    }
}

#[test] 
fn test_completion_relevance() {
    let context = CompletionContext::from_file("tests/completion_test.cln");
    let completions = provide_completions(context);
    
    // Verify completions are accurate for current compiler version
    assert_completions_valid(completions);
}
```

### Documentation Updates

**1. Automatic Documentation Generation:**

```bash
# Generate API documentation
cargo doc --open --no-deps

# Update architecture diagrams
python scripts/generate_architecture_docs.py

# Update README with new features
python scripts/update_readme.py --version $VERSION
```

**2. Change Log Management:**

```markdown
# CHANGELOG.md

## [1.1.0] - 2025-01-15
### Added
- Go-to-definition support
- Find references functionality
- Integration with compiler v0.2.3

### Changed
- Improved completion performance by 40%
- Enhanced error recovery for malformed apply blocks

### Fixed
- Fixed memory leak in document synchronization
- Corrected hover information for generic types
```

---

## Performance Optimization

### Memory Management

**1. Document Storage Optimization:**

```rust
// Use Rope for efficient text manipulation
use ropey::Rope;

impl Backend {
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(mut document) = self.documents.get_mut(&params.text_document.uri) {
            for change in params.content_changes {
                match change.range {
                    Some(range) => {
                        // Incremental update - O(log n) complexity
                        let start_idx = self.position_to_byte_offset(&document.text, range.start);
                        let end_idx = self.position_to_byte_offset(&document.text, range.end);
                        document.text.remove(start_idx..end_idx);
                        document.text.insert(start_idx, &change.text);
                    }
                    None => {
                        // Full document replace - only when necessary
                        document.text = Rope::from_str(&change.text);
                    }
                }
            }
        }
    }
}
```

**2. AST Caching:**

```rust
use std::collections::HashMap;
use std::sync::Arc;

struct CachedAnalysis {
    ast: Arc<CleanASTNode>,
    version: i32,
    timestamp: std::time::Instant,
}

impl Backend {
    // Cache ASTs to avoid re-parsing unchanged documents
    async fn get_cached_ast(&self, uri: &Url) -> Option<Arc<CleanASTNode>> {
        if let Some(document) = self.documents.get(uri) {
            if let Some(cached) = self.ast_cache.get(uri) {
                if cached.version == document.version && 
                   cached.timestamp.elapsed().as_secs() < 300 { // 5 minute cache
                    return Some(cached.ast.clone());
                }
            }
        }
        None
    }
}
```

### CPU Performance

**1. Parallel Processing:**

```rust
use tokio::task;
use futures::future::join_all;

impl DiagnosticsProvider {
    async fn analyze_multiple_documents(&self, documents: Vec<&TextDocument>) -> Vec<Vec<Diagnostic>> {
        let analysis_futures = documents
            .into_iter()
            .map(|doc| task::spawn(self.analyze_single_document(doc.clone())))
            .collect::<Vec<_>>();
            
        let results = join_all(analysis_futures).await;
        results.into_iter().map(|r| r.unwrap()).collect()
    }
}
```

**2. Lazy Loading:**

```rust
impl CompletionProvider {
    // Load completion data on-demand
    fn get_stdlib_completions(&self) -> &Vec<CompletionItem> {
        self.stdlib_completions.get_or_init(|| {
            self.load_stdlib_completions_from_compiler()
        })
    }
}
```

### Benchmarking

**1. Performance Tests:**

```rust
// benches/language_server_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn completion_benchmark(c: &mut Criterion) {
    let server = setup_test_server();
    let test_document = load_large_test_document();
    
    c.bench_function("completion_at_method_call", |b| {
        b.iter(|| {
            black_box(server.provide_completions(
                &test_document.text,
                Position { line: 100, character: 20 }
            ))
        })
    });
}

criterion_group!(benches, completion_benchmark);
criterion_main!(benches);
```

**2. Memory Profiling:**

```bash
# Profile memory usage
cargo build --release
valgrind --tool=massif ./target/release/clean-language-server

# Analyze with heap profiler
cargo install dhat
# Add dhat profiling to main.rs
```

---

## Troubleshooting

### Common Issues

**1. Language Server Not Starting:**

```bash
# Check if server binary exists
ls -la ./target/release/clean-language-server

# Test server manually
./target/release/clean-language-server --version

# Check logs
tail -f ~/.local/share/clean-language-server/logs/server.log
```

**2. Completion Not Working:**

```rust
// Debug completion context
debug!("Completion requested at line: {}, char: {}", 
       position.line, position.character);
debug!("Line content: '{}'", line_str);
debug!("Detected context: {:?}", completion_context);
```

**3. High Memory Usage:**

```rust
// Monitor document cache size
impl Backend {
    fn log_memory_stats(&self) {
        let doc_count = self.documents.len();
        let ast_cache_size = self.ast_cache.len();
        
        info!("Memory stats - Documents: {}, Cached ASTs: {}", 
              doc_count, ast_cache_size);
              
        // Clear old cache entries
        if ast_cache_size > 100 {
            self.cleanup_old_cache_entries().await;
        }
    }
}
```

### Debugging Guide

**1. Enable Debug Logging:**

```rust
// Temporary debug logging
#[cfg(debug_assertions)]
{
    eprintln!("DEBUG: Processing document change for {}", uri);
    eprintln!("DEBUG: Change range: {:?}", change.range);
}
```

**2. LSP Message Tracing:**

```bash
# Enable LSP message logging in VS Code
"clean-language.trace.server": "verbose"
```

**3. Parser Debug Mode:**

```rust
impl CleanParser {
    pub async fn parse_debug(&self, text: &str) -> Result<CleanASTNode, Vec<ParseError>> {
        println!("=== PARSING DEBUG ===");
        println!("Input length: {}", text.len());
        println!("Input preview: {:.100}...", text);
        
        let result = self.parse(text).await;
        
        match &result {
            Ok(ast) => println!("Parse successful, AST nodes: {}", ast.node_count()),
            Err(errors) => println!("Parse failed with {} errors", errors.len()),
        }
        
        println!("=== END DEBUG ===");
        result
    }
}
```

---

## API Reference

### Core Types

```rust
// Document representation
pub struct TextDocumentItem {
    pub uri: Url,
    pub text: Rope,
    pub version: i32,
}

// AST Node types
pub enum CleanASTNode {
    Program { functions: Vec<CleanASTNode>, classes: Vec<CleanASTNode>, start_function: Option<Box<CleanASTNode>> },
    Function { name: String, parameters: Vec<Parameter>, return_type: Option<String>, body: Vec<CleanASTNode>, range: Range },
    Class { name: String, extends: Option<String>, fields: Vec<CleanASTNode>, constructor: Option<Box<CleanASTNode>>, methods: Vec<CleanASTNode>, range: Range },
    ApplyBlock { target: String, items: Vec<CleanASTNode>, range: Range },
    VariableDeclaration { var_type: String, name: String, value: Option<Box<CleanASTNode>>, range: Range },
    // Additional node types...
}

// Error types
pub struct ParseError {
    pub range: Range,
    pub message: String,
}
```

### Provider Interfaces

```rust
// Completion Provider
impl CompletionProvider {
    pub fn new() -> Self;
    pub async fn provide_completions(&self, text: &Rope, position: Position) -> Vec<CompletionItem>;
    fn get_keyword_completions(&self) -> Vec<CompletionItem>;
    fn get_type_completions(&self) -> Vec<CompletionItem>;
    fn get_method_completions(&self, prefix: &str) -> Vec<CompletionItem>;
}

// Diagnostics Provider  
impl DiagnosticsProvider {
    pub fn new() -> Self;
    pub async fn analyze(&self, ast: &CleanASTNode, text: &str) -> Vec<Diagnostic>;
    fn check_syntax_errors(&self, ast: &CleanASTNode) -> Vec<Diagnostic>;
    fn check_semantic_errors(&self, ast: &CleanASTNode) -> Vec<Diagnostic>;
}

// Hover Provider
impl HoverProvider {
    pub fn new() -> Self;
    pub async fn provide_hover(&self, text: &Rope, position: Position) -> Option<Hover>;
    fn get_symbol_at_position(&self, text: &Rope, position: Position) -> Option<Symbol>;
    fn create_hover_content(&self, symbol: &Symbol) -> MarkedString;
}
```

### Configuration Options

```rust
pub struct LanguageServerConfig {
    pub max_cached_documents: usize,
    pub completion_timeout_ms: u64,
    pub diagnostics_debounce_ms: u64,
    pub enable_semantic_analysis: bool,
    pub enable_performance_logging: bool,
}

impl Default for LanguageServerConfig {
    fn default() -> Self {
        Self {
            max_cached_documents: 100,
            completion_timeout_ms: 1000,
            diagnostics_debounce_ms: 500,
            enable_semantic_analysis: true,
            enable_performance_logging: false,
        }
    }
}
```

---

## Contributing

### Development Workflow

1. **Fork & Clone:**
   ```bash
   git clone <your-fork>
   cd clean-language-compiler/language-server
   ```

2. **Create Feature Branch:**
   ```bash
   git checkout -b feature/new-lsp-capability
   ```

3. **Development:**
   ```bash
   cargo watch -x "build --release"
   # Develop your feature
   cargo test
   cargo clippy
   cargo fmt
   ```

4. **Testing:**
   ```bash
   # Unit tests
   cargo test --lib
   
   # Integration tests
   cargo test --test integration
   
   # Manual testing with VS Code
   code test-project/
   ```

5. **Documentation:**
   ```bash
   # Update this documentation
   # Add inline documentation to new code
   cargo doc --no-deps --open
   ```

6. **Pull Request:**
   ```bash
   git add .
   git commit -m "feat: add new LSP capability"
   git push origin feature/new-lsp-capability
   # Create PR on GitHub
   ```

### Code Standards

**Rust Standards:**
- Follow `rustfmt` formatting
- Pass `clippy` lints
- Include comprehensive error handling
- Add integration tests for new features
- Document public APIs with `///` comments

**LSP Standards:**
- Follow LSP specification strictly
- Handle all error cases gracefully
- Provide meaningful error messages
- Support incremental updates
- Maintain backwards compatibility

### Testing Requirements

**Required Tests for New Features:**
- Unit tests for core logic
- Integration tests with real Clean files
- Error handling tests
- Performance regression tests
- VS Code extension integration tests

---

## Conclusion

The Clean Language Server provides a robust foundation for IDE support with comprehensive LSP capabilities. This documentation serves as both a user guide and developer reference for maintaining and extending the server's functionality.

For the most up-to-date information, always refer to the source code and inline documentation. The language server is designed to stay synchronized with the Clean Language compiler, ensuring consistent and accurate language support across all development tools.

**Key Contacts:**
- **Lead Developer:** Ivan Pasco
- **Project Repository:** [Clean Language Compiler](https://github.com/Ivan-Pasco/clean-language-extension)
- **Documentation:** `/documentation/language-server.md`

**Version History:**
- **1.0.0:** Initial release with core LSP features
- **1.1.0:** Planned - Go-to-definition and references
- **1.2.0:** Planned - Advanced refactoring support
- **1.3.0:** Planned - Semantic highlighting and symbols