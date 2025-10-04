# Clean Language Compiler — AST Specification

Status: Normative for this codebase
Spec source: documentation/Clean_Language_Specification.md (authoritative)
Compiler: clean-language-compiler v0.7.5
Updated: September 2025 (exact alignment with language specification)

## 1. Overview and Normativity

- Purpose: Define the exact Abstract Syntax Tree structure that represents Clean Language programs as specified in the Clean Language Specification.
- Authority: The language specification (Clean_Language_Specification.md) is the single source of truth. This document must represent exactly what the language defines - no more, no less.
- Scope: AST node structures, field semantics, and parsing rules that directly correspond to language constructs defined in the specification.
- Constraint: Every AST construct must have a direct correspondence to a language specification feature. No additional constructs beyond the specification are permitted.

## 2. Grammar → AST Mapping

Based on Clean Language Specification features:

### Lexical Elements → AST
- **Identifiers** → `Variable(String)` for simple names
- **Literals** → `Literal(Value)` with Value variants matching spec types
- **Keywords** → No direct AST representation (parsing constructs)
- **Operators** → `BinaryOperator` and `UnaryOperator` enums
- **Comments** → No AST representation (parsing phase only)

### Expressions → AST
- **Arithmetic operations** (`a + b`) → `Binary(lhs, Add, rhs)`
- **Method calls** (`obj.method()`) → `MethodCall { object, method, arguments }`
- **Namespace calls** (`math.sqrt()`) → `NamespaceCall { namespace, function, arguments }`
- **Static method calls** (`Class.method()`) → `StaticMethodCall { class_name, method, arguments }`
- **Property access** (`obj.property`) → `PropertyAccess { object, property }`
- **Property assignment** (`obj.property = value`) → `PropertyAssignment { object, property, value }`
- **List access** (`list[index]`) → `ListAccess(list, index)`
- **Matrix access** (`matrix[row, col]`) → `MatrixAccess(matrix, row, col)`
- **String interpolation** (`"Hello {name}"`) → `StringInterpolation(Vec<StringPart>)`
- **Conditional expressions** (`if cond then a else b`) → `Conditional { condition, then_expr, else_expr }`
- **Range expressions** (`1..10`) → `Range { start, end, inclusive }`
- **Console input** (`input("prompt")`) → `Input { prompt, input_type }`
- **Object creation** (`Point(x, y)`) → `ObjectCreation { class_name, arguments }`
- **Base constructor calls** (`base(args)`) → `BaseCall { arguments }`
- **Error handling** (`expr onError fallback`) → `OnError { expression, fallback }`
- **Async expressions** (`start expr`) → `StartExpression { expression }`

### Statements → AST
- **Variable declarations** (`integer x = 5`) → `VariableDecl { name, type_, initializer }`
- **Function blocks** (`functions: ...`) → `FunctionsBlock { functions }`
- **Apply blocks** → `TypeApplyBlock`, `FunctionApplyBlock`, `MethodApplyBlock`, `ConstantApplyBlock`
- **Assignment** (`x = value`) → `Assignment { target, value }`
- **Print statements** (`print "text"`) → `Print { expression, newline }`
- **Control flow** (`if`, `while`, `iterate`) → `If`, `While`, `Iterate`, `RangeIterate`
- **Return statements** (`return value`) → `Return { value }`
- **Error statements** (`error("message")`) → `Error { message }`
- **Class definitions** (`class Name ...`) → `ClassDefinition { class }`
- **Import statements** (`import: ...`) → `Import { imports }`

## 3. AST Node Definitions

This section defines AST nodes based exactly on Clean Language Specification constructs.

### Core Data Types

**SourceLocation**: `{ line: usize, column: usize, file: String }`
- Required for error reporting and debugging
- Maps to source positions in Clean Language files

**Value** (Language Spec §3 - Type System):
```rust
enum Value {
    // Core types (§3.1)
    Boolean(bool),           // true, false
    Integer(i64),           // 42, -17 (32-bit default, i64 for range)
    Number(f64),            // 3.14, 6.02e23 (64-bit default)
    String(String),         // "Hello, World!"
    
    // Precision modifiers (§3.2)
    Integer8(i8),           // integer:8
    Integer8u(u8),          // integer:8u
    Integer16(i16),         // integer:16
    Integer16u(u16),        // integer:16u
    Integer32(i32),         // integer:32
    Integer64(i64),         // integer:64
    Number32(f32),          // number:32
    Number64(f64),          // number:64
    
    // Composite types (§3.3)
    List(Vec<Value>),       // [1, 2, 3]
    Matrix(Vec<Vec<f64>>),  // [[1, 2], [3, 4]]
    
    // Special types
    Void,                   // void return type
}
```

**Type** (Language Spec §3 - Type System):
```rust
enum Type {
    // Core types (§3.1)
    Boolean,                // boolean
    Integer,                // integer (default)
    Number,                 // number (default)
    String,                 // string
    Void,                   // void
    
    // Precision modifiers (§3.2)
    IntegerSized { bits: u8, unsigned: bool },  // integer:64, integer:32u
    NumberSized { bits: u8 },                   // number:32
    
    // Composite types (§3.3)
    List(Box<Type>),        // list<type>
    Matrix(Box<Type>),      // matrix<type>
    Pairs(Box<Type>, Box<Type>),  // pairs<key,value>
    
    // Generic types (§7.3)
    Any,                    // any (universal generic type)
    
    // Object types (§11)
    Object(String),         // class instances
    Class { name: String, type_args: Vec<Type> },  // generic classes
    
    // Function types (§7)
    Function(Vec<Type>, Box<Type>),  // parameter types, return type
    
    // Async types (§17)
    Future(Box<Type>),      // later assignments
}
```

### Operators (Language Spec §5.1)

**BinaryOperator**:
```rust
enum BinaryOperator {
    // Arithmetic (§5.1)
    Add,         // +
    Subtract,    // -
    Multiply,    // *
    Divide,      // /
    Modulo,      // %
    Power,       // ^
    
    // Comparison (§5.1)
    Equal,       // ==
    NotEqual,    // !=
    Less,        // <
    Greater,     // >
    LessEqual,   // <=
    GreaterEqual,// >=
    Is,          // is
    Not,         // not (binary)
    
    // Logical (§5.1)
    And,         // and
    Or,          // or
}
```

**UnaryOperator**:
```rust
enum UnaryOperator {
    Negate,      // - (unary minus)
    Not,         // not (unary logical negation)
}
```

### Expressions (Language Spec §5)

```rust
enum Expression {
    // Literals (§2.3)
    Literal(Value),
    
    // Variables (§2.2)
    Variable(String),
    
    // Operators (§5.1)
    Binary(Box<Expression>, BinaryOperator, Box<Expression>),
    Unary(UnaryOperator, Box<Expression>),
    
    // Function calls (§7.6)
    Call(String, Vec<Expression>),
    
    // Namespace calls (§16.2 - math.sqrt())
    NamespaceCall {
        namespace: String,
        function: String,
        arguments: Vec<Expression>,
    },
    
    // Method calls (§16.1 - obj.method())
    MethodCall {
        object: Box<Expression>,
        method: String,
        arguments: Vec<Expression>,
    },
    
    // Static method calls (§11.5 - Class.method())
    StaticMethodCall {
        class_name: String,
        method: String,
        arguments: Vec<Expression>,
    },
    
    // Property access (§11.4 - obj.property)
    PropertyAccess {
        object: Box<Expression>,
        property: String,
    },
    
    // Property assignment (§11.4 - obj.property = value)
    // Includes list behavior: list.type = "line" (§3.4)
    PropertyAssignment {
        object: Box<Expression>,
        property: String,
        value: Box<Expression>,
    },
    
    // List access (§3.3 - list[index])
    ListAccess(Box<Expression>, Box<Expression>),
    
    // Matrix access (§3.3 - matrix[row, col])
    MatrixAccess(Box<Expression>, Box<Expression>, Box<Expression>),
    
    // String interpolation (§2.3.2)
    StringInterpolation(Vec<StringPart>),
    
    // Object creation (§11.3)
    ObjectCreation {
        class_name: String,
        arguments: Vec<Expression>,
    },
    
    // Conditional expressions (§9.1)
    Conditional {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    
    // Range expressions (§9.2 - 1..10)
    Range {
        start: Box<Expression>,
        end: Box<Expression>,
        inclusive: bool,
    },
    
    // Console input (§6.3)
    Input {
        prompt: Option<String>,
        input_type: InputType,
    },
    
    // Error handling (§10.1)
    OnError {
        expression: Box<Expression>,
        fallback: Box<Expression>,
    },
    
    // Error handling with block (§10.1)
    OnErrorBlock {
        expression: Box<Expression>,
        error_handler: Vec<Statement>,
    },
    
    // Error variable access (§10.1 - only valid within error handlers)
    ErrorVariable,
    
    // Constructor calls (§11.2 - base(args))
    BaseCall {
        arguments: Vec<Expression>,
    },
    
    // Async expressions (§17.1 - start expr)
    StartExpression {
        expression: Box<Expression>,
    },
    
    // Later assignment expressions (§17.1)
    LaterAssignment {
        variable: String,
        expression: Box<Expression>,
    },
    
    // Pattern matching expressions
    Match {
        value: Box<Expression>,
        cases: Vec<MatchCase>,
    },
}
```

### Supporting Types

**StringPart** (Language Spec §2.3.2):
```rust
enum StringPart {
    Text(String),                    // Plain text
    Interpolation(Expression),       // {variable} or {expression}
}
```

**InputType** (Language Spec §6.3):
```rust
enum InputType {
    String,     // input("prompt")
    Integer,    // input.integer("prompt")
    Number,     // input.number("prompt")
    Boolean,    // input.yesNo("prompt")
}
```

**MatchCase** (Pattern matching):
```rust
struct MatchCase {
    pattern: Pattern,
    guard: Option<Expression>,    // Optional when condition
    body: Vec<Statement>,
}
```

**Pattern** (Pattern matching constructs):
```rust
enum Pattern {
    // Literal patterns: 42, "hello", true
    Literal(Value),
    
    // Variable patterns: x (binds to variable)
    Variable(String),
    
    // Wildcard pattern: _
    Wildcard,
    
    // Constructor patterns: Some(x), Point(x, y)
    Constructor {
        name: String,
        patterns: Vec<Pattern>,
    },
    
    // List patterns: [x, y, z] or [head, ...tail]
    List {
        patterns: Vec<Pattern>,
        rest: Option<String>,    // For spread patterns like [x, ...rest]
    },
    
    // Object patterns: { x, y } or { x: pattern }
    Object {
        fields: Vec<FieldPattern>,
    },
    
    // Or patterns: pattern1 | pattern2
    Or(Vec<Pattern>),
    
    // Range patterns: 1..10
    Range {
        start: Box<Expression>,
        end: Box<Expression>,
        inclusive: bool,
    },
}
```

**FieldPattern** (Object pattern fields):
```rust
struct FieldPattern {
    name: String,
    pattern: Option<Pattern>,    // None means shorthand { x } instead of { x: pattern }
}
```

### Statements (Language Spec §6)

```rust
enum Statement {
    // Variable declarations (§6.1 - type-first)
    VariableDecl {
        name: String,
        type_: Type,
        initializer: Option<Expression>,
    },
    
    // Function blocks (§7.1 - functions:)
    FunctionsBlock {
        functions: Vec<Function>,
    },
    
    // Apply blocks (§4)
    TypeApplyBlock {
        type_: Type,
        assignments: Vec<VariableAssignment>,
    },
    FunctionApplyBlock {
        function_name: String,
        expressions: Vec<Expression>,
    },
    MethodApplyBlock {
        object_name: String,
        method_chain: Vec<String>,
        expressions: Vec<Expression>,
    },
    ConstantApplyBlock {
        constants: Vec<ConstantAssignment>,
    },
    
    // Assignment (§6.2)
    Assignment {
        target: String,
        value: Expression,
    },
    
    // Print statements (§6.4)
    Print {
        expression: Expression,
        newline: bool,
    },
    
    // Print block statements (§6.4)
    PrintBlock {
        expressions: Vec<Expression>,
        newline: bool,
    },
    
    // Control flow (§9)
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
    },
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    Iterate {
        iterator: String,
        collection: Expression,
        body: Vec<Statement>,
    },
    RangeIterate {
        iterator: String,
        start: Expression,
        end: Expression,
        step: Option<Expression>,
        body: Vec<Statement>,
    },
    
    // Return statements (§6.5)
    Return {
        value: Option<Expression>,
    },
    
    // Error handling (§10.1)
    Error {
        message: Expression,
    },
    
    // Testing (§8)
    Test {
        name: Option<String>,
        test_expression: Expression,
        expected_value: Expression,
    },
    TestsBlock {
        tests: Vec<TestCase>,
    },
    
    // Class definitions (§11)
    ClassDefinition {
        class: Class,
    },
    
    // Imports (§12.2)
    Import {
        imports: Vec<ImportItem>,
    },
    
    // Async statements (§17)
    LaterAssignment {
        variable: String,
        expression: Expression,
    },
    Background {
        expression: Expression,
    },
    
    // Private blocks (§12.1)
    PrivateBlock {
        items: Vec<Statement>,
    },
    
    // Pattern matching statements
    Match {
        value: Expression,
        cases: Vec<MatchCase>,
    },
    
    // Expression statements
    Expression {
        expr: Expression,
    },
}
```

### Declarations and Supporting Types

**Parameter** (Language Spec §7.2):
```rust
struct Parameter {
    name: String,
    type_: Type,
    default_value: Option<Expression>,  // §7.4 default parameters
}
```

**Function** (Language Spec §7):
```rust
struct Function {
    name: String,
    parameters: Vec<Parameter>,
    return_type: Type,
    body: Vec<Statement>,
    description: Option<String>,        // §7.2 description blocks
    syntax: FunctionSyntax,
    visibility: Visibility,
    modifier: FunctionModifier,
}

enum FunctionSyntax {
    Simple,      // Basic function
    Detailed,    // With description/input blocks
    Block,       // Inside functions: block
    Standalone,  // start() function only
    Background,  // background modifier
}

enum FunctionModifier {
    None,
    Background,  // §17.2 background functions
}

enum Visibility {
    Public,      // default
    Private,     // in private: blocks
}
```

**Class** (Language Spec §11):
```rust
struct Class {
    name: String,
    description: Option<String>,
    base_class: Option<String>,         // §11.6 inheritance
    fields: Vec<Field>,
    methods: Vec<Function>,
    constructor: Option<Constructor>,
}

struct Field {
    name: String,
    type_: Type,
    visibility: Visibility,
    default_value: Option<Expression>,
}

struct Constructor {
    parameters: Vec<Parameter>,
    body: Vec<Statement>,
}
```

**TestCase** (Language Spec §8):
```rust
struct TestCase {
    description: Option<String>,        // None for anonymous tests
    test_expression: Expression,
    expected_value: Expression,
}
```

**ImportItem** (Language Spec §12.2):
```rust
struct ImportItem {
    name: String,
    alias: Option<String>,
}
```

**Supporting Assignment Types**:
```rust
struct VariableAssignment {
    name: String,
    initializer: Option<Expression>,
}

struct ConstantAssignment {
    type_: Type,
    name: String,
    value: Expression,
}
```

**Program** (Top-level structure):
```rust
struct Program {
    imports: Vec<ImportItem>,
    statements: Vec<Statement>,
    functions: Vec<Function>,
    classes: Vec<Class>,
    start_function: Option<Function>,   // §7.1 special start function
    tests: Vec<TestCase>,
}
```

## 4. Operator Precedence (Language Spec §5.1)

Precedence from highest to lowest:
1. **Primary** - `()`, function calls, method calls, property access
2. **Unary** - `not`, `-` (unary minus)
3. **Exponentiation** - `^` (right-associative)
4. **Multiplicative** - `*`, `/`, `%`
5. **Additive** - `+`, `-`
6. **Comparison** - `<`, `>`, `<=`, `>=`
7. **Equality** - `==`, `!=`, `is`, `not`
8. **Logical AND** - `and`
9. **Logical OR** - `or`
10. **Assignment** - `=`

## 5. Dotted Syntax Disambiguation (Language Spec §16)

The AST must distinguish between:
- `math.sqrt(9)` → `NamespaceCall` (lowercase namespace)
- `obj.method(9)` → `MethodCall` (object method)
- `Class.method(9)` → `StaticMethodCall` (class method)

## 6. Multi-Line Expression Handling (Language Spec §5.2)

Multi-line expressions must be wrapped in parentheses:
```clean
result = (a + b + c +
          d + e + f)
```
The parser continues until all parentheses are balanced.

## 7. Special Language Constructs

### Apply Blocks (Language Spec §4)
Four types based on specification:
- **Type apply**: `integer: x = 1, y = 2`
- **Function apply**: `println: "line1", "line2"`
- **Method apply**: `list.add: item1, item2`
- **Constant apply**: `constant: integer MAX = 100`

### List Behavior Properties (Language Spec §3.4)
Handled via PropertyAssignment:
```clean
list.type = "line"     // FIFO queue
list.type = "pile"     // LIFO stack
list.type = "unique"   // Set behavior
```

### Console Input (Language Spec §6.3)
Four input types mapped to InputType enum:
- `input("prompt")` → `String`
- `input.integer("prompt")` → `Integer`
- `input.number("prompt")` → `Number`
- `input.yesNo("prompt")` → `Boolean`

### Testing Framework (Language Spec §8)
Two test forms:
- Named: `"description": expression = expected`
- Anonymous: `expression = expected`

## 8. AST Construction Rules

1. **Exact Language Mapping**: Every AST node corresponds to a Clean Language construct
2. **No Implementation Details**: AST represents language semantics, not implementation choices
3. **Source Preservation**: SourceLocation tracks original code positions
4. **Type Safety**: Strong typing matches language specification type system
5. **Precedence Respect**: Binary operations follow specified precedence rules

## 9. Validation Rules

- All identifiers must follow language specification naming rules
- Type annotations must use specified type syntax
- Operator precedence must be preserved in Binary expression trees
- Multi-line expressions must have balanced parentheses
- Apply blocks must follow specification patterns

## 10. Future Language Extensions

When the Clean Language Specification adds new features:
1. Update this AST specification first
2. Ensure new AST nodes exactly match language semantics
3. Maintain backward compatibility where possible
4. Update parser and semantic analysis to match

---

**Authority Note**: This AST specification is derived entirely from Clean_Language_Specification.md. Any discrepancies indicate either a specification interpretation error or a language specification ambiguity that requires clarification. We do not want to add any additional features that are not in the specification. We also dont need backward compatibility. If the specification changes, we should update the AST specification to match the new specification.