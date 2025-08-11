# Clean Language Compiler Project Overview

This document provides a high-level architectural overview of the Clean Language compiler project for Claude. This serves as the primary entry point for understanding the system architecture, key components, and their relationships.

## Project Mission

Clean Language is a modern, type-safe programming language designed to compile to WebAssembly (WASM). It combines JavaScript-like readability with Rust-like safety features while being approachable for beginners. The language emphasizes strong static typing, first-class functions, matrix operations, and comprehensive error handling.

## System Architecture

The Clean Language compiler follows a traditional multi-stage compilation pipeline with modern enhancements:

```
┌─────────────────┐    ┌──────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  Clean Source   │ => │    Parser    │ => │ Semantic Analysis│ => │ Code Generation │
│     (.cln)      │    │   (Pest)     │    │  (Type Checker) │    │     (WASM)      │
└─────────────────┘    └──────────────┘    └─────────────────┘    └─────────────────┘
                              │                       │                       │
                              v                       v                       v
                       ┌──────────────┐    ┌─────────────────┐    ┌─────────────────┐
                       │     AST      │    │ Symbol Tables   │    │  WASM Module    │
                       │              │    │ Type Info       │    │   (.wasm)       │
                       └──────────────┘    └─────────────────┘    └─────────────────┘
```

## Core Components

### 1. **Parser System** (`src/parser/`)
- **Grammar Engine**: Pest-based parsing with `grammar.pest` defining Clean Language syntax
- **Error Recovery**: Sophisticated error recovery that continues parsing after syntax errors
- **AST Generation**: Creates typed Abstract Syntax Tree nodes with location information
- **Key Features**: Indentation-based syntax, apply-blocks, string interpolation, function-first architecture

### 2. **Semantic Analysis** (`src/semantic/`)
- **Type Checker**: Comprehensive type inference and compatibility checking
- **Scope Management**: Nested scope handling with proper variable visibility
- **Symbol Resolution**: Function overloading, method resolution, class inheritance validation
- **Error Reporting**: Detailed semantic errors with suggestions and location context

### 3. **Code Generation** (`src/codegen/`)
- **WASM Generation**: Multi-section WebAssembly module assembly
- **Memory Management**: Reference counting with garbage collection
- **Type Mapping**: Clean Language types → WebAssembly types
- **Optimization**: Instruction-level optimizations and stack management

### 4. **Standard Library** (`src/stdlib/`)
- **Built-in Functions**: Math, String, List, File, HTTP operations
- **Memory System**: Automatic memory management with ARC
- **Host Integration**: WebAssembly imports for system operations
- **Type Conversions**: Seamless type conversion between Clean types

### 5. **Language Server** (`language-server/`)
- **LSP Implementation**: Full Language Server Protocol support for IDE integration
- **Real-time Analysis**: Syntax highlighting, error detection, and type checking
- **Developer Tools**: Intelligent autocompletion, hover information, code formatting
- **Editor Integration**: Native support for VS Code, Neovim, Emacs, and other LSP clients

### 6. **Runtime System** (`src/runtime/`)
- **Async Support**: Task scheduling and future resolution
- **I/O Operations**: File and network I/O through host imports
- **Error Handling**: Runtime error propagation and recovery

## Key Design Principles

### 1. **Type Safety First**
- Strong static typing with comprehensive type inference
- Memory-safe operations with bounds checking
- Runtime type validation for cross-type operations
- No null pointer exceptions through careful design

### 2. **WebAssembly Native**
- All code compiles to efficient WebAssembly bytecode
- Memory layout compatible with WASM linear memory model
- Host imports for operations requiring system access
- Optimization for WASM execution characteristics

### 3. **Developer Experience**
- Excellent error messages with suggestions and context
- Error recovery that continues compilation after errors
- Comprehensive testing framework with built-in test syntax
- Clear separation between language features and implementation

### 4. **Performance Oriented**
- Memory pools and reference counting for efficient allocation
- String interning to eliminate duplicates
- Instruction-level optimizations during code generation
- Lazy evaluation and deferred operations where beneficial

## Language Features Overview

### **Core Syntax**
```clean
// Type-first variable declarations
integer count = 0
string name = "Alice"
boolean flag = true

// Functions must be in functions: blocks (except start())
functions:
    integer add(integer a, integer b)
        return a + b

// Classes with inheritance
class Person
    string name
    integer age
    
    constructor(string name, integer age)
    
    functions:
        string greet()
            return "Hello, I'm " + name

class Student is Person
    string school
    
    constructor(string name, integer age, string school)
        base(name, age)
        this.school = school
```

### **Advanced Features**
```clean
// Apply blocks for structured configuration
integer:
    x = 10
    y = 20
    z = 30

// String interpolation with property access
string message = "User {user.name} has {user.points} points"

// Error handling with onError syntax
string data = readFile("config.txt") onError "default config"

// Async programming
later result = start fetchData("https://api.example.com")
background logAction("user_login")

// Built-in testing
tests:
    "addition works": add(2, 3) = 5
    "string length": "hello".length() = 5
```

### **Type System**
- **Primitive Types**: `boolean`, `integer`, `number`, `string`, `void`
- **Precision Control**: `integer:64`, `number:32` for explicit sizing
- **Composite Types**: `list<any>`, `matrix<any>`, `pairs<any,any>`
- **Generic Support**: `any` type for universal generic programming (no explicit type parameters needed)
- **Method Chaining**: `value.toString().trim().length()`

## Compilation Pipeline Details

### 1. **Parsing Phase**
```rust
CleanParser::parse(source) -> Result<Program, Vec<CompilerError>>
```
- **Input**: Clean Language source code (`.cln` files)
- **Output**: Abstract Syntax Tree with location information
- **Features**: Error recovery, multiple error reporting, module resolution

### 2. **Semantic Analysis Phase**
```rust
SemanticAnalyzer::analyze(ast) -> Result<TypedAST, Vec<CompilerError>>
```
- **Input**: Parsed AST from previous phase
- **Output**: Type-checked AST with resolved symbols
- **Features**: Type inference, scope validation, function overload resolution

### 3. **Code Generation Phase**
```rust
CodeGenerator::generate(typed_ast) -> Result<WasmModule, Vec<CompilerError>>
```
- **Input**: Type-checked AST with symbol information
- **Output**: WebAssembly module ready for execution
- **Features**: Memory management, instruction optimization, host import registration

## Memory Management Architecture

Clean Language uses a sophisticated memory management system:

### **Reference Counting + Garbage Collection**
```
Object Layout:
┌─────────────┬─────────────┬─────────────┬─────────────┬─────────────┐
│ RefCount:4  │ TypeID:4    │ Size:4      │ Flags:4     │ UserData... │
└─────────────┴─────────────┴─────────────┴─────────────┴─────────────┘
```

### **String Management**
- **Interning**: Automatic deduplication of identical strings
- **UTF-8 Encoding**: Full Unicode support with efficient storage
- **Copy-on-Write**: Optimization for string operations

### **List Management**
- **Dynamic Arrays**: Automatic resizing with exponential growth
- **Type Safety**: Runtime type checking for heterogeneous collections
- **Behavior Modifiers**: Queue, stack, and set behaviors through `.type` property

## Integration Points

### **Host Environment Integration**
The compiled WebAssembly modules integrate with host environments through:

```javascript
// Runtime imports required by Clean Language programs
const imports = {
    env: {
        // Memory management
        malloc: (size) => wasmMemory.allocate(size),
        free: (ptr) => wasmMemory.deallocate(ptr),
        
        // Console operations
        console_print: (ptr, len) => console.log(readString(ptr, len)),
        
        // File operations
        file_read: (pathPtr, pathLen) => readFileFromHost(pathPtr, pathLen),
        file_write: (pathPtr, pathLen, contentPtr, contentLen) => 
            writeFileToHost(pathPtr, pathLen, contentPtr, contentLen),
        
        // HTTP operations
        http_get: (urlPtr, urlLen) => fetchFromUrl(urlPtr, urlLen),
        http_post: (urlPtr, urlLen, bodyPtr, bodyLen) => 
            postToUrl(urlPtr, urlLen, bodyPtr, bodyLen),
    }
};
```

### **Runtime Environment**
Clean Language programs expect:
- **Linear Memory**: WebAssembly linear memory for object storage
- **Host Functions**: System operations through WASM imports
- **Error Handling**: Exception propagation through WASM
- **Async Support**: Promise/Future integration with host async model

## Development Workflow

### **Project Structure**
```
clean-language-compiler/
├── src/
│   ├── parser/           # Pest-based parsing system
│   ├── semantic/         # Type checking and analysis
│   ├── codegen/          # WebAssembly code generation
│   ├── stdlib/           # Standard library implementation
│   ├── runtime/          # Async and I/O runtime
│   └── bin/              # Executable binaries
├── tests/                # Comprehensive test suite
├── examples/             # Example Clean Language programs
├── docs/                 # Additional documentation
└── .documentation/       # Claude-specific documentation
```

### **Build and Test Commands**
```bash
# Build the compiler
cargo build --release

# Run comprehensive tests
cargo test

# Compile a Clean Language program
cargo run --bin clean-language-compiler compile -i program.cln -o program.wasm

# Run with debugging
cargo run --bin clean-language-compiler debug -i program.cln --show-ast
```

### **Key Files and Entry Points**
- **Main Compiler**: `src/main.rs` - Primary compiler executable
- **Library Interface**: `src/lib.rs` - Programmatic compiler API
- **Grammar Definition**: `src/parser/grammar.pest` - Language syntax rules
- **Standard Library**: `src/stdlib/mod.rs` - Built-in function registry
- **Code Generation**: `src/codegen/mod.rs` - WASM module assembly

## Error Handling Philosophy

Clean Language emphasizes helpful error reporting throughout the compilation pipeline:

### **Parse Errors**
```
Error: Unexpected token 'if' at line 15, column 8
Expected: variable declaration or function definition
Suggestion: Functions must be declared within 'functions:' blocks

    15 │     if condition
       │     ^^
```

### **Semantic Errors**
```
Error: Type mismatch in binary operation
Expected: number + number -> number
Found: string + integer at line 23, column 15

    23 │     result = name + age
       │                   ^^^
Suggestion: Use age.toString() to convert integer to string
```

### **Runtime Errors**
```
Error: Index out of bounds
List length: 3, attempted index: 5
Location: list_operations.cln:42:18

Suggestion: Check list bounds using list.length() before access
```

## Performance Characteristics

### **Compilation Performance**
- **Parser**: ~1000 lines/second with error recovery
- **Semantic Analysis**: ~800 lines/second with full type checking
- **Code Generation**: ~1200 lines/second with optimizations
- **Memory Usage**: ~50MB for typical 10,000 line programs

### **Runtime Performance**
- **Function Calls**: Near-native WebAssembly performance
- **Memory Operations**: Reference counting overhead ~5-10%
- **String Operations**: Optimized with interning and copy-on-write
- **Math Operations**: Direct WebAssembly instruction mapping

### **Generated Code Size**
- **Base Runtime**: ~20KB WebAssembly module overhead
- **Function Code**: ~10-50 bytes per Clean Language function
- **String Literals**: Deduplicated and compressed
- **Standard Library**: Tree-shaken to include only used functions

## Extension Points

The compiler architecture supports several extension mechanisms:

### **Language Features**
- **New Syntax**: Extend `grammar.pest` and corresponding parsers
- **Type System**: Add new types in `src/ast/mod.rs` and type checker
- **Control Flow**: Implement in statement parser and code generator

### **Standard Library**
- **New Functions**: Add to appropriate `src/stdlib/` module
- **Host Integration**: Define new import signatures
- **Type Conversions**: Extend `src/stdlib/type_conv.rs`

### **Code Generation**
- **Optimizations**: Add passes in `src/codegen/instruction_generator.rs`
- **Target Platforms**: Extend beyond WebAssembly (future)
- **Debug Information**: Enhanced debugging support

### **Runtime Systems**
- **Async Models**: Extend `src/runtime/async_runtime.rs`
- **I/O Operations**: Add new I/O primitives
- **Memory Strategies**: Alternative garbage collection algorithms

This architecture provides a solid foundation for a modern, type-safe programming language while maintaining excellent performance characteristics and developer experience. The modular design allows for easy extension and modification while preserving the core safety and performance guarantees that make Clean Language suitable for both educational and production use cases.