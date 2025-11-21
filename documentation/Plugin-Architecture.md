# Clean Language Plugin Architecture

> Extend Clean Language with custom DSL blocks that compile to WebAssembly

## Table of Contents

1. [Introduction](#introduction)
2. [Architecture Overview](#architecture-overview)
3. [How Plugins Work](#how-plugins-work)
4. [Creating Your First Plugin](#creating-your-first-plugin)
5. [Plugin API Reference](#plugin-api-reference)
6. [Built-in Plugins](#built-in-plugins)
7. [Advanced Topics](#advanced-topics)
8. [Best Practices](#best-practices)
9. [Troubleshooting](#troubleshooting)

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
└── frame_web.rs     # Built-in web plugin (endpoints:)
```

| Component | Purpose |
|-----------|---------|
| `FrameworkPlugin` | Trait that all plugins implement |
| `PluginRegistry` | Stores and dispatches to registered plugins |
| `PluginExpander` | Walks AST and triggers plugin expansion |
| `FrameworkBlock` | AST node representing a DSL block |

---

## How Plugins Work

### The Plugin Lifecycle

```
1. REGISTRATION
   Plugin registers with the compiler, declaring which blocks it handles

2. PARSING
   Parser encounters unknown "identifier:" block
   Creates FrameworkBlock AST node (doesn't parse content)

3. EXPANSION
   PluginExpander finds FrameworkBlock nodes
   Dispatches to registered plugin handler
   Plugin returns standard Clean Language AST

4. COMPILATION
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

*Document Version: 1.0.0*
*Last Updated: 2025*
*Clean Language Compiler Version: 0.13.1+*
