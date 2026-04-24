# AST Implementation Notes

> The authoritative AST node reference is **[spec/ast.md](../../spec/ast.md)**.
> That document is the 1:1 formal definition of every node in `src/ast/mod.rs`.
> Read it first. This file covers compiler-internal implementation details only.

## Source File

All AST types are defined in [`src/ast/mod.rs`](../src/ast/mod.rs).

## Key Implementation Details

### Location Tracking

Every node carries a `Location { line: usize, column: usize }`. The parser fills this from `TokenKind` metadata. Semantic analysis uses it for error reporting — never strip it.

### AssignmentTarget

`Statement::Assignment` uses `AssignmentTarget` (not a raw `String`) to distinguish:
- `Variable(String)` — simple name
- `Index { collection, index }` — `arr[i]`
- `Property { object, path }` — `obj.field`

The parser builds the correct variant based on trailing `[` vs `.` after the identifier.

### PostfixOperator vs UnaryOperator

`!` is postfix-only (`PostfixOperator::Required`), not a unary prefix. At compile-time it asserts non-null; at runtime it halts on null. Unary prefix operators are: `-` (negate), `not` (boolean).

### String Representation in MIR

String literals lower to `MirType::Ptr(I8)`. Host-function return strings (toString, etc.) lower to `MirType::Ptr(U8)`. Both map to `i32` in WASM. `is_string_operand()` checks both variants.

### Function Call Variants

The parser emits four call shapes depending on syntax:
- `FunctionCall` — plain `name(args)`
- `MethodCall` — `expr.name(args)`
- `ChainedMethodCall` — `expr.a().b()`
- `StaticMethodCall` — `Type.method(args)`

Code generation handles each path separately in `codegen/mod.rs`.
