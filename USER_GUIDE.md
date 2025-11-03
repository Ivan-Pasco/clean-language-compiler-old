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
# 1. Check syntax
cln parse myapp.cln

# 2. Type check
cln check myapp.cln

# 3. Compile with debugging
cln compile myapp.cln --debug --verbose

# 4. Run the program
cln run myapp.cln
```

### Command Reference

```
COMMANDS:
    compile <input> [output]    Compile Clean source to WebAssembly
    run <input>                 Compile and run a Clean program
    parse <input>               Parse and validate syntax only
    check <input>               Type check without compilation
    targets <subcommand>        Manage compilation targets
    runtime <subcommand>        Manage WebAssembly runtimes
    version                     Show version information
    help                        Show this help message

OPTIONS:
    --target, -t <target>       Target platform
    --runtime, -r <runtime>     WebAssembly runtime
    --optimization, -O <level>  Optimization level
    --debug, -d                 Include debug information
    --verbose, -v               Verbose output
```

### Getting Help

```bash
# Show all commands
cln help

# Show version
cln version

# List targets
cln targets list

# List runtimes
cln runtime list
```

### Success Indicators

✅ **99.7% Compilation Success Rate** - 296 out of 297 test files compile successfully

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
