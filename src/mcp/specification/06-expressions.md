# 06. Expressions

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


### Operator Precedence

**Operators with higher precedence bind more tightly than those with lower precedence. All binary operators are left-associative except `^`, which is right-associative.**

From highest to lowest precedence:

1. **Primary** — `()`, function calls, method calls, property access
2. **Postfix** — `!` (required not-none assertion)
3. **Unary** — `not`, `-` (unary minus)
4. **Exponentiation** — `^` (**right-associative**: `2^3^2` evaluates as `2^(3^2)` = 512, not `(2^3)^2` = 64)
5. **Multiplicative** — `*`, `/`, `%`
6. **Additive** — `+`, `-`
7. **Comparison** — `<`, `>`, `<=`, `>=`
8. **Equality** — `==`, `!=`, `is`
9. **Logical AND** — `and`
10. **Logical OR** — `or`
11. **None-Coalescing** — `default`
12. **Assignment** — `=`

All binary operators at levels 5–11 are **left-associative**: `a - b - c` evaluates as `(a - b) - c`.

There is no unary plus. `+x` is a syntax error (SYN002); write `x`.

The conditional expression `if … then … else …` (see [Conditional Expression](#conditional-expression)) has no level of its own: its `else` part reaches to the end of the expression.

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

**Joining text with `+`.** When either side of `+` is a `string`, `+` joins text. An `integer`, `number` or `boolean` on the other side is turned into its text first, exactly as `.toString()` would:

```clean
integer count = 5
print("count=" + count)        // count=5
print(count + " items")        // 5 items
print("ready: " + true)        // ready: true
print("x=" + 2.5)              // x=2.5
```

`-`, `*`, `/`, `%` and `^` never take text.

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

**Both sides must have compatible types.** `==`, `!=` and `is` compare values of the same type, or of types that convert to each other (`integer` and `number`, a class and its parent). A `T?` may be compared with a `T` and with `none`. Comparing unrelated types is a type error (SEM001), the same as for `<` and `>`:

```clean
integer n = 1
string? name = none
print(n == 1.0)          // ✅ true: integer and number compare as numbers
print(name == none)      // ✅ a T? compared with none
print(name == "Ada")     // ✅ a T? compared with a T
print(true == 1)         // ❌ SEM001: boolean and integer are unrelated
print("5" == 5)          // ❌ SEM001: string and integer are unrelated
```

### Conditional Expression

`if condition then a else b` is an expression: it gives `a` when the condition is true and `b` otherwise. Both branches must have the same type.

```clean
integer x = 5
string sign = if x > 0 then "positive" else "not positive"
integer size = if x > 10 then 3 else if x > 4 then 2 else 1     // chained: 2
```

**Rules:**

- The condition is a comparison (`x > 0`, `a == b`), a name or call that gives a `boolean`, or `not` followed by one. A condition with `and` or `or` must be in parentheses: `if (x > 0 and x < 10) then 1 else 0`.
- `else` is required. Chain more cases with `else if … then …`.
- The `else` part reaches to the end of the expression: `if c then 1 else 2 + 5` means `if c then 1 else (2 + 5)`. To use the result inside a larger expression, put the conditional in parentheses: `(if c then 1 else 2) + 5`.
- Use the statement form of `if` (see [Control Flow](./13-control-flow.md)) when a branch needs more than one value.

### Logical Operators

```clean
a and b     // Logical AND
a or b      // Logical OR
not a       // Logical NOT (unary prefix)
```

### None-Handling Operators

Clean Language provides two operators for working with values that may be `none` (a `T?`):

#### Default Operator (`default`)

The `default` operator provides a fallback value when the left operand is `none`. This is also known as none-coalescing (null-coalescing).

```clean
value default fallback    // Returns value if not none, otherwise fallback
```

**Important:** The `default` operator only checks for `none`, not for "falsy" values like `0`, `false`, or `""`.

On a plain (non-optional) value, `default` is allowed and does nothing: the value is never none, so the result is the value itself. The same holds for `!`.

```clean
// None-coalescing with 'default':
none default "x"           // Returns "x" (left is none)
"y" default "x"            // Returns "y" (left is not none)

// 'default' only coalesces none, NOT falsy values:
false default true         // Returns false (false is NOT none)
0 default 10               // Returns 0 (0 is NOT none)
"" default "fallback"      // Returns "" (empty string is NOT none)

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

**`!` is a postfix operator that asserts a value is not none at runtime, and narrows its type at compile time.**

The `!` operator is written immediately after the expression it applies to — it comes after the value, never before:

```clean
value!    // ✅ Correct postfix form
!value    // ❌ Not valid — ! is not a prefix here
```

**Runtime behavior:** If the value is `none` at the point of evaluation, execution halts with a runtime reference error (RUN004). If the value is not none, it is returned unchanged.

**Compile-time behavior:** After `!`, the compiler treats the result as the underlying `T` of the `T?` for subsequent type checking. None checks and `default` branches that follow are not required.

```clean
// maybeNone has type string? (it may be none)
string? maybeNone = getUser()

// After !, the compiler treats the result as string (not none)
string name = maybeNone!    // Runtime check: halts if none; compile-time: treated as string

// Chaining with method calls
string upper = getText()!.toUpperCase()    // getText() is checked for none before .toUpperCase()

// Use when you're certain a value is not none
integer firstScore = scores.first()!
```

**When to Use:**
- Use `!` when you are certain a value is not none and want to express that intent explicitly
- Use `default` when you want to provide a fallback value instead of halting
- Prefer `default` for user-facing code; use `!` for internal assertions where none would indicate a programming error

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

#### Named Arguments

An argument may name the parameter it is for: `name: value`. Named arguments may come in any order.

```clean
start:
	print(area(width: 3, height: 4))     // 12
	print(area(height: 2, width: 5))     // 10
	print(area(3, height: 6))            // 18: positional first, then named

functions:
	integer area(integer width, integer height)
		return width * height
```

**Rules:**

- The name must be one of the function's parameter names (FUNC008).
- Each name appears at most once (FUNC009).
- Positional arguments come before named ones (FUNC010): `area(3, height: 6)` is valid, `area(width: 3, 6)` is not.
- Each parameter gets exactly one value, positional or named (FUNC011).
- Named arguments work the same way for methods and constructors.

---

[← Previous: Apply-Blocks](./05-apply-blocks.md) | [Index](./README.md) | [Next: Statements](./07-statements.md) →
