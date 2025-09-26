# Clean Language Server

A high-performance Language Server Protocol (LSP) implementation for Clean Language, providing intelligent code editing features for editors and IDEs.

## Features

### ✨ **Real-time Language Features**
- **Syntax Highlighting**: Semantic tokenization for keywords, types, functions, and more
- **Error Diagnostics**: Real-time compilation errors with helpful tips and suggestions
- **IntelliSense**: Context-aware autocompletion for language constructs, types, and methods
- **Hover Information**: Rich documentation and type information on hover
- **Code Actions**: Quick fixes for common issues (indentation, functions blocks)

### 🔧 **Advanced Capabilities**
- **Document Formatting**: Automatic code formatting with Clean Language conventions
- **Range Formatting**: Format selected code regions
- **Incremental Sync**: Efficient document updates for large files
- **Compiler Integration**: Uses the main Clean Language compiler for accurate analysis

### 🎯 **Clean Language Specific**
- **Apply-block Support**: Intelligent completion for apply-block syntax
- **Function Block Validation**: Ensures proper `functions:` block structure
- **Tab-based Indentation**: Enforces Clean Language indentation standards
- **Standalone start() Support**: Recognizes standalone start functions per specification

## Installation

### Building from Source

```bash
# Clone the Clean Language compiler repository
git clone <repository-url>
cd clean-language-compiler/language-server

# Build the language server
cargo build --release

# The binary will be available at ../target/release/clean-language-server
```

### Using Pre-built Binary

The language server binary is located at `target/debug/clean-language-server` (debug build) or `target/release/clean-language-server` (optimized build).

## Usage

### Starting the Language Server

The language server communicates via JSON-RPC over stdin/stdout:

```bash
./target/debug/clean-language-server
```

For debugging with verbose logging:
```bash
RUST_LOG=debug ./target/debug/clean-language-server
```

### Editor Integration

#### Visual Studio Code

1. Install a generic LSP extension or create a custom extension
2. Configure the language server:

```json
{
    "languageServer": {
        "command": "/path/to/clean-language-server",
        "args": [],
        "filetypes": ["clean"],
        "settings": {}
    }
}
```

#### Neovim (with nvim-lspconfig)

```lua
local lspconfig = require('lspconfig')

lspconfig.clean_language_server = {
    default_config = {
        cmd = {'/path/to/clean-language-server'},
        filetypes = {'clean'},
        root_dir = lspconfig.util.root_pattern('.git', 'package.clean.toml'),
        settings = {},
    },
}

lspconfig.clean_language_server.setup{}
```

#### Emacs (with lsp-mode)

```elisp
(lsp-register-client
 (make-lsp-client :new-connection (lsp-stdio-connection "/path/to/clean-language-server")
                  :major-modes '(clean-mode)
                  :server-id 'clean-language-server))
```

### File Association

Configure your editor to associate `.cln` files with Clean Language:

- **File Extension**: `.cln`
- **Language ID**: `clean`
- **MIME Type**: `text/x-clean`

## Language Server Capabilities

### Implemented Features

| Feature | Status | Description |
|---------|--------|-------------|
| **textDocument/publishDiagnostics** | ✅ | Real-time error reporting |
| **textDocument/completion** | ✅ | Code completion with snippets |
| **textDocument/hover** | ✅ | Symbol information on hover |
| **textDocument/formatting** | ✅ | Document formatting |
| **textDocument/rangeFormatting** | ✅ | Range formatting |
| **textDocument/codeAction** | ✅ | Quick fixes and refactoring |
| **textDocument/semanticTokens** | ✅ | Semantic syntax highlighting |
| **textDocument/didOpen** | ✅ | Document lifecycle management |
| **textDocument/didChange** | ✅ | Incremental document updates |
| **textDocument/didClose** | ✅ | Document cleanup |

### Completion Triggers

The language server provides completions on:
- `.` (method and property access)
- `:` (apply-block contexts)
- `(` (function parameters)
- `\t` (indentation-based completions)

### Code Actions

Available quick fixes:
- **Fix Indentation**: Convert spaces to tabs
- **Add Functions Block**: Wrap functions in `functions:` block
- **Syntax Corrections**: Automatic fixes for common syntax errors

## Configuration

The language server accepts these initialization options:

```json
{
    "settings": {
        "clean": {
            "enableDiagnostics": true,
            "enableCompletion": true,
            "enableHover": true,
            "enableFormatting": true,
            "tabSize": 1,
            "maxCompletionItems": 100
        }
    }
}
```

## Development

### Architecture

```
language-server/src/
├── main.rs              # LSP server implementation
├── completion.rs        # Code completion provider
├── diagnostics.rs       # Error diagnostics converter
├── hover.rs            # Hover information provider
└── formatting.rs       # Code formatting provider
```

### Key Components

- **Backend**: Main LSP server handling client communication
- **CompletionProvider**: Context-aware code completions
- **DiagnosticsProvider**: Converts compiler errors to LSP diagnostics
- **HoverProvider**: Documentation and type information
- **FormattingProvider**: Code formatting and style enforcement

### Integration with Compiler

The language server uses the main Clean Language compiler (`clean-language-compiler`) for:
- Parsing and syntax validation
- Semantic analysis and type checking
- Error reporting and diagnostics
- Symbol resolution

### Adding New Features

1. **Add capability** to `initialize()` method in `main.rs`
2. **Implement handler** method in the `LanguageServer` trait
3. **Create provider** module for the feature logic
4. **Test integration** with a Clean Language file

## Examples

### Sample Clean Language File

```clean
// hello.cln
start()
    print("Hello, Clean Language!")

functions:
    integer add(integer a, integer b)
        return a + b

    string greet(string name)
        return "Hello, " + name + "!"

class Calculator
    number result

    constructor(number initial)
        result = initial

    number add(number value)
        result = result + value
        return result
```

### Expected Language Server Features

1. **Syntax Highlighting**: Keywords (`start`, `functions`, `class`), types (`integer`, `string`), and operators
2. **Error Diagnostics**: Missing return types, syntax errors, type mismatches
3. **Completions**: Language keywords, built-in types, method suggestions
4. **Hover Info**: Function signatures, type information, documentation
5. **Formatting**: Consistent tab-based indentation, proper spacing

## Troubleshooting

### Common Issues

**Language Server Not Starting**
- Ensure the binary has execute permissions: `chmod +x clean-language-server`
- Check that all dependencies are installed
- Verify the path to the binary is correct

**No Diagnostics Appearing**
- Check that `.cln` files are properly associated with Clean Language
- Verify the language server is receiving document change events
- Enable debug logging: `RUST_LOG=debug`

**Completions Not Working**
- Ensure the cursor is in a valid completion context
- Check completion trigger characters are configured
- Verify the language ID is set to `clean`

**Formatting Issues**
- Clean Language uses tab-based indentation (not spaces)
- Ensure the document is saved as a `.cln` file
- Check that formatting is enabled in your editor

### Debug Logging

Enable detailed logging for troubleshooting:

```bash
# Full debug output
RUST_LOG=debug ./target/debug/clean-language-server

# Specific module debugging
RUST_LOG=clean_language_server::completion=debug ./target/debug/clean-language-server

# Log to file
RUST_LOG=debug ./target/debug/clean-language-server 2> lsp.log
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes following the coding standards
4. Test with real Clean Language files
5. Submit a pull request

### Testing

```bash
# Run unit tests
cd language-server
cargo test

# Test with a real editor integration
# (Setup instructions for VS Code, Neovim, etc.)
```

## Version History

- **v0.7.0**: Initial release with full LSP support
  - Real-time diagnostics with compiler integration
  - Complete code completion system
  - Hover documentation
  - Document and range formatting
  - Semantic tokenization
  - Code actions for common fixes

## License

This project is licensed under the same license as the Clean Language compiler.

## Support

For issues, feature requests, or questions:
- Create an issue in the main Clean Language compiler repository
- Check the troubleshooting section above
- Enable debug logging for detailed error information