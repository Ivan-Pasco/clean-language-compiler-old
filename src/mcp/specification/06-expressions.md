# 06. Expressions

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


### Operator Precedence

**Operators with higher precedence bind more tightly than those with lower precedence. All binary operators are left-associative except `^`, which is right-associative.**

From highest to lowest precedence:

1. **Primary** — `()`, function calls, method calls, property access
2. **Postfix** — `!` (required non-null assertion)
3. **Unary** — `not`, `-` (unary minus)
4. **Exponentiation** — `^` (**right-associative**: `2^3^2` evaluates as `2^(3^2)` = 512, not `(2^3)^2` = 64)
5. **Multiplicative** — `*`, `/`, `%`
6. **Additive** — `+`, `-`
7. **Comparison** — `<`, `>`, `<=`, `>=`
8. **Equality** — `==`, `!=`, `is`
9. **Logical AND** — `and`
10. **Logical OR** — `or`
11. **Null-Coalescing** — `default`
12. **Assignment** — `=`

All binary operators at levels 5–11 are **left-associative**: `a - b - c` evaluates as `(a - b) - c`.

### Multi-Line Expressions

**Rule**: If an expression spans multiple lines, it must be wrapped in parentheses.

**Parsing Logic**: The expression continues until all parentheses are properly balanced and closed. The parser will consume tokens across multiple lines until the opening parenthesis has its matching closing parenthesis.

**Syntax**:
```clean
// Single line expressions (no parentheses required)
result = a + b + c
value = functionCall(arg1, arg2)

// Multi-line expressions (parentheses required)
result = (a + b + c +
          d + e + f)

complex = (functionCall(arg1, arg2) +
           anotherFunction(arg3) *
           (nested + expression))

calculation = (matrix1 * matrix2 +
               matrix3.transpose() *
               scalar_value)
```

**Application Logic**:
1. **Single Line**: Expressions on a single line do not require parentheses
2. **Multi-Line Detection**: When the parser encounters an expression that continues to the next line, parentheses are mandatory
3. **Balanced Parsing**: The parser tracks parentheses depth and continues reading until:
   - All opening parentheses have matching closing parentheses
   - No unmatched parentheses remain
4. **Nested Support**: Multi-line expressions can contain nested parentheses for sub-expressions
5. **Error Handling**: Unmatched parentheses result in compilation errors with clear error messages

**Examples**:

```clean
// ✅ Valid: Single line, no parentheses needed
total = price + tax + shipping

// ✅ Valid: Multi-line with parentheses
total = (price + tax + 
         shipping + handling)

// ✅ Valid: Complex multi-line expression
result = (calculateBase(width, height) +
          calculateTax(subtotal) +
          (shippingCost * quantity))

// ✅ Valid: Multi-line function call
value = functionCall(
    (arg1 + arg2),
    (arg3 * arg4),
    defaultValue
)

// ❌ Invalid: Multi-line without parentheses
total = price + tax + 
        shipping         // Compilation error

// ❌ Invalid: Unmatched parentheses
result = (a + b + c      // Compilation error: missing closing parenthesis
```

**Benefits**:
- **Clarity**: Explicit parentheses make multi-line expressions unambiguous
- **Consistency**: Clear rules for when parentheses are required vs. optional
- **Readability**: Developers can format complex expressions across multiple lines
- **Error Prevention**: Prevents accidental statement termination in multi-line expressions

### Arithmetic Operators

```clean
a + b       // Addition
a - b       // Subtraction
a * b       // Multiplication
a / b       // Division
a % b       // Modulo
a ^ b       // Exponentiation
```

### Comparison Operators

```clean
a == b      // Equal
a != b      // Not equal
a < b       // Less than
a > b       // Greater than
a <= b      // Less than or equal
a >= b      // Greater than or equal
a is b      // Identity comparison
a not b     // Negated identity comparison
```

### Logical Operators

```clean
a and b     // Logical AND
a or b      // Logical OR
not a       // Logical NOT (unary prefix)
```

### Null-Handling Operators

Clean Language provides two operators for working with potentially null values:

#### Default Operator (`default`)

The `default` operator provides a fallback value when the left operand is `null`. This is also known as null-coalescing.

```clean
value default fallback    // Returns value if not null, otherwise fallback
```

**Important:** The `default` operator only checks for `null`, not for "falsy" values like `0`, `false`, or `""`.

```clean
// Null-coalescing with 'default':
null default "x"           // Returns "x" (left is null)
"y" default "x"            // Returns "y" (left is not null)

// 'default' only coalesces null, NOT falsy values:
false default true         // Returns false (false is NOT null)
0 default 10               // Returns 0 (0 is NOT null)
"" default "fallback"      // Returns "" (empty string is NOT null)

// Boolean logic with 'or' remains unchanged:
false or true              // Returns true (traditional boolean OR)
true or false              // Returns true
```

**Use Cases:**
```clean
// Provide default values for optional data
string username = userData.name default "Guest"
integer count = config.maxItems default 100
number price = product.price default 0.0

// Chain multiple defaults
string value = primary default secondary default "final fallback"
```

#### Required Assertion Operator (`!`)

**`!` is a postfix operator that asserts a value is non-null at runtime, and narrows its type at compile time.**

The `!` operator is written immediately after the expression it applies to — it comes after the value, never before:

```clean
value!    // ✅ Correct postfix form
!value    // ❌ Not valid — ! is not a prefix here
```

**Runtime behavior:** If the value is `null` at the point of evaluation, execution halts with a runtime reference error (RUN004). If the value is not null, it is returned unchanged.

**Compile-time behavior:** After `!`, the compiler treats the result as a guaranteed non-null value for subsequent type checking. Null checks and `default` branches that follow are not required.

```clean
// maybeNull has type string (nullable)
string? maybeNull = getUser()

// After !, the compiler treats the result as string (non-null)
string name = maybeNull!    // Runtime check: halts if null; compile-time: treated as string

// Chaining with method calls
string upper = getText()!.toUpperCase()    // getText() is checked for null before .toUpperCase()

// Use when you're certain a value is not null
integer count = list.find(item)!
```

**When to Use:**
- Use `!` when you are certain a value is not null and want to express that intent explicitly
- Use `default` when you want to provide a fallback value instead of halting
- Prefer `default` for user-facing code; use `!` for internal assertions where null would indicate a programming error

### Matrix Operations

Clean Language uses **type-based operator overloading** for basic operations and **method calls** for advanced operations:

```clean
// Basic operations (type-based overloading)
A * B       // Matrix multiplication (when A, B are matrix<T>)
A + B       // Matrix addition (when A, B are matrix<T>)
A - B       // Matrix subtraction (when A, B are matrix<T>)
a * b       // Scalar multiplication (when a, b are numbers)

// Advanced operations (methods)
A.transpose()    // Matrix transpose
A.inverse()      // Matrix inverse
A.determinant()  // Matrix determinant
```

### Method Calls and Property Access

```clean
obj.method()            // Method call
obj.property            // Property access
obj.method(arg1, arg2)  // Method with arguments
"string".length()       // Method on literal
myList.get(0)           // Built-in method
```

### Function Calls

**Function arguments are evaluated left-to-right before the function is called.** Side effects in argument expressions (such as function calls that mutate state) occur in left-to-right order.

```clean
functionName()                     // No arguments
functionName(arg1)                 // Single argument
functionName(arg1, arg2, arg3)     // Multiple arguments
```

---

[← Previous: Apply-Blocks](./05-apply-blocks.md) | [Index](./README.md) | [Next: Statements](./07-statements.md) →
