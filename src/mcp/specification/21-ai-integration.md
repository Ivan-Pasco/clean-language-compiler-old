# 21. AI Integration

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Clean Language provides built-in features to support AI-assisted development by linking code to formal specifications and documenting function intent.

### Overview

The AI Integration features enable:
- **Traceability**: Link functions to their specification documents
- **Intent Documentation**: Describe function purpose in natural language
- **Source Attribution**: Mark generated files with their specification source

These features are language-level constructs that enhance code clarity and enable tools to understand the relationship between specifications and implementations.

### The `spec` Keyword

The `spec` keyword links a function or method to its specification document.

**Syntax:**
```clean
spec "path/to/specification.spec.cln"
```

**Rules:**
- Can only appear inside function or method bodies
- Must appear before other statements (except `intent`)
- Path is relative to the project root
- Multiple `spec` declarations are allowed (for referencing multiple specs)

**Example:**
```clean
functions:
    number calculateDiscount(number price, number percentage)
        spec "specs/pricing/discount.spec.cln"
        require percentage >= 0 and percentage <= 100
        return price * (percentage / 100)
```

### The `intent` Keyword

The `intent` keyword describes a function's purpose in natural language.

**Syntax:**
```clean
intent "Natural language description of function purpose"
```

**Rules:**
- Can only appear inside function or method bodies
- Must appear before other statements (except `spec`)
- Provides human-readable documentation of what the function does
- Multiple `intent` declarations are allowed

**Example:**
```clean
functions:
    void processPayment(number amount, string method)
        intent "Process a payment transaction using the specified payment method"
        spec "specs/payment/process.spec.cln"
        require amount > 0
        require method in ["credit", "debit", "paypal"]
        // ... implementation
```

### The `source:` Block

The `source:` block marks a file as generated from a specification document.

**Syntax:**
```clean
source:
    spec: "path/to/specification.spec.cln"
    version: "commit-hash-or-version"
```

**Rules:**
- Must appear at the top of the file (before any other declarations)
- Contains two required fields:
  - `spec`: Path to the source specification file
  - `version`: Version identifier (git hash, version number, etc.)
- Indicates the file was generated from a formal specification

**Example:**
```clean
source:
    spec: "specs/payment.spec.cln"
    version: "a3f2c1d"

functions:
    boolean validateCard(string cardNumber)
        intent "Validate credit card number using Luhn algorithm"
        spec "specs/payment.spec.cln"
        require cardNumber.length() >= 13 and cardNumber.length() <= 19
        // ... implementation
```

### Combined Usage Example

Here's a complete example showing all AI integration features together:

```clean
source:
    spec: "specs/authentication.spec.cln"
    version: "2.1.0"

functions:
    boolean authenticateUser(string username, string password)
        intent "Authenticate a user with username and password credentials"
        spec "specs/authentication.spec.cln"
        require username.length() > 0
        require password.length() >= 8

        hash = hashPassword(password)
        stored = database.query("SELECT password FROM users WHERE username = ?", username)

        if stored.length() == 0
            return false

        return compareHashes(hash, stored[0].password)

    string hashPassword(string password)
        intent "Generate secure hash of password using bcrypt"
        spec "specs/authentication.spec.cln"
        require password.length() >= 8

        // _crypto_hash_password is provided by the frame.auth plugin
        return _crypto_hash_password(password)

    boolean compareHashes(string hash1, string hash2)
        intent "Securely compare two password hashes in constant time"
        spec "specs/authentication.spec.cln"
        require hash1.length() > 0
        require hash2.length() > 0

        // _crypto_verify_password is provided by the frame.auth plugin
        return _crypto_verify_password(hash1, hash2)

tests:
    "authenticate valid user": authenticateUser("john", "SecurePass123") = true
    "reject invalid password": authenticateUser("john", "WrongPass") = false
```

### Use Cases

**1. Specification-Driven Development:**
```clean
functions:
    number calculateTax(number income, string state)
        spec "specs/tax/calculation.spec.cln"
        intent "Calculate state income tax based on tax brackets"
        require income >= 0
        require state in ["CA", "NY", "TX", "FL"]
        // Implementation follows specification
```

**2. Documentation and Traceability:**
```clean
functions:
    void sendEmail(string to, string subject, string body)
        intent "Send email via SMTP with retry logic and error handling"
        spec "specs/email/smtp.spec.cln"
        require to.contains("@")
        require subject.length() > 0
        // Implementation traceable to spec
```

**3. Generated Code Attribution:**
```clean
source:
    spec: "specs/api/rest_endpoints.spec.cln"
    version: "v1.2.3"

// Generated API endpoint handlers
functions:
    string handleGetUser(string userId)
        intent "Handle GET /api/users/:id endpoint"
        spec "specs/api/rest_endpoints.spec.cln"
        // ... implementation
```

### Best Practices

1. **Use relative paths**: Spec paths should be relative to project root
   ```clean
   spec "specs/module/feature.spec.cln"  // ✅ Good
   spec "/absolute/path/spec.cln"         // ❌ Avoid
   ```

2. **Place metadata early**: `spec` and `intent` should come before `require` and implementation
   ```clean
   functions:
       void myFunc()
           intent "..."    // ✅ Good: before require
           spec "..."      // ✅ Good: before require
           require x > 0
           // implementation
   ```

3. **Keep intent concise**: One clear sentence describing the function's purpose
   ```clean
   intent "Calculate compound interest over a given period"  // ✅ Good
   intent "This function does stuff with numbers"            // ❌ Too vague
   ```

4. **Version tracking**: Use meaningful version identifiers in `source:` blocks
   ```clean
   source:
       spec: "specs/payment.spec.cln"
       version: "a3f2c1d"    // ✅ Git hash
   ```

### Notes

- These features are metadata and don't affect runtime behavior
- The compiler can use this information for validation and tooling
- Specification files (`.spec.cln`) follow the same Clean Language syntax
- Tools can verify that implementations match their specifications

Numbered error codes for missing spec paths or intent descriptions live in `foundation/spec/semantic-rules.md` (SYN100, SYN101).

### AI Agent Setup (MCP Server)

The Clean Language compiler includes a built-in MCP (Model Context Protocol) server that allows AI agents to write, compile, and debug Clean Language programs. The MCP server exposes 12 tools over a stdio-based JSON-RPC 2.0 transport.

#### Quick Setup

Generate the configuration for your AI platform:

```bash
# For Claude Desktop
cln mcp-config --format claude-desktop

# For VS Code / Cursor
cln mcp-config --format vscode

# For Claude Code
cln mcp-config --format claude-code

# Generic (all platforms)
cln mcp-config
```

Each command outputs the JSON configuration to add to your platform's settings file. For example, Claude Desktop requires adding to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "clean-language": {
      "command": "cln",
      "args": ["mcp-server"]
    }
  }
}
```

#### Starting the MCP Server Manually

```bash
cln mcp-server
```

The server reads JSON-RPC 2.0 requests from stdin and writes responses to stdout. Debug logging goes to stderr.

#### Available MCP Tools

| Tool | Description |
|------|-------------|
| `get_quick_reference` | **Call first.** Returns a concise cheat sheet with syntax, types, control flow, and examples. |
| `check` | Fast type-check (no WASM generation). Use during iterative development. |
| `compile` | Full compilation to WebAssembly. Returns WASM as base64. |
| `parse` | Returns the AST as JSON. Useful for code analysis. |
| `diagnostics` | Detailed error/warning diagnostics for source code. |
| `explain_error` | Explains any error code (e.g., SYN001, SEM002) with examples and fixes. |
| `list_functions` | Lists all functions in a source file with signatures. |
| `list_types` | Lists all class/type definitions in a source file. |
| `list_builtins` | Complete catalog of built-in functions, classes, and namespaces. |
| `get_specification` | Query the language specification by section or keyword. Auto-updates when spec changes. |
| `list_error_codes` | All compiler error codes with descriptions across 6 categories. |
| `list_plugins` | Available plugins with AI context metadata. |

#### Recommended AI Agent Workflow

1. **Learn the language**: Call `get_quick_reference` to receive a complete syntax cheat sheet
2. **Write code**: Create `.cln` files following the patterns from the quick reference
3. **Iterate fast**: Call `check` for rapid type-checking feedback (no WASM generation overhead)
4. **Compile**: Call `compile` when the code is ready for WebAssembly output
5. **Debug errors**: Call `explain_error` with the error code for detailed fix suggestions
6. **Deep reference**: Call `get_specification` with a section name or search query for detailed documentation

#### MCP Protocol Example

Initialize the connection:
```json
{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}
```

Call a tool:
```json
{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
  "name": "check",
  "arguments": {
    "source": "start:\n\tprint(\"Hello\")",
    "file_path": "hello.cln"
  }
}}
```

#### Plugin AI Context

Plugins can include an `[ai]` section in their `plugin.toml` manifest to provide context for AI agents:

```toml
[ai]
description = "HTTP endpoint DSL for Clean Language"
examples = ["examples/basic_api.cln", "examples/crud.cln"]
constraints = ["All endpoints must have authentication", "Use REST conventions"]
```

AI agents can discover this context via the `list_plugins` MCP tool, enabling them to understand what plugins are available and how to use them correctly.

---

[← Previous: Memory Management](./20-memory-management.md) | [Index](./README.md) | [Next: Plugin System](./22-plugin-system.md) →
