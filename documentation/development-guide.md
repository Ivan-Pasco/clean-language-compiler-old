# Clean Language Development Guide

This document provides comprehensive development guidance for working with the Clean Language compiler project. It covers best practices, common workflows, debugging techniques, and development patterns specific to this codebase.

> 🔗 **Essential References**: [Language Specification](../docs/language/Clean_Language_Specification.md) • [CLAUDE.md Commands](../CLAUDE.md) • [Parser](./parser.md) • [Semantic Analysis](./semantic-analysis.md) • [WebAssembly](./webassembly.md) • [Standard Library](./standard-library.md)

## Getting Started

### Project Context and Goals

When working on this project, always keep in mind:
- **Type Safety**: Clean Language prioritizes compile-time type safety
- **WebAssembly Target**: All code ultimately compiles to WASM
- **Developer Experience**: Excellent error messages and debugging support
- **Memory Safety**: Automatic memory management with reference counting
- **Performance**: Efficient compilation and runtime execution

### Key Development Principles

1. **Follow the Language Specification**: Always refer to `/docs/language/Clean_Language_Specification.md` before implementing features
2. **Maintain WebAssembly Compatibility**: Ensure all language features can be efficiently compiled to WASM
3. **Preserve Error Recovery**: Never break the error recovery mechanisms
4. **Test Comprehensively**: Add tests for all new functionality
5. **Document Changes**: Update relevant documentation when modifying behavior

## Development Workflows

### 1. Adding New Language Features

When implementing new Clean Language syntax or semantics:

**Step 1: Update the Grammar**
```pest
// In src/parser/grammar.pest
new_feature = { 
    "keyword" ~ identifier ~ ":" ~ expression 
}
```

**Step 2: Extend the AST**
```rust
// In src/ast/mod.rs
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    // ... existing variants
    NewFeature {
        name: String,
        value: Expression,
        location: SourceLocation,
    },
}
```

**Step 3: Update the Parser**
```rust
// In src/parser/statement_parser.rs
fn parse_new_feature(&mut self) -> Result<Statement, ParseError> {
    // Implementation following existing patterns
    // Always include location tracking
    // Handle error recovery appropriately
}
```

**Step 4: Implement Semantic Analysis**
```rust
// In src/semantic/mod.rs
fn check_new_feature(&mut self, stmt: &NewFeatureStatement) -> Result<(), CompilerError> {
    // Type checking
    // Scope validation
    // Symbol registration
}
```

**Step 5: Add Code Generation**
```rust
// In src/codegen/mod.rs
fn compile_new_feature(&mut self, stmt: &NewFeatureStatement) -> Result<Vec<Instruction>, CodeGenError> {
    // WebAssembly instruction generation
    // Memory management
    // Type conversions
}
```

**Step 6: Write Tests**
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_new_feature_parsing() {
        let source = "newfeature test: 42";
        let result = parse(source).unwrap();
        // Comprehensive test assertions
    }
    
    #[test]
    fn test_new_feature_compilation() {
        let source = "newfeature test: 42";
        let wasm = compile_to_wasm(source).unwrap();
        // Validate generated WebAssembly
    }
}
```

### 2. Extending the Standard Library

When adding new standard library functions:

**Step 1: Choose the Appropriate Module**
```rust
// Add to existing module (e.g., src/stdlib/string_class.rs)
// Or create new module if needed
```

**Step 2: Implement the Function**
```rust
// In src/stdlib/your_module.rs
impl YourOperations {
    pub fn register_your_function(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        register_stdlib_function(
            codegen,
            "your.function",
            &[WasmType::I32], // Parameter types
            Some(WasmType::I32), // Return type
            self.generate_your_function_instructions(codegen)?
        )
    }
    
    fn generate_your_function_instructions(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        Ok(vec![
            // WebAssembly instructions
            // Remember to handle memory management
            // Include bounds checking for safety
        ])
    }
}
```

**Step 3: Register with Standard Library**
```rust
// In src/stdlib/mod.rs
impl StandardLibrary {
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // ... existing registrations
        self.your_operations.register_functions(codegen)?;
        Ok(())
    }
}
```

**Step 4: Update Semantic Analysis**
```rust
// In src/semantic/mod.rs
fn register_builtin_functions(&mut self) {
    // ... existing functions
    self.register_builtin("your.function", vec![Type::Integer], Type::Integer);
}
```

**Step 5: Add Tests and Documentation**
```rust
#[test]
fn test_your_function() {
    let source = r#"
        functions:
            void start()
                integer result = your.function(42)
                println(result.toString())
    "#;
    
    let wasm = compile_to_wasm(source).unwrap();
    let result = execute_wasm(wasm).unwrap();
    assert_eq!(result, "expected_output");
}
```

### 3. Fixing Bugs and Issues

When addressing bugs in the compiler:

**Step 1: Reproduce the Issue**
```bash
# Create a minimal test case
echo 'problematic_code_here' > test_case.cln

# Run with debugging enabled
cargo run --bin clean-language-compiler debug -i test_case.cln --show-ast
```

**Step 2: Identify the Root Cause**
- **Parse Error**: Check grammar rules and parser implementation
- **Semantic Error**: Examine type checking and scope management
- **Codegen Error**: Look at instruction generation and memory management
- **Runtime Error**: Investigate WebAssembly execution and host imports

**Step 3: Write a Failing Test**
```rust
#[test]
fn test_bug_reproduction() {
    let source = "code that triggers the bug";
    // This should fail before the fix
    let result = compile(source);
    assert!(result.is_ok()); // Will fail until bug is fixed
}
```

**Step 4: Implement the Fix**
- Follow the existing code patterns
- Maintain error recovery where possible
- Consider edge cases and boundary conditions
- Ensure WebAssembly compatibility

**Step 5: Verify the Fix**
```bash
# Run the specific test
cargo test test_bug_reproduction

# Run full test suite
cargo test

# Test with real programs
cargo run --bin clean-language-compiler compile -i examples/complex_program.cln
```

**Step 6: Update TASKS.md**
```markdown
## 🔴 CRITICAL
- [x] Fix string interpolation with nested property access
  - Fixed parser to handle complex expressions in interpolation
  - Added comprehensive test coverage
  - Verified WebAssembly generation is correct
```

## Common Patterns and Best Practices

### 1. Error Handling Patterns

**Always Use Result Types**
```rust
// Good
fn parse_expression(&mut self) -> Result<Expression, ParseError> {
    // Implementation
}

// Bad
fn parse_expression(&mut self) -> Expression {
    // Might panic or return invalid data
}
```

**Provide Helpful Error Messages**
```rust
// Good
Err(CompilerError::TypeError {
    expected: Type::Integer,
    found: Type::String,
    location: expr.location.clone(),
    suggestion: Some("Use .toString() to convert to string".to_string()),
})

// Bad
Err(CompilerError::Generic("Type error".to_string()))
```

**Implement Error Recovery**
```rust
// Continue parsing after errors when possible
fn parse_with_recovery(&mut self) -> (Option<Statement>, Vec<ParseError>) {
    match self.parse_statement() {
        Ok(stmt) => (Some(stmt), vec![]),
        Err(error) => {
            // Log error and skip to recovery point
            self.skip_to_recovery_point();
            (None, vec![error])
        }
    }
}
```

### 2. Memory Management Patterns

**Always Use Reference Counting**
```rust
// Good
fn allocate_string(&mut self, content: &str) -> Result<usize, CompilerError> {
    let ptr = self.memory_manager.allocate(content.len() + 16, STRING_TYPE_ID)?;
    // Initialize with ref count = 1
    self.memory_manager.init_header(ptr, 1, STRING_TYPE_ID, content.len())?;
    Ok(ptr)
}

// Bad - Manual memory management
fn allocate_string(&mut self, content: &str) -> usize {
    let ptr = raw_malloc(content.len());
    // No reference counting, no type information
    ptr
}
```

**Include Bounds Checking**
```rust
// Good
fn list_get(&self, list_ptr: usize, index: i32) -> Result<usize, CompilerError> {
    let length = self.get_list_length(list_ptr)?;
    if index < 0 || index >= length as i32 {
        return Err(CompilerError::IndexOutOfBounds { index, length: length as i32 });
    }
    // Safe to access
    Ok(self.get_list_element(list_ptr, index as usize))
}

// Bad - No bounds checking
fn list_get(&self, list_ptr: usize, index: i32) -> usize {
    // Might access invalid memory
    self.get_list_element(list_ptr, index as usize)
}
```

### 3. WebAssembly Generation Patterns

**Use Typed Instruction Generation**
```rust
// Good
fn generate_integer_add(&self) -> Vec<Instruction> {
    vec![
        Instruction::LocalGet(0),  // Load first operand
        Instruction::LocalGet(1),  // Load second operand
        Instruction::I32Add,       // Add integers
    ]
}

// Bad - Mixing types
fn generate_mixed_add(&self) -> Vec<Instruction> {
    vec![
        Instruction::LocalGet(0),  // Could be any type
        Instruction::LocalGet(1),  // Could be any type
        Instruction::I32Add,       // Assumes integers
    ]
}
```

**Handle Type Conversions Explicitly**
```rust
fn generate_mixed_arithmetic(&self, left_type: &WasmType, right_type: &WasmType) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    
    // Load operands
    instructions.extend(vec![
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
    ]);
    
    // Handle type coercion
    match (left_type, right_type) {
        (WasmType::I32, WasmType::F64) => {
            instructions.insert(instructions.len() - 2, Instruction::F64ConvertI32S);
            instructions.push(Instruction::F64Add);
        }
        (WasmType::F64, WasmType::I32) => {
            instructions.push(Instruction::F64ConvertI32S);
            instructions.push(Instruction::F64Add);
        }
        (WasmType::I32, WasmType::I32) => {
            instructions.push(Instruction::I32Add);
        }
        (WasmType::F64, WasmType::F64) => {
            instructions.push(Instruction::F64Add);
        }
    }
    
    instructions
}
```

### 4. Testing Patterns

**Write Comprehensive Tests**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_feature_basic_functionality() {
        // Test the happy path
        let source = "basic valid code";
        let result = compile(source).unwrap();
        assert!(result.is_valid());
    }
    
    #[test]
    fn test_feature_error_cases() {
        // Test error conditions
        let invalid_sources = [
            "syntax error case",
            "type error case", 
            "semantic error case",
        ];
        
        for source in invalid_sources {
            let result = compile(source);
            assert!(result.is_err());
            // Verify specific error types
        }
    }
    
    #[test]
    fn test_feature_edge_cases() {
        // Test boundary conditions
        let edge_cases = [
            "empty input",
            "very long input",
            "deeply nested structures",
            "maximum size limits",
        ];
        
        for source in edge_cases {
            let result = compile(source);
            // Verify appropriate handling
        }
    }
    
    #[test]
    fn test_feature_integration() {
        // Test interaction with other features
        let complex_source = r#"
            functions:
                void start()
                    // Complex program using multiple features
        "#;
        
        let wasm = compile_to_wasm(complex_source).unwrap();
        let execution_result = execute_wasm(wasm).unwrap();
        assert_eq!(execution_result, "expected_output");
    }
}
```

**Use Property-Based Testing**
```rust
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    
    proptest! {
        #[test]
        fn test_integer_operations_dont_panic(a: i32, b: i32) {
            let source = format!(r#"
                functions:
                    integer test()
                        return {} + {}
            "#, a, b);
            
            // Should never panic, even with overflow
            let result = compile(&source);
            prop_assert!(result.is_ok() || result.is_err()); // No panic
        }
    }
}
```

## Debugging Techniques

### 1. Compiler Debugging

**Use Debug Mode for Detailed Output**
```bash
# Show AST structure
cargo run --bin clean-language-compiler debug -i program.cln --show-ast

# Show parsing details
RUST_LOG=debug cargo run --bin clean-language-compiler parse -i program.cln

# Show code generation details
RUST_LOG=debug cargo run --bin clean-language-compiler compile -i program.cln -o output.wasm
```

**Add Debug Prints Strategically**
```rust
// In parser code
fn parse_expression(&mut self) -> Result<Expression, ParseError> {
    debug!("Parsing expression at token: {:?}", self.current_token());
    let result = self.parse_primary_expression()?;
    debug!("Parsed expression: {:?}", result);
    Ok(result)
}

// In semantic analysis
fn check_type_compatibility(&self, expected: &Type, found: &Type) -> bool {
    let compatible = expected.is_compatible_with(found);
    debug!("Type compatibility check: {:?} vs {:?} = {}", expected, found, compatible);
    compatible
}

// In code generation
fn generate_function_call(&mut self, name: &str, args: &[Expression]) -> Result<Vec<Instruction>, CodeGenError> {
    debug!("Generating function call: {} with {} args", name, args.len());
    let instructions = self.compile_function_call(name, args)?;
    debug!("Generated {} instructions", instructions.len());
    Ok(instructions)
}
```

### 2. WebAssembly Debugging

**Inspect Generated WASM**
```bash
# Convert WASM to WAT for inspection
cargo run --bin wasm2wat input.wasm > output.wat

# Validate WASM structure
cargo run --bin debug_wasm input.wasm

# Run with wasmtime for detailed execution
cargo run --bin wasmtime_runner input.wasm
```

**Add WebAssembly Debug Information**
```rust
fn generate_with_debug_info(&mut self, stmt: &Statement) -> Result<Vec<Instruction>, CodeGenError> {
    let mut instructions = Vec::new();
    
    // Add debug markers
    if self.debug_mode {
        instructions.push(Instruction::Nop); // Debug marker
        instructions.extend(self.encode_source_location(&stmt.location)?);
    }
    
    // Generate actual instructions
    instructions.extend(self.compile_statement(stmt)?);
    
    Ok(instructions)
}
```

### 3. Memory Debugging

**Track Memory Allocations**
```rust
impl MemoryManager {
    pub fn allocate_with_debug(&mut self, size: usize, type_id: u32, location: &str) -> Result<usize, CompilerError> {
        let ptr = self.allocate(size, type_id)?;
        
        if self.debug_mode {
            eprintln!("ALLOC: {:08x} size={} type={} location={}", 
                     ptr, size, type_id, location);
        }
        
        Ok(ptr)
    }
    
    pub fn debug_print_heap(&self) {
        eprintln!("=== HEAP DUMP ===");
        for (ptr, block) in &self.allocations {
            eprintln!("{:08x}: size={} refs={} type={}", 
                     ptr, block.size, block.ref_count, block.type_id);
        }
        eprintln!("=== END HEAP ===");
    }
}
```

## Integration with MCP Tools

When using MCP tools for web research and file operations:

### 1. Researching Language Features

Use Firecrawl for comprehensive research:
```
mcp__firecrawl__firecrawl_search:
  query: "WebAssembly instruction optimization techniques"
  limit: 10
  
mcp__firecrawl__firecrawl_deep_research:
  query: "Modern compiler type inference algorithms"
  maxDepth: 3
```

### 2. Analyzing WebAssembly Standards

```
mcp__firecrawl__firecrawl_scrape:
  url: "https://webassembly.github.io/spec/"
  formats: ["markdown"]
  onlyMainContent: true
```

### 3. Studying Other Compiler Implementations

```
mcp__firecrawl__firecrawl_extract:
  urls: ["https://github.com/rust-lang/rust/tree/master/compiler"]
  prompt: "Extract information about type checking and error handling patterns"
```

## Performance Optimization Guidelines

### 1. Compilation Performance

**Optimize Parser Performance**
```rust
// Cache frequently accessed grammar rules
lazy_static! {
    static ref EXPRESSION_CACHE: HashMap<String, Expression> = HashMap::new();
}

// Use string interning for identifiers
struct IdentifierInterner {
    strings: HashMap<String, u32>,
    next_id: u32,
}
```

**Optimize Semantic Analysis**
```rust
// Cache type compatibility results
struct TypeCompatibilityCache {
    cache: HashMap<(Type, Type), bool>,
}

impl TypeCompatibilityCache {
    fn is_compatible(&mut self, a: &Type, b: &Type) -> bool {
        let key = (a.clone(), b.clone());
        *self.cache.entry(key).or_insert_with(|| a.is_compatible_with(b))
    }
}
```

### 2. Generated Code Performance

**Optimize Instruction Sequences**
```rust
fn optimize_instruction_sequence(&self, instructions: Vec<Instruction>) -> Vec<Instruction> {
    let mut optimized = Vec::new();
    let mut i = 0;
    
    while i < instructions.len() {
        match (&instructions.get(i), &instructions.get(i + 1)) {
            // Constant folding
            (Some(Instruction::I32Const(a)), Some(Instruction::I32Const(b))) 
                if i + 2 < instructions.len() && instructions[i + 2] == Instruction::I32Add => {
                optimized.push(Instruction::I32Const(a + b));
                i += 3; // Skip all three instructions
            }
            
            // Dead code elimination
            (Some(Instruction::Drop), Some(Instruction::Drop)) => {
                optimized.push(Instruction::Drop);
                i += 2; // Combine drops
            }
            
            _ => {
                optimized.push(instructions[i].clone());
                i += 1;
            }
        }
    }
    
    optimized
}
```

**Memory Layout Optimization**
```rust
// Pack struct fields efficiently
fn optimize_class_layout(&self, class: &Class) -> ClassLayout {
    let mut fields = class.fields.clone();
    
    // Sort by size (largest first) for better packing
    fields.sort_by(|a, b| {
        let size_a = self.get_type_size(&a.field_type);
        let size_b = self.get_type_size(&b.field_type);
        size_b.cmp(&size_a)
    });
    
    ClassLayout::new(fields)
}
```

## Security Considerations

### 1. Memory Safety

**Always Validate Pointers**
```rust
fn validate_pointer(&self, ptr: usize, expected_type: u32) -> Result<(), CompilerError> {
    // Check bounds
    if ptr < HEADER_SIZE || ptr >= self.memory_size {
        return Err(CompilerError::InvalidPointer { ptr });
    }
    
    // Check alignment
    if ptr % ALIGNMENT != 0 {
        return Err(CompilerError::MisalignedPointer { ptr, alignment: ALIGNMENT });
    }
    
    // Check type
    let actual_type = self.get_type_id(ptr - HEADER_SIZE)?;
    if actual_type != expected_type {
        return Err(CompilerError::TypeMismatch { 
            expected: expected_type, 
            found: actual_type 
        });
    }
    
    Ok(())
}
```

**Prevent Integer Overflow**
```rust
fn safe_add(&self, a: i32, b: i32) -> Result<i32, CompilerError> {
    a.checked_add(b)
        .ok_or_else(|| CompilerError::IntegerOverflow { 
            operation: "addition".to_string(),
            operands: vec![a, b] 
        })
}
```

### 2. Host Import Security

**Validate Host Functions**
```rust
fn register_host_import(&mut self, module: &str, name: &str, func_type: FuncType) -> Result<(), CompilerError> {
    // Whitelist allowed host functions
    let allowed_imports = [
        ("env", "console_print"),
        ("env", "file_read"),
        ("env", "http_get"),
        // ... other safe functions
    ];
    
    if !allowed_imports.contains(&(module, name)) {
        return Err(CompilerError::UnauthorizedImport { 
            module: module.to_string(),
            name: name.to_string() 
        });
    }
    
    self.add_import(module, name, func_type)
}
```

## Contributing Guidelines

### 1. Code Review Checklist

Before submitting changes:

- [ ] All tests pass (`cargo test`)
- [ ] Code follows Rust conventions (`cargo clippy`)
- [ ] Documentation is updated
- [ ] Error messages are helpful and include suggestions
- [ ] Memory safety is preserved
- [ ] WebAssembly compatibility is maintained
- [ ] Performance impact is considered
- [ ] Security implications are evaluated

### 2. Commit Message Format

Use descriptive commit messages:

```
feat(parser): add support for string interpolation with property access

- Extend grammar to handle nested property access in interpolation
- Update AST to represent complex interpolation expressions
- Add comprehensive test coverage for edge cases
- Fixes #123

Co-authored-by: Development Team <dev@cleanlanguage.org>
```

### 3. Documentation Standards

- Update `.documentation/` files when adding major features
= When adding language features, update the language specification in `/docs/language/Clean_Language_Specification.md`
- Include code examples in documentation
- Explain the "why" not just the "what"
- Keep examples simple but realistic
- Update TASKS.md when completing tasks

This development guide provides the foundation for productive work on the Clean Language compiler. Always prioritize code quality, safety, and maintainability over quick fixes or shortcuts.