# Clean Language - User Guide

## Quick Start

The Clean Language compiler provides a simple, user-friendly `cln` command for all your needs.

### Installation

Download the pre-built binary for your platform from the [releases page](https://github.com/Ivan-Pasco/clean-language-compiler/releases/latest):

- **macOS (Intel)**: `cln-macos-x86_64`
- **macOS (Apple Silicon)**: `cln-macos-aarch64`
- **Linux**: `cln-linux-x86_64`
- **Windows**: `cln-windows-x86_64.exe`

Make it executable (Unix/macOS):
```bash
chmod +x cln-macos-aarch64
sudo mv cln-macos-aarch64 /usr/local/bin/cln
```

### Basic Usage

#### 1. Compile a Program

```bash
# Simple compilation
cln compile hello.cln

# Specify output file
cln compile hello.cln hello.wasm

# With optimization
cln compile app.cln --optimization production

# For web target
cln compile web-app.cln --target web
```

**Example:**
```bash
$ cln compile hello.cln
🔨 Compiling hello.cln → hello.wasm
✅ Compilation successful! Generated hello.wasm
```

#### 2. Run a Program

```bash
# Compile and run in one command
cln run hello.cln

# Run with verbose output
cln run app.cln --verbose
```

#### 3. Check Syntax

```bash
# Just parse and validate syntax
cln parse hello.cln

# Type check without compilation
cln check hello.cln
```

### Advanced Features

#### Package Management

```bash
# Initialize a new Clean Language project
cln package init

# Add a dependency
cln package add package-name

# Install dependencies
cln package install

# List dependencies
cln package list

# Remove a dependency
cln package remove package-name

# Publish package
cln package publish
```

#### Testing & Quality Assurance

```bash
# Run the full test suite
cln test

# Lint a file for code quality
cln lint -i app.cln

# Debug with enhanced error reporting
cln debug -i app.cln --show-ast

# Parse with error recovery
cln parse -i app.cln --recover-errors
```

#### Compilation Targets

```bash
# List available targets
cln targets list

# Get detailed target information
cln targets info web

# Compile for specific target
cln compile app.cln --target nodejs
```

**Available Targets:**
- `web` - Web browsers
- `nodejs` - Node.js runtime
- `native` - Native execution (Wasmtime/Wasmer)
- `embedded` - Embedded systems
- `wasi` - WebAssembly System Interface
- `auto` - Auto-detect best target (default)

#### WebAssembly Runtimes

```bash
# List available runtimes
cln runtime list

# Auto-detect best runtime
cln runtime detect

# Use specific runtime
cln run app.cln --runtime wasmtime
```

**Available Runtimes:**
- `wasmtime` - High-performance runtime
- `wasmer` - Universal runtime
- `auto` - Auto-select (default)

#### Optimization Levels

```bash
# Development mode (fast compilation)
cln compile app.cln --optimization development

# Production mode (optimized)
cln compile app.cln --optimization production

# Size optimization
cln compile app.cln --optimization size

# Speed optimization
cln compile app.cln --optimization speed
```

### Examples

#### Hello World

Create `hello.cln`:
```clean
functions:
    void start()
        print("Hello, World!")
```

Compile and run:
```bash
cln compile hello.cln
# Output: hello.wasm (2.0 KB)
```

#### Web Application

Create `webapp.cln`:
```clean
functions:
    void start()
        print("Clean Language Web App")
        number result = calculate(10, 20)
        print("Result: " + result.toString())

    number calculate(integer a, integer b)
        return a + b
```

Compile for web:
```bash
cln compile webapp.cln --target web --optimization production
```

#### Full Development Workflow

```bash
# 1. Initialize project
cln package init

# 2. Check syntax
cln parse myapp.cln

# 3. Type check
cln check myapp.cln

# 4. Lint code
cln lint -i myapp.cln

# 5. Run tests
cln test

# 6. Compile with debugging
cln compile myapp.cln --debug --verbose

# 7. Run the program
cln run myapp.cln
```

### Command Reference

```
COMMANDS:
    compile <input> [output]    Compile Clean source to WebAssembly
    run <input>                 Compile and run a Clean program
    parse <input>               Parse and validate syntax only
    check <input>               Type check without compilation

    package <subcommand>        Manage packages and dependencies
      init                      Initialize a new Clean Language project
      add <name>                Add a dependency
      remove <name>             Remove a dependency
      install                   Install dependencies
      list                      List dependencies
      publish                   Publish package

    test                        Run the test suite
    lint -i <input>             Lint code for quality issues
    debug -i <input>            Debug with enhanced error reporting

    targets <subcommand>        Manage compilation targets
      list                      List available targets
      info <target>             Get target information

    runtime <subcommand>        Manage WebAssembly runtimes
      list                      List available runtimes
      detect                    Auto-detect best runtime

    options                     Export compile options (IDE integration)
      --export-json             Export as JSON format

    version                     Show version information
    help                        Show this help message

OPTIONS:
    --target, -t <target>       Target platform
    --runtime, -r <runtime>     WebAssembly runtime
    --optimization, -O <level>  Optimization level
    --debug, -d                 Include debug information
    --verbose, -v               Verbose output
    --show-ast                  Show AST structure (debug mode)
    --recover-errors            Enable error recovery (parse mode)
```

### Getting Help

```bash
# Show all commands
cln help

# Show version
cln version

# List all targets
cln targets list

# List all runtimes
cln runtime list

# Get help for specific command
cln compile --help
cln package --help
cln test --help
```

### Success Indicators

✅ **100% Compilation Success Rate** - 297 out of 297 test files compile successfully

✅ **Multi-Platform Support** - Works on Linux, macOS (Intel & ARM), and Windows

✅ **Production Ready** - Used for real applications with comprehensive testing

## Troubleshooting

### Common Issues

**Q: "Command not found: cln"**
- Make sure the binary is in your PATH
- On Unix/Mac: `sudo mv cln /usr/local/bin/`
- On Windows: Add to System PATH

**Q: "Permission denied"**
- Make executable: `chmod +x cln`

**Q: "WASM runtime not found"**
- Install Wasmtime: `curl https://wasmtime.dev/install.sh -sSf | bash`
- Or use Wasmer: `curl https://get.wasmer.io -sSf | sh`

## More Information

- **Website**: https://www.cleanlanguage.dev
- **Documentation**: https://docs.cleanlanguage.dev
- **GitHub**: https://github.com/Ivan-Pasco/clean-language-compiler
- **Language Spec**: See `Language-Specification.md`

---

**Author**: Ivan Pasco Lizarraga
**Version**: 0.11.0
**License**: MIT
