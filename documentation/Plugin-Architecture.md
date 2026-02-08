# Clean Language Plugin Architecture

> Extend Clean Language with custom DSL blocks that compile to WebAssembly

## Table of Contents

1. [Introduction](#introduction)
2. [Architecture Overview](#architecture-overview)
3. [How Plugins Work](#how-plugins-work)
4. [Creating Your First Plugin](#creating-your-first-plugin)
5. [Plugin API Reference](#plugin-api-reference)
6. [Language Server Integration](#language-server-integration)
7. [Built-in Plugins](#built-in-plugins)
8. [Advanced Topics](#advanced-topics)
9. [Best Practices](#best-practices)
10. [Troubleshooting](#troubleshooting)

---

## Introduction

The Clean Language Plugin Architecture allows you to extend the language with custom Domain-Specific Language (DSL) blocks. These blocks are transformed into standard Clean Language code before compilation, enabling powerful abstractions without modifying the core compiler.

### What Can Plugins Do?

- **Add new syntax blocks** like `endpoints:`, `data`, or `component`
- **Generate boilerplate code** automatically
- **Create framework-specific features** (web, database, UI)
- **Build internal DSLs** for your domain

### Key Benefits

| Benefit | Description |
|---------|-------------|
| **Non-invasive** | Core language stays minimal and clean |
| **Composable** | Multiple plugins work together seamlessly |
| **Type-safe** | Generated code goes through full type checking |
| **Debuggable** | Expansion happens before compilation, errors point to source |

---

## Architecture Overview

### The Compilation Pipeline

Clean Language uses a 7-stage compilation pipeline. Plugins operate at **Stage 2.5**, between parsing and semantic analysis:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CLEAN LANGUAGE COMPILER PIPELINE                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ┌──────────┐    ┌──────────┐    ┌──────────────────┐    ┌──────────┐     │
│   │  Source  │───▶│  Lexer   │───▶│     Parser       │───▶│   AST    │     │
│   │  (.cln)  │    │  (1)     │    │     (2)          │    │          │     │
│   └──────────┘    └──────────┘    └──────────────────┘    └────┬─────┘     │
│                                                                 │           │
│                                          ┌──────────────────────▼─────┐     │
│                                          │   PLUGIN EXPANSION (2.5)   │     │
│                                          │                            │     │
│                                          │  ┌────────────────────┐    │     │
│                                          │  │  PluginRegistry    │    │     │
│                                          │  │  ┌──────────────┐  │    │     │
│                                          │  │  │ WebPlugin    │  │    │     │
│                                          │  │  │ DataPlugin   │  │    │     │
│                                          │  │  │ YourPlugin   │  │    │     │
│                                          │  │  └──────────────┘  │    │     │
│                                          │  └────────────────────┘    │     │
│                                          └──────────────────────┬─────┘     │
│                                                                 │           │
│   ┌──────────┐    ┌──────────┐    ┌──────────────────┐    ┌────▼─────┐     │
│   │   WASM   │◀───│   MIR    │◀───│   TypeChecker    │◀───│   HIR    │     │
│   │   (7)    │    │   (6)    │    │      (5)         │    │   (3)    │     │
│   └──────────┘    └──────────┘    └──────────────────┘    └──────────┘     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Core Components

```
src/plugins/
├── mod.rs           # Plugin trait and exports
├── registry.rs      # Plugin registration and dispatch
├── expander.rs      # AST transformation engine
├── plugin_abi.rs    # Plugin manifest and ABI definitions (NEW)
├── wasm_adapter.rs  # Wraps WASM plugins as FrameworkPlugin (NEW)
└── wasm_loader.rs   # External plugin loader from ~/.cleen/plugins/ (NEW)
```

| Component | Purpose |
|-----------|---------|
| `FrameworkPlugin` | Trait that all plugins implement |
| `PluginRegistry` | Stores and dispatches to registered plugins |
| `PluginExpander` | Walks AST and triggers plugin expansion |
| `FrameworkBlock` | AST node representing a DSL block |
| `WasmPluginLoader` | Discovers and loads external WASM plugins |
| `WasmPluginAdapter` | Wraps WASM modules to implement FrameworkPlugin |
| `PluginManifest` | Plugin metadata from plugin.toml |

---

## How Plugins Work

### Plugin Types

Clean Language supports two types of plugins:

| Type | Written In | Location | Registration |
|------|------------|----------|--------------|
| **Built-in** | Rust | Compiled into compiler | Manual in lib.rs |
| **External** | Clean Language | `~/.cleen/plugins/` | Auto-loaded from `import:` |

### Plugin Auto-Detection (v2.1)

The compiler automatically detects and loads plugins based on the file's location in the project structure. This eliminates the need for explicit `plugins:` declarations in most cases:

| File Path Pattern | Auto-Detected Plugins |
|-------------------|----------------------|
| `/api/`, `/backend/api/`, `/server/api/`, `/endpoints/` | `frame.httpserver`, `frame.data`, `frame.auth` |
| `/data/`, `/models/`, `/server/models/` | `frame.data` |
| `/auth/`, `/config/auth/` | `frame.auth` |
| `/canvas/` | `frame.canvas` |
| `/ui/`, `/components/`, `/screens/` | `frame.ui` |

**Example - No explicit imports needed:**

```clean
// File: app/api/users.cln
// Plugins auto-detected: frame.httpserver, frame.data, frame.auth

functions:
    string getUsers()
        string result = _db_query("SELECT * FROM users", "[]")
        return result

start:
    integer s = _http_route("GET", "/users", 0)
    printl("Users API ready")
```

The compiler merges auto-detected plugins with any explicitly declared plugins, so you can still add additional plugins via the `plugins:` block when needed.

### External Plugin Architecture (v2)

External plugins are written in Clean Language, compiled to WASM, and loaded at compile-time:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         EXTERNAL PLUGIN SYSTEM                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ~/.cleen/plugins/                                                         │
│   ├── frame.web/                                                            │
│   │   └── 1.0.0/                                                            │
│   │       ├── plugin.toml      ◀─── Plugin manifest                         │
│   │       └── plugin.wasm      ◀─── Compiled Clean Language                 │
│   └── frame.data/                                                           │
│       └── 1.0.0/                                                            │
│           ├── plugin.toml                                                   │
│           └── plugin.wasm                                                   │
│                                                                             │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                      WasmPluginLoader                               │   │
│   │                                                                     │   │
│   │   1. Parse import: blocks from source code                         │   │
│   │   2. Find plugin directories in ~/.cleen/plugins/                  │   │
│   │   3. Load plugin.toml manifest                                     │   │
│   │   4. Compile plugin.wasm with wasmtime                             │   │
│   │   5. Create WasmPluginAdapter for each plugin                      │   │
│   │   6. Register adapters in PluginRegistry                           │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Plugin Manifest (plugin.toml)

Every external plugin requires a manifest file:

```toml
[plugin]
name = "frame.web"
version = "1.0.0"
description = "Web framework plugin for Clean Language"
author = "Clean Language Team"

[compatibility]
min_compiler_version = "0.15.0"

[handles]
blocks = ["server", "route", "middleware"]

[exports]
expand = "expand"              # Required: expand(block_ptr, block_len) -> result_ptr
validate = "validate"          # Optional: validate(block_ptr, block_len) -> error_ptr
get_keywords = "get_keywords"  # Optional: () -> keywords_ptr
```

### The Plugin Lifecycle

```
1. IMPORT DETECTION
   Compiler parses import: blocks in source file
   Extracts plugin names (e.g., "frame.web", "frame.data")

2. PLUGIN LOADING
   WasmPluginLoader finds plugins in ~/.cleen/plugins/
   Loads plugin.toml manifest and plugin.wasm module
   Creates WasmPluginAdapter wrapping each WASM module

3. REGISTRATION
   Plugin adapters register with PluginRegistry
   Each declares which blocks it handles

4. PARSING
   Parser encounters unknown "identifier:" block
   Creates FrameworkBlock AST node (doesn't parse content)

5. EXPANSION
   PluginExpander finds FrameworkBlock nodes
   Dispatches to registered plugin handler
   Plugin returns standard Clean Language AST

6. COMPILATION
   Expanded AST continues through normal compilation
   Type checking, optimization, WASM generation
```

### Example: What Happens to `endpoints:`

**Input (your code):**
```clean
endpoints:
    GET "/users" -> listUsers
    POST "/users" -> createUser
```

**After Parsing (AST):**
```rust
Statement::FrameworkBlock {
    name: "endpoints",
    content: "GET \"/users\" -> listUsers\nPOST \"/users\" -> createUser",
    attributes: [],
    location: Some(...)
}
```

**After Plugin Expansion (Clean AST):**
```rust
Statement::FunctionsBlock {
    functions: [
        Function {
            name: "__frame_register_routes",
            body: [
                // router.get("/users", listUsers)
                // router.post("/users", createUser)
            ]
        }
    ]
}
```

**Final (compiled to WASM):**
The generated function is compiled like any other Clean code.

---

## Using External Plugins

### Importing Plugins in Clean Code

To use external plugins, add an `import:` block at the top of your Clean file:

```clean
import:
    frame.web
    frame.data

server: port=3000
    route: method="GET" path="/users"
        users = User.all()
        return users

model: name="User" table="users"
    string name
    string email
```

### Compiling with Plugins

Use the `--plugins` flag to enable external plugin loading:

```bash
# Compile with external plugins
cln compile app.cln -o app.wasm --plugins

# Or use compile_with_external_plugins() programmatically
```

### Plugin Installation

External plugins are installed to `~/.cleen/plugins/`:

```bash
# Install from registry (future)
cleen plugin install frame.web

# Install local plugin
cleen plugin install ./my-plugin/

# List installed plugins
cleen plugin list

# Plugin structure after installation:
~/.cleen/plugins/
├── frame.web/
│   └── 1.0.0/
│       ├── plugin.toml
│       └── plugin.wasm
└── frame.data/
    └── 1.0.0/
        ├── plugin.toml
        └── plugin.wasm
```

### Programmatic Plugin Loading

You can load external plugins programmatically:

```rust
use clean_language_compiler::plugins::WasmPluginLoader;

// Create loader (uses ~/.cleen/plugins/ by default)
let mut loader = WasmPluginLoader::new()?;

// Load specific plugins
let registry = loader.load_plugins(&["frame.web".to_string()])?;

// Or use custom plugins directory
let mut loader = WasmPluginLoader::with_plugins_dir("/path/to/plugins")?;

// Check if plugin is installed
if loader.is_plugin_installed("frame.web") {
    println!("frame.web is available");
}

// List all installed plugins
for (name, version) in loader.list_installed_plugins()? {
    println!("{} v{}", name, version);
}
```

---

## Creating Your First Plugin

Let's build a simple `log:` plugin that generates logging statements.

### Step 1: Create the Plugin File

Create `src/plugins/log_plugin.rs`:

```rust
use crate::ast::{Expression, Statement, Value, SourceLocation};
use crate::plugins::{FrameworkBlock, FrameworkPlugin, PluginError, PluginResult};

/// A simple logging plugin that expands log: blocks into print statements
pub struct LogPlugin;

impl LogPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FrameworkPlugin for LogPlugin {
    /// Unique name for this plugin
    fn name(&self) -> &'static str {
        "clean.log"
    }

    /// Block identifiers this plugin handles
    fn handles(&self) -> &'static [&'static str] {
        &["log"]
    }

    /// Transform the DSL block into Clean Language statements
    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        let mut statements = Vec::new();

        for line in block.content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Generate: print("[LOG] " + message)
            statements.push(Statement::Print {
                expression: Expression::Literal(Value::String(
                    format!("[LOG] {}", trimmed)
                )),
                newline: true,
                location: block.location.clone(),
            });
        }

        Ok(statements)
    }

    /// Optional: Validate before expansion
    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        if block.content.trim().is_empty() {
            return Err(PluginError::ValidationFailed {
                plugin_name: self.name().to_string(),
                message: "log: block cannot be empty".to_string(),
                location: block.location.clone(),
            });
        }
        Ok(())
    }

    /// Plugin version
    fn version(&self) -> &'static str {
        "1.0.0"
    }
}
```

### Step 2: Register the Module

Add to `src/plugins/mod.rs`:

```rust
pub mod log_plugin;
pub use log_plugin::LogPlugin;
```

### Step 3: Register with the Compiler

In `src/lib.rs`, add your plugin to the registry:

```rust
use crate::plugins::{PluginRegistry, PluginExpander, WebPlugin, LogPlugin};
use std::sync::Arc;

let mut registry = PluginRegistry::new();
registry.register(Arc::new(WebPlugin::new()))?;
registry.register(Arc::new(LogPlugin::new()))?;  // Add this line
```

### Step 4: Use Your Plugin

Now you can use the `log:` block in Clean code:

```clean
start()
    log:
        Application starting
        Loading configuration
        Ready to serve requests

    // Rest of your code
```

This expands to:

```clean
start()
    print("[LOG] Application starting")
    print("[LOG] Loading configuration")
    print("[LOG] Ready to serve requests")
```

---

## Plugin API Reference

### The `FrameworkPlugin` Trait

```rust
pub trait FrameworkPlugin: Send + Sync {
    /// Returns the plugin name (e.g., "frame.web", "mycompany.auth")
    /// Used for error messages and debugging
    fn name(&self) -> &'static str;

    /// Returns block identifiers this plugin handles
    /// Example: &["endpoints", "routes"] to handle both endpoints: and routes:
    fn handles(&self) -> &'static [&'static str];

    /// Transforms a framework block into standard Clean Language statements
    /// This is where your DSL logic lives
    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>>;

    /// Optional: Validate block content before expansion
    /// Return Err to reject invalid DSL syntax
    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        Ok(())  // Default: accept all
    }

    /// Optional: Plugin version for compatibility
    fn version(&self) -> &'static str {
        "1.0.0"
    }
}
```

### The `FrameworkBlock` Struct

```rust
/// Represents a DSL block in the AST
pub struct FrameworkBlock {
    /// Block identifier: "endpoints", "data", "component", etc.
    pub name: String,

    /// Raw content of the block (your DSL to parse)
    pub content: String,

    /// Optional attributes: @version("v1"), @auth, etc.
    pub attributes: Vec<FrameworkAttribute>,

    /// Source location for error reporting
    pub location: Option<SourceLocation>,
}

/// Attribute on a framework block
pub struct FrameworkAttribute {
    pub name: String,              // "version", "auth", "cache"
    pub value: Option<String>,     // Some("v1"), None for flags
    pub location: Option<SourceLocation>,
}
```

### Error Types

```rust
pub enum PluginError {
    /// No plugin registered for the block type
    UnknownBlockType {
        block_name: String,
        location: Option<SourceLocation>,
    },

    /// Plugin expansion failed
    ExpansionFailed {
        plugin_name: String,
        block_name: String,
        message: String,
        location: Option<SourceLocation>,
    },

    /// Validation rejected the block
    ValidationFailed {
        plugin_name: String,
        message: String,
        location: Option<SourceLocation>,
    },

    /// Two plugins tried to handle the same block type
    RegistrationConflict {
        block_name: String,
        existing_plugin: String,
        new_plugin: String,
    },

    /// Syntax error in DSL content
    ParseError {
        message: String,
        line: usize,
        column: usize,
        location: Option<SourceLocation>,
    },
}
```

### The `PluginRegistry`

```rust
impl PluginRegistry {
    /// Create empty registry
    pub fn new() -> Self;

    /// Register a plugin (fails if block type already registered)
    pub fn register(&mut self, plugin: Arc<dyn FrameworkPlugin>) -> Result<(), PluginError>;

    /// Check if a block type is handled
    pub fn handles(&self, block_name: &str) -> bool;

    /// Expand a single block
    pub fn expand(&self, block: &FrameworkBlock) -> Result<Vec<Statement>, PluginError>;

    /// List registered plugins
    pub fn registered_plugins(&self) -> &[String];

    /// List handled block types
    pub fn handled_block_types(&self) -> Vec<&str>;
}
```

---

## Language Server Integration

Plugins can provide IDE support by implementing optional Language Server Protocol (LSP) methods. This enables autocomplete, hover documentation, and real-time diagnostics for your custom DSL blocks.

### Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                    PLUGIN-AWARE LANGUAGE SERVER                   │
├──────────────────────────────────────────────────────────────────┤
│   ┌──────────────┐         ┌──────────────────────┐              │
│   │   Plugins    │────────▶│   PluginRegistry     │              │
│   │              │         │   - handles: [...]    │              │
│   │ - endpoints: │         │   - completions       │              │
│   │ - data:      │         │   - hover docs        │              │
│   │ - myblock:   │         │   - diagnostics       │              │
│   └──────────────┘         └───────────┬──────────┘              │
│                                        │                          │
│                                        ▼                          │
│   ┌────────────────────────────────────────────────────────┐     │
│   │                  Language Server                        │     │
│   │   CompletionProvider  ◀──── registry.get_completions() │     │
│   │   HoverProvider       ◀──── registry.get_hover_info()  │     │
│   │   DiagnosticsProvider ◀──── registry.get_diagnostics() │     │
│   │   SemanticTokens      ◀──── registry.get_all_keywords()│     │
│   └────────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────────┘
```

### LSP Methods

Implement these optional methods in your `FrameworkPlugin` to provide IDE support:

```rust
pub trait FrameworkPlugin: Send + Sync {
    // ... core methods (name, handles, expand) ...

    /// Keywords for syntax highlighting
    fn get_keywords(&self) -> &'static [&'static str] {
        &[]  // Default: no keywords
    }

    /// Autocomplete suggestions
    fn get_completions(&self, ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
        Vec::new()  // Default: no completions
    }

    /// Hover documentation
    fn get_hover_info(&self, keyword: &str) -> Option<PluginHoverInfo> {
        None  // Default: no hover info
    }

    /// Real-time diagnostics
    fn get_diagnostics(&self, content: &str) -> Vec<PluginDiagnostic> {
        Vec::new()  // Default: no diagnostics
    }

    /// Plugin description for documentation
    fn description(&self) -> &'static str {
        "A Clean Language plugin"
    }
}
```

### Example: Adding IDE Support

Here's a complete example of a plugin with full IDE support:

```rust
use crate::plugins::*;

pub struct HttpPlugin;

impl FrameworkPlugin for HttpPlugin {
    fn name(&self) -> &'static str {
        "clean.http"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["endpoints"]
    }

    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        // ... expansion logic ...
    }

    // =========================================================================
    // Language Server Integration
    // =========================================================================

    /// Keywords for syntax highlighting
    fn get_keywords(&self) -> &'static [&'static str] {
        &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
    }

    /// Autocomplete suggestions
    fn get_completions(&self, ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
        let prefix = ctx.prefix.to_uppercase();

        // HTTP method completions
        let methods = vec![
            ("GET", "HTTP GET request", "GET \"${1:/path}\" -> ${2:handler}"),
            ("POST", "HTTP POST request", "POST \"${1:/path}\" -> ${2:handler}"),
            ("PUT", "HTTP PUT request", "PUT \"${1:/path}\" -> ${2:handler}"),
            ("DELETE", "HTTP DELETE request", "DELETE \"${1:/path}\" -> ${2:handler}"),
        ];

        methods
            .into_iter()
            .filter(|(m, _, _)| m.starts_with(&prefix) || prefix.is_empty())
            .map(|(label, detail, snippet)| PluginCompletionItem {
                label: label.to_string(),
                kind: PluginCompletionKind::Keyword,
                detail: Some(detail.to_string()),
                documentation: Some(format!(
                    "**{}**\n\nDefine a {} endpoint.\n\n```clean\n{}\n```",
                    label, label, snippet.replace("${1:", "").replace("${2:", "").replace("}", "")
                )),
                insert_text: Some(snippet.to_string()),
                is_snippet: true,
            })
            .collect()
    }

    /// Hover documentation
    fn get_hover_info(&self, keyword: &str) -> Option<PluginHoverInfo> {
        match keyword.to_uppercase().as_str() {
            "GET" => Some(PluginHoverInfo {
                content: "**GET** - HTTP GET Request\n\n\
                    Retrieves data from the server.\n\n\
                    ```clean\nGET \"/users\" -> listUsers\n```\n\n\
                    The handler function receives query parameters.".to_string(),
            }),
            "POST" => Some(PluginHoverInfo {
                content: "**POST** - HTTP POST Request\n\n\
                    Submits data to the server.\n\n\
                    ```clean\nPOST \"/users\" -> createUser\n```\n\n\
                    The handler function receives the request body.".to_string(),
            }),
            // ... other methods ...
            _ => None,
        }
    }

    /// Real-time diagnostics
    fn get_diagnostics(&self, content: &str) -> Vec<PluginDiagnostic> {
        let mut diagnostics = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Check for valid HTTP method
            let first_word = trimmed.split_whitespace().next().unwrap_or("");
            let valid_methods = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

            if !valid_methods.contains(&first_word.to_uppercase().as_str()) {
                diagnostics.push(PluginDiagnostic {
                    message: format!(
                        "Unknown HTTP method '{}'. Expected one of: {}",
                        first_word,
                        valid_methods.join(", ")
                    ),
                    severity: PluginDiagnosticSeverity::Error,
                    line: line_num + 1,
                    column: 1,
                    length: first_word.len(),
                });
            }

            // Check for missing arrow
            if !trimmed.contains("->") {
                diagnostics.push(PluginDiagnostic {
                    message: "Missing '->' between path and handler".to_string(),
                    severity: PluginDiagnosticSeverity::Error,
                    line: line_num + 1,
                    column: trimmed.len(),
                    length: 1,
                });
            }
        }

        diagnostics
    }

    fn description(&self) -> &'static str {
        "Define HTTP API endpoints with a clean DSL syntax"
    }
}
```

### LSP Types Reference

#### PluginCompletionItem

```rust
pub struct PluginCompletionItem {
    /// Label shown in completion list
    pub label: String,

    /// Kind of completion
    pub kind: PluginCompletionKind,

    /// Short description
    pub detail: Option<String>,

    /// Full documentation (markdown)
    pub documentation: Option<String>,

    /// Text to insert (with snippet placeholders)
    pub insert_text: Option<String>,

    /// Whether insert_text contains snippet placeholders
    pub is_snippet: bool,
}

pub enum PluginCompletionKind {
    Keyword,    // GET, POST, etc.
    Function,   // Handler functions
    Snippet,    // Code templates
    Type,       // Type annotations
    Property,   // Configuration options
    Variable,   // Variables
    Operator,   // Operators
}
```

#### PluginLspContext

```rust
pub struct PluginLspContext<'a> {
    /// Block name (e.g., "endpoints")
    pub block_name: &'a str,

    /// Full content inside the block
    pub block_content: &'a str,

    /// Current line (0-based)
    pub line: usize,

    /// Current column (0-based)
    pub column: usize,

    /// Word being typed (for filtering)
    pub prefix: &'a str,
}
```

#### PluginDiagnostic

```rust
pub struct PluginDiagnostic {
    /// Error/warning message
    pub message: String,

    /// Severity level
    pub severity: PluginDiagnosticSeverity,

    /// Line number (1-based)
    pub line: usize,

    /// Column number (1-based)
    pub column: usize,

    /// Length of problematic text
    pub length: usize,
}

pub enum PluginDiagnosticSeverity {
    Error,   // Compilation will fail
    Warning, // Potential problem
    Info,    // Informational
    Hint,    // Suggestion
}
```

### Benefits of IDE Integration

| Feature | User Experience |
|---------|-----------------|
| **Autocomplete** | Type `GET` and see method suggestions with snippets |
| **Hover Docs** | Hover over `POST` to see usage documentation |
| **Diagnostics** | See red squiggles for invalid HTTP methods |
| **Syntax Highlighting** | Keywords like `GET`, `POST` are colorized |

---

## Built-in Plugins

### WebPlugin (`frame.web`)

Handles the `endpoints:` block for HTTP API definition.

**DSL Syntax:**
```clean
endpoints:
    METHOD "path" -> handlerFunction
    METHOD /path/without/quotes -> handler
```

**Supported Methods:** GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS

**Example:**
```clean
import:
    frame.web

endpoints:
    GET "/api/users" -> listUsers
    GET "/api/users/{id}" -> getUser
    POST "/api/users" -> createUser
    PUT "/api/users/{id}" -> updateUser
    DELETE "/api/users/{id}" -> deleteUser

functions:
    list<User> listUsers()
        return database.query("SELECT * FROM users")

    User getUser(string id)
        return database.findById("users", id)
```

**Expands To:**
```clean
functions:
    void __frame_register_routes()
        router.get("/api/users", listUsers)
        router.get("/api/users/{id}", getUser)
        router.post("/api/users", createUser)
        router.put("/api/users/{id}", updateUser)
        router.delete("/api/users/{id}", deleteUser)
```

---

## Advanced Topics

### Generating Complex AST Nodes

Here's how to generate different Clean Language constructs:

```rust
use crate::ast::*;

// Variable declaration: integer count = 0
Statement::VariableDecl {
    name: "count".to_string(),
    type_: Type::Integer,
    initializer: Some(Expression::Literal(Value::Integer(0))),
    location: None,
}

// Function call: print("Hello")
Statement::Expression {
    expr: Expression::Call(
        "print".to_string(),
        vec![Expression::Literal(Value::String("Hello".to_string()))]
    ),
    location: None,
}

// Method call: router.get("/path", handler)
Statement::Expression {
    expr: Expression::MethodCall {
        object: Box::new(Expression::Variable("router".to_string())),
        method: "get".to_string(),
        arguments: vec![
            Expression::Literal(Value::String("/path".to_string())),
            Expression::Variable("handler".to_string()),
        ],
        location: SourceLocation::default(),
    },
    location: None,
}

// If statement
Statement::If {
    condition: Expression::Binary(
        Box::new(Expression::Variable("x".to_string())),
        BinaryOperator::Greater,
        Box::new(Expression::Literal(Value::Integer(0))),
    ),
    then_branch: vec![/* statements */],
    else_branch: Some(vec![/* statements */]),
    location: None,
}

// Function definition
Function {
    name: "myFunction".to_string(),
    type_parameters: vec![],
    type_constraints: vec![],
    parameters: vec![
        Parameter::new("arg1".to_string(), Type::String),
        Parameter::new("arg2".to_string(), Type::Integer),
    ],
    return_type: Type::Boolean,
    body: vec![/* statements */],
    description: Some("Generated function".to_string()),
    syntax: FunctionSyntax::Simple,
    visibility: Visibility::Public,
    modifier: FunctionModifier::None,
    location: None,
}
```

### Parsing Custom DSL Syntax

For complex DSLs, implement a proper parser:

```rust
impl MyPlugin {
    fn parse_dsl(&self, content: &str, location: &Option<SourceLocation>)
        -> PluginResult<Vec<MyDslNode>>
    {
        let mut nodes = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Parse your DSL syntax
            let node = self.parse_line(trimmed, line_num + 1, location)?;
            nodes.push(node);
        }

        Ok(nodes)
    }

    fn parse_line(&self, line: &str, line_num: usize, location: &Option<SourceLocation>)
        -> PluginResult<MyDslNode>
    {
        // Your parsing logic here
        // Return PluginError::ParseError for syntax errors

        if !line.contains("->") {
            return Err(PluginError::ParseError {
                message: format!("Expected '->' in: {}", line),
                line: line_num,
                column: 1,
                location: location.clone(),
            });
        }

        // Parse and return
        Ok(MyDslNode { /* ... */ })
    }
}
```

### Handling Attributes

Access attributes passed to your block:

```clean
@version("v2")
@auth
@cache(ttl: 300)
endpoints:
    GET "/users" -> listUsers
```

```rust
fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
    // Check for specific attributes
    let version = block.attributes.iter()
        .find(|a| a.name == "version")
        .and_then(|a| a.value.as_ref())
        .unwrap_or(&"v1".to_string());

    let requires_auth = block.attributes.iter()
        .any(|a| a.name == "auth");

    let cache_ttl = block.attributes.iter()
        .find(|a| a.name == "cache")
        .and_then(|a| a.value.as_ref())
        .and_then(|v| v.parse::<u32>().ok());

    // Generate code based on attributes
    // ...
}
```

### Nested Block Expansion

The expander automatically handles nested framework blocks:

```rust
// If your plugin generates code that contains another framework block,
// it will be expanded by the appropriate plugin

fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
    Ok(vec![
        // This will be expanded by the log plugin!
        Statement::FrameworkBlock {
            name: "log".to_string(),
            content: "Generated by my plugin".to_string(),
            attributes: vec![],
            location: block.location.clone(),
        }
    ])
}
```

---

## Best Practices

### 1. Provide Clear Error Messages

```rust
// Bad
Err(PluginError::ParseError {
    message: "Invalid syntax".to_string(),
    ...
})

// Good
Err(PluginError::ParseError {
    message: format!(
        "Expected HTTP method (GET, POST, etc.) but found '{}'. \
         Valid format: METHOD \"/path\" -> handler",
        found_token
    ),
    line: line_num,
    column: 1,
    location: block.location.clone(),
})
```

### 2. Validate Before Expanding

```rust
impl FrameworkPlugin for MyPlugin {
    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        // Check required structure
        if !block.content.contains("->") {
            return Err(PluginError::ValidationFailed {
                plugin_name: self.name().to_string(),
                message: "Block must contain at least one mapping with '->'".to_string(),
                location: block.location.clone(),
            });
        }

        // Check for common mistakes
        if block.content.contains("=>") {
            return Err(PluginError::ValidationFailed {
                plugin_name: self.name().to_string(),
                message: "Use '->' not '=>' for mappings".to_string(),
                location: block.location.clone(),
            });
        }

        Ok(())
    }
}
```

### 3. Preserve Source Locations

Always propagate locations for good error messages:

```rust
fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
    Ok(vec![Statement::Print {
        expression: /* ... */,
        newline: true,
        location: block.location.clone(),  // Preserve location!
    }])
}
```

### 4. Generate Deterministic Output

The same input should always produce the same output:

```rust
// Bad - non-deterministic
let id = uuid::Uuid::new_v4();  // Different every time!

// Good - deterministic
let id = format!("__generated_{}_{}", block.name, line_num);
```

### 5. Use Descriptive Generated Names

```rust
// Bad
let func_name = "f1";

// Good
let func_name = format!("__frame_{}_register", block.name);
```

### 6. Document Your DSL

Include examples in your plugin:

```rust
/// Handles the `myblock:` DSL for XYZ functionality.
///
/// ## Syntax
///
/// ```clean
/// myblock:
///     item1 -> target1
///     item2 -> target2
/// ```
///
/// ## Attributes
///
/// - `@async` - Generate async handlers
/// - `@prefix("api")` - Add path prefix
///
/// ## Example
///
/// ```clean
/// @prefix("api/v1")
/// myblock:
///     users -> handleUsers
///     posts -> handlePosts
/// ```
pub struct MyPlugin;
```

---

## Troubleshooting

### "Unexpanded framework block" Error

**Cause:** A `FrameworkBlock` reached compilation without being expanded.

**Solutions:**
1. Ensure your plugin is registered in `src/lib.rs`
2. Check that `handles()` returns the correct block name
3. Verify the block name in your code matches exactly (case-sensitive)

### "Registration conflict" Error

**Cause:** Two plugins trying to handle the same block type.

**Solution:** Each block type can only have one handler. Either:
- Rename one of the block types
- Merge the plugins into one

### Plugin Not Being Called

**Debugging steps:**

```rust
// Add tracing to your plugin
fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
    tracing::debug!(
        plugin = self.name(),
        block = %block.name,
        content_len = block.content.len(),
        "Plugin expand called"
    );

    // ... rest of expansion
}
```

Run with debug logging:
```bash
RUST_LOG=debug cargo run -- compile input.cln -o output.wasm
```

### Generated Code Has Type Errors

Your plugin generates syntactically correct but semantically invalid code.

**Solution:** Test your generated AST by manually writing the equivalent Clean code:

```clean
// If your plugin should generate this:
functions:
    void __register()
        router.get("/path", handler)

// Write it manually first and compile to verify it works
// Then match your AST generation to this structure
```

---

## Summary

The Clean Language Plugin Architecture provides a powerful way to extend the language with custom DSLs while keeping the core language minimal. Key points:

1. **Plugins transform DSL blocks into standard Clean code** before type checking
2. **Implement `FrameworkPlugin` trait** with `name()`, `handles()`, and `expand()`
3. **Register plugins** in the compiler's plugin registry
4. **Generated code is fully type-checked** like hand-written code
5. **Preserve source locations** for good error messages

Happy plugin development!

---

*Document Version: 2.1.0*
*Last Updated: December 2025*
*Clean Language Compiler Version: 0.15.0+*
