# Token-Driven Parser Implementation

**Date:** October 8, 2025
**Status:** ✅ Complete and Compiling
**Location:** `src/parser/token_parser.rs`

## Overview

Successfully implemented a **token-driven parser** for Clean Language following the rustc architecture pattern. This eliminates the problematic source reconstruction step that was used in the previous `SpecificationParser`.

## Architecture

### Design Pattern: Recursive Descent with Token Cursor

Following **rust-lang/rustc-dev-guide** best practices:

```
Source Code → Lexer → Token Stream → TokenParser → AST
                                          ↓
                            (No source reconstruction!)
```

### Core Parser Utilities (rustc-style)

| Method | Purpose | rustc Equivalent |
|--------|---------|------------------|
| `bump()` | Consume current token and advance | `Parser::bump()` |
| `check()` | Check if current token matches without consuming | `Parser::check()` |
| `eat()` | Consume if matches, return bool | `Parser::eat()` |
| `expect()` | Consume or error if mismatch | `Parser::expect()` |
| `look_ahead()` | Peek N tokens ahead | `Parser::look_ahead()` |

### Parser Structure

```rust
pub struct TokenParser {
    tokens: Vec<Token>,
    cursor: usize,
    file_path: String,
    errors: Vec<CompilerError>,
}
```

## Implementation Details

### 1. Top-Level Parsing

```rust
pub fn parse_program(&mut self) -> Result<Program, CompilerError>
```

Parses:
- **Functions** (`function` keyword)
- **Classes** (`class` keyword)
- **Tests** (`tests` block)
- **Imports** (`import` keyword)

### 2. Parsing Methods (779 lines)

| Method | Returns | Purpose |
|--------|---------|---------|
| `parse_function()` | `Function` | Parse function declarations |
| `parse_class()` | `Class` | Parse class declarations |
| `parse_field()` | `Field` | Parse class fields |
| `parse_method()` | `Function` | Parse class methods |
| `parse_tests_block()` | `Vec<TestCase>` | Parse test cases |
| `parse_test()` | `TestCase` | Parse individual test |
| `parse_import()` | `Vec<ImportItem>` | Parse imports |
| `parse_parameter_list()` | `Vec<Parameter>` | Parse function parameters |
| `parse_parameter()` | `Parameter` | Parse single parameter |
| `parse_type()` | `Type` | Parse type annotations |
| `parse_block()` | `Vec<Statement>` | Parse statement blocks |
| `parse_statement()` | `Statement` | Parse individual statements |
| `parse_return()` | `Statement` | Parse return statements |
| `parse_if()` | `Statement` | Parse if/else |
| `parse_while()` | `Statement` | Parse while loops (→ Iterate) |
| `parse_for()` | `Statement` | Parse for loops (→ Iterate) |
| `parse_print()` | `Statement` | Parse print statements |
| `parse_println()` | `Statement` | Parse println statements |
| `parse_expression()` | `Expression` | Entry point for expressions |
| `parse_comparison()` | `Expression` | Comparison operators |
| `parse_term()` | `Expression` | Addition/subtraction |
| `parse_factor()` | `Expression` | Multiplication/division/modulo |
| `parse_primary()` | `Expression` | Literals, variables, grouped expressions |

### 3. AST Alignment Fixes

Fixed all mismatches between parser and actual AST structures:

#### Function
- ✅ `return_type: Type` (not `Option<Type>`, defaults to `Void`)
- ✅ `modifier: FunctionModifier::None` (not `is_background: bool`)
- ✅ Added: `type_parameters`, `type_constraints`, `description`, `syntax`, `visibility`

#### Class
- ✅ `base_class: Option<String>` (uses `is` keyword, not `:`)
- ✅ Added: `type_parameters`, `description`, `base_class_type_args`, `constructor`

#### Field
- ✅ `type_: Type` (not `field_type`, required not optional)
- ✅ Added: `visibility`, `is_static`, `default_value`

#### Parameter
- ✅ `type_: Type` (not `param_type`, required not optional)

#### Type
- ✅ `Type::Object(String)` for custom types (not `Type::Custom`)
- ✅ Added `Type::Void` support

#### Statement Variants
- ✅ `Return { value, location }` (struct variant, not tuple)
- ✅ `If { condition, then_branch, else_branch, location }` (not `then_block`/`else_block`)
- ✅ `Iterate { iterator, collection, body, location }` (for both `for` and `while`)
- ✅ `Print { expression, newline, location }` (unified print/println)

#### Expression
- ✅ `Binary(Box<Expr>, BinaryOperator, Box<Expr>)` (tuple variant, not struct)
- ✅ Used `BinaryOperator` enum (not `BinaryOp`)

#### TestCase
- ✅ `TestCase { description, test_expression, expected_value, location }`
- ✅ Uses `is` keyword for expected values

#### ImportItem
- ✅ `ImportItem { name, alias }`
- ✅ Returns `Vec<ImportItem>` for comma-separated imports

### 4. Error Handling

All parse errors use correct signature:
```rust
CompilerError::parse_error(
    message: impl Into<String>,
    location: Option<SourceLocation>,
    help: Option<String>
)
```

## Integration

### SpecificationParser Update

**Before (98 lines with source reconstruction):**
```rust
pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
    let source = self.reconstruct_source_from_tokens(); // ❌ BAD
    let mut parser = ErrorRecoveringParser::new(&source, &self.file_path);
    parser.parse_program()
}
```

**After (35 lines, token-driven):**
```rust
pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
    let mut parser = TokenParser::new(
        std::mem::take(&mut self.token_stream),
        self.file_path.clone(),
    );
    parser.parse_program() // ✅ GOOD
}
```

### Module Structure

```
src/parser/
├── mod.rs                    # Exports TokenParser
├── specification_parser.rs   # Delegates to TokenParser
├── token_parser.rs          # ✨ NEW: Token-driven implementation
└── ...
```

## Benefits

### 1. Performance
- ❌ **Old:** Lexer → Tokens → Reconstruct Source → Re-parse
- ✅ **New:** Lexer → Tokens → Parse

### 2. Accuracy
- No information loss from token → source → token round-trip
- Preserves exact token spans and locations
- Better error messages with precise token information

### 3. Maintainability
- Clear separation of concerns
- Follows industry-standard architecture (rustc pattern)
- Easier to debug (single parsing pass)

### 4. Extensibility
- Easy to add new parsing rules
- Token cursor utilities make parsing straightforward
- Error recovery built into the design

## Compilation Status

✅ **Successfully compiles with 0 errors**
- 6 deprecation warnings (expected, for legacy IR module)
- All AST field names aligned
- All error call signatures correct

## Testing

### Unit Tests Required
- [ ] Parse function declarations
- [ ] Parse class declarations
- [ ] Parse expressions (binary, unary, literals)
- [ ] Parse statements (if, iterate, return, print)
- [ ] Parse imports and tests
- [ ] Error recovery and reporting

### Integration Tests Required
- [ ] End-to-end parsing of `.cln` files
- [ ] Compatibility with existing test suite
- [ ] Performance benchmarks vs old parser

## Next Steps

1. ✅ Token parser implemented and compiling
2. ⏳ Write unit tests for token parser
3. ⏳ Remove `parse_with_file` special-case in `parser_impl.rs`
4. ⏳ Integrate with 7-stage pipeline testing
5. ⏳ Performance benchmarking

## References

- **rustc-dev-guide:** Parser architecture patterns
- **src/lexer/specification_token.rs:** Token definitions
- **src/ast/mod.rs:** AST structure definitions
- **src/error/mod.rs:** Error reporting utilities

---

## Code Statistics

- **Total Lines:** 779
- **Parsing Methods:** 25
- **Token Utilities:** 7
- **Error Handling:** Comprehensive with location tracking
- **AST Coverage:** Functions, Classes, Statements, Expressions, Tests, Imports
