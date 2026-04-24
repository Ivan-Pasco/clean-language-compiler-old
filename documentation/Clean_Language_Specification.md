# Clean Language Comprehensive Specification

## Table of Contents

1. [Overview](#overview)
2. [Lexical Structure](#lexical-structure)
3. [Type System](#type-system)
4. [Apply-Blocks](#apply-blocks)
5. [Expressions](#expressions)
6. [Statements](#statements)
7. [Functions](#functions)
8. [Testing](#testing)
9. [Control Flow](#control-flow)
10. [Error Handling](#error-handling)
11. [Classes and Objects](#classes-and-objects)
12. [Modules and Imports](#modules-and-imports)
13. [Package Management](#package-management)
14. [Standard Library](#standard-library)
15. [Memory Management](#memory-management)
16. [Advanced Types](#advanced-types)
17. [Asynchronous Programming](#asynchronous-programming)
18. [AI Integration](#ai-integration)
19. [Plugin System](#plugin-system)
20. [State Management](#state-management)

## Overview

Clean Language is a modern, type-safe programming language designed to compile to WebAssembly (WASM). The language emphasizes strong static typing, first-class functions, matrix operations, and comprehensive error handling.

### Design Goals
- **Type Safety**: Strong static typing with type inference
- **Simplicity**: Clean, readable syntax with "one way to do things"
- **Performance**: Efficient compilation to WebAssembly
- **Expressiveness**: First-class support for mathematical operations and data structures
- **Error Handling**: Comprehensive error handling and recovery mechanisms
- **Developer Experience**: Method-style syntax and intuitive patterns

### File Extension
Clean Language source files use the `.cln` extension.

## Language Design Rules

These are core design rules that define how Clean Language code must be structured:

1. **Functions must be in a `functions:` block**
   - No standalone `function name(...)` at top level
   - Use `functions:` for top-level and class functions
   - Entry point uses `start:` block (not a function)
   ```clean
   // ❌ Invalid
   function myFunc()
       return 42

   // ✅ Valid
   functions:
       integer myFunc()
           return 42
   ```

2. **Helper methods require parentheses**
   - ✅ `x.toString()`
   - ❌ `x.toString`
   ```clean
   value = 42
   text = value.toString()  // ✅ Correct
   ```

3. **Use `any` when the type cannot be known at compile time**
   - `any` is a **compile-time escape hatch** — the compiler trusts that the developer knows the type is correct, similar to TypeScript's `any`.
   - Use `any` when the type genuinely cannot be known at compile time (plugin returns, JSON parsing, external data).
   - There is no runtime boxing or type tag — the compiler simply skips type checking for that value.
   - For type-safe collections, use explicit type parameters: `list<integer>`, `list<string>`, etc.
   ```clean
   functions:
       any identity(any value)
           return value
   ```

4. **Use `functions:` inside `class`**
   - All class methods go inside a `functions:` block
   ```clean
   class MyClass
       integer value
       
       functions:
           void setValue(integer newValue)
               value = newValue
   ```

5. **Use lowercase namespace functions**
   - Use `math.sqrt()`, `string.concat()`, `list.concat()` — not `Math.sqrt()`, `String.concat()`
   - Uppercase namespace names are not valid in Clean Language

6. **Use natural generic container syntax**
   - ✅ `list<item>`, `matrix<type>`
   - ❌ No angle brackets in user code (`<>`) - these are internal representations

7. **`any` is a compile-time generic escape hatch**
   - `any` tells the compiler to skip type checking for that value — the developer takes responsibility for correctness.
   - Use `any` only when the type genuinely cannot be known at compile time (plugin returns, JSON parsing, external data).
   - There is no runtime boxing or type tag involved — `any` is purely a compile-time concept.
   - For type-safe collections, use `list<integer>`, `list<string>`, etc. — use `list<any>` only when you genuinely need a heterogeneous collection.

8. **One way to do things**
   - Basic math: Use operators (`a + b`, `a * b`)
   - Comparisons: Use operators (`a == b`, `a > b`, `a >= b`)
   - Logic: Use operators (`a and b`, `a or b`, `not a`)
   - Advanced math: Use functions (`math.sqrt()`, `math.sin()`)
   - Object operations: Use method-style (`text.length()`, `value.toString()`, `items.add(x)`)
   - Utility functions: Use namespace calls (`string.concat()`, `list.concat()`, `list.range()`)

## Lexical Structure

### Comments

```clean
// Single line comment

/* 
   Multi-line
   comment
*/
```

### Whitespace and Indentation

Clean Language uses **tab-based indentation** for code structure:

- **Indentation**: Uses tabs only. Each tab represents one block level
- **Spaces**: May be used within expressions for alignment and formatting, but not for indentation
- **Block Structure**: Indentation defines code blocks (no braces `{}`)
- **Whitespace**: Includes spaces, tabs, carriage returns, and newlines

**Example:**
```clean
start:
⇥⇥⇥⇥integer x = 5    // Tab indentation
⇥⇥⇥⇥if x > 0
⇥⇥⇥⇥⇥⇥⇥⇥print("positive")    // Nested tab indentation
⇥⇥⇥⇥else
⇥⇥⇥⇥⇥⇥⇥⇥print("zero or negative")
```

**Indentation Rules:**
- Each indentation level must use exactly one tab character
- Mixing tabs and spaces for indentation is not allowed
- Spaces within expressions are permitted for readability:
  ```clean
  result = function(arg1,  arg2,  arg3)    // Spaces for alignment
  value  = x + y                           // Spaces around operators
  ```

### Identifiers

Identifiers must:
- Start with a letter (`A-Z`, `a-z`)
- Contain only letters, digits, and underscores
- Follow camelCase conventions (e.g. `myVariable`, `calculateSum`)

**Valid Examples:**
```clean
x
count
myVariable
value1
calculateSum
```

**Invalid Examples:**
```clean
1value      // Cannot start with digit
my-var      // Hyphens not allowed
$name       // Special characters not allowed
```

### Keywords

Reserved keywords in Clean Language:

```
and        class       computed     constructor  default     else
error      false       for          from         function    guard
if         import      in           intent       iterate     not         null
onError    or          print        reset        return
screen     source      spec         start        state       step         test        tests
this       to          true         watch        is          returns
description while      input        unit         private     constant
functions  rules       build        break        continue    require
```

#### Contextual Keywords

The following keywords are **contextual** — they are reserved when used as block headers (followed by `:`) but may be used as variable names, function parameters, and in expressions:

```
rules      computed    state       guard       watch       reset
screen     source      build       spec        intent      default
test       error       input       unit        step        description
```

This allows code like `string rules = ""` while still recognizing `rules:` as a block header.

### Literals

#### Numeric Literals

**Integers:**
```clean
42          // Decimal
-17         // Negative decimal
0xff        // Hexadecimal
0b1010      // Binary
0o777       // Octal
```

**Floating-Point:**
```clean
3.14        // Standard decimal
.5          // Leading zero optional
6.02e23     // Scientific notation
-2.5        // Negative number
```

#### String Literals

**Basic Strings:**
```clean
"Hello, World!"
"Line 1\nLine 2"
""          // Empty string
```

**Escape Sequences:**

Clean Language supports standard escape sequences within string literals:

| Sequence | Result | Example |
|----------|--------|---------|
| `\"` | Double quote | `"say \"hi\""` → `say "hi"` |
| `\\` | Backslash | `"path\\file"` → `path\file` |
| `\n` | Newline | `"line1\nline2"` → two lines |
| `\t` | Tab | `"col1\tcol2"` → tab-separated |
| `\r` | Carriage return | `"text\r"` → with CR |
| `\{` | Literal left brace | `"\{not interpolation\}"` → `{not interpolation}` |
| `\}` | Literal right brace | `"\{literal\}"` → `{literal}` |
| `\0` | Null character | `"text\0"` → null-terminated |

**Escape Sequences in JSON Strings:**

Escape sequences are particularly useful when working with JSON:

```clean
// JSON with escaped quotes
string jsonStr = "{\"name\": \"Alice\", \"age\": 25}"

// Parse JSON containing escape sequences
any data = json.textToData("{\"count\": 42}")
integer count = data.count  // Returns 42

// Nested JSON with multiple escape sequences
string nestedJson = "{\"user\": {\"name\": \"Bob\", \"active\": true}}"
any parsed = json.textToData(nestedJson)
string userName = parsed.user.name  // Returns "Bob"
```

**String Interpolation:**
```clean
name = "World"
greeting = "Hello, {name}!"     // Results in "Hello, World!"

// Simple property access allowed
user = User("Alice", 25)
message = "User {user.name} is {user.age} years old"

// Note: Complex method calls in strings are not supported
// ❌ "Hello {user.name}, you have {messages.count()} messages"
```

**Interpolation vs Literal Braces:**

The compiler distinguishes between interpolation and literal braces:
- `"{variable}"` → Interpolation (evaluates `variable`)
- `"{obj.prop}"` → Interpolation (evaluates `obj.prop`)
- `"{(expr)}"` → Interpolation (evaluates expression)
- `"{\"literal\"}"` → NOT interpolation (produces `{"literal"}`)
- `"\{literal\}"` → NOT interpolation (produces `{literal}`)

#### Boolean Literals
```clean
true
false
```

#### Null Literal

The `null` value represents the absence of a value. It is distinct from `0`, `false`, or empty string `""`.

```clean
null        // The null value
```

**Null Semantics:**
- `null` is its own type that is compatible with any nullable context
- `null == null` is `true`
- `null == anything_else` is `false` (except for another null)
- Use the `default` operator to provide fallback values for null
- Use the `!` operator to assert a value is not null

#### List Literals
```clean
[1, 2, 3, 4]           // Integer list
["a", "b", "c"]        // String list
[]                     // Empty list
[true, false, true]    // Boolean list
```

#### Matrix Literals
```clean
[[1, 2], [3, 4]]                    // 2x2 matrix
[[1, 2, 3], [4, 5, 6], [7, 8, 9]]   // 3x3 matrix
[[]]                                // Empty matrix
```

## Type System

### Core Types

| Type&nbsp;(keyword) | Description | Default Mapping | Literal Examples |
|---------------------|-------------|-----------------|------------------|
| `boolean`  | Logical value (`true` / `false`) | 1 bit | `true`, `false` |
| `integer`  | Whole numbers, signed | 32-bit | `42`, `-17` |
| `number`    | Decimal numbers | 64-bit | `3.14`, `6.02e23` |
| `string`   | UTF-8 text, dynamically sized | — | `"Hello"` |
| `void`     | No value / empty return type | 0 bytes | *(function return only)* |

**Type System Philosophy:**
Clean Language uses platform-optimal defaults for all numeric types. The `integer` type is 32-bit and the `number` type is 64-bit, providing the best balance of performance and precision for most applications.

### Precision Control for Larger Numbers

Clean Language supports **precision modifiers** for both integers and numbers when you need larger ranges or different precision levels:

#### Integer Precision Modifiers

```clean
// Standard integer (32-bit, -2,147,483,648 to 2,147,483,647)
integer standard = 2147483647

// 8-bit integer (-128 to 127)
integer:8 small = 127

// 16-bit integer (-32,768 to 32,767)  
integer:16 medium = 32767

// 32-bit integer (same as standard integer)
integer:32 large = 2147483647

// 64-bit integer (-9,223,372,036,854,775,808 to 9,223,372,036,854,775,807)
integer:64 huge = 9223372036854775807

// Unsigned variants (positive numbers only)
integer:8u smallUnsigned = 255      // 0 to 255
integer:16u mediumUnsigned = 65535  // 0 to 65,535
integer:32u largeUnsigned = 4294967295  // 0 to 4,294,967,295
integer:64u hugeUnsigned = 18446744073709551615  // 0 to 18,446,744,073,709,551,615
```

#### Number Precision Modifiers

```clean
// Standard number (64-bit double precision)
number standard = 3.141592653589793

// 32-bit single precision (faster, less precision)
number:32 singlePrecision = 3.14

// 64-bit double precision (same as standard number)
number:64 doublePrecision = 3.141592653589793
```

#### When to Use Precision Modifiers

**Use Larger Precision When:**
- **`integer:64`**: Working with very large whole numbers (timestamps, IDs, big calculations)
- **`number:64`**: Scientific computing, financial calculations requiring high precision
- **Unsigned integers**: When you know values will always be positive (array indices, counts)

**Use Smaller Precision When:**
- **`integer:8`**: Small counters, flags, or when memory is critical
- **`integer:16`**: Medium-sized numbers with memory constraints
- **`number:32`**: Graphics programming, real-time applications where speed matters more than precision

**Examples:**
```clean
functions:
    void demonstratePrecision()
        // Large calculations
        integer:64 population = 8000000000
        integer:64 timestamp = 1640995200000
        
        // High-precision calculations
        number:64 pi = 3.141592653589793
        number:64 e = 2.718281828459045
        
        // Memory-efficient small numbers
        integer:8 counter = 0
        integer:16 portNumber = 8080
        
        // Graphics calculations (speed over precision)
        number:32 screenX = 1920.0
        number:32 screenY = 1080.0
```

**Performance Characteristics:**
- **Memory Usage**: Smaller precision types use less memory
- **Speed**: 32-bit operations are typically faster than 64-bit on most platforms
- **Precision**: 64-bit numbers provide ~15 decimal digits vs ~7 for 32-bit
- **Range**: 64-bit integers can handle numbers up to ~9 quintillion vs ~2 billion for 32-bit

### Composite & Generic Types

| Type syntax | What it is | Example |
|-------------|------------|---------|
| `list<T>`  | Homogeneous resizable list (T is the element type) | `list<integer>`, `list<string>` |
| `matrix<T>` | 2-D list of lists (T is the element type) | `matrix<number>`, `[[1.0, 2.0], [3.0, 4.0]]` |
| `pairs<K,V>`  | Key-value associative container | `pairs<string, integer>` |
| `any`         | Compile-time generic: compiler skips type checking for this value | Used when the type genuinely cannot be known at compile time (plugin returns, JSON, external data) |

Lists in Clean are zero-indexed by default (list[0] is the first element).
For readability, you can access elements starting from 1 using:

list.at(index)
This returns the element at position index - 1.

### List Behaviors — Collection Behavior Modifiers

**List behaviors change how a list handles insertions and removals without changing its type.** Clean Language extends the core `list<T>` type with dot-notation behavior modifiers. This provides a unified, consistent approach to different collection patterns while maintaining type safety and simplicity.

#### Behavior Type Syntax

The type is declared inline in the variable declaration using dot notation:

```clean
list<integer>.line numbers = []          // FIFO queue of integers
list<string>.unique visitors = []        // Set of strings (no duplicates)
list<string>.line.unique taskQueue = []  // FIFO queue with uniqueness
```

The canonical behavior suffixes are:
- `.line` — FIFO queue behavior
- `.pile` — LIFO stack behavior
- `.unique` — set behavior (no duplicates)
- `.line.pile` — FIFO + LIFO combined
- `.line.unique` — FIFO queue with uniqueness
- `.pile.unique` — LIFO stack with uniqueness
- `.line.unique.pile` — all three behaviors

#### Supported Properties

**`.line` — Queue Behavior (FIFO)**

**First-In-First-Out behavior: elements are added to the back and removed from the front.**

```clean
functions:
	void processTaskQueue()
		list<string>.line tasks = []

		// Add tasks (to back)
		tasks.add("Task 1")
		tasks.add("Task 2")
		tasks.add("Task 3")

		// Process tasks (from front)
		iterate i in 1 to 3
			string currentTask = tasks.remove()  // Gets "Task 1", then "Task 2", etc.
			print("Processing: " + currentTask) +
```

**Modified Operations**:
- `add(item)` → Adds to the **back** of the list
- `remove()` → Removes from the **front** of the list
- `peek()` → Views the **front** element without removing
- Standard list operations (`get(index)`, `size()`) remain unchanged

**`.pile` — Stack Behavior (LIFO)**

**Last-In-First-Out behavior: elements are added and removed from the same end (top).**

```clean
functions:
	void undoSystem()
		list<string>.pile actions = []

		// Perform actions (add to top)
		actions.add("Create file")
		actions.add("Edit text")
		actions.add("Save file")

		// Undo actions (remove from top)
		iterate i in 1 to 3
			string lastAction = actions.remove()  // Gets "Save file", then "Edit text", etc.
			print("Undoing: " + lastAction) +
```

**Modified Operations**:
- `add(item)` → Adds to the **top** of the list
- `remove()` → Removes from the **top** of the list
- `peek()` → Views the **top** element without removing
- Standard list operations (`get(index)`, `size()`) remain unchanged

**`.unique` — Set Behavior (Uniqueness Constraint)**

**Only unique elements are stored; duplicate additions are silently ignored.**

```clean
functions:
	void trackUniqueVisitors()
		list<string>.unique visitors = []

		// Add visitors (duplicates ignored)
		visitors.add("Alice")    // Added
		visitors.add("Bob")      // Added
		visitors.add("Alice")    // Ignored (duplicate)
		visitors.add("Charlie")  // Added

		print("Unique visitors: " + visitors.size().toString()) +  // Prints: 3

		if visitors.contains("Alice")
			print("Alice has visited") +
```

**Modified Operations**:
- `add(item)` → Adds only if `item` is not already present
- `remove()` → Removes from default position (front)
- `contains(item)` → Optimized for membership testing
- Standard list operations remain available

#### Behavior Combinations

**Behaviors combine by chaining dot suffixes on the type declaration.**

```clean
// Unique queue — FIFO with no duplicates
list<string>.line.unique uniqueQueue = []

// Unique stack — LIFO with no duplicates
list<integer>.pile.unique uniqueStack = []

// All three behaviors combined
list<integer>.line.unique.pile allFeatures = []
```

#### Available Methods

All list types support these methods regardless of behavior:

**Core Methods:**
- `add(item)` → Adds an item to the list (behavior determines position)
- `remove()` → Removes and returns an item (behavior determines which item)
- `peek()` → Views the next item to be removed without removing it
- `contains(item)` → Returns `true` if the item exists in the list
- `size()` → Returns the number of items in the list

**Standard List Methods:**
- `get(index)` → Gets item at specific index (0-based)
- `set(index, item)` → Sets item at specific index
- `isEmpty()` → Returns `true` if list is empty
- `isNotEmpty()` → Returns `true` if list contains items

**Behavior Declaration:**
- Behavior is declared as part of the type at variable declaration time: `list<T>.line`, `list<T>.pile`, `list<T>.unique`, or combinations thereof.

#### Performance Characteristics
- `.line`: O(1) add, O(1) remove, O(1) peek
- `.pile`: O(1) add, O(1) remove, O(1) peek
- `.unique`: O(1) add/contains (hash-based), O(1) remove

#### Advantages

1. **Unified Type System**: Single `list<any>` type instead of multiple collection types
2. **Consistent API**: All lists share the same base methods
3. **Flexible Behavior**: Properties can be changed at runtime if needed
4. **Type Safety**: Full generic type support with compile-time validation
5. **Simplicity**: Easier to learn and remember than separate collection classes
6. **Interoperability**: All property-modified lists are still `list<any>` types

#### Complete Example

```clean
start:
	// Test line behavior (FIFO queue)
	list<integer>.line lineList = []
	lineList.add(1)
	lineList.add(2)
	lineList.add(3)

	integer first = lineList.remove()   // Returns 1 (first in, first out)
	integer second = lineList.remove()  // Returns 2

	// Pile behavior (LIFO stack)
	list<integer>.pile pileList = []
	pileList.add(10)
	pileList.add(20)
	pileList.add(30)

	integer top = pileList.remove()     // Returns 30 (last in, first out)

	// Unique behavior (set)
	list<integer>.unique uniqueList = []
	uniqueList.add(100)
	uniqueList.add(200)
	uniqueList.add(100)  // Ignored (duplicate)

	boolean hasHundred = uniqueList.contains(100)  // Returns true
	integer listSize = uniqueList.size()            // Returns 2 (no duplicates)

	print("List demonstrates flexible behavior via type declaration") +
```

### Type Annotations and Variable Declaration

Variables use **type-first** syntax:

```clean
// Basic variable declarations
integer count = 0
number temperature = 23.5
boolean isActive = true
string name = "Alice"

// Uninitialized variables
integer sum
string message
```

### Type Conversion

**Implicit conversions (safe widening):**
- `integer` → `number` (with precision loss warning)
- Same-sign, wider types → OK

**Explicit conversions (all require parentheses):**
```clean
value.toInteger()   // convert to integer
value.toNumber()    // convert to floating-point
value.toString()    // convert to string
value.toBoolean()   // convert to boolean
```

**Examples:**
```clean
integer num = 42
number numFloat = num.toNumber()      // ✅ Works: converts 42 to 42.0
integer piInt = 3.14.toInteger()      // ✅ Works: converts 3.14 to 3 (truncated)
boolean flag = 0.toBoolean()          // ✅ Works: converts 0 to false
boolean nonZero = 5.toBoolean()       // ✅ Works: converts 5 to true
```

## Apply-Blocks

**Apply-blocks are a core language feature where `identifier:` applies that identifier to each indented item.**

### Function Calls

Apply-blocks work with any function or method that takes a single argument. `print` is not valid as an apply-block target — use parenthesized `print()` calls instead:

```clean
items.add:
	item1
	item2
	item3
// Equivalent to: items.add(item1), items.add(item2), items.add(item3)
```

For printing multiple values, use individual `print()` calls:

```clean
print("First line") +
print(variable_name) +
print(result.toString()) +
```

### Variable Declarations
```clean
integer:
    count = 0
    maxSize = 100
    currentIndex = -1
// Equivalent to: integer count = 0, integer maxSize = 100, integer currentIndex = -1

string:
    name = "Alice"
    version = "1.0"
// Equivalent to: string name = "Alice", string version = "1.0"
```

### Constants
```clean
constant:
    integer MAX_SIZE = 100
    number PI = 3.14159
    string VERSION = "1.0.0"
```

## Expressions

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

## Statements

### Variable Declaration

```clean
// Type-first variable declarations
integer x = 10
number y = 3.14
string z
boolean flag = true
```

### Assignment

```clean
x = 42              // Simple assignment
arr[0] = value      // List element assignment
obj.property = val  // Property assignment
```

### Print Statements

Clean Language provides an intuitive and clean print syntax that distinguishes between output with and without newlines. The syntax uses a simple pattern: bare `print` for no newline, and `print() +` for adding a newline.

#### Simple Syntax
The print statement uses two distinct forms based on whether you want a newline:

**Print without newline:**
```clean
print "Hello"           // Prints "Hello" (no newline)
print variable          // Prints variable content (no newline)
print expression        // Prints expression result (no newline)
print 42                // Prints "42" (no newline)
```

**Print with newline:**
```clean
print("Hello") +        // Prints "Hello" and adds newline
print(variable) +       // Prints variable content and adds newline
print(expression) +     // Prints expression result and adds newline
print(42) +             // Prints "42" and adds newline
```

**Key Design Principles:**
- **Intuitive**: Parentheses + plus sign clearly indicate "adding" a newline
- **Clean**: Simple distinction between the two behaviors
- **Readable**: The `+` visually represents adding the newline functionality
- **Consistent**: Follows Clean Language's principle of clear, unambiguous syntax

#### Automatic String Conversion

**Print functions work seamlessly with all data types through the toString() method system**. The compiler automatically handles string conversion when needed:

```clean
// toString() method calls work perfectly
integer age = 25
number price = 19.99
boolean isValid = true

print(age.toString())       // Prints: 25
print(price.toString())     // Prints: 19.99  
print(isValid.toString())   // Prints: true

// String variables and literals work directly
string name = "Alice"
print(name)                 // Prints: Alice
print("Hello World")        // Prints: Hello World

// Mixed usage in the same program
print("Age:")
print(age.toString())
print("Price:")
print(price.toString())
```

#### Default toString() Behavior

Every type in Clean Language has a built-in `toString()` method with sensible defaults:

**Built-in Types:**
- **Integers**: `42` → `"42"`
- **Floats**: `3.14` → `"3.14"`
- **Booleans**: `true` → `"true"`, `false` → `"false"`
- **Strings**: `"hello"` → `"hello"` (no change)
- **Lists**: `[1, 2, 3]` → `"[1, 2, 3]"`
- **Objects**: `MyClass` instance → `"MyClass"` (default) or custom representation

**Custom Classes:**
```clean
class Person
    string name
    integer age
    
    // Optional: Override default toString() for custom output
    functions:
        string toString()
            return name + " (" + age.toString() + " years old)"

// Usage
Person user = Person("Alice", 30)
print(user)             // Prints: Alice (30 years old)

// Without custom toString(), would print: Person
```

**Default Class Behavior:**
- Classes without custom `toString()` method print their class name
- You can override `toString()` in any class for custom string representation
- The custom `toString()` method is automatically used by print functions

#### Printing Multiple Values

To print multiple values in sequence, use the `print:` block form or individual `print()` calls.

**Block form — `print:` block:**

The `print:` block prints each indented expression on its own line. This is the idiomatic way to print several values in a row:

```clean
print:
    "User: " + username
    "Score: " + score
    "Status: active"
```

Each line in the block is equivalent to a `print(expr) +` call. The above is exactly the same as:

```clean
print("User: " + username) +
print("Score: " + score) +
print("Status: active") +
```

Use the block form when printing several related values together; use the individual call form when the print statements are spread through conditional or loop logic.

### Console Input

Console input in Clean lets you ask the user for a value with a simple prompt. Use `input()` for text, `input.integer()` and `input.number()` for numbers, and `input.yesNo()` for true/false — all with safe defaults and clear syntax.

```clean
// Get text input from user
string name = input("What's your name? ")
string message = input()  // Simple prompt with no text

// Get numeric input with automatic conversion
integer age = input.integer("How old are you? ")
number height = input.number("Your height in meters: ")

// Get yes/no input as boolean
boolean confirmed = input.yesNo("Are you sure? ")
boolean subscribe = input.yesNo("Subscribe to newsletter? ")
```

#### Input Features

- **Safe defaults**: Invalid input automatically retries with helpful messages
- **Type conversion**: `input.integer()` and `input.number()` handle numeric conversion safely
- **Boolean parsing**: `input.yesNo()` accepts "yes"/"no", "y"/"n", "true"/"false", "1"/"0"
- **Clean prompts**: Prompts are displayed clearly and wait for user input
- **Error handling**: Invalid input shows friendly error messages and asks again

#### Usage Examples

```clean
start:
    // Basic user interaction
    string userName = input("Enter your name: ")
    print("Hello, " + userName + "!")

    // Numeric calculations
    integer num1 = input.integer("First number: ")
    integer num2 = input.integer("Second number: ")
    integer sum = num1 + num2
    print("Sum: " + sum.toString())

    // Decision making
    boolean wantsCoffee = input.yesNo("Would you like coffee? ")
    if wantsCoffee
        print("Great! Coffee coming right up.")
    else
        print("No problem, maybe next time.")
```

### Return Statement

```clean
return              // Return void
return value        // Return a value
return expression   // Return expression result
```

## File Structure

A Clean Language file (`.cln`) is organized into top-level sections. Each section is optional, but when present, they must appear in this order:

| Order | Section | Purpose |
|-------|---------|---------|
| 1 | `import:` | Bring in code from other modules |
| 2 | `start:` | Where your program begins running |
| 3 | `state:` | Variables that persist and can be watched |
| 4 | `class` | Define your own types |
| 5 | `functions:` | Reusable helper functions |

### Why Order Matters

Clean Language enforces section order to keep code consistent and readable. When you open any `.cln` file, you always know where to find things.

If sections are out of order, the compiler tells you exactly what's wrong:

```
Error: 'state:' must appear after 'start:' block
Error: 'import:' must be the first section in the file
Error: 'functions:' must appear after class declarations
```

### A Complete Example

Here's a file that uses all sections in the correct order:

```clean
import:
    utils
    math_helpers

start:
    print("Hello, World!")
    integer result = add(5, 3)
    print(result)

state:
    integer count = 0
    string username = ""

class Point
    integer x
    integer y

functions:
    integer add(integer a, integer b)
        return a + b
```

### What Can't Go at the Top Level

Only the sections listed above can appear at the top level. You can't write loose statements like assignments, function calls, or loops outside of a block.

```clean
// ❌ Invalid - can't have loose code at top level
integer x = 5
print("hello")

// ✅ Valid - code goes inside start: block
start:
    integer x = 5
    print("hello")
```

## Build Configuration

When using state `rules:`, a `build:` block is required in your source file before `start:`. This block configures how the compiler handles rules checking.

**Basic structure:**
```clean
build:
    rules = true

state:
    integer count = 0
    rules:
        count >= 0

start:
    count = 5
```

**Rules options:**

| Value | Behavior |
|-------|----------|
| `true` | Always check rules |
| `false` | Never check rules (production optimization) |
| `"development"` | Check only in development builds |

**Example for production apps:**
```clean
build:
    rules = "development"

state:
    integer balance = 0
    rules:
        balance >= 0

start:
    balance = 100
```

**CLI behavior:**
```bash
cln compile app.cln              # Development: rules ON
cln compile app.cln --release    # Production: follows build: setting
```

**Rules for `build:`:**
- Must appear at the top level, before `start:`
- Only one `build:` block per file
- Only required when using state `rules:`
- If you use `rules:` in your state but don't have a `build:` block, the compiler will show an error

## Functions

Clean Language uses **functions blocks** for all function declarations. This ensures consistency and organization in code structure.

**Design philosophy for free functions:**
- The `functions:` section exists for pure helper logic
- Functions should be stateless and side-effect free where possible
- Intended for math, construction helpers, and reusable algorithms
- Not intended for application orchestration or domain flow — use classes for that

### The Start Block (Entry Point)

Every Clean program begins with a `start:` block. This is where your program starts running.

The `start:` block uses block syntax — just a colon followed by indented code:

```clean
start:
    print("Hello, World!")
    integer x = 42
    print(x)
```

**Rules for `start:`:**
- Use block syntax with a colon, not parentheses
- Must be at the top level (not inside `functions:` or any other block)
- Only one `start:` block per file
- Library modules can skip `start:` entirely

**Important:** The `start:` entry block is different from the `start` keyword used for background expressions. See [Asynchronous Programming](#asynchronous-programming) for details on `start` as an expression.

#### Migrating from Old Syntax

The function-style `start()` syntax is no longer supported:

```clean
// ❌ No longer valid
start()
    print("Hello")

// ❌ No longer valid
functions:
    void start()
        print("Hello")

// ✅ Use this instead
start:
    print("Hello")
```

### Functions Blocks (Required)

**All functions must be declared within a `functions:` block.** This is the only supported syntax for function declarations:

```clean
functions:
    integer add(integer a, integer b)
        return a + b

    integer multiply(integer a, integer b)
        description "Multiplies two integers"
        input
            integer a
            integer b
        return a * b
    
    integer square(integer x)
        return x * x
    
    void printMessage()
        print("Hello World")
```

### Generic Functions with `any`

Clean Language uses `any` as the universal generic type. No explicit type parameter declarations are needed:

```clean
functions:
    any identity(any value)
        return value
    
    any getFirst(list<any> items)
        return items[0]
    
    void printAny(any value)
        print(value.toString())

// Usage - type is inferred at compile time
string result = identity("hello")    // any → string
integer number = identity(42)        // any → integer
number decimal = identity(3.14)       // any → number
```

### Function Features

Functions support optional documentation and input blocks:

```clean
functions:
    integer calculate(integer x, integer y)
        description "Calculates something important"
        input
            integer x
            integer y
        return x + y
```

### Default Parameter Values

Clean Language supports default parameter values in both function declarations and input blocks. This feature enhances code readability and provides sensible defaults for optional parameters.

#### Input Block Default Values

Default values are particularly useful in input blocks, allowing functions to work with sensible defaults when parameters are not provided:

```clean
functions:
    integer calculateArea()
        description "Calculate area with default dimensions"
        input
            integer width = 10      // Default width
            integer height = 5      // Default height
        return width * height

    string formatMessage()
        description "Format a message with optional parameters"
        input
            string text = "Hello"   // Default message
            string prefix = ">> "   // Default prefix
            boolean uppercase = false  // Default formatting
        if uppercase
            return prefix + text.toUpperCase()
        else
            return prefix + text
```

#### Function Parameter Default Values

Default values can also be used in regular function parameters:

```clean
functions:
    string greet(string name = "World")
        return "Hello, " + name
    
    integer power(integer base, integer exponent = 2)
        // Default exponent of 2 for squaring
        return base ^ exponent
    
    void logMessage(string message, string level = "INFO")
        print("[" + level + "] " + message)
```

#### Usage Examples

```clean
start:
    // Using functions with default values
    print(greet())              // "Hello, World" (uses default)
    print(greet("Alice"))       // "Hello, Alice" (overrides default)

    integer squared = power(5)  // 25 (uses default exponent=2)
    integer cubed = power(5, 3) // 125 (overrides exponent)

    logMessage("System started")           // [INFO] System started
    logMessage("Error occurred", "ERROR")  // [ERROR] Error occurred

    // Input blocks with defaults work seamlessly
    integer area1 = calculateArea()        // Uses defaults: 10 * 5 = 50
    // When calling functions with input blocks, defaults are applied automatically
```

#### Default Value Rules

1. **Expression Support**: Default values can be any valid Clean Language expression.
2. **Type Compatibility**: Default values must match the parameter's declared type (SEM001).
3. **Lazy Evaluation**: Default values are evaluated **at call time, only when the argument is omitted** — they are not evaluated when the function is defined. If the default is a function call, it runs fresh each time the default is used.
4. **Optional Nature**: Parameters with default values become optional in function calls — they must still appear after all required parameters.

**Examples of Valid Default Values:**
```clean
functions:
    void examples()
        input
            integer count = 42                    // Literal value
            string message = "Default text"       // String literal
            boolean flag = true                   // Boolean literal
            number ratio = 3.14                    // Number literal
            integer calculated = 10 + 5           // Expression
            string formatted = "Value: " + "test" // String concatenation
```

### Method Calls (Require Parentheses)

All method calls must include parentheses, even when no arguments are provided:

```clean
functions:
    void demonstrateMethods()
        integer value = 42
        string text = value.toString()    // ✅ Correct - parentheses required
        integer length = text.length()   // ✅ Correct - parentheses required
        
        // ❌ Invalid - missing parentheses
        // string bad = value.toString
        // integer badLength = text.length
```

### Function Call Syntax

Functions are called using standard syntax:

```clean
start:
    integer result = add(5, 3)
    integer value = multiply(2, 4)
    integer squared = square(7)
    printMessage()
```

### Automatic Return

If a function doesn't use explicit `return`, Clean automatically returns the value of the last expression:

```clean
functions:
    integer addOne(integer x)
        x + 1    // Automatically returned
    
    string greet(string name)
        "Hello, " + name    // Automatically returned
```

## Contracts: `require`

Use `require` to declare preconditions that must be true for a function to execute. If the condition is false, execution stops with a contract violation error.

**Syntax:**
```clean
require <boolean_expression>
```

**Example:**
```clean
functions:
    integer divide(integer a, integer b)
        require b != 0
        return a / b

    void setAge(integer age)
        require age >= 0
        require age <= 150
        // implementation
```

**Rules for `require`:**
- Can only appear inside functions or class methods
- Must appear before other statements in the function body
- Multiple `require` statements are allowed
- Always checked at runtime (cannot be disabled)

**Error on violation:**
```
Contract violation: require failed at divide:2
  Expression: b != 0
```

## Testing

Clean Language includes a built-in testing framework with a simple and readable syntax. Tests can be embedded directly in your source code using the `tests:` block.

### Test Block Syntax

Tests are defined within a `tests:` block and can be either named or anonymous:

```clean
tests:
    // Named tests with descriptions
    "adds numbers": add(2, 3) = 5
    "squares a number": square(4) = 16
    "detects empty string": string.isEmpty("") = true
    
    // Anonymous tests (no description)
    "hi".toUpperCase() = "HI"
    math.abs(-42) = 42
    [1, 2, 3].length() = 3
```

### Test Syntax Rules

1. **Named Tests**: `"description": expression = expected`
   - The description is a string literal that will be used as a label in test output
   - The colon (`:`) separates the description from the test expression
   - Useful for documenting what the test is verifying

2. **Anonymous Tests**: `expression = expected`
   - No description provided - the expression itself serves as documentation
   - Simpler syntax for obvious test cases

3. **Test Expressions**: Can be any valid Clean Language expression
   - Function calls: `add(2, 3)`
   - Method calls: `string.isEmpty("")`
   - Complex expressions: `(x + y) * 2`
   - Object creation and method chaining: `Point(3, 4).distanceFromOrigin()`

4. **Expected Values**: The right side of `=` is the expected result
   - Must be a compile-time evaluable expression or literal
   - Type must match the test expression's return type

### Test Execution

When a Clean program contains a `tests:` block, the compiler can run tests in several ways:

```bash
# Run tests during compilation
cleanc --test myprogram.cln

# Compile and run tests separately
cleanc myprogram.cln --include-tests
./myprogram --run-tests
```

### Test Output Format

The test runner provides clear, readable output:

```
Running tests for myprogram.cln...

✅ adds numbers: add(2, 3) = 5 (PASS)
✅ squares a number: square(4) = 16 (PASS) 
❌ detects empty string: string.isEmpty("") = true (FAIL: expected true, got false)
✅ "hi".toUpperCase() = "HI" (PASS)

Test Results: 3 passed, 1 failed, 4 total
```

### Advanced Testing Features

#### Testing Functions with Error Handling

```clean
functions:
    integer safeDivide(integer a, integer b)
        if b == 0
            error("Division by zero")
        return a / b

tests:
    "normal division": safeDivide(10, 2) = 5
    "division by zero throws error": safeDivide(10, 0) = error("Division by zero")
```

#### Testing Object Methods

```clean
class Calculator
    integer value
    
    constructor(integer initialValue)
        value = initialValue
    
    functions:
        integer add(integer x)
            value = value + x
            return value

tests:
    "calculator addition": Calculator(10).add(5) = 15
    "calculator chaining": Calculator(0).add(3).add(7) = 10
```

#### Testing List and String Operations

```clean
tests:
    "list operations": [1, 2, 3].length() = 3
    "list contains": [1, 2, 3].contains(2) = true
    "string operations": "hello".toUpperCase() = "HELLO"
    "string indexing": "world".indexOf("r") = 2
```

### Best Practices

1. **Descriptive Test Names**: Use clear, descriptive names for complex tests
   ```clean
   tests:
       "calculates compound interest correctly": calculateCompoundInterest(1000, 0.05, 2) = 1102.5
   ```

2. **Test Edge Cases**: Include tests for boundary conditions
   ```clean
   tests:
       "handles empty list": [].length() = 0
       "handles single character": "a".toUpperCase() = "A"
       "handles zero input": factorial(0) = 1
   ```

3. **Group Related Tests**: Organize tests logically within the `tests:` block
   ```clean
   tests:
       // Basic arithmetic
       "addition": add(2, 3) = 5
       "subtraction": subtract(5, 2) = 3
       
       // String operations  
       "uppercase conversion": "hello".toUpperCase() = "HELLO"
       "lowercase conversion": "WORLD".toLowerCase() = "world"
   ```

4. **Test Both Success and Failure Cases**: Include tests for error conditions
   ```clean
   tests:
       "valid input": processInput("valid") = "processed: valid"
       "invalid input": processInput("") = error("Input cannot be empty")
   ```

## Control Flow

### Conditional Statements

```clean
// Basic if statement
if condition
    // statements

// If-else
if condition
    statements
else
    statements

// If-else if chain
if condition1
    statements
else if condition2
    statements
else
    statements
```

### Loops

#### Iterate Loop (for-each)

```clean
// Iterate over list elements
iterate item in list
    print(item)

// Iterate over string characters
iterate char in "hello"
    print(char)
```

#### Range-based Loops

```clean
iterate name in source [step n]
    // body

// Examples:
iterate i in 1 to 10
    print(i)

iterate k in 10 to 1 step -2
    print(k)                 // 10, 8, 6, 4, 2

iterate ch in "Clean"
    print(ch)

iterate row in matrix
    iterate value in row
        print(value)

iterate idx in 0 to 100 step 5
    print(idx)               // 0, 5, 10, …, 100
```

#### While Loop

The `while` loop executes a block of code repeatedly as long as a condition remains true. This is useful when you don't know in advance how many iterations are needed.

**Syntax:**
```clean
while condition
    // body - executed while condition is true
```

**Examples:**

```clean
// Basic counter loop
integer count = 0
while count < 5
    print(count.toString())
    count = count + 1
// Prints: 0, 1, 2, 3, 4

// Loop with boolean condition
boolean running = true
integer iterations = 0
while running
    iterations = iterations + 1
    if iterations >= 3
        running = false
// Stops after 3 iterations

// Nested while loops
integer outer = 0
while outer < 3
    integer inner = 0
    while inner < 2
        print("outer: " + outer.toString() + ", inner: " + inner.toString())
        inner = inner + 1
    outer = outer + 1

// While loop with if statement inside
integer i = 0
while i < 10
    integer remainder = i % 2
    if remainder == 0
        print("Even: " + i.toString())
    else
        print("Odd: " + i.toString())
    i = i + 1
```

**Rules:**
- The condition must evaluate to a boolean value
- The body is indented one level deeper than the `while` keyword
- Variables modified in the loop body are properly updated each iteration
- Infinite loops occur if the condition never becomes false (ensure loop variables are updated)

**Important Notes:**
- Use `break` to exit a loop early and `continue` to skip to the next iteration
- The while loop is useful for input validation, processing until a condition is met, or when the number of iterations is unknown

## Error Handling

### Raising Errors

```clean
functions:
    integer divide()
        input
            integer a
            integer b
        if b == 0
            error("Cannot divide by zero")
        return a / b
```

### Error Handling with onError

```clean
value = riskyCall() onError 0
data = readFile("file") onError print(error)

```

If an expression fails, onError runs the next line or block.
The error is available as error.


## Classes and Objects

Classes are the primary mechanism for expressing domain behavior and responsibility in Clean Language.

**Design philosophy:**
- Long-lived behavior and stateful logic should be expressed as class methods
- Application behavior (update, render, interaction, I/O coordination) naturally lives in classes
- Entry blocks (`start:`) should delegate to classes rather than implement domain logic directly
- Clean does not require all logic to be object-oriented, but encourages classes as the natural home for complex or evolving behavior

### Class Definition

**All class methods must be declared within a `functions:` block:**

```clean
class Point
	integer x
	integer y

	constructor(integer x, integer y)

	functions:
		integer distanceFromOrigin()
			return sqrt(x * x + y * y)

		void move(integer dx, integer dy)
			x = x + dx
			y = y + dy
```

### Generic Classes with `any`

Clean Language uses `any` for generic class fields and methods:

```clean
class Container
	any value

	constructor(any value)

	functions:
		any get()
			return value

		void set(any newValue)
			value = newValue
```

### Inheritance

Clean Language supports single inheritance using the `is` keyword. Child classes inherit all public fields and methods from their parent class.

```clean
class Shape
    string color
    
    constructor(string colorParam)
        color = colorParam          // Implicit context - no 'this' needed
    
    functions:
        string getColor()
            return color            // Direct field access

class Circle is Shape
    number radius
    
    constructor(string colorParam, number radiusParam)
        base(colorParam)            // Call parent constructor with 'base'
        radius = radiusParam        // Implicit context
    
    functions:
        number area()
            return 3.14159 * radius * radius
        
        string getInfo()
            return color + " circle"    // Access inherited field directly
```

#### Inheritance Features

- **Syntax**: Use `class Child is Parent` to inherit from a parent class
- **Base Constructor**: Use `base(args...)` to call the parent constructor
- **Implicit Context**: No need for `this` or `self` - fields are directly accessible
- **Name Safety**: Parameters must have different names than fields to prevent conflicts
- **Method Inheritance**: Child classes inherit all public methods from parent classes
- **Field Inheritance**: Child classes inherit all public fields from parent classes
- **Method Overriding**: Child classes can override parent methods by defining methods with the same name

#### Implicit Context Rules

Clean Language uses implicit context for accessing class fields:

- ✅ `color = colorParam` (field assignment — implicit `this`)
- ✅ `return color` (field access — implicit `this`)
- ✅ `radius = radiusParam` (works in child classes too)
- ✅ `this.render()` (explicit self-method call)
- ✅ `this.name` (explicit field access — equivalent to just `name`)
- ❌ Parameter names cannot match field names (compiler enforced)

Fields can be accessed directly by name (implicit `this`) or with explicit `this.field`. The `this` keyword is available inside all class methods and refers to the current instance. Explicit `this` is useful only for self-method calls.

### Object Creation and Usage

```clean
start:
    // Create objects
    Point point = Point(3, 4)
    Circle circle = Circle("red", 5.0)

    // Call methods (parentheses required)
    integer distance = point.distanceFromOrigin()
    point.move(1, -2)

    // Access properties
    integer xCoord = point.x
    string color = circle.color
```

### Static Methods

You can call class methods directly on the class name if they don't use instance fields:

```clean
class MathUtils
    functions:
        number add(number a, number b)
            return a + b
        
        number max(number a, number b)
            return if a > b then a else b

class DatabaseService
    functions:
        boolean connect(string url)
            // implementation that doesn't use instance fields
            return true
        
        User findUser(integer id)
            // implementation that doesn't use instance fields
            return User.loadFromDatabase(id)

// Static method calls - ClassName.method()
start:
    number result = MathUtils.add(5.0, 3.0)
    number maximum = MathUtils.max(10.0, 7.5)
    boolean connected = DatabaseService.connect("mysql://localhost")
    User user = DatabaseService.findUser(42)
```

**Rules for Static Methods:**
- Use `ClassName.method()` syntax for static calls
- Only allowed if the method doesn't access instance fields (`this.field`)
- All methods must be in `functions:` blocks
- Method calls require parentheses: `MathUtils.add()` not `MathUtils.add`
- Ideal for helpers, services, utilities, and database access functions

**Example - Mixed Static and Instance Methods:**
```clean
class User
    string name
    integer age
    
    constructor(string name, integer age)
    
    functions:
        // Instance method - accesses fields
        string getInfo()
            return "User: {name}, Age: {age}"
        
        // Static method - no field access
        boolean isValidAge(integer age)
            return age >= 0 and age <= 150

// Usage
start:
    User user = User("Alice", 25)
    string info = user.getInfo()                    // Instance method call
    boolean valid = User.isValidAge(30)             // Static method call
```

### Design Philosophy: Flexible Organization

Clean Language supports both class-based organization and top-level functions, providing flexibility for different coding styles and project needs:

#### Class-Based Organization (Recommended for complex projects)
- **Better code organization**: Related functionality is grouped together
- **Namespace management**: No global function name conflicts  
- **Consistent syntax**: All method calls use the same `Class.method()` or `object.method()` pattern
- **Extensibility**: Easy to add related methods to existing classes

```clean
class Calculator
    functions:
        number calculateTax(number amount)
            return amount * 0.15
        
        string formatResult(number value)
            return "Result: " + value.toString()
```

#### Top-Level Functions (Suitable for simpler projects)
- **Direct approach**: Functions can be declared directly in `functions:` blocks
- **Simplicity**: No need for class wrapper when functionality is standalone
- **Scripting style**: Perfect for utility scripts and simple programs

```clean
start:
    number tax = calculateTax(100.0)
    string result = formatResult(tax)
    print(result)

functions:
    number calculateTax(number amount)
        return amount * 0.15

    string formatResult(number value)
        return "Result: " + value.toString()
```

**Both approaches are valid and can be mixed within the same program.** The choice depends on project complexity and developer preference.

## Standard Library

Clean Language provides built-in utility classes for common operations. All standard library classes follow the compiler instructions:

- All methods are in `functions:` blocks
- Method calls require parentheses
- No `Utils` suffix in class names
- Use `any` for generic operations

### Math Module

The math module follows Clean Language's "one way to do things" principle. Basic arithmetic operations use operators, while advanced mathematical functions use methods.

**Basic Arithmetic - Use Operators:**
- Addition: `a + b` (not `math.add(a, b)`)
- Subtraction: `a - b` (not `math.subtract(a, b)`)
- Multiplication: `a * b` (not `math.multiply(a, b)`)
- Division: `a / b` (not `math.divide(a, b)`)
- Exponentiation: `a ^ b` (not `math.pow(a, b)`)

**Advanced Mathematics - Use Functions:**

**Available functions:**

| Function | Returns | Description |
|----------|---------|-------------|
| `math.sqrt(x)` | number | Square root |
| `math.abs(x)` | number or integer | Absolute value |
| `math.max(a, b)` | number | Larger of two values |
| `math.min(a, b)` | number | Smaller of two values |
| `math.floor(x)` | number | Round down to nearest integer |
| `math.ceil(x)` | number | Round up to nearest integer |
| `math.round(x)` | number | Round to nearest integer |
| `math.trunc(x)` | number | Remove decimal part |
| `math.sign(x)` | number | Returns -1, 0, or 1 |
| `math.sin(x)` | number | Sine (radians) |
| `math.cos(x)` | number | Cosine (radians) |
| `math.tan(x)` | number | Tangent (radians) |
| `math.asin(x)` | number | Arc sine |
| `math.acos(x)` | number | Arc cosine |
| `math.atan(x)` | number | Arc tangent |
| `math.atan2(y, x)` | number | Two-argument arc tangent |
| `math.ln(x)` | number | Natural logarithm (base e) |
| `math.log10(x)` | number | Base-10 logarithm |
| `math.log2(x)` | number | Base-2 logarithm |
| `math.exp(x)` | number | e raised to the power of x |
| `math.exp2(x)` | number | 2 raised to the power of x |
| `math.sinh(x)` | number | Hyperbolic sine |
| `math.cosh(x)` | number | Hyperbolic cosine |
| `math.tanh(x)` | number | Hyperbolic tangent |
| `math.pi()` | number | π ≈ 3.14159 |
| `math.e()` | number | Euler's number ≈ 2.71828 |
| `math.tau()` | number | τ = 2π ≈ 6.28318 |

**Examples:**

```clean
start:
	// Basic calculations — use operators for basic math, math.* for advanced operations
	number result = 5.0 + 3.0               // Use + operator, not math.add()
    number maximum = math.max(10.5, 7.2)    // Use math functions for advanced operations

    // Geometry - calculate circle area
    number radius = 5.0
    number area = math.pi() * (radius ^ 2.0)  // Use operators for basic arithmetic

    // Trigonometry - find triangle sides
    number angle = math.pi() / 4.0           // Use / operator, not math.divide()
    number opposite = 10.0 * math.sin(angle) // Use * operator, not math.multiply()
    number adjacent = 10.0 * math.cos(angle)

    // Rounding numbers for display
    number price = 19.99567
    number rounded = math.round(price)  // 20.0
    number floored = math.floor(price)  // 19.0

    // Logarithmic calculations
    number growth = math.exp(0.05)      // e^0.05 for 5% growth
    number halfLife = math.log2(100.0)  // How many times to halve 100 to get 1

    // Distance calculations using Pythagorean theorem
    number dx = 3.0
    number dy = 4.0
    number distance = math.sqrt((dx ^ 2.0) + (dy ^ 2.0))  // Use + operator, not math.add()

    // Absolute values for different types
    number numberAbs = math.abs(-5.7)    // 5.7
    integer intAbs = math.abs(-42)     // 42
```

### String Module

The string module provides powerful text manipulation capabilities. Whether you're processing user input, formatting output, or analyzing text data, string has all the tools you need for effective text handling.

**Method-style calls (act on a string value):**

| Call | Returns | Description |
|------|---------|-------------|
| `text.length()` | integer | Number of characters |
| `text.toUpperCase()` | string | All letters uppercase |
| `text.toLowerCase()` | string | All letters lowercase |
| `text.trim()` | string | Remove leading/trailing whitespace |
| `text.trimStart()` | string | Remove leading whitespace only |
| `text.trimEnd()` | string | Remove trailing whitespace only |
| `text.contains(search)` | boolean | Returns true if search string is found |
| `text.indexOf(search)` | integer | First position of search, or -1 |
| `text.lastIndexOf(search)` | integer | Last position of search, or -1 |
| `text.startsWith(prefix)` | boolean | True if text begins with prefix |
| `text.endsWith(suffix)` | boolean | True if text ends with suffix |
| `text.replace(old, new)` | string | Replace all occurrences of old with new |
| `text.split(delimiter)` | list\<string\> | Split into a list on the delimiter |
| `text.charAt(index)` | string | Character at position (0-based) |
| `text.charCodeAt(index)` | integer | Numeric code of character at position |
| `text.isEmpty()` | boolean | True if length is zero |
| `text.isBlank()` | boolean | True if empty or only whitespace |
| `text.padStart(length, pad)` | string | Pad beginning to reach length |
| `text.padEnd(length, pad)` | string | Pad end to reach length |
| `text.substring(start, end)` | string | Extract substring from start to end |

**Namespace-style calls (utility functions on the `string` module):**

| Call | Returns | Description |
|------|---------|-------------|
| `string.concat(a, b)` | string | Concatenate two strings |
| `string.join(parts, sep)` | string | Join a list of strings with a separator |

**Examples:**

```clean
start:
	// Basic text processing
    string userInput = "  Hello World!  "
    string cleaned = userInput.trim()              // "Hello World!"
    integer length = cleaned.length()              // 12

    // Case normalization for comparisons
    string email1 = "USER@EXAMPLE.COM"
    string email2 = "user@example.com"
    boolean same = email1.toLowerCase() == email2.toLowerCase()  // true

    // Text searching and validation
    string filename = "document.pdf"
    boolean isPdf = filename.endsWith(".pdf")      // true
    integer dotPos = filename.lastIndexOf(".")     // 8

    // URL processing
    string url = "https://api.example.com/users"
    boolean isHttps = url.startsWith("https://")  // true
    boolean hasApi = url.contains("api")          // true

    // Text parsing and reconstruction
    string csvLine = "John,Doe,25,Engineer"
    list<string> fields = csvLine.split(",")       // ["John", "Doe", "25", "Engineer"]
    string fullName = string.join([fields[0], fields[1]], " ")  // "John Doe"

    // Text replacement and cleaning
    string messyText = "Hello    World"
    string cleanedText = messyText.replace("    ", " ")    // "Hello World"

    // Formatting and padding
    string number = "42"
    string padded = number.padStart(5, "0")        // "00042"

    // Character-level operations
    string word = "Hello"
    string firstChar = word.charAt(0)              // "H"
    integer charCode = word.charCodeAt(0)          // 72 (ASCII for 'H')

    // Input validation
    string userField = "   "
    boolean isValid = !userField.isBlank()         // false
```

### List Module

The list module provides powerful data collection capabilities. Whether you're managing lists of items, processing data sets, or organizing information, list has all the tools you need for effective data manipulation.

**Method-style calls (act on a list value):**

| Call | Returns | Description |
|------|---------|-------------|
| `items.length()` | integer | Number of elements |
| `items.get(index)` | element type | Element at index (0-based) |
| `items.set(index, value)` | void | Set element at index |
| `items.add(item)` | void | Add item to the end |
| `items.remove(index)` | element type | Remove and return element at index |
| `items.removeLast()` | element type | Remove and return the last element |
| `items.insert(index, item)` | void | Insert item at position |
| `items.contains(item)` | boolean | True if item is in the list |
| `items.indexOf(item)` | integer | First position of item, or -1 |
| `items.lastIndexOf(item)` | integer | Last position of item, or -1 |
| `items.slice(start, end)` | list | New list with elements from start to end |
| `items.reverse()` | list | New list in reverse order |
| `items.sort()` | list | New list sorted ascending |
| `items.isEmpty()` | boolean | True if length is zero |
| `items.isNotEmpty()` | boolean | True if length is non-zero |
| `items.first()` | element type | First element |
| `items.last()` | element type | Last element |
| `items.map(fn)` | list | New list with fn applied to each element |
| `items.filter(fn)` | list | New list with only elements where fn is true |
| `items.reduce(fn, init)` | any | Combine all elements into one value |
| `items.forEach(fn)` | void | Execute fn for each element |

**Namespace-style calls (utility functions on the `list` module):**

| Call | Returns | Description |
|------|---------|-------------|
| `list.concat(a, b)` | list | Combine two lists into one |
| `list.range(start, end)` | list\<integer\> | List of integers from start to end |
| `list.fill(size, value)` | list | New list of size filled with value |
| `list.join(items, sep)` | string | Join list elements into a string with separator |

**Examples:**

```clean
start:
	// Basic list operations
    list<integer> numbers = [1, 2, 3]
    integer count = numbers.length()              // 3
    integer first = numbers.get(0)                // 1
    numbers.set(1, 99)                            // [1, 99, 3]

    // Building and modifying lists
    list<string> fruits = ["apple", "banana"]
    fruits.add("orange")                          // ["apple", "banana", "orange"]
    string removed = fruits.remove(2)             // "orange", fruits becomes ["apple", "banana"]

    // Searching through data
    list<integer> scores = [85, 92, 78, 96, 88]
    boolean hasHighScore = scores.contains(96)    // true
    integer position = scores.indexOf(92)         // 1

    // Data processing and transformation
    list<integer> data = [1, 2, 3, 4, 5]
    list<integer> doubled = data.map(x => x * 2)          // [2, 4, 6, 8, 10]
    list<integer> evens = data.filter(x => x % 2 == 0)    // [2, 4]
    integer sum = data.reduce((total, x) => total + x, 0) // 15

    // List manipulation
    list<string> names1 = ["Alice", "Bob"]
    list<string> names2 = ["Charlie", "Diana"]
    list<string> allNames = list.concat(names1, names2)    // ["Alice", "Bob", "Charlie", "Diana"]
    list<string> reversed = allNames.reverse()             // ["Diana", "Charlie", "Bob", "Alice"]

    // Working with sections of lists
    list<integer> bigList = [10, 20, 30, 40, 50]
    list<integer> middle = bigList.slice(1, 4)             // [20, 30, 40]

    // Text processing with lists
    list<string> words = ["hello", "world", "from", "Clean"]
    string sentence = list.join(words, " ")                // "hello world from Clean"

    // Creating lists programmatically
    list<string> greetings = list.fill(3, "Hello")         // ["Hello", "Hello", "Hello"]
    list<integer> countdown = list.range(5, 1)             // [5, 4, 3, 2, 1]

    // Validation and utility
    boolean empty = [].isEmpty()                           // true
    string firstWord = words.first()                       // "hello"
    string lastWord = words.last()                         // "Clean"
```

### File Module

The file module makes working with files simple and straightforward. Whether you need to read configuration files, save user data, or process text documents, file has you covered with easy-to-use methods.

| Call | Returns | Description |
|------|---------|-------------|
| `file.read(path)` | string | Read entire file as a string |
| `file.lines(path)` | list\<string\> | Read file and return each line as a list element |
| `file.write(path, content)` | void | Write content to file (creates if missing, replaces if existing) |
| `file.append(path, content)` | void | Append content to end of file (creates if missing) |
| `file.exists(path)` | boolean | True if a file exists at path |
| `file.delete(path)` | void | Delete a file (does nothing if not found) |

**Examples:**

```clean
start:
	// Read a configuration file
    string config = file.read("settings.txt")

    // Process a log file line by line (read content, then split by newline)
    string logContent = file.read("app.log")
    list<string> logLines = logContent.split("\n")

    // Save user data
    file.write("user_data.txt", "John Doe, 25, Engineer")

    // Add to a log file
    file.append("activity.log", "User logged in at 2:30 PM")

    // Check if a file exists before reading
    if file.exists("backup.txt")
        string backup = file.read("backup.txt")

    // Clean up temporary files
    file.delete("temp_data.txt")
```

### Http Module

The http module makes web requests simple and intuitive. Whether you're fetching data from APIs, submitting forms, or building web applications, http provides all the essential HTTP methods you need.

| Call | Returns | Description |
|------|---------|-------------|
| `http.get(url)` | string | Send a GET request; return response body |
| `http.post(url, body)` | string | Send a POST request with body; return response body |
| `http.put(url, body)` | string | Send a PUT request with body; return response body |
| `http.patch(url, body)` | string | Send a PATCH request with body; return response body |
| `http.delete(url)` | string | Send a DELETE request; return response body |

**Examples:**

```clean
start:
	// Fetch user data from an API
    string users = http.get("https://api.example.com/users")

    // Create a new user
    string newUser = "{\"name\": \"Alice\", \"email\": \"alice@example.com\"}"
    string response = http.post("https://api.example.com/users", newUser)

    // Update user information
    string updatedUser = "{\"name\": \"Alice Smith\", \"email\": \"alice.smith@example.com\"}"
    http.put("https://api.example.com/users/123", updatedUser)

    // Partially update user (just the email)
    string emailUpdate = "{\"email\": \"newemail@example.com\"}"
    http.patch("https://api.example.com/users/123", emailUpdate)

    // Remove a user
    http.delete("https://api.example.com/users/123")

    // Fetch weather data
    string weather = http.get("https://api.weather.com/current?city=London")
```

### JSON Module

The json module provides functions for parsing JSON text into Clean Language data structures and serializing data back to JSON text. This is essential for working with web APIs, configuration files, and data exchange.

```clean
// Core operations
json.textToData(text), json.dataToText(data)
json.tryTextToData(text), json.prettyDataToText(data)
```

#### Parsing JSON

| Call | Returns | Description |
|------|---------|-------------|
| `json.textToData(text)` | any | Parse a JSON string into a Clean value. Throws on invalid JSON. |
| `json.tryTextToData(text)` | any | Parse a JSON string; returns `null` on invalid JSON instead of throwing. |

JSON types map to Clean types:
- JSON object → `pairs<string, any>`
- JSON array → `list<any>`
- JSON string → `string`
- JSON number → `number`
- JSON boolean → `boolean`
- JSON null → `null`

Both functions support nested structures with unlimited depth.

#### Accessing JSON Data

The `any` type returned by `json.textToData()` supports both dot notation and bracket notation for accessing nested data. **Dot notation is preferred** for its readability; use bracket notation only when dot notation cannot be used (dynamic keys, computed indices).

```clean
// Preferred: Dot notation for field access
any data = json.textToData(jsonString)
any fieldValue = data.fieldName         // Access field using dot notation

// Bracket notation for dynamic keys or indices
any arrayData = json.textToData(arrayJson)
any element = arrayData[0]              // Array element by integer index
any dynamicKey = data[keyVariable]      // Dynamic key from variable

// Chained access for nested structures
any nested = data.user.profile.name     // Preferred: dot notation chain
any item = data.items[0].id             // Mixed: dot notation with array index
```

**Dot Notation on `any` Type:**

Clean Language automatically converts dot notation on `any` type values to the equivalent bracket access. This enables clean, intuitive code when working with JSON objects:

```clean
// These are equivalent:
any name = data.name                    // Preferred - more readable
any name = data["name"]                 // Bracket notation - verbose

// Nested access:
any city = data.user.address.city       // Preferred
any city = data["user"]["address"]["city"]  // Bracket notation
```

**Access Notation Guidelines:**
- **Dot notation** (`data.field`): Preferred for known field names, returns `any`
- **String keys** (`data["key"]`): Use for dynamic/computed keys, returns `any`
- **Integer indices** (`data[0]`): Use for array element access, returns `any`
- **Mixed access**: Combine dot and bracket notation as needed (`data.items[0].name`)
- **Missing fields**: Returns `null` when field doesn't exist or index is out of bounds

```clean
start:
    string jsonText = '{"name": "Alice", "scores": [85, 92, 78], "profile": {"city": "NYC"}}'
    any data = json.textToData(jsonText)

    // Preferred: Dot notation for object fields
    any name = data.name              // Returns "Alice"
    any city = data.profile.city      // Returns "NYC"
    any missing = data.unknown        // Returns null

    // Bracket notation for array access
    any scores = data.scores
    any first = scores[0]             // Returns 85
    any outOfBounds = scores[100]     // Returns null

    // Mixed notation for complex structures
    any firstScore = data.scores[0]   // Returns 85

    // Use default operator for fallback values
    string userName = data.name default "Guest"
    integer score = data.scores[0] default 0
```

#### Serializing to JSON

| Call | Returns | Description |
|------|---------|-------------|
| `json.dataToText(data)` | string | Compact JSON string from a Clean value |
| `json.prettyDataToText(data)` | string | Formatted, human-readable JSON string |

#### Usage Examples

```clean
start:
    // Parse JSON from an API response
    string apiResponse = http.get("https://api.example.com/user/123")
    any userData = json.textToData(apiResponse)

    // Access parsed data (userData is a pairs<string, any>)
    string name = userData["name"] default "Unknown"
    integer age = userData["age"] default 0

    // Safe parsing with tryTextToData
    string maybeJson = getUserInput()
    any parsed = json.tryTextToData(maybeJson)
    if parsed == null
        print("Invalid JSON provided")
    else
        print("Successfully parsed JSON")

    // Parse nested objects with escape sequences
    string nestedJson = "{\"user\": {\"name\": \"Alice\", \"age\": 25, \"address\": {\"city\": \"NYC\"}}}"
    any data = json.textToData(nestedJson)

    // Access nested fields using dot notation
    string userName = data.user.name        // Returns "Alice"
    integer userAge = data.user.age         // Returns 25
    string city = data.user.address.city    // Returns "NYC"

    // Parse nested arrays
    string matrixJson = "{\"matrix\": [[1, 2, 3], [4, 5, 6], [7, 8, 9]]}"
    any matrixData = json.textToData(matrixJson)
    any firstRow = matrixData.matrix[0]     // Returns [1, 2, 3]
    any element = matrixData.matrix[0][1]   // Returns 2

    // Parse arrays of objects
    string usersJson = "[{\"id\": 1, \"name\": \"Alice\"}, {\"id\": 2, \"name\": \"Bob\"}]"
    any users = json.textToData(usersJson)
    any firstUser = users[0]                // Returns {"id": 1, "name": "Alice"}
    string firstName = users[0].name        // Returns "Alice"
    integer secondId = users[1].id          // Returns 2

    // Complex nested structure
    string complexJson = "{\"users\": [{\"id\": 1, \"tags\": [\"admin\", \"staff\"]}, {\"id\": 2, \"tags\": [\"user\"]}]}"
    any complex = json.textToData(complexJson)
    any adminTags = complex.users[0].tags   // Returns ["admin", "staff"]
    string firstTag = complex.users[0].tags[0]  // Returns "admin"

    // Create data and serialize to JSON
    pairs<string, any> user = {}
    user["name"] = "Bob"
    user["email"] = "bob@example.com"
    user["active"] = true

    string jsonString = json.dataToText(user)
    // Result: {"name":"Bob","email":"bob@example.com","active":true}

    // Pretty-print for debugging or config files
    string prettyJson = json.prettyDataToText(user)
    // Result:
    // {
    //   "name": "Bob",
    //   "email": "bob@example.com",
    //   "active": true
    // }

    // Working with JSON arrays
    list<any> items = [1, 2, 3, "four", true, null]
    string arrayJson = json.dataToText(items)
    // Result: [1,2,3,"four",true,null]

    // Nested structures
    pairs<string, any> config = {}
    config["database"] = {}
    config["database"]["host"] = "localhost"
    config["database"]["port"] = 5432
    config["features"] = ["auth", "logging", "caching"]

    file.write("config.json", json.prettyDataToText(config))
```

#### JSON Type Mapping

| JSON Type | Clean Type | Example |
|-----------|------------|---------|
| object | `pairs<string, any>` | `{"key": "value"}` → `pairs` |
| array | `list<any>` | `[1, 2, 3]` → `list<any>` |
| string | `string` | `"hello"` → `"hello"` |
| number | `number` | `3.14` → `3.14` |
| boolean | `boolean` | `true` → `true` |
| null | `null` | `null` → `null` |

**Note on Nested Structures**: The JSON parser fully supports nested objects and arrays. When parsing nested structures, inner objects and arrays are recursively parsed and stored as `any` values within the parent structure.

```clean
// Nested object example
string json = "{\"user\": {\"name\": \"Alice\", \"age\": 25}}"
any data = json.textToData(json)
// data type: pairs<string, any>
// data.user type: any (contains a pairs<string, any>)
// data.user.name type: any (contains a string)

// Nested array example
string arrayJson = "{\"matrix\": [[1, 2], [3, 4]]}"
any matrix = json.textToData(arrayJson)
// matrix type: pairs<string, any>
// matrix.matrix type: any (contains a list<any>)
// matrix.matrix[0] type: any (contains a list<any>)
// matrix.matrix[0][0] type: any (contains a number)
```

#### Nested Structure Support

The JSON parser uses a **recursive architecture** to support arbitrary nesting depth:

**Capabilities:**
- **Unlimited nesting depth**: Parse structures with any level of nesting
- **Mixed structures**: Combine objects and arrays at any depth
- **Complex data**: Handle real-world JSON from APIs and config files

**Implementation:**
The parser uses three mutually recursive helper functions:
- `__json_parse_value`: Dispatcher for all JSON value types
- `__json_parse_object`: Recursive object parser
- `__json_parse_array`: Recursive array parser

**Examples of supported structures:**

```clean
// Deep object nesting (5 levels)
string deepJson = "{\"a\": {\"b\": {\"c\": {\"d\": {\"e\": 42}}}}}"
any deep = json.textToData(deepJson)
integer value = deep.a.b.c.d.e  // Returns 42

// Deep array nesting
string arrayJson = "[[[[[1]]]]]"
any arrays = json.textToData(arrayJson)
integer val = arrays[0][0][0][0][0]  // Returns 1

// Real-world API response
string apiResponse = "{\"data\": {\"users\": [{\"id\": 1, \"profile\": {\"name\": \"Alice\", \"tags\": [\"admin\", \"user\"]}}]}}"
any response = json.textToData(apiResponse)
string name = response.data.users[0].profile.name  // Returns "Alice"
string tag = response.data.users[0].profile.tags[0]  // Returns "admin"
```

**Performance:** The recursive parser is optimized for WebAssembly execution and can efficiently handle typical JSON structures from web APIs (up to ~50 nesting levels, limited by WebAssembly stack depth).

#### Error Handling

```clean
start:
    // textToData throws on invalid JSON
    string badJson = "{ invalid json }"
    any data = json.textToData(badJson) onError null

    // Or use tryTextToData for null-based error handling
    any safeData = json.tryTextToData(badJson)
    if safeData == null
        print("JSON parsing failed")

    // Use default operator for fallback values
    any result = json.tryTextToData(maybeJson) default {}
```

## Method-Style Syntax

Clean Language follows the "one way to do things" principle. Every operation has exactly one name and one preferred calling style.

### Method Style — The Primary Pattern

When an operation acts on a specific value, use method-style syntax. The value comes first, followed by a dot and the operation name:

```clean
// String operations
string text = "Hello World"
integer length = text.length()
string upper = text.toUpperCase()
string lower = text.toLowerCase()
string trimmed = text.trim()
boolean found = text.contains("World")
list<string> words = text.split(" ")
string cleaned = text.replace("Hello", "Hi")

// List operations
list<integer> numbers = [1, 2, 3]
integer count = numbers.length()
boolean empty = numbers.isEmpty()
numbers.add(4)
numbers.remove(0)
boolean has = numbers.contains(2)
list<integer> sorted = numbers.sort()

// Value conversions
integer age = 25
string ageText = age.toString()
number decimal = age.toNumber()

// Object properties
user.name
user.age
user.toString()
```

### Namespace Functions — For Utilities Only

Namespace functions are used only when an operation does not belong to a single value — typically utility functions with multiple independent inputs:

```clean
// Math utilities (no single owner)
math.sqrt(16)
math.max(10, 20)
math.pow(2, 8)

// Creating new collections
list.concat(listA, listB)
list.range(1, 10)
list.fill(5, 0)

// Joining (utility taking a list + separator)
string.concat("Hello", " World")
list.join(words, ", ")
string.join(parts, "-")
```

### The Rule

- If the operation acts **on a value** → use method style: `value.operation()`
- If the operation **creates something new** from multiple inputs → use namespace: `module.operation(a, b)`
- Every operation has **one name** — no aliases, no shortcuts, no alternate forms

## Modules and Imports

**Clean Language supports multi-file programs through a module system.** Each `.cln` file is a module that can import and use code from other modules.

**Important:** The `import:` statement is **exclusively for Clean Language source files** (`.cln` files). Plugins (frame.data, frame.server, frame.ui, etc.) are declared in `configuration.cln` using the `plugins:` block — they are never imported. See the [Plugin System](#plugin-system) section for details.

### Module Definition

Every `.cln` file is implicitly a module. The module name is derived from the filename (without the `.cln` extension).

```clean
// file: utils.cln
// This file defines the "utils" module

functions:
    integer add(integer a, integer b)
        return a + b

    integer multiply(integer a, integer b)
        return a * b
```

### Importing Modules

Use the `import:` block to import other modules. All public functions and classes from the imported module become available.

```clean
// file: main.cln
import:
    utils

start:
    // Use functions from utils module
    integer sum = add(5, 3)
    integer product = multiply(4, 2)
    print(sum)
    print(product)
```

#### Import Block Syntax

The import block uses indentation to list imported modules:

```clean
import:
    utils           // Import the utils module
    math_helpers    // Import the math_helpers module
    data.models     // Import from nested path (data/models.cln)
```

#### Import Variations

```clean
import:
    Math                // whole module
    math.sqrt           // single symbol (import specific function)
    Utils as U          // module alias
    Json.decode as jd   // symbol alias
```

#### File Path Imports

In addition to module-name imports, Clean Language supports **direct file path imports** using string literals. This is useful for importing files from specific locations without relying on module resolution.

```clean
// Import a file using its relative path
import "app/data/models.cln"
import "../lib/utils.cln"
import "./helpers.cln"
```

**Key differences from module imports:**

| Feature | Module Import | File Path Import |
|---------|---------------|------------------|
| Syntax | `import: module_name` | `import "path/to/file.cln"` |
| Resolution | Search paths (`./`, `./lib/`, etc.) | Relative to importing file |
| Nested paths | `data.models` → `data/models.cln` | `"data/models.cln"` (explicit) |

**Path Resolution:**

File path imports are resolved **relative to the directory of the importing file**, not the project root:

```
project/
├── main.cln              # import "app/data/models.cln"
├── app/
│   ├── data/
│   │   └── models.cln    # import "../../lib/utils.cln"
│   └── services/
│       └── api.cln
└── lib/
    └── utils.cln
```

```clean
// file: main.cln
import "app/data/models.cln"  // Resolves to ./app/data/models.cln

start:
    integer result = double(21)
    print(result)
```

```clean
// file: app/data/models.cln
import "../../lib/utils.cln"  // Resolves to ./lib/utils.cln (relative to app/data/)

functions:
    integer double(integer x)
        return x * 2

    integer squareDouble(integer x)
        integer doubled = double(x)
        return square(doubled)  // From utils.cln
```

**Chained Imports:**

File path imports can be chained - imported files can import other files:

```clean
// main.cln imports models.cln which imports utils.cln
// All functions from all three files are available in the final WASM
```

**When to use each import style:**

- **Module imports** (`import: utils`): For project modules in standard locations
- **File path imports** (`import "path/file.cln"`): For explicit paths, external files, or non-standard project structures

### Multi-File Compilation

The compiler automatically discovers and compiles all imported modules (both module-name and file path imports). Both `build` and `compile` commands support multi-file compilation:

```bash
# Build a multi-file project (resolves all imports)
cln build main.cln

# Compile also supports multi-file imports
cln compile main.cln -o app.wasm

# Build with custom output path
cln build main.cln -o app.wasm

# Build with library search paths (for module-name imports)
cln build main.cln -L ./lib -L ./modules

# Build with optimization level
cln build main.cln -O3
```

**Note:** File path imports (`import "path/file.cln"`) are resolved relative to the importing file and do not use `-L` search paths. Module-name imports (`import: module`) use the search paths.

#### Module Resolution

When resolving an import, the compiler searches in the following order:

1. **Current directory** - Same directory as the importing file
2. **./lib/** - Library directory
3. **./modules/** - Modules directory
4. **./src/** - Source directory
5. **Custom paths** - Paths specified with `-L` flag

For each search path, the compiler tries these file patterns:
- `{module}.cln` (e.g., `utils.cln`)
- `{module}/mod.cln` (e.g., `utils/mod.cln`)
- `{module}/index.cln` (e.g., `utils/index.cln`)

#### Dependency Graph

The compiler builds a dependency graph of all modules and compiles them in topological order (dependencies before dependents). This ensures that when a module is compiled, all its dependencies are already available.

```
main.cln → math_helpers.cln → utils.cln
         ↘ data.cln
```

In this example, `utils.cln` is compiled first, then `math_helpers.cln` and `data.cln`, and finally `main.cln`.

#### Circular Dependencies

Circular dependencies are detected and reported as errors:

```clean
// file: a.cln
import:
    b  // a imports b

// file: b.cln
import:
    a  // b imports a - CIRCULAR DEPENDENCY ERROR!
```

### Built-in Modules

The following modules are built into the language and don't need to be imported from files:

| Module | Description |
|--------|-------------|
| `math` | Mathematical functions (sin, cos, sqrt, etc.) |
| `string` | String manipulation functions |
| `list` | List operations |
| `file` | File I/O operations |
| `http` | HTTP client functions |
| `json` | JSON parsing (`textToData`) and serialization (`dataToText`) |
| `console` | Console I/O |

Built-in modules are automatically available when imported:

```clean
import:
    math
    string
    list

start:
    number pi = math.pi
    string upper = "hello".toUpperCase()
    list<integer> nums = list.range(1, 10)
```

### Visibility Model

**Public by default** - functions and classes are exported unless marked private:

```clean
// All public by default
functions:
    calculateTotal()
        // implementation

    formatCurrency()
        // implementation

// Mark functions as private
private:
    internalHelper
    secretKey
```

Private functions cannot be accessed from other modules:

```clean
// file: mymodule.cln
functions:
    integer publicFunc()
        return helperFunc() * 2

    integer helperFunc()
        return 42

private:
    helperFunc  // Not accessible from outside

// file: main.cln
import:
    mymodule

start:
    integer x = publicFunc()   // OK
    integer y = helperFunc()   // ERROR: helperFunc is private
```

### Example: Multi-File Project

Here's a complete example of a multi-file Clean Language project:

```clean
// file: utils.cln
functions:
    integer add(integer a, integer b)
        return a + b

    integer multiply(integer a, integer b)
        return a * b

    integer double_value(integer n)
        return n * 2
```

```clean
// file: math_helpers.cln
import:
    utils

functions:
    integer square(integer n)
        return multiply(n, n)

    integer quadruple(integer n)
        return double_value(double_value(n))
```

```clean
// file: main.cln
import:
    utils
    math_helpers

start:
    // Use functions from utils
    integer sum = add(10, 5)
    print(sum)  // Output: 15

    // Use functions from math_helpers
    integer sq = square(4)
    print(sq)  // Output: 16

    // Combined usage
    integer result = multiply(sq, 2)
    print(result)  // Output: 32
```

Compile with:
```bash
cln build main.cln -o app.wasm
```

## Asynchronous Programming

Clean uses `start`, `later`, and `background` for simple background execution:
- `start` begins a task in the background
- `later` declares that the result will be available in the future
- The value blocks only when accessed
- Use `background` to run a task without keeping the result
- Mark a function as `background` to always run it in the background

### Two Uses of the `start` Keyword

The `start` keyword has two distinct meanings in Clean Language:

| Context | Syntax | Purpose |
|---------|--------|---------|
| Entry point | `start:` | Block that marks where your program begins (top-level only) |
| Background expression | `start functionCall()` | Starts a function running in the background |

These are completely separate features that happen to share a keyword. The compiler tells them apart by context.

### Start Expression

Use `start` before a function call to run it in the background:

```clean
start:
    // Using 'start' for background — different from the entry block!
    later data = start fetchData("url")
    print("Working...")
    print(data)          // blocks here only

    background logAction("login")    // runs and ignores result
```

### Background Functions

**Mark a function as `background` so every call to it runs in the background automatically.**

```clean
functions:
	void syncCache() background
		sendUpdateToServer()
		clearLocalTemp()

start:
	syncCache()    // runs in background automatically
```

### Background Task Error Handling

**Errors in background tasks do not propagate to the calling context.** If a background task throws, it fails silently unless an `onError` handler is attached to the expression that started it.

The `loading` pattern — setting a flag before an async operation and clearing it after — should always be wrapped in `onError` to ensure cleanup happens even on failure:

```clean
functions:
	void fetchUser(integer id) background
		loading = true
		any user = start api.getUser(id) onError:
			loading = false
			error("Failed to fetch user")
		username = user.name
		loading = false
```

Without the `onError` block, if `api.getUser(id)` fails, `loading` would stay `true` and the UI would be stuck in a loading state.

## Memory Management

Clean uses Automatic Reference Counting (ARC) for memory management.

## AI Integration

Clean Language provides built-in features to support AI-assisted development by linking code to formal specifications and documenting function intent.

### Overview

The AI Integration features enable:
- **Traceability**: Link functions to their specification documents
- **Intent Documentation**: Describe function purpose in natural language
- **Source Attribution**: Mark generated files with their specification source

These features are language-level constructs that enhance code clarity and enable tools to understand the relationship between specifications and implementations.

### The `spec` Keyword

The `spec` keyword links a function or method to its specification document.

**Syntax:**
```clean
spec "path/to/specification.spec.cln"
```

**Rules:**
- Can only appear inside function or method bodies
- Must appear before other statements (except `intent`)
- Path is relative to the project root
- Multiple `spec` declarations are allowed (for referencing multiple specs)

**Example:**
```clean
functions:
    number calculateDiscount(number price, number percentage)
        spec "specs/pricing/discount.spec.cln"
        require percentage >= 0 and percentage <= 100
        return price * (percentage / 100)
```

### The `intent` Keyword

The `intent` keyword describes a function's purpose in natural language.

**Syntax:**
```clean
intent "Natural language description of function purpose"
```

**Rules:**
- Can only appear inside function or method bodies
- Must appear before other statements (except `spec`)
- Provides human-readable documentation of what the function does
- Multiple `intent` declarations are allowed

**Example:**
```clean
functions:
    void processPayment(number amount, string method)
        intent "Process a payment transaction using the specified payment method"
        spec "specs/payment/process.spec.cln"
        require amount > 0
        require method in ["credit", "debit", "paypal"]
        // ... implementation
```

### The `source:` Block

The `source:` block marks a file as generated from a specification document.

**Syntax:**
```clean
source:
    spec: "path/to/specification.spec.cln"
    version: "commit-hash-or-version"
```

**Rules:**
- Must appear at the top of the file (before any other declarations)
- Contains two required fields:
  - `spec`: Path to the source specification file
  - `version`: Version identifier (git hash, version number, etc.)
- Indicates the file was generated from a formal specification

**Example:**
```clean
source:
    spec: "specs/payment.spec.cln"
    version: "a3f2c1d"

functions:
    boolean validateCard(string cardNumber)
        intent "Validate credit card number using Luhn algorithm"
        spec "specs/payment.spec.cln"
        require cardNumber.length() >= 13 and cardNumber.length() <= 19
        // ... implementation
```

### Combined Usage Example

Here's a complete example showing all AI integration features together:

```clean
source:
    spec: "specs/authentication.spec.cln"
    version: "2.1.0"

functions:
    boolean authenticateUser(string username, string password)
        intent "Authenticate a user with username and password credentials"
        spec "specs/authentication.spec.cln"
        require username.length() > 0
        require password.length() >= 8

        hash = hashPassword(password)
        stored = database.query("SELECT password FROM users WHERE username = ?", username)

        if stored.length() == 0
            return false

        return compareHashes(hash, stored[0].password)

    string hashPassword(string password)
        intent "Generate secure hash of password using bcrypt"
        spec "specs/authentication.spec.cln"
        require password.length() >= 8

        // _crypto_hash_password is provided by the frame.auth plugin
        return _crypto_hash_password(password)

    boolean compareHashes(string hash1, string hash2)
        intent "Securely compare two password hashes in constant time"
        spec "specs/authentication.spec.cln"
        require hash1.length() > 0
        require hash2.length() > 0

        // _crypto_verify_password is provided by the frame.auth plugin
        return _crypto_verify_password(hash1, hash2)

tests:
    "authenticate valid user": authenticateUser("john", "SecurePass123") = true
    "reject invalid password": authenticateUser("john", "WrongPass") = false
```

### Use Cases

**1. Specification-Driven Development:**
```clean
functions:
    number calculateTax(number income, string state)
        spec "specs/tax/calculation.spec.cln"
        intent "Calculate state income tax based on tax brackets"
        require income >= 0
        require state in ["CA", "NY", "TX", "FL"]
        // Implementation follows specification
```

**2. Documentation and Traceability:**
```clean
functions:
    void sendEmail(string to, string subject, string body)
        intent "Send email via SMTP with retry logic and error handling"
        spec "specs/email/smtp.spec.cln"
        require to.contains("@")
        require subject.length() > 0
        // Implementation traceable to spec
```

**3. Generated Code Attribution:**
```clean
source:
    spec: "specs/api/rest_endpoints.spec.cln"
    version: "v1.2.3"

// Generated API endpoint handlers
functions:
    string handleGetUser(string userId)
        intent "Handle GET /api/users/:id endpoint"
        spec "specs/api/rest_endpoints.spec.cln"
        // ... implementation
```

### Best Practices

1. **Use relative paths**: Spec paths should be relative to project root
   ```clean
   spec "specs/module/feature.spec.cln"  // ✅ Good
   spec "/absolute/path/spec.cln"         // ❌ Avoid
   ```

2. **Place metadata early**: `spec` and `intent` should come before `require` and implementation
   ```clean
   functions:
       void myFunc()
           intent "..."    // ✅ Good: before require
           spec "..."      // ✅ Good: before require
           require x > 0
           // implementation
   ```

3. **Keep intent concise**: One clear sentence describing the function's purpose
   ```clean
   intent "Calculate compound interest over a given period"  // ✅ Good
   intent "This function does stuff with numbers"            // ❌ Too vague
   ```

4. **Version tracking**: Use meaningful version identifiers in `source:` blocks
   ```clean
   source:
       spec: "specs/payment.spec.cln"
       version: "a3f2c1d"    // ✅ Git hash
   ```

### Notes

- These features are metadata and don't affect runtime behavior
- The compiler can use this information for validation and tooling
- Specification files (`.spec.cln`) follow the same Clean Language syntax
- Tools can verify that implementations match their specifications

### AI Agent Setup (MCP Server)

The Clean Language compiler includes a built-in MCP (Model Context Protocol) server that allows AI agents to write, compile, and debug Clean Language programs. The MCP server exposes 12 tools over a stdio-based JSON-RPC 2.0 transport.

#### Quick Setup

Generate the configuration for your AI platform:

```bash
# For Claude Desktop
cln mcp-config --format claude-desktop

# For VS Code / Cursor
cln mcp-config --format vscode

# For Claude Code
cln mcp-config --format claude-code

# Generic (all platforms)
cln mcp-config
```

Each command outputs the JSON configuration to add to your platform's settings file. For example, Claude Desktop requires adding to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "clean-language": {
      "command": "cln",
      "args": ["mcp-server"]
    }
  }
}
```

#### Starting the MCP Server Manually

```bash
cln mcp-server
```

The server reads JSON-RPC 2.0 requests from stdin and writes responses to stdout. Debug logging goes to stderr.

#### Available MCP Tools

| Tool | Description |
|------|-------------|
| `get_quick_reference` | **Call first.** Returns a concise cheat sheet with syntax, types, control flow, and examples. |
| `check` | Fast type-check (no WASM generation). Use during iterative development. |
| `compile` | Full compilation to WebAssembly. Returns WASM as base64. |
| `parse` | Returns the AST as JSON. Useful for code analysis. |
| `diagnostics` | Detailed error/warning diagnostics for source code. |
| `explain_error` | Explains any error code (e.g., SYN001, SEM002) with examples and fixes. |
| `list_functions` | Lists all functions in a source file with signatures. |
| `list_types` | Lists all class/type definitions in a source file. |
| `list_builtins` | Complete catalog of built-in functions, classes, and namespaces. |
| `get_specification` | Query the language specification by section or keyword. Auto-updates when spec changes. |
| `list_error_codes` | All compiler error codes with descriptions across 6 categories. |
| `list_plugins` | Available plugins with AI context metadata. |

#### Recommended AI Agent Workflow

1. **Learn the language**: Call `get_quick_reference` to receive a complete syntax cheat sheet
2. **Write code**: Create `.cln` files following the patterns from the quick reference
3. **Iterate fast**: Call `check` for rapid type-checking feedback (no WASM generation overhead)
4. **Compile**: Call `compile` when the code is ready for WebAssembly output
5. **Debug errors**: Call `explain_error` with the error code for detailed fix suggestions
6. **Deep reference**: Call `get_specification` with a section name or search query for detailed documentation

#### MCP Protocol Example

Initialize the connection:
```json
{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}
```

Call a tool:
```json
{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
  "name": "check",
  "arguments": {
    "source": "start:\n\tprint(\"Hello\")",
    "file_path": "hello.cln"
  }
}}
```

#### Plugin AI Context

Plugins can include an `[ai]` section in their `plugin.toml` manifest to provide context for AI agents:

```toml
[ai]
description = "HTTP endpoint DSL for Clean Language"
examples = ["examples/basic_api.cln", "examples/crud.cln"]
constraints = ["All endpoints must have authentication", "Use REST conventions"]
```

AI agents can discover this context via the `list_plugins` MCP tool, enabling them to understand what plugins are available and how to use them correctly.

## Plugin System

The Clean Language Plugin System allows you to extend the language with custom Domain-Specific Language (DSL) blocks. Plugins transform DSL syntax into standard Clean Language code before compilation.

### Important: Plugins vs Imports

**`import:` is only for `.cln` source files. Plugins are never imported — they are declared in the project configuration.**

| Mechanism | What it is for | Where it goes |
|-----------|---------------|---------------|
| `import:` | Other `.cln` source files from your project or external Clean Language modules | At the top of each `.cln` file that needs them |
| `plugins:` | Framework plugins (frame.data, frame.server, frame.ui, etc.) | In `configuration.cln` at the project root |

**Plugins work differently from modules:**
- Plugins are declared once at the **project level** in `configuration.cln` — they are never declared in individual `.cln` files.
- Plugin-generated functions and types are **automatically available** across the project when the plugin is declared — no import statement is needed in individual files.
- Plugins cannot export `.cln` modules that are then imported via `import:`.

```clean
// configuration.cln — declare plugins here, once, for the whole project
plugins:
	frame.server
	frame.ui
	frame.data
```

```clean
// app.cln — use plugin blocks and functions directly, no import needed
endpoints:
	GET "/users" -> listUsers
```

```clean
// ❌ WRONG — never import a plugin
import:
	frame.server

// ✅ CORRECT — import another .cln file you wrote
import:
	utils
	models
```

### Overview

Plugins operate during the compilation pipeline, transforming custom blocks (like `endpoints:`, `data:`, `component:`) into standard Clean Language AST. This enables powerful abstractions without modifying the core language.

```
Source → Lexer → Parser → [PLUGIN EXPANSION] → HIR → TypeChecker → MIR → WASM
                              ↑
                      Plugins transform here
```

### Project Configuration

Every Clean Language project that uses plugins must have a `configuration.cln` file in the project root:

```clean
// configuration.cln
project:
    name: "my-web-app"
    version: "1.0.0"

plugins:
    frame.web       // Enables endpoints: blocks
    frame.ui        // Enables component: blocks
    frame.data      // Enables schema: blocks
```

The compiler reads this configuration and loads the specified plugins before compilation.

### Plugin Auto-Detection (v0.30.15+)

The compiler can automatically detect and load plugins based on the file's location in the project structure. This eliminates the need for explicit `plugins:` declarations in most cases.

**Auto-Detection Rules:**

| File Path Pattern | Auto-Loaded Plugins |
|-------------------|---------------------|
| `/api/`, `/backend/api/`, `/server/api/`, `/endpoints/` | `frame.httpserver`, `frame.data`, `frame.auth` |
| `/data/`, `/models/`, `/server/models/` | `frame.data` |
| `/auth/`, `/config/auth/` | `frame.auth` |
| `/canvas/` | `frame.canvas` |
| `/ui/`, `/components/`, `/screens/` | `frame.ui` |

**Example - No explicit plugin declaration needed:**

```clean
// File: app/api/users.cln
// Plugins auto-detected: frame.httpserver, frame.data, frame.auth

functions:
    string getUsers()
        string result = _db_query("SELECT * FROM users", "[]")
        return result

start:
    integer s = _http_route("GET", "/users", 0)
```

**Precedence:**
1. Explicit `plugins:` block declarations are always respected
2. Auto-detected plugins are merged with explicit declarations
3. No duplicates are loaded

This feature is especially useful for Frame Framework projects where folder conventions are standard.

### Framework Blocks

Framework blocks are custom DSL blocks defined by plugins. They follow the apply-block syntax:

```clean
// Custom DSL block
blockname:
    DSL content here
    Each plugin defines its own syntax
```

### Example: HTTP Endpoints Plugin

With `frame.web` declared in `configuration.cln`, the `endpoints:` block becomes available:

```clean
// configuration.cln
plugins:
    frame.web

// app.cln
endpoints:
    GET "/users" -> listUsers
    GET "/users/{id}" -> getUser
    POST "/users" -> createUser
    PUT "/users/{id}" -> updateUser
    DELETE "/users/{id}" -> deleteUser

functions:
    list<User> listUsers()
        return database.query("SELECT * FROM users")

    User getUser(string id)
        return database.findById("users", id)

    User createUser()
        // Handle POST body
        return database.insert("users", request.body)
```

This expands to route registration code that integrates with your web framework.

### Block Attributes

Plugins can support attributes that modify behavior:

```clean
@version("v2")
@auth
@cache(ttl: 300)
endpoints:
    GET "/api/users" -> listUsers
```

Attributes are passed to the plugin and can affect code generation.

### Plugin Categories

| Category | Example Blocks | Purpose |
|----------|---------------|---------|
| **Web** | `endpoints:`, `routes:` | HTTP API definition |
| **Data** | `schema:`, `model:` | Database models |
| **UI** | `component:`, `view:` | UI components |
| **Config** | `config:`, `settings:` | Configuration DSLs |

### Handler Parameter Type (v0.30.39+)

Bridge functions in plugin.toml can declare a `"handler"` parameter type for callback dispatch. This allows developers to pass function names instead of numeric indices:

```toml
# plugin.toml
[[bridge.functions]]
name = "_ui_onEvent"
params = ["string", "string", "handler"]
returns = "integer"
expand_strings = true
```

In Clean Language source code, the developer passes a function name:

```clean
start:
    integer s = ui.onEvent(".btn", "click", loadUsers)

functions:
    loadUsers()
        print("Loading users...")
```

The compiler:
1. Verifies `loadUsers` exists in the `functions:` block
2. Assigns a unique handler index (auto-incrementing, starting from 0)
3. Passes the index as i32 to the bridge function
4. Exports the function as `handle_event_N` in the WASM module

The same function referenced in multiple handler parameters gets the same index. At the WASM level, `handler` compiles to `i32` — the runtime dispatches callbacks via `handle_event_{index}` exports.

Supported bridge parameter types: `"string"`, `"integer"`, `"number"`, `"boolean"`, `"void"`, `"handler"`.

### IDE Support

Plugins provide IDE integration through the Language Server:

- **Autocomplete**: Plugin keywords appear in completion lists
- **Hover Documentation**: Hover over keywords for documentation
- **Diagnostics**: Real-time error checking for DSL syntax
- **Syntax Highlighting**: Plugin keywords are colorized

### Creating Custom Plugins

For detailed information on creating plugins, see the [Plugin Architecture Documentation](./Plugin-Architecture.md).

Basic plugin structure:

```rust
impl FrameworkPlugin for MyPlugin {
    fn name(&self) -> &'static str {
        "my.plugin"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["myblock"]
    }

    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        // Transform DSL block into Clean Language AST
    }

    // Optional: IDE support
    fn get_keywords(&self) -> &'static [&'static str] {
        &["KEYWORD1", "KEYWORD2"]
    }

    fn get_completions(&self, ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
        // Return autocomplete suggestions
    }
}
```

### Key Benefits

- **Non-invasive**: Core language stays minimal
- **Type-safe**: Generated code is fully type-checked
- **IDE Support**: Full autocomplete and diagnostics
- **Composable**: Multiple plugins work together

## State Management

State is a first-class concept in Clean Language. It provides persistent memory that outlives function calls, with built-in observability and sequential update guarantees.

### Core Principles

1. **Persistent Memory**: State stores values beyond function execution. Variables are temporary; state is remembered.
2. **Mutable**: State values are updated using normal assignment.
3. **Observable**: The runtime detects all state changes and can react to them.
4. **Explicit Scope**: State is declared in known scopes (app or screen level).
5. **Sequential Updates**: State mutations are processed in order, preventing race conditions.
6. **Background Compatible**: Background operations update state on completion, following sequential rules.
7. **In-Memory Default**: State lives in memory. Persistence is optional via plugins.
8. **First-Class**: State is recognized by the compiler and enforced by the runtime.

### State Declaration

State scope is determined by where it's declared. Use the `state:` block to define state variables.

**App-level state** — declared at module level (persists for application lifetime):

```clean
state:
    integer count = 0
    string username = ""
    boolean isLoggedIn = false
```

**Screen-level state** — declared inside a screen (persists for screen lifetime):

```clean
screen Home:
    state:
        string searchQuery = ""
        list<string> results = []
```

**Rules:**
- State can be declared at **two levels**: top-level (`state:` at module level) for app-wide state, and inside a `screen:` block for screen-local state.
- Top-level `state:` creates app-scoped state — accessible from any function or screen in the module.
- `state:` inside `screen:` creates screen-scoped state — accessible only within that screen block. Screen-local state is destroyed when the screen is removed.
- Initial values are required for all state variables.
- App-level names and screen-level names are **separate namespaces** — a screen can declare a variable with the same name as an app-level variable without conflict. Within each scope, names must be unique.

### State Rules

Use `rules:` inside a `state:` block to declare persistent invariants that must always be true. Rules are checked at **operation boundaries** — predictable points where your code completes a logical unit of work.

**When rules are checked:**
- After `start:` finishes executing
- After each `frame:` finishes (if present)
- After a handler finishes (e.g., HTTP endpoint, job handler, command handler)

This approach is simple and predictable: rules are verified when your code hands control back to the runtime.

**Syntax:**
```clean
state:
    integer balance
    integer limit

    rules:
        balance >= 0
        limit > 0
```

**Example:**
```clean
state:
    integer count = 0
    integer maxCount = 100

    rules:
        count >= 0
        count <= maxCount

start:
    count = 50
    count = count + 10
    // rules checked here, after start: completes
```

**Rules for `rules:`:**
- Can only appear inside a `state:` block
- Must appear after all state variable declarations
- Each rule must be a boolean expression
- If a rule becomes false, execution traps with a contract failure

**Error on violation:**
```
Rule violation: rule failed in state block
  Rule: balance >= 0
  Actual: balance = -50
```

**Why operation boundaries?** Checking rules after every assignment would be slow and could catch intermediate states that are valid in context. By checking at operation boundaries, you get meaningful guarantees without performance overhead.

### State Access

**Access state directly by name.** Within a scope (app-level or screen-level), state variable names are unique, so no prefix is ever needed.

```clean
state:
    integer count = 0
    string username = ""

functions:
    void showStatus()
        integer current = count
        string name = username
        print("User: " + name + ", Count: " + current.toString())
```

### State Mutation

Mutate state with standard assignment.

```clean
state:
    integer count = 0

functions:
    void increment()
        count = count + 1

    void reset()
        count = 0
```

### Observing State Changes

Use `watch:` to react when state changes. The block runs automatically after the state is updated.

```clean
state:
    integer count = 0

watch count:
    print("Count changed to: " + count.toString())

functions:
    void increment()
        count = count + 1    // Triggers the watch block
```

**Watching multiple state variables:**

```clean
state:
    string firstName = ""
    string lastName = ""

watch (firstName, lastName):
    print("Name changed")
```

### Computed State

**Computed state is a read-only derived value that is automatically re-evaluated when its dependencies change.** Declare computed values inside a `computed:` block within `state:`.

```clean
state:
	string firstName = ""
	string lastName = ""

	computed:
		string fullName
			return firstName + " " + lastName

functions:
	void setName(string first, string last)
		firstName = first
		lastName = last

start:
	setName("Alice", "Smith")
	print(fullName)    // Prints: Alice Smith
```

**Rules (STATE003):**
- Computed state is **read-only** — it cannot be assigned to directly.
- **Dependency tracking is performed by static analysis at compile time.** The compiler inspects which state variables appear in the computed block's body and registers them as dependencies.
- External function calls inside a computed body are treated as opaque — their internal dependencies cannot be tracked. If a computed value depends on an external function, it is conservatively re-evaluated on every state change.
- **Circular dependencies between computed state variables are a compile error** (SEM001 / STATE003). For example, if `fullName` references `displayName` and `displayName` references `fullName`, compilation fails.
- The return type of the computed body must match the declared type of the computed variable (STATE003).

### State Guards

**Guards validate a proposed new value before it is written to a state variable.** If the guard condition is false, the assignment is rejected and the state variable retains its current value.

```clean
state:
	integer count = 0
		guard value >= 0 else "Count cannot be negative"

	string email = ""
		guard isValidEmail(value) else "Invalid email format"

functions:
	void decrement()
		count = count - 1    // Throws error if result < 0 (STATE001)
```

**Rules (STATE001):**
- Guards are evaluated **before** the state variable is updated.
- If the guard condition is `false`, the update is rejected — the state variable stays unchanged.
- If the guard condition is `true`, the new value is written.
- `value` inside the guard expression refers to the **proposed new value**, not the current value.
- Guard conditions must be **pure boolean expressions** — side effects in guard conditions are not allowed.
- The `else` clause is a **string literal** containing the error message shown on rejection. It is not a statement.

### State Reset

Reset state to its initial value using the `reset` keyword.

```clean
state:
    integer count = 0
    string username = ""

functions:
    void clearCount()
        reset count          // count returns to 0

    void clearAll()
        reset state          // all state returns to initial values
```

### State with Background Tasks

**Background functions can update state when they complete. Updates remain sequential, preventing race conditions.**

Errors in background tasks do not propagate to the caller — always attach an `onError` handler when updating state in a background function, so cleanup happens even on failure:

```clean
state:
	string username = ""
	boolean isLoggedIn = false
	boolean loading = false

functions:
	void fetchUser(integer id) background
		loading = true
		any user = start api.getUser(id) onError:
			loading = false
			error("Failed to fetch user")
		username = user.name
		isLoggedIn = true
		loading = false
```

### State in Screens

Screens have their own state scope. Screen state is destroyed when the screen is removed.

```clean
state:
    string currentUser = ""    // App-level state

screen Home:
    state:
        integer homeCount = 0      // Screen-level state
        boolean homeLoading = false

    watch homeCount:
        print("Home count: " + homeCount.toString())

    functions:
        void increment()
            homeCount = homeCount + 1

        void showUser()
            // Access app-level state directly (unique names)
            print("Logged in as: " + currentUser)

screen Settings:
    state:
        boolean settingsDarkMode = false    // Different screen, different state

    functions:
        void toggleDarkMode()
            settingsDarkMode = not settingsDarkMode
```

### Complete Example

```clean
// App-level state
state:
    string user = ""
    string theme = "light"

    computed:
        string greeting
            return "Hello, " + user

// React to user changes
watch user:
    print(greeting)

// Core functions
functions:
    void setUser(string name)
        user = name

    void setTheme(string newTheme)
        theme = newTheme

    void loadProfile() background
        any profile = start fetchProfile()
        user = profile.name

// Home screen with its own state
screen Home:
    state:
        integer visitCount = 0
            guard value >= 0 else "Cannot be negative"
        list<string> recentItems = []

    watch visitCount:
        print("Visits: " + visitCount.toString())

    functions:
        void recordVisit()
            visitCount = visitCount + 1

        void addItem(string item)
            recentItems.add(item)

        void clearHistory()
            reset recentItems

// Entry point
start:
    setUser("Alice")       // Triggers watch, prints "Hello, Alice"
```

### Summary

| Syntax | Purpose |
|--------|---------|
| `state:` (top-level) | App-scoped state |
| `state:` (in screen) | Screen-scoped state |
| `fieldName` | Access state by name |
| `fieldName = value` | Mutate state |
| `watch fieldName:` | React to state changes |
| `watch (a, b):` | React to multiple state changes |
| `computed:` | Define derived state |
| `guard condition else msg` | Validate before mutation |
| `reset fieldName` | Reset to initial value |
| `reset state` | Reset all state in scope |

### State vs Variables

| Aspect | Variables | State |
|--------|-----------|-------|
| Lifetime | Function scope | App or screen lifetime |
| Observability | Not observable | Observable via `watch:` |
| Persistence | Lost after function returns | Persists until reset or scope ends |
| Validation | None | Optional guards |
| Declaration | `integer x = 0` | Inside `state:` block |
