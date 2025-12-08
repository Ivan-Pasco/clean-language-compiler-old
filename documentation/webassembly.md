# WebAssembly Code Generation Architecture

This document provides comprehensive documentation for Claude on how the Clean Language compiler generates WebAssembly code. This information will help Claude understand and work with the WebAssembly compilation pipeline.

> 🔗 **Related Documentation**: [Parser Documentation](./parser.md) • [Semantic Analysis](./semantic-analysis.md) • [Standard Library](./standard-library.md) • [Development Guide](./development-guide.md)

## Overview

The Clean Language compiler uses a sophisticated multi-layered architecture to translate Clean Language source code into efficient WebAssembly bytecode. The system emphasizes type safety, memory management, and runtime performance while maintaining compatibility with WebAssembly's execution model.

## Architecture Components

### 1. Code Generator Core (`src/codegen/mod.rs`)

The `CodeGenerator` struct serves as the central orchestrator for WebAssembly generation:

```rust
pub struct CodeGenerator {
    type_section: TypeSection,
    function_section: FunctionSection,
    code_section: CodeSection,
    memory_section: MemorySection,
    import_section: ImportSection,
    export_section: ExportSection,
    
    // State management
    function_indices: HashMap<String, u32>,
    variable_info: HashMap<String, (u32, WasmType)>,
    class_info: HashMap<String, ClassInfo>,
    memory_allocator: MemoryAllocator,
}
```

**Key Features:**
- **Section Management**: Organizes WebAssembly module sections
- **Function Indexing**: O(1) function lookup with HashMap-based resolution
- **Variable Tracking**: Local variable information with index and type mapping
- **Class Hierarchy**: Object-oriented feature support
- **Memory Management**: Reference counting and allocation tracking

### 2. Type System Mapping

Clean Language types are mapped to WebAssembly types as follows:

```rust
match clean_type {
    Type::Integer => WasmType::I32,
    Type::Number => WasmType::F64,
    Type::Boolean => WasmType::I32,      // 0 = false, 1 = true
    Type::String => WasmType::I32,       // Pointer to string data
    Type::List(_) => WasmType::I32,      // Pointer to list structure
    Type::Matrix(_) => WasmType::I32,    // Pointer to matrix data
    Type::Object(_) => WasmType::I32,    // Pointer to object instance
    Type::Void => None,                  // No return value
}
```

**Precision Control:**
- `integer:8` → I32 with clamping to 8-bit range
- `integer:64` → I64 for large numbers
- `number:32` → F32 for graphics and performance
- `number:64` → F64 for scientific computing

### 3. Instruction Generation (`src/codegen/instruction_generator.rs`)

The instruction generator handles low-level WebAssembly instruction creation:

**Expression Compilation:**
```rust
pub fn compile_expression(&mut self, expr: &Expression) -> Result<Vec<Instruction>, CodeGenError> {
    match expr {
        Expression::Literal(lit) => self.compile_literal(lit),
        Expression::Variable(name) => self.compile_variable_access(name),
        Expression::Binary { left, op, right } => self.compile_binary_op(left, op, right),
        Expression::Call { name, args } => self.compile_function_call(name, args),
        Expression::MethodCall { object, method, args } => self.compile_method_call(object, method, args),
        // ... additional expression types
    }
}
```

**Type Coercion System:**
```rust
fn insert_type_conversion(&mut self, from: WasmType, to: WasmType, instructions: &mut Vec<Instruction>) {
    match (from, to) {
        (WasmType::I32, WasmType::F64) => {
            instructions.push(Instruction::F64ConvertI32S);
        },
        (WasmType::F64, WasmType::I32) => {
            instructions.push(Instruction::I32TruncF64S);
        },
        // ... additional conversions
    }
}
```

### 4. Memory Management (`src/codegen/memory.rs`)

The memory system implements sophisticated allocation strategies:

**Memory Layout:**
```
Object Header (16 bytes):
├── Size (4 bytes)      - Object size in bytes
├── RefCount (4 bytes)  - Reference count for ARC
├── TypeID (4 bytes)    - Type identifier
└── NextFree (4 bytes)  - Next free block (for allocator)

Object Data:
└── Type-specific content based on object type
```

**Allocation Strategies:**
1. **Memory Pools**: Size-segregated pools (64B, 256B, 1KB, 4KB)
2. **Reference Counting**: Automatic memory management with ARC
3. **String Deduplication**: Hash-based string pooling
4. **Garbage Collection**: Mark-and-sweep for circular references

**Key Functions:**
```rust
// Allocate memory for objects
wasm_alloc(size: i32) -> i32

// Increment reference count
wasm_retain(ptr: i32) -> i32

// Decrement reference count and free if needed
wasm_release(ptr: i32)

// Garbage collection cycle
wasm_gc()
```

### 5. String Pool Management (`src/codegen/string_pool.rs`)

Optimizes string storage through deduplication:

**String Storage Format:**
```
String Header (8 bytes):
├── Length (4 bytes)    - UTF-8 byte length
└── Hash (4 bytes)      - String hash for dedup

String Data:
└── UTF-8 encoded content
```

**Benefits:**
- Eliminates duplicate string allocations
- Reduces memory usage for string-heavy applications
- Accelerates string comparison operations

## Compilation Pipeline

### 1. AST to WebAssembly Flow

```
Clean Source → Parser → AST → Semantic Analysis → Code Generation → WebAssembly
```

**Detailed Steps:**
1. **AST Input**: Parsed and type-checked abstract syntax tree
2. **Function Registration**: Build function index table
3. **Type Analysis**: Map Clean types to WebAssembly types
4. **Instruction Generation**: Convert AST nodes to WASM instructions
5. **Memory Layout**: Allocate data section for constants and strings
6. **Module Assembly**: Combine all sections into WebAssembly module

### 2. Function Compilation Process

```rust
fn compile_function(&mut self, func: &Function) -> Result<(), CodeGenError> {
    // 1. Set up function context
    self.enter_function_scope(&func.name);
    
    // 2. Allocate local variables
    for param in &func.parameters {
        self.allocate_local(&param.name, &param.type_annotation);
    }
    
    // 3. Compile function body
    let mut instructions = Vec::new();
    for stmt in &func.body {
        instructions.extend(self.compile_statement(stmt)?);
    }
    
    // 4. Add return instruction if needed
    if func.return_type != Type::Void && !self.has_explicit_return(&instructions) {
        instructions.push(Instruction::Return);
    }
    
    // 5. Register compiled function
    self.add_function_to_module(func.name.clone(), instructions);
    
    self.exit_function_scope();
    Ok(())
}
```

### 3. Class Compilation

Object-oriented features are compiled to struct-like memory layouts:

```rust
struct ClassInfo {
    name: String,
    fields: Vec<(String, WasmType, u32)>,  // name, type, offset
    methods: HashMap<String, u32>,          // method -> function index
    parent: Option<String>,                 // inheritance
    vtable_offset: u32,                     // virtual method table
}
```

**Object Instance Layout:**
```
Object Instance:
├── Header (16 bytes)   - Standard object header
├── VTable (4 bytes)    - Pointer to virtual method table
├── Field1 (N bytes)    - First field data
├── Field2 (M bytes)    - Second field data
└── ...                 - Additional fields
```

## Integration with Runtime

> ⚠️ **WebAssembly Compatibility Note**: The instruction examples in this document use the standard WebAssembly instruction set. Mathematical functions like trigonometric operations (sin, cos, tan) require host imports as they are not native WebAssembly instructions.

### 1. Import Functions

The compiler imports essential runtime functions:

```rust
// Memory management
("env", "malloc", FuncType::new([I32], [I32])),
("env", "free", FuncType::new([I32], [])),
("env", "gc", FuncType::new([], [])),

// Console operations
("env", "print_i32", FuncType::new([I32], [])),
("env", "print_f64", FuncType::new([F64], [])),
("env", "print_string", FuncType::new([I32], [])),

// String operations
("env", "string.concat", FuncType::new([I32, I32, I32, I32], [I32])),  // ptr1, len1, ptr2, len2 -> result_ptr
("env", "string_compare", FuncType::new([I32, I32], [I32])),

// List operations
("env", "list_create", FuncType::new([I32], [I32])),
("env", "list_push", FuncType::new([I32, I32], [])),
```

### 2. Export Functions

The generated module exports key functions:

```rust
// Main entry point
("start", start_function_index),

// Memory access for host
("memory", 0),

// Debugging helpers
("get_heap_size", heap_size_function_index),
("get_string_pool_stats", string_pool_stats_function_index),
```

## Optimization Strategies

### 1. Instruction-Level Optimizations

- **Dead Code Elimination**: Remove unreachable instructions
- **Constant Folding**: Evaluate constants at compile time
- **Local Variable Reuse**: Minimize local variable allocations
- **Stack Management**: Optimize stack operations

### 2. Memory Optimizations

- **Pool Allocation**: Reduce fragmentation with size-based pools
- **String Interning**: Eliminate duplicate strings
- **Reference Counting**: Immediate deallocation when possible
- **Lazy GC**: Defer garbage collection until necessary

### 3. Function Optimizations

- **Inline Small Functions**: Eliminate call overhead
- **Tail Call Optimization**: Convert recursion to loops where possible
- **Parameter Optimization**: Use locals efficiently

## Error Handling

### 1. Compilation Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum CodeGenError {
    #[error("Undefined function: {name}")]
    UndefinedFunction { name: String },
    
    #[error("Type mismatch: expected {expected}, found {found}")]
    TypeMismatch { expected: String, found: String },
    
    #[error("Invalid memory access at offset {offset}")]
    InvalidMemoryAccess { offset: u32 },
    
    #[error("Stack underflow in expression")]
    StackUnderflow,
}
```

### 2. Runtime Error Handling

Clean's `onError` syntax is compiled to WebAssembly exception handling:

```rust
// Clean: value = risky_operation() onError 0
// Compiles to:
try {
    call $risky_operation
} catch {
    i32.const 0  // Default value
}
```

## Testing and Validation

### 1. Unit Tests

Each component includes comprehensive unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_integer_compilation() {
        let mut generator = CodeGenerator::new();
        let expr = Expression::Literal(Literal::Integer(42));
        let instructions = generator.compile_expression(&expr).unwrap();
        assert_eq!(instructions, vec![Instruction::I32Const(42)]);
    }
}
```

### 2. Integration Tests

Full compilation pipeline tests ensure correctness:

```bash
# Test compilation of example programs
cargo test --test integration_tests

# Test WebAssembly output validation
cargo test --test wasm_validation
```

## Future Enhancements

### 1. Advanced WebAssembly Features

- **SIMD Instructions**: Vector operations for matrix computations
- **Exception Handling**: Native WebAssembly exception support
- **Multi-Threading**: WebAssembly threads for parallel execution
- **Bulk Memory Operations**: Efficient memory copying and initialization

### 2. Optimization Improvements

- **Profile-Guided Optimization**: Runtime feedback for hot path optimization
- **Cross-Function Optimization**: Whole-program optimization
- **Async Optimization**: Better async/await code generation
- **Memory Layout Optimization**: Improved data structure packing

### 3. Debugging Support

- **Source Maps**: Map WebAssembly back to Clean Language source
- **Debug Symbols**: Rich debugging information
- **Profiling Integration**: Performance analysis tools
- **Runtime Inspection**: Live object and memory inspection

## Best Practices for Claude

When working with the WebAssembly code generation system:

1. **Type Safety**: Always ensure Clean types map correctly to WebAssembly types
2. **Memory Management**: Use the built-in allocation functions rather than direct memory access
3. **Stack Management**: Ensure WebAssembly stack is properly balanced after operations
4. **Error Handling**: Provide meaningful error messages with source location context
5. **Testing**: Add comprehensive tests for any new compilation features
6. **Performance**: Consider both compile-time and runtime performance implications

This architecture provides a robust foundation for compiling Clean Language to efficient WebAssembly while maintaining the language's safety guarantees and modern features.