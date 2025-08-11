# Clean Language Parser Architecture

This document provides comprehensive documentation for Claude on how the Clean Language parser works.

> 🔗 **Related Documentation**: [Semantic Analysis](./semantic-analysis.md) • [WebAssembly Generation](./webassembly.md) • [Development Guide](./development-guide.md) • [Language Specification](../docs/language/Clean_Language_Specification.md) This information will help Claude understand and work with the parsing system to build and modify the compiler.

## Overview

The Clean Language parser uses **Pest** (Parsing Expression Grammar) to transform source code into an Abstract Syntax Tree (AST). The system emphasizes error recovery, precise location tracking, and support for Clean Language's unique syntax features.

## Architecture Components

### 1. Grammar Definition (`src/parser/grammar.pest`)

The grammar defines Clean Language's syntax using Pest's PEG format:

**Core Design Principles:**
- **Indentation-based structure**: Like Python, Clean uses indentation for code blocks
- **Type-first declarations**: Variables declared as `integer x = 5`
- **Apply-blocks**: Structured configuration with `identifier:` syntax
- **Functions-first architecture**: All functions must be in `functions:` blocks (except `start()`)
- **String interpolation**: `"Hello {name}!"` with property access support
- **Method chaining**: Object-oriented dot notation

**Key Grammar Sections:**

```pest
// Core tokens and keywords
KEYWORD = { 
    "and" | "class" | "constructor" | "else" | "error" | "false" | "for" | 
    "from" | "function" | "if" | "import" | "in" | "iterate" | "not" | 
    "onError" | "or" | "print" | "println" | "return" | "start" | "step" | 
    "test" | "tests" | "this" | "to" | "true" | "while" | "is" | "returns" | 
    "description" | "input" | "unit" | "private" | "constant" | "functions"
}

// Type system
type_annotation = {
    primitive_type | composite_type | generic_type | class_type
}

primitive_type = {
    ("integer" | "number" | "boolean" | "string" | "void") ~ size_specifier?
}

size_specifier = { ":" ~ ASCII_DIGIT+ ~ unsigned_modifier? }
unsigned_modifier = { "u" }

composite_type = {
    "list" ~ "<" ~ type_annotation ~ ">" |
    "matrix" ~ "<" ~ type_annotation ~ ">" |
    "pairs" ~ "<" ~ type_annotation ~ "," ~ type_annotation ~ ">"
}
```

**Apply Block Grammar:**
```pest
apply_block = {
    constant_apply | type_apply | method_apply | function_apply
}

constant_apply = {
    "constant" ~ ":" ~ NEWLINE ~ INDENT ~ 
    (constant_declaration ~ NEWLINE)+ ~ DEDENT
}

type_apply = {
    type_annotation ~ ":" ~ NEWLINE ~ INDENT ~
    (variable_declaration ~ NEWLINE)+ ~ DEDENT
}

method_apply = {
    identifier ~ ("." ~ identifier)* ~ ":" ~ NEWLINE ~ INDENT ~
    (expression ~ NEWLINE)+ ~ DEDENT
}

function_apply = {
    identifier ~ ":" ~ NEWLINE ~ INDENT ~
    (expression ~ NEWLINE)+ ~ DEDENT
}
```

### 2. Parser Implementation (`src/parser/mod.rs`)

The main parser module provides the interface for parsing Clean Language source:

```rust
#[derive(Parser)]
#[grammar = "parser/grammar.pest"]
pub struct CleanParser;

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

pub struct ParseResult {
    pub ast: Option<Program>,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerWarning>,
}
```

**Key Functions:**
```rust
// Basic parsing
pub fn parse(source: &str) -> Result<Program, CompilerError>

// Parsing with file context
pub fn parse_with_file(source: &str, file_path: &str) -> ParseResult

// Error recovery parsing
pub fn parse_with_recovery(source: &str, file_path: &str) -> ParseResult

// Module-aware parsing
pub fn parse_module(source: &str, module_path: &str, resolver: &ModuleResolver) -> ParseResult
```

### 3. Error Recovery System

The parser includes sophisticated error recovery to continue parsing after syntax errors:

```rust
pub struct ErrorRecoveringParser {
    pub source: String,
    pub file_path: String,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerWarning>,
    pub max_errors: usize,
}

impl ErrorRecoveringParser {
    fn identify_recovery_points(&mut self, source: &str) {
        // Identifies safe points to resume parsing:
        // - Function boundaries (functions:)
        // - Class boundaries (class Name)
        // - Statement boundaries (newlines with proper indentation)
        // - Apply block boundaries
    }
    
    fn split_into_recoverable_segments(&self) -> Vec<String> {
        // Splits source into independently parseable segments
        // Each segment can be parsed in isolation
    }
    
    fn create_partial_node(&self, segment: &str) -> Option<PartialNode> {
        // Creates partial AST nodes for segments that fail to parse
        // Allows semantic analysis to continue with available information
    }
}
```

**Recovery Strategies:**
1. **Function-level recovery**: Skip malformed functions, continue with next
2. **Statement-level recovery**: Skip malformed statements within functions
3. **Expression-level recovery**: Use placeholder nodes for invalid expressions
4. **Block-level recovery**: Resume at next indentation-aligned block

### 4. Expression Parser (`src/parser/expression_parser.rs`)

Handles all expression types with proper precedence and associativity:

**Operator Precedence (highest to lowest):**
```rust
impl ParsedOperator {
    fn precedence(&self) -> u8 {
        match self {
            ParsedOperator::Binary(BinaryOperator::Power) => 6,        // ^
            ParsedOperator::Unary(_) => 5,                             // -, not
            ParsedOperator::Binary(BinaryOperator::Multiply | 
                                   BinaryOperator::Divide | 
                                   BinaryOperator::Modulo) => 4,       // *, /, %
            ParsedOperator::Binary(BinaryOperator::Add | 
                                   BinaryOperator::Subtract) => 3,     // +, -
            ParsedOperator::Binary(BinaryOperator::Comparison) => 2,   // <, >, <=, >=, ==, !=
            ParsedOperator::Binary(BinaryOperator::And) => 1,          // and
            ParsedOperator::Binary(BinaryOperator::Or) => 0,           // or
        }
    }
}
```

**Expression Types Supported:**

1. **Primary Expressions:**
   ```rust
   Expression::Literal(value)           // 42, "hello", true
   Expression::Variable(name)           // myVariable
   Expression::FunctionCall { name, args }  // myFunction(a, b)
   ```

2. **Binary Operations:**
   ```rust
   Expression::Binary { left, operator, right }
   // Supports: +, -, *, /, %, ^, <, >, <=, >=, ==, !=, and, or
   ```

3. **Method Calls:**
   ```rust
   Expression::MethodCall {
       object: Box<Expression>,
       method: String,
       arguments: Vec<Expression>,
       location: SourceLocation,
   }
   // Example: obj.method(arg1, arg2)
   ```

4. **Property Access:**
   ```rust
   Expression::PropertyAccess {
       object: Box<Expression>,
       property: String,
       location: SourceLocation,
   }
   // Example: user.name, config.database.host
   ```

5. **List/Matrix Access:**
   ```rust
   Expression::IndexAccess {
       object: Box<Expression>,
       index: Box<Expression>,
       location: SourceLocation,
   }
   // Example: array[0], matrix[row][col]
   ```

6. **String Interpolation:**
   ```rust
   Expression::StringInterpolation {
       parts: Vec<InterpolationPart>,
       location: SourceLocation,
   }
   
   enum InterpolationPart {
       Text(String),
       Expression(Expression),
   }
   // Example: "Hello {user.name}! You have {count} messages."
   ```

7. **Conditional Expressions:**
   ```rust
   Expression::Conditional {
       condition: Box<Expression>,
       then_expr: Box<Expression>,
       else_expr: Box<Expression>,
       location: SourceLocation,
   }
   // Example: if age >= 18 then "adult" else "minor"
   ```

8. **Error Handling:**
   ```rust
   Expression::OnError {
       expression: Box<Expression>,
       error_handler: Box<Expression>,
       location: SourceLocation,
   }
   // Example: readFile("data.txt") onError "default content"
   ```

### 5. Statement Parser (`src/parser/statement_parser.rs`)

Handles all statement types and control structures:

**Statement Types:**

1. **Variable Declarations:**
   ```rust
   Statement::VariableDeclaration {
       type_annotation: Type,
       name: String,
       initializer: Option<Expression>,
       location: SourceLocation,
   }
   // Example: integer count = 0
   ```

2. **Assignments:**
   ```rust
   Statement::Assignment {
       target: AssignmentTarget,
       value: Expression,
       location: SourceLocation,
   }
   
   enum AssignmentTarget {
       Variable(String),
       Property { object: Expression, property: String },
       Index { object: Expression, index: Expression },
   }
   // Examples: x = 5, obj.prop = value, array[0] = item
   ```

3. **Control Flow:**
   ```rust
   Statement::If {
       condition: Expression,
       then_body: Vec<Statement>,
       else_body: Option<Vec<Statement>>,
       location: SourceLocation,
   }
   
   Statement::Iterate {
       variable: String,
       iterable: IterableSource,
       body: Vec<Statement>,
       location: SourceLocation,
   }
   
   enum IterableSource {
       Range { start: Expression, end: Expression, step: Option<Expression> },
       Expression(Expression),
   }
   ```

4. **Function Calls:**
   ```rust
   Statement::ExpressionStatement {
       expression: Expression,
       location: SourceLocation,
   }
   // Used for function calls, method calls, assignments as expressions
   ```

5. **Print Statements:**
   ```rust
   Statement::Print {
       expressions: Vec<Expression>,
       newline: bool,  // true for println, false for print
       location: SourceLocation,
   }
   ```

6. **Error Handling:**
   ```rust
   Statement::Error {
       message: Expression,
       location: SourceLocation,
   }
   // Example: error("Invalid input")
   ```

7. **Async Statements:**
   ```rust
   Statement::Later {
       variable: String,
       expression: Expression,
       location: SourceLocation,
   }
   // Example: later data = start fetchData("url")
   
   Statement::Background {
       expression: Expression,
       location: SourceLocation,
   }
   // Example: background logAction("user_login")
   ```

### 6. Program Structure Parser

Handles top-level program constructs:

```rust
pub struct Program {
    pub imports: Vec<ImportStatement>,
    pub constants: Vec<ConstantDeclaration>,
    pub functions: Vec<Function>,
    pub classes: Vec<Class>,
    pub tests: Vec<Test>,
    pub start_function: Option<Function>,
    pub apply_blocks: Vec<ApplyBlock>,
}

pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
    pub body: Vec<Statement>,
    pub description: Option<String>,
    pub input_block: Option<InputBlock>,
    pub is_background: bool,
    pub location: SourceLocation,
}

pub struct Class {
    pub name: String,
    pub parent: Option<String>,
    pub fields: Vec<Field>,
    pub constructor: Option<Constructor>,
    pub functions: Vec<Function>,
    pub location: SourceLocation,
}
```

### 7. Special Features

**Functions Block Parsing:**
```rust
// All functions must be in functions: blocks (except start())
functions_block = {
    "functions" ~ ":" ~ NEWLINE ~ INDENT ~
    function_definition+ ~
    DEDENT
}

// Exception: start() can be standalone
start_function = {
    "start" ~ "(" ~ ")" ~ NEWLINE ~ INDENT ~
    statement+ ~
    DEDENT
}
```

**Apply Block Parsing:**
Apply blocks are Clean Language's unique feature for structured configuration:

```rust
// Type apply: integer: x = 5, y = 10
type_apply_block = {
    type_annotation ~ ":" ~ NEWLINE ~ INDENT ~
    (variable_declaration ~ NEWLINE)+ ~
    DEDENT
}

// Method apply: println: "Hello", "World"  
method_apply_block = {
    method_chain ~ ":" ~ NEWLINE ~ INDENT ~
    (expression ~ NEWLINE)+ ~
    DEDENT
}
```

**String Interpolation Parsing:**
```rust
interpolated_string = {
    "\"" ~ interpolation_part* ~ "\""
}

interpolation_part = {
    text_part | interpolation_expression
}

interpolation_expression = {
    "{" ~ expression ~ "}"
}

// Supports property access: "Hello {user.name}!"
// Supports method calls: "Count: {items.length()}"
```

## Integration with Semantic Analysis

### 1. AST Structure

The parser produces a rich AST that carries all necessary information for semantic analysis:

```rust
pub trait ASTNode {
    fn location(&self) -> &SourceLocation;
    fn children(&self) -> Vec<&dyn ASTNode>;
    fn accept<V: ASTVisitor>(&self, visitor: &mut V);
}

pub trait ASTVisitor {
    fn visit_program(&mut self, program: &Program);
    fn visit_function(&mut self, function: &Function);
    fn visit_class(&mut self, class: &Class);
    fn visit_statement(&mut self, statement: &Statement);
    fn visit_expression(&mut self, expression: &Expression);
}
```

### 2. Symbol Information

The parser preserves symbol information for semantic analysis:

```rust
pub struct Symbol {
    pub name: String,
    pub symbol_type: SymbolType,
    pub location: SourceLocation,
    pub scope_level: usize,
}

pub enum SymbolType {
    Variable(Type),
    Function(FunctionSignature),
    Class(ClassInfo),
    Module(String),
}
```

### 3. Location Tracking

Every AST node includes precise source location information:

```rust
impl SourceLocation {
    pub fn new(start: usize, end: usize, source: &str) -> Self {
        let (line, column) = calculate_line_column(start, source);
        Self { start, end, line, column }
    }
    
    pub fn span(&self, other: &SourceLocation) -> SourceLocation {
        SourceLocation {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line,
            column: self.column,
        }
    }
}
```

## Error Handling and Diagnostics

### 1. Parse Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Unexpected token '{found}' at line {line}, expected {expected}")]
    UnexpectedToken {
        found: String,
        expected: String,
        line: usize,
        column: usize,
    },
    
    #[error("Indentation error at line {line}: expected {expected} spaces, found {found}")]
    IndentationError {
        line: usize,
        expected: usize,
        found: usize,
    },
    
    #[error("Unclosed string literal starting at line {line}")]
    UnclosedString { line: usize },
    
    #[error("Invalid function definition: functions must be in 'functions:' blocks")]
    InvalidFunctionDefinition { location: SourceLocation },
    
    #[error("Invalid apply block syntax at line {line}")]
    InvalidApplyBlock { line: usize },
}
```

### 2. Warning Generation

The parser generates helpful warnings:

```rust
#[derive(Debug)]
pub enum ParseWarning {
    UnusedImport { name: String, location: SourceLocation },
    DeprecatedSyntax { old: String, new: String, location: SourceLocation },
    MissingDocumentation { function: String, location: SourceLocation },
    InconsistentIndentation { location: SourceLocation },
}
```

### 3. Error Recovery Examples

```rust
// Example: Malformed function recovered
fn recover_from_function_error(&mut self, start_pos: usize) -> Option<Function> {
    // Skip to next 'functions:' or class definition
    while let Some(token) = self.current_token() {
        if token.rule == Rule::functions_keyword || token.rule == Rule::class_keyword {
            break;
        }
        self.advance();
    }
    
    // Create placeholder function node
    Some(Function {
        name: "parse_error_placeholder".to_string(),
        parameters: vec![],
        return_type: Type::Void,
        body: vec![],
        location: SourceLocation::new(start_pos, self.current_position(), &self.source),
        // ... other fields with defaults
    })
}
```

## Testing and Validation

### 1. Parser Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_variable_declaration() {
        let source = "integer x = 42";
        let result = parse(source).unwrap();
        
        assert_eq!(result.statements.len(), 1);
        match &result.statements[0] {
            Statement::VariableDeclaration { name, type_annotation, .. } => {
                assert_eq!(name, "x");
                assert_eq!(type_annotation, &Type::Integer);
            }
            _ => panic!("Expected variable declaration"),
        }
    }
    
    #[test]
    fn test_string_interpolation() {
        let source = r#""Hello {name}!""#;
        let expr = parse_expression(source).unwrap();
        
        match expr {
            Expression::StringInterpolation { parts, .. } => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(parts[0], InterpolationPart::Text(_)));
                assert!(matches!(parts[1], InterpolationPart::Expression(_)));
                assert!(matches!(parts[2], InterpolationPart::Text(_)));
            }
            _ => panic!("Expected string interpolation"),
        }
    }
}
```

### 2. Grammar Testing

```bash
# Test grammar rules individually
cargo test --test grammar_tests

# Test error recovery
cargo test --test error_recovery_tests

# Test parser performance
cargo test --test parser_performance --release
```

## Best Practices for Claude

When working with the parser system:

1. **Preserve Location Information**: Always maintain source location data for error reporting
2. **Handle Error Recovery**: Use the error recovery system rather than failing on first error
3. **Respect Grammar Rules**: Follow the established grammar patterns for consistency
4. **Test Thoroughly**: Add comprehensive tests for any new parsing features
5. **Consider Performance**: Be mindful of parsing performance for large files
6. **Maintain Backward Compatibility**: Ensure changes don't break existing Clean Language code
7. **Document Grammar Changes**: Update grammar documentation when adding new syntax

## Future Enhancements

### 1. Performance Improvements

- **Incremental Parsing**: Parse only changed portions of large files
- **Parallel Parsing**: Parse independent modules concurrently
- **Lazy AST Construction**: Build AST nodes on-demand
- **Memory Optimization**: Reduce AST memory footprint

### 2. Enhanced Error Recovery

- **Better Recovery Points**: More sophisticated recovery strategies
- **Contextual Suggestions**: AI-powered syntax error suggestions
- **Interactive Parsing**: IDE integration for real-time error feedback
- **Semantic Recovery**: Continue semantic analysis with partial AST

### 3. Advanced Features

- **Macro System**: Parse-time code generation
- **DSL Support**: Embedded domain-specific languages
- **Language Server Protocol**: Full IDE integration
- **Source Code Formatting**: Automatic code formatting based on AST

This parser architecture provides a solid foundation for Clean Language's unique syntax while maintaining excellent error handling and extensibility for future enhancements.