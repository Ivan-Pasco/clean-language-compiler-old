# Clean Language Compiler - Software Specification

## Overview
The Clean Language Compiler is a modern compiler for the Clean programming language that compiles to WebAssembly (WASM). It provides a type-safe, readable syntax with support for object-oriented programming, functional programming concepts, and modern features like modules and asynchronous programming.

## Architecture

### Core Components

#### 1. Abstract Syntax Tree (`src/ast/mod.rs`)
- **Purpose**: Defines the data structures representing parsed Clean Language code
- **Key Features**:
  - Type system with support for primitives, objects, arrays, and futures
  - Expression and statement representations
  - Class and function definitions with visibility modifiers
  - **NEW**: Module import items and async function modifiers
  - **NEW**: Future types for asynchronous programming
  - **NEW**: Background function execution support

#### 2. Parser (`src/parser/`)
- **Purpose**: Converts source code text into AST structures
- **Components**:
  - `mod.rs`: Main parser interface and utilities
  - `parser_impl.rs`: Core parsing logic and error recovery
  - `grammar.pest`: PEG grammar definition for Clean Language syntax
  - `statement_parser.rs`: Statement-specific parsing logic
  - `type_parser.rs`: Type annotation parsing
- **Key Features**:
  - Error recovery and detailed error reporting
  - Support for Clean Language syntax including classes, functions, and control flow
  - **NEW**: Import statement parsing with alias support
  - **NEW**: Later assignment and background statement parsing
  - **NEW**: Start expression parsing for async operations

#### 3. Semantic Analyzer (`src/semantic/`)
- **Purpose**: Performs type checking, scope analysis, and semantic validation
- **Components**:
  - `mod.rs`: Main semantic analysis logic
  - `scope.rs`: Scope management for variables and functions
  - `type_constraint.rs`: Type constraint system for advanced type checking
- **Key Features**:
  - Type compatibility checking and inference
  - Class inheritance validation
  - Function signature verification
  - **NEW**: Module import resolution and validation
  - **NEW**: Async programming semantic analysis
  - **NEW**: Future type checking and compatibility

#### 4. **NEW**: Module System (`src/module/`)
- **Purpose**: Handles module loading, resolution, and linking
- **Components**:
  - `mod.rs`: Module resolver and export management
- **Key Features**:
  - File-based module discovery with multiple search paths
  - Public symbol export tracking
  - Import alias resolution
  - Module caching for performance
  - Cross-module type checking and validation

#### 5. Code Generator (`src/codegen/`)
- **Purpose**: Generates WebAssembly bytecode from validated AST
- **Components**:
  - `mod.rs`: Main code generation logic
  - `wasm_builder.rs`: WebAssembly-specific code generation utilities
- **Key Features**:
  - Type-safe WASM generation
  - Function call compilation with proper stack management
  - Class method compilation with vtables
  - **NEW**: Module linking and import compilation
  - **NEW**: Asynchronous function compilation to WASM
  - **NEW**: Background task scheduling code generation

#### 6. Error Handling (`src/error/`)
- **Purpose**: Comprehensive error reporting and recovery system
- **Components**:
  - `mod.rs`: Error types and reporting utilities
- **Key Features**:
  - Detailed error messages with source location information
  - Multiple error types (syntax, type, memory, codegen, runtime, validation)
  - **NEW**: Module-specific error types and messages
  - **NEW**: Import resolution error handling
  - **NEW**: Symbol resolution error reporting

#### 7. Standard Library (`src/stdlib/`)
- **Purpose**: Provides built-in functions and utilities for Clean Language programs
- **Components**:
  - `mod.rs`: Standard library interface and management
  - `error.rs`: Error handling utilities
  - `memory.rs`: Memory management functions
  - `type_conv.rs`: Type conversion utilities
  - `collection_ops.rs`: Collection operation functions
- **Key Features**:
  - I/O operations (print, println functions)
  - Mathematical operations (abs, arithmetic)
  - String manipulation utilities
  - List and collection operations
  - HTTP request functions
  - Memory management utilities

#### 8. **NEW**: Example Modules (`modules/`)
- **Purpose**: Demonstrates the module system with practical, reusable libraries
- **Components**:
  - `MathUtils.clean`: Mathematical utility functions (abs, max, min, sqrt, pow, factorial, pi, e)
  - `StringOps.clean`: String manipulation operations (length, concat, substring, contains, etc.)
  - `FileReader.clean`: Asynchronous file I/O operations with background processing
- **Key Features**:
  - Public function exports with type safety
  - Async file operations using background modifiers
  - Cross-module compatibility and reusability

### **NEW**: Language Features

#### Module System
- **Import Syntax**: `import: MathUtils, StringOps as StrOps, FileReader`
- **Public Functions**: Functions marked with `public` visibility are exportable
- **Module Resolution**: Automatic discovery in `./`, `./modules/`, `./lib/`, `./stdlib/` directories
- **Alias Support**: Import modules with custom names for namespace management
- **Type Safety**: Full type checking across module boundaries

#### Asynchronous Programming
- **Later Assignments**: `later result = start longRunningOperation()`
- **Background Processing**: `background print("This runs asynchronously")`
- **Background Functions**: `function processData(data) background`
- **Future Types**: Automatic `Future<T>` type creation for async operations
- **Start Expressions**: `start operation()` to begin asynchronous execution

#### Enhanced Type System
- **Future Types**: `Future<T>` for representing asynchronous computations
- **Background Modifiers**: Function modifiers for async execution
- **Import Items**: Special AST nodes for module imports with aliases

### Build and Execution

#### Compilation Pipeline
1. **Parsing**: Source code → AST using PEG grammar
2. **Module Resolution**: Import statements → Loaded modules with exports
3. **Semantic Analysis**: Type checking, scope validation, async verification
4. **Code Generation**: AST → WebAssembly bytecode with module linking
5. **Runtime**: WASM execution with async task scheduling

#### Example Programs
- `examples/test_module_imports.clean`: Demonstrates importing and using external modules
- `examples/test_async_programming.clean`: Shows later assignments and background processing
- `examples/test_background_functions.clean`: Illustrates background function definitions

#### **NEW**: Module Usage Examples

```clean
// Import modules with aliases
import: MathUtils, StringOps as StrOps, FileReader

function main() -> void
    // Use imported math functions
    number result = MathUtils.sqrt(16)
    number max_val = MathUtils.max(result, 10)
    
    // Use string operations with alias
    string text = StrOps.concat("Hello", " World")
    boolean contains = StrOps.contains(text, "World")
    
    // Async file operations
    later content = start FileReader.readFile("data.txt")
    background FileReader.writeFile("output.txt", text)
    
    print("Computation complete")
```

#### **NEW**: Async Programming Examples

```clean
function processLargeDataset(array data) -> Future<number> background
    // Background processing for heavy computation
    number sum = 0
    for item in data do
        sum = sum + item
    return sum

function main() -> void
    array numbers = [1, 2, 3, 4, 5]
    
    // Start async operation
    later total = start processLargeDataset(numbers)
    
    // Do other work while computation runs
    print("Processing started...")
    
    // Background task (fire and forget)
    background print("This prints asynchronously")
```

### Dependencies
- **wasmtime**: WebAssembly runtime for execution and testing
- **pest**: Parser generator for grammar-based parsing
- **tempfile**: Temporary file utilities for testing

### Testing Strategy
- Unit tests for each compiler component
- Integration tests for end-to-end compilation
- Module system tests with example modules
- Async programming feature validation
- WebAssembly output verification

### **NEW**: Module System Testing
- File-based module resolution testing
- Import alias functionality validation  
- Cross-module type checking verification
- Export symbol accessibility testing
- Module caching performance validation

### **NEW**: Async Programming Testing
- Future type generation and compatibility
- Background function execution validation
- Later assignment type inference testing
- Start expression compilation verification
- Async task scheduling in WebAssembly runtime

This specification reflects the current implementation status with foundational support for modules and asynchronous programming, providing a robust foundation for modern Clean Language development. 