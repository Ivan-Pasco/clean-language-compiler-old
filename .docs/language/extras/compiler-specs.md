# Clean Language Compiler Specification

## Overview

The Clean Language compiler is implemented in Rust and transforms Clean Language source code into WebAssembly binaries. This document details the compiler's architecture, components, and implementation.

## Architecture

### Core Components

The compiler is organized into several Rust modules, each handling a specific part of the compilation process:

```rust
mod ast;      // Abstract Syntax Tree definitions
mod parser;   // Source code parsing
mod semantic; // Semantic analysis
mod codegen;  // WebAssembly code generation
mod validation; // Code validation
```


Clean Language Compilation Flow

The Clean Language compiler follows these main steps to transform your code into executable WebAssembly:

Parsing: The source code is read and converted into tokens, checking for syntax errors.

AST Creation: Tokens are organized into an Abstract Syntax Tree (AST) representing the program's structure (functions, expressions, statements).

Type Analysis: The compiler analyzes the AST to determine types of expressions and variables, ensuring type compatibility.

Code Generation: The AST is converted to WebAssembly (WASM) instructions through several components:

Type Manager: Handles type conversions and function signatures
Instruction Generator: Creates WASM instructions for each language construct

Memory Manager: Allocates memory for strings, arrays, matrices

String Pool: Efficiently stores and deduplicates strings

Module Assembly: The generated WASM components (functions, memory, globals) are assembled into a complete WASM module.

Output: The final result is a binary WebAssembly module that can be executed by any WASM runtime.

This modular approach makes the compiler easier to maintain and extend with new language features.


### Compilation Pipeline

1. Source Code → Parse Tree (parser)
2. Parse Tree → AST (parser)
3. AST → Validated AST (semantic)
4. Validated AST → WebAssembly (codegen)
5. WebAssembly → Validated Binary (validation)

## Module Specifications

### 1. Parser Module (`parser.rs`)

Uses the `pest` crate for parsing Clean Language source code:

```rust
mod parser {
   use pest::Parser;
  
   #[derive(Parser)]
   #[grammar = "grammar.pest"]
   pub struct CleanParser;
  
   impl CleanParser {
       pub fn parse_program(source: &str) -> Result<Program> {
           // Parse source code into AST
       }
   }
}
```

**Grammar Definition (`grammar.pest`):**
- Tokenization rules
- Syntax patterns
- Error recovery strategies

### 2. AST Module (`ast.rs`)

Defines the language's abstract syntax tree structure:

```rust
mod ast {
   #[derive(Debug, Clone)]
   pub enum Statement {
       VariableDecl { type_: Type, name: String, initializer: Option<Expression> },
       Assignment { target: String, value: Expression },
       Print(Expression),
       Return(Expression),
       // ...
   }

   #[derive(Debug, Clone)]
   pub enum Expression {
       Number(f64),
       Boolean(bool),
       BinaryOp { left: Box<Expression>, operator: Operator, right: Box<Expression> },
       Condition { left: Box<Expression>, operator: ComparisonOperator, right: Box<Expression> },
       // ...
   }

   #[derive(Debug)]
   pub struct Program {
       statements: Vec<Statement>,
       functions: Vec<Function>,
   }
}
```

### 3. Semantic Analysis (`semantic.rs`)

Performs static analysis and validation:

```rust
mod semantic {
   pub struct SemanticAnalyzer {
       symbol_table: SymbolTable,
       current_scope: ScopeId,
   }

   impl SemanticAnalyzer {
       pub fn analyze(&mut self, program: &Program) -> Result<()> {
           // Type checking
           // Scope validation
           // Symbol resolution
       }
   }
}
```

### 4. Code Generation (`codegen.rs`)

Generates WebAssembly using `wasm-encoder`:

```rust
mod codegen {
   pub struct CodeGenerator {
       variables: HashMap<String, VariableInfo>,
       function_types: HashMap<String, u32>,
       function_indices: HashMap<String, u32>,
       global_indices: HashMap<String, u32>,
       local_variables: HashMap<String, LocalVarInfo>,
       memory_offset: u64,
       string_pool: Vec<String>,
   }

   impl CodeGenerator {
       pub fn generate(&mut self, program: &Program) -> Result<Vec<u8>> {
           // Generate WebAssembly binary
       }
   }
}
```

### 5. Validation (`validation.rs`)

Ensures correctness of the generated WebAssembly:

```rust
mod validation {
   pub struct WasmValidator {
       features: WasmFeatures,
   }

   impl WasmValidator {
       pub fn validate_and_analyze(&self, wasm_binary: &[u8]) -> Result<WasmAnalysis> {
           // Validate WebAssembly binary
       }
   }
}
```

## Memory Management

### Type Mapping

| Clean Type | WebAssembly Type | Size (bytes) | Alignment | Implementation     |
|------------|------------------|--------------|-----------|-------------------|
| boolean    | i32              | 1            | 1         | 0 = false, 1 = true |
| byte       | i32              | 1            | 1         | 0-255 value      |
| number     | f64              | 8            | 8         | IEEE 754 double  |
| integer    | i32              | 4            | 4         | Signed int32     |
| unsigned   | i32              | 4            | 4         | Unsigned int32   |
| long       | i64              | 8            | 8         | Signed int64     |
| ulong      | i64              | 8            | 8         | Unsigned int64   |
| string     | i32, i32         | 8            | 4         | (ptr, len) pair  |
| array      | i32, i32         | 8            | 4         | (ptr, len) pair  |
| object     | i32              | 4            | 4         | Memory pointer   |

### Memory Layout

```rust
// Memory organization
struct MemoryLayout {
   // Static data section (0x0000 - 0x1000)
   static_data: Region,
  
   // String pool (0x1000 - 0x2000)
   string_pool: Region,
  
   // Heap allocations (0x2000 - ...)
   heap: Region,
}
```

### Memory Management Implementation

#### Heap Memory Management

The compiler implements a reference-counted memory management system with the following characteristics:

- **Static Data Section:** First 64KB (0x0000 - 0xFFFF) reserved for static data
- **Heap Start:** Begins at 0x10000 (after static data section)
- **Minimum Allocation:** 16 bytes for alignment
- **Header Size:** 8 bytes per allocation (4 bytes size + 4 bytes ref count)

#### Memory Management Functions

```rust
// Memory management functions exported by the WebAssembly module
pub mod memory {
   // Allocates size bytes of memory, returns pointer to allocated region
   pub fn malloc(size: i32) -> i32;
  
   // Frees memory at given pointer
   pub fn free(ptr: i32);
  
   // Increments reference count for object at pointer
   pub fn retain(ptr: i32);
  
   // Decrements reference count, frees if zero
   pub fn release(ptr: i32);
}
```

#### Allocation Header Layout

```rust
struct HeapAllocation {
   size: u32,     // Total allocation size including header
   ref_count: u32, // Number of references to this allocation
   data: [u8],    // Actual allocated data
}
```

#### Reference Counting Rules

1. **Allocation:** Initial reference count of 1
2. **Assignment:** Increment reference count via `retain`
3. **Scope Exit:** Decrement reference count via `release`
4. **Free:** When reference count reaches 0

### List and Object Memory Layout

#### List Layout
```rust
struct ListLayout {
   header: HeapAllocation, // Standard allocation header
   length: u32,           // Number of elements
   elements: [Element],   // List elements
}
```

#### Object Layout
```rust
struct ObjectLayout {
   header: HeapAllocation, // Standard allocation header
   vtable: *const VTable,  // Pointer to virtual method table
   fields: [Field],       // Object fields
}
``` 