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
18. [Plugin System](#plugin-system)

## Overview

Clean Language is a modern, type-safe programming language designed to compile to WebAssembly (WASM). It combines the readability of Python with the safety of Rust while being approachable for beginners. The language emphasizes strong static typing, first-class functions, matrix operations, and comprehensive error handling.

### Design Goals
- **Type Safety**: Strong static typing with type inference
- **Simplicity**: Clean, readable syntax with "one way to do things"
- **Performance**: Efficient compilation to WebAssembly
- **Expressiveness**: First-class support for mathematical operations and data structures
- **Error Handling**: Comprehensive error handling and recovery mechanisms
- **Developer Experience**: Method-style syntax and intuitive patterns

### 🎯 Quality Assurance Standards

**PRODUCTION QUALITY REQUIREMENT: 100% COMPILATION SUCCESS RATE**

The Clean Language compiler MUST achieve and maintain:
- **100% success rate** on ALL test files in `tests/clean_files/`
- **Zero tolerance** for compilation failures in production
- **Comprehensive feature support** across all language constructs
- **Full WebAssembly compatibility** for all generated code

**Quality Gates:**
1. ALL core language features (00-20 series) MUST compile successfully
2. ALL advanced features (21-30+ series) MUST compile successfully
3. ALL error handling, I/O, networking, and memory management features MUST work
4. NO placeholder implementations or partial feature support allowed

### File Extension
Clean Language source files use the `.cln` extension.

## Compiler Instructions (Core Implementation Rules)

### 🛠 Clean Language Compiler Instructions (Core Fixes)

These are essential implementation rules that must be followed by the Clean Language compiler:

1. **Functions must be in a `functions:` block (except start())**
   - ❌ No standalone `function name(...)` allowed at top level
   - ✅ Use `functions:` for top-level and class functions
   - ✅ Exception: `start()` can be standalone
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

3. **Use `any` for generic types**
   - ✅ `any identity(any value) -> any`
   - Treat any capitalized type name not declared as a concrete type as a generic
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
   - ✅ Use `math.sqrt()`, `string.concat()`, `list.sort()` — not `Math.sqrt()`, `String.concat()`
   - ❌ No capitalized namespace names
   - Policy clarification: Uppercase namespace “synonyms” (e.g., `Math`, `String`, `List`, `File`, `Http`, `Console`) are not supported and must be treated as errors by the compiler. Previous drafts or tooling that accepted uppercase variants are deprecated. The grammar and semantic layers must enforce lowercase-only namespaces and emit a clear diagnostic when uppercase is used (e.g., “Use lowercase namespace ‘math.sqrt()’ instead of ‘Math.sqrt()’”).

6. **Use natural generic container syntax**
   - ✅ `list<item>`, `matrix<type>`
   - ❌ No angle brackets in user code (`<>`) - these are internal representations

7. **Clean uses `any` as the single generic placeholder type**
   - It represents a value of any type, determined when the function or class is used
   - No explicit type parameter declarations needed - `any` is automatically generic

8. **One way to do things**
   - Basic math: Use operators (`a + b`, `a * b`)
   - Advanced math: Use functions (`math.sqrt()`, `math.sin()`)
   - Object operations: Use method-style (`text.length()`, `value.toString()`)
   - Utility functions: Use namespace calls (`string.concat()`, `list.sort()`)

### Implementation Notes
- These rules ensure consistency with Clean's philosophy of simplicity and readability
- The compiler should enforce these patterns and provide helpful error messages when violated
- Generic type resolution happens at compile time based on usage context

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
start()
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
and        class       constructor  default     else        error
false      for         from         function    if          import
in         iterate     not          null        onError     or
print      println     return       start       step        test
tests      this        to           true        is          returns
description while      input        unit        private     constant
functions
```

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
| `list<any>`  | Homogeneous resizable list | `list<integer>`, `[1, 2, 3]` |
| `list<any>` | Flexible list with behavior properties | `list<string>`, `[]`, behavior via `.type` property |
| `matrix<any>` | 2-D list (list of lists) | `matrix<number>`, `[[1.0, 2.0], [3.0, 4.0]]` |
| `pairs<any,any>`  | Key-value associative container | `pairs<string, integer>` |
| `any`         | Generic type parameter | Used in function definitions |

Lists in Clean are zero-indexed by default (list[0] is the first element).
For readability, you can access elements starting from 1 using:

list.at(index)
This returns the element at position index - 1.

### List Properties - Collection Behavior Modifiers

Clean Language extends the core `list<any>` type with **property modifiers** that change the list's behavior without requiring separate collection types. This provides a unified, consistent approach to different collection patterns while maintaining type safety and simplicity.

#### Property Syntax

```clean
list<any> myList = []                    // Create empty list
myList.type = "behavior_type"            // Set behavior using string
```

Where `behavior_type` is a string that defines how the list handles insertions, removals, and access patterns.

**Supported behavior strings:**
- `"default"` - Standard list behavior
- `"line"` - FIFO queue behavior  
- `"pile"` - LIFO stack behavior
- `"unique"` - Set behavior (no duplicates)
- `"line-unique"` - FIFO queue with uniqueness
- `"pile-unique"` - LIFO stack with uniqueness

#### Supported Properties

**`"line"` - Queue Behavior (FIFO)**

First-In-First-Out behavior. Elements are added to the back and removed from the front.

```clean
functions:
    void processTaskQueue()
        list<string> tasks = []
        tasks.type = "line"
        
        // Add tasks (to back)
        tasks.add("Task 1")
        tasks.add("Task 2") 
        tasks.add("Task 3")
        
        // Process tasks (from front)
        iterate i in 1 to 3
            string currentTask = tasks.remove()  // Gets "Task 1", then "Task 2", etc.
            println("Processing: " + currentTask)
```

**Modified Operations**:
- `add(item)` → Adds to the **back** of the list
- `remove()` → Removes from the **front** of the list  
- `peek()` → Views the **front** element without removing
- Standard list operations (`get(index)`, `size()`) remain unchanged

**`"pile"` - Stack Behavior (LIFO)**

Last-In-First-Out behavior. Elements are added and removed from the same end (top).

```clean
functions:
    void undoSystem()
        list<string> actions = []
        actions.type = "pile"
        
        // Perform actions (add to top)
        actions.add("Create file")
        actions.add("Edit text")
        actions.add("Save file")
        
        // Undo actions (remove from top)
        iterate i in 1 to 3
            string lastAction = actions.remove()  // Gets "Save file", then "Edit text", etc.
            println("Undoing: " + lastAction)
```

**Modified Operations**:
- `add(item)` → Adds to the **top** of the list
- `remove()` → Removes from the **top** of the list
- `peek()` → Views the **top** element without removing
- Standard list operations (`get(index)`, `size()`) remain unchanged

**`"unique"` - Set Behavior (Uniqueness Constraint)**

Only allows unique elements. Duplicate additions are ignored.

```clean
functions:
    void trackUniqueVisitors()
        list<string> visitors = []
        visitors.type = "unique"
        
        // Add visitors (duplicates ignored)
        visitors.add("Alice")    // Added
        visitors.add("Bob")      // Added  
        visitors.add("Alice")    // Ignored (duplicate)
        visitors.add("Charlie")  // Added
        
        println("Unique visitors: " + visitors.size().toString())  // Prints: 3
        
        if visitors.contains("Alice")
            println("Alice has visited")
```

**Modified Operations**:
- `add(item)` → Adds only if `item` is not already present
- `remove()` → Removes from default position (implementation-dependent)
- `contains(item)` → Optimized for membership testing
- Standard list operations remain available

#### Property Combinations

Properties can be combined by setting the type to a combined behavior string:

```clean
// Unique queue - FIFO with no duplicates
list<string> uniqueQueue = []
uniqueQueue.type = "line-unique"

// Unique stack - LIFO with no duplicates  
list<integer> uniqueStack = []
uniqueStack.type = "pile-unique"

// All combinations are supported
list<integer> allFeatures = []
allFeatures.type = "line-unique-pile"  // Advanced combination
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

**Behavior Management:**
- Setting `myList.type = "behavior"` changes the list's behavior at runtime

#### Performance Characteristics
- `"line"`: O(1) add, O(1) remove, O(1) peek
- `"pile"`: O(1) add, O(1) remove, O(1) peek  
- `"unique"`: O(1) add/contains (hash-based), O(1) remove

#### Advantages

1. **Unified Type System**: Single `list<any>` type instead of multiple collection types
2. **Consistent API**: All lists share the same base methods
3. **Flexible Behavior**: Properties can be changed at runtime if needed
4. **Type Safety**: Full generic type support with compile-time validation
5. **Simplicity**: Easier to learn and remember than separate collection classes
6. **Interoperability**: All property-modified lists are still `list<any>` types

#### Complete Example

```clean
start()
    // Test different list behaviors
    list<integer> myList = []
    
    // Test line behavior (FIFO queue)
    myList.type = "line"
    myList.add(1)
    myList.add(2)
    myList.add(3)
    
    integer first = myList.remove()   // Returns 1 (first in, first out)
    integer second = myList.remove()  // Returns 2
    
    // Switch to pile behavior (LIFO stack)
    myList.type = "pile"
    myList.add(10)
    myList.add(20)
    myList.add(30)
    
    integer top = myList.remove()     // Returns 30 (last in, first out)
    
    // Switch to unique behavior (set)
    myList.type = "unique"
    myList.add(100)
    myList.add(200)
    myList.add(100)  // Ignored (duplicate)
    
    boolean hasHundred = myList.contains(100)  // Returns true
    integer listSize = myList.size()           // Returns 2 (no duplicates)
    
    print("List demonstrates flexible behavior at runtime")
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

**Explicit conversions:**
```clean
value.toInteger   // convert to integer
value.toNumber     // convert to floating-point
value.toString    // convert to string
value.toBoolean   // convert to boolean
```

**Implementation Status:**
- ✅ **Numeric Conversions**: `integer.toNumber`, `number.toInteger`, `integer.toBoolean` fully implemented
- ✅ **Boolean Conversions**: `integer.toBoolean` (0 = false, non-zero = true) implemented
- ⚠️ **String Conversions**: `value.toString()` requires runtime functions (not yet implemented)

**Examples:**
```clean
integer num = 42
number numFloat = num.toNumber      // ✅ Works: converts 42 to 42.0
integer piInt = 3.14.toInteger    // ✅ Works: converts 3.14 to 3 (truncated)
boolean flag = 0.toBoolean        // ✅ Works: converts 0 to false
boolean nonZero = 5.toBoolean     // ✅ Works: converts 5 to true
```

## Apply-Blocks

Apply-blocks are a core language feature where `identifier:` applies that identifier to each indented item.

### Function Calls
```clean
println:
    "Hello"
    "World"
// Equivalent to: println("Hello"), println("World")

list.push:
    item1
    item2
    item3
// Equivalent to: list.push(item1), list.push(item2), list.push(item3)
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

From highest to lowest precedence:

1. **Primary** - `()`, function calls, method calls, property access
2. **Postfix** - `!` (required assertion)
3. **Unary** - `not`, `-` (unary minus)
4. **Exponentiation** - `^` (right-associative)
5. **Multiplicative** - `*`, `/`, `%`
6. **Additive** - `+`, `-`
7. **Comparison** - `<`, `>`, `<=`, `>=`
8. **Equality** - `==`, `!=`, `is`, `not`
9. **Logical AND** - `and`
10. **Logical OR** - `or`
11. **Null-Coalescing** - `default`
12. **Assignment** - `=`

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
a not b     // Logical NOT (binary, equivalent to !=)
// Note: Unary not operator not yet implemented
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

The `!` (required) operator asserts that a value is not null. If the value is null, it causes a runtime error.

```clean
value!    // Asserts value is not null, returns value or fails
```

**Usage:**
```clean
// Assert that a value exists
string name = maybeNull!    // Fails if maybeNull is null

// Use when you're certain a value is not null
integer count = list.find(item)!

// Combine with method calls
string upper = getText()!.toUpperCase()
```

**When to Use:**
- Use `!` when you're confident a value is not null and want to express that intent
- Use `default` when you want to provide a fallback instead of failing
- Prefer `default` for user-facing code; use `!` for internal assertions

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
"string".length         // Property on literal
list.get(0)           // Built-in method
```

### Function Calls

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

**Implementation Status:**
- ✅ **toString() method calls**: `print(value.toString())` works perfectly
- ✅ **String variables**: `print(string_var)` works perfectly  
- ✅ **String literals**: `print("text")` works perfectly
- ✅ **Variable assignment**: `string result = value.toString()` works perfectly

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

#### Block Syntax
For multiple values or complex formatting, use the block syntax with colon (consistent with Clean Language's block patterns):

```clean
print:
    "First line"
    variable_name
    (complex + expression)
    result.toString()

println:
    "Header:"
    value1
    value2
    "Footer"
```

The block syntax allows for cleaner formatting when printing multiple values sequentially, maintaining consistency with other Clean Language block constructs like `functions:`, `string:`, etc.

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
functions:
    void start()
        // Basic user interaction
        string userName = input("Enter your name: ")
        println("Hello, " + userName + "!")
        
        // Numeric calculations
        integer num1 = input.integer("First number: ")
        integer num2 = input.integer("Second number: ")
        integer sum = num1 + num2
        println("Sum: " + sum.toString())
        
        // Decision making
        boolean wantsCoffee = input.yesNo("Would you like coffee? ")
        if wantsCoffee
            println("Great! Coffee coming right up.")
        else
            println("No problem, maybe next time.")
```

### Return Statement

```clean
return              // Return void
return value        // Return a value
return expression   // Return expression result
```

## Functions

Clean Language uses **functions blocks** for all function declarations. This ensures consistency and organization in code structure.

### The Start Function

Every Clean program begins with a `start()` function. The start function is **special** and can be declared standalone (outside of functions: blocks):

```clean
start()
    print("Hello, World!")
    integer x = 42
    print(x)
```

Alternatively, it can be declared within a `functions:` block:

```clean
functions:
    void start()
        print("Hello, World!")
        integer x = 42
        print(x)
```

### Functions Blocks (Required)

**All functions except `start()` must be declared within a `functions:` block.** This is the only supported syntax for function declarations:

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
functions:
    void start()
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

1. **Expression Support**: Default values can be any valid Clean Language expression
2. **Type Compatibility**: Default values must match the parameter's declared type
3. **Evaluation Time**: Default values are evaluated at function call time
4. **Optional Nature**: Parameters with default values become optional in function calls

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
functions:
    void start()
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
    string.toUpperCase("hi") = "HI"
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
✅ string.toUpperCase("hi") = "HI" (PASS)

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
- Clean Language does not currently support `break` or `continue` keywords
- To exit a while loop early, modify the condition variable or use a boolean flag
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

### Class Definition

**All class methods must be declared within a `functions:` block:**

```clean
class Point
    integer x
    integer y

    constructor(integer x, integer y)        // Auto-stores matching parameter names

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
    any value                  // any makes class generic

    constructor(any value)     // Auto-stores to matching field

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

- ✅ `color = colorParam` (field assignment)
- ✅ `return color` (field access)  
- ✅ `radius = radiusParam` (works in child classes too)
- ❌ No `this.color` or `self.color` needed
- ❌ Parameter names cannot match field names (compiler enforced)

This makes code cleaner while maintaining type safety through name conflict prevention.

### Object Creation and Usage

```clean
functions:
    void start()
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
functions:
    void start()
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
functions:
    void start()
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
functions:
    number calculateTax(number amount)
        return amount * 0.15
    
    string formatResult(number value)
        return "Result: " + value.toString()
    
    void start()
        number tax = calculateTax(100.0)
        string result = formatResult(tax)
        print(result)
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

```clean
// Core mathematical operations
math.sqrt(x), math.abs(x), math.max(a, b), math.min(a, b)

// Rounding and precision functions
math.floor(x), math.ceil(x), math.round(x), math.trunc(x), math.sign(x)

// Trigonometric functions (complete support)
math.sin(x), math.cos(x), math.tan(x), math.asin(x), math.acos(x), math.atan(x), math.atan2(y, x)

// Logarithmic and exponential functions
math.ln(x), math.log10(x), math.log2(x), math.exp(x), math.exp2(x)

// Hyperbolic functions
math.sinh(x), math.cosh(x), math.tanh(x)

// Mathematical constants
math.pi(), math.e(), math.tau()
```
    functions:
        
        // Core mathematical operations
        number sqrt(number x)
        number abs(number x)          // Absolute value for numbers
        integer abs(integer x)      // Absolute value for integers
        number max(number a, number b)
        number min(number a, number b)
        
        // Rounding and precision functions
        number floor(number x)    // Round down to nearest integer
        number ceil(number x)     // Round up to nearest integer  
        number round(number x)    // Round to nearest integer
        number trunc(number x)    // Remove decimal part
        number sign(number x)     // Returns -1, 0, or 1
        
        // Trigonometric functions - work with radians
        number sin(number x)      // Sine
        number cos(number x)      // Cosine
        number tan(number x)      // Tangent
        number asin(number x)     // Arc sine (inverse sine)
        number acos(number x)     // Arc cosine (inverse cosine)
        number atan(number x)     // Arc tangent (inverse tangent)
        number atan2(number y, number x)  // Two-argument arc tangent
        
        // Logarithmic and exponential functions
        number ln(number x)       // Natural logarithm (base e)
        number log10(number x)    // Base-10 logarithm
        number log2(number x)     // Base-2 logarithm
        number exp(number x)      // e raised to the power of x
        number exp2(number x)     // 2 raised to the power of x
        
        // Hyperbolic functions - useful for advanced calculations
        number sinh(number x)     // Hyperbolic sine
        number cosh(number x)     // Hyperbolic cosine
        number tanh(number x)     // Hyperbolic tangent
        
        // Mathematical constants
        number pi()              // π ≈ 3.14159
        number e()               // Euler's number ≈ 2.71828
        number tau()             // τ = 2π ≈ 6.28318

// Usage Examples
functions:
    void start()
        // Basic calculations - using operators for basic math
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

```clean
// Core operations
string.length(text), string.concat(a, b), string.contains(text, search)
string.split(text, delimiter), string.upper(text), string.lower(text)
string.trim(text), string.replace(text, old, new)
```
    functions:
        // Basic operations
        integer length(string text)
            // Returns the number of characters in the string
            // Perfect for validation and loop bounds
        
        string concat(string a, string b)
            // Joins two strings together
            // Creates a new string without modifying the originals
        
        string substring(string text, integer start, integer end)
            // Extracts a portion of the string from start to end position
            // Great for parsing and text extraction
        
        // Case operations - useful for user input normalization
        string toUpperCase(string text)
            // Converts all letters to uppercase
            // Perfect for case-insensitive comparisons
        
        string toLowerCase(string text)
            // Converts all letters to lowercase
            // Ideal for standardizing user input
        
        // Search and validation operations
        boolean contains(string text, string search)
            // Checks if the text contains the search string
            // Returns true if found, false otherwise
        
        integer indexOf(string text, string search)
            // Finds the first position of search string in text
            // Returns -1 if not found, position index if found
        
        integer lastIndexOf(string text, string search)
            // Finds the last position of search string in text
            // Useful for finding file extensions or repeated patterns
        
        boolean startsWith(string text, string prefix)
            // Checks if text begins with the given prefix
            // Great for URL validation or command parsing
        
        boolean endsWith(string text, string suffix)
            // Checks if text ends with the given suffix
            // Perfect for file type checking
        
        // Text cleaning and formatting
        string trim(string text)
            // Removes whitespace from both ends of the string
            // Essential for cleaning user input
        
        string trimStart(string text)
            // Removes whitespace from the beginning only
            // Useful for preserving trailing spaces
        
        string trimEnd(string text)
            // Removes whitespace from the end only
            // Helpful for cleaning line endings
        
        // Advanced text manipulation - powerful tools for text transformation
        string replace(string text, string oldValue, string newValue)
            // Replaces the first occurrence of oldValue with newValue
            // Like find-and-replace in a word processor, but only changes the first match
            // Example: replace("Hello Hello", "Hello", "Hi") → "Hi Hello"
        
        string replaceAll(string text, string oldValue, string newValue)
            // Replaces ALL occurrences of oldValue with newValue
            // Like find-and-replace-all - changes every match in the text
            // Example: replaceAll("Hello Hello", "Hello", "Hi") → "Hi Hi"
        
        list<string> split(string text, string delimiter)
            // Breaks a string into pieces using a separator character
            // Like cutting a rope at specific points - very useful for data processing
            // Example: split("apple,banana,orange", ",") → ["apple", "banana", "orange"]
        
        string join(list<string> parts, string separator)
            // Combines an array of strings into one string with separators
            // The opposite of split - like gluing pieces back together
            // Example: join(["apple", "banana", "orange"], ", ") → "apple, banana, orange"
        
        // Character operations - work with individual letters and symbols
        string charAt(string text, integer index)
            // Gets the character (letter/symbol) at a specific position
            // Like picking out the 3rd letter from a word
            // Example: charAt("Hello", 1) → "e" (positions start at 0)
        
        integer charCodeAt(string text, integer index)
            // Gets the numeric code of a character (useful for sorting or encoding)
            // Every character has a number - 'A' is 65, 'a' is 97, etc.
            // Example: charCodeAt("Hello", 0) → 72 (the code for 'H')
        
        // Validation helpers - check if text meets certain conditions
        boolean isEmpty(string text)
            // Checks if a string has no characters at all
            // Like checking if a box is completely empty
            // Example: isEmpty("") → true, isEmpty("Hi") → false
        
        boolean isBlank(string text)
            // Checks if a string is empty OR contains only spaces/tabs
            // More thorough than isEmpty - catches "invisible" content too
            // Example: isBlank("   ") → true, isBlank("Hi") → false
        
        // Padding operations - add characters to make text a specific length
        string padStart(string text, integer length, string padString)
            // Adds characters to the beginning until the text reaches desired length
            // Like adding zeros before a number: "42" becomes "00042"
            // Example: padStart("42", 5, "0") → "00042"
        
        string padEnd(string text, integer length, string padString)
            // Adds characters to the end until the text reaches desired length
            // Like adding spaces after text to align it in columns
            // Example: padEnd("Name", 10, " ") → "Name      "
        
        // Conversion utilities
        string toString(any value)
            // Converts any value to its string representation
            // Universal conversion for display purposes

// Usage Examples - Real-world string processing scenarios
functions:
    void start()
        // Basic text processing
        string userInput = "  Hello World!  "
        string cleaned = string.trim(userInput)        // "Hello World!"
        integer length = string.length(cleaned)        // 12
        
        // Case normalization for comparisons
        string email1 = "USER@EXAMPLE.COM"
        string email2 = "user@example.com"
        boolean same = string.lower(email1) == string.lower(email2)  // true
        
        // Text searching and validation
        string filename = "document.pdf"
        boolean isPdf = string.endsWith(filename, ".pdf")     // true
        integer dotPos = string.lastIndexOf(filename, ".")    // 8
        
        // URL processing
        string url = "https://api.example.com/users"
        boolean isHttps = string.startsWith(url, "https://")  // true
        boolean hasApi = string.contains(url, "api")          // true
        
        // Text parsing and reconstruction
        string csvLine = "John,Doe,25,Engineer"
        list<string> fields = string.split(csvLine, ",")     // ["John", "Doe", "25", "Engineer"]
        string fullName = string.join([fields[0], fields[1]], " ")  // "John Doe"
        
        // Text replacement and cleaning
        string messyText = "Hello    World"
        string cleaned = string.replaceAll(messyText, "    ", " ")  // "Hello World"
        
        // Formatting and padding
        string number = "42"
        string padded = string.padStart(number, 5, "0")       // "00042"
        
        // Character-level operations
        string word = "Hello"
        string firstChar = string.charAt(word, 0)             // "H"
        integer charCode = string.charCodeAt(word, 0)         // 72 (ASCII for 'H')
        
        // Input validation
        string userField = "   "
        boolean isValid = !string.isBlank(userField)          // false
```

### List Module

The list module provides powerful data collection capabilities. Whether you're managing lists of items, processing data sets, or organizing information, list has all the tools you need for effective data manipulation.

```clean
// Essential operations
list.add(list, item), list.remove(list, index), list.get(list, index)
list.size(list), list.contains(list, item)
list.sort(list), list.reverse(list), list.join(list, separator)
```
    functions:
        // Basic operations - fundamental list access
        integer size(list<any> array)
            // Returns the number of elements in the list
            // Like counting how many items are in a box
            // Example: size([1, 2, 3]) → 3
        
        any get(list<any> array, integer index)
            // Gets the element at the specified position
            // Like picking out the 3rd item from a list
            // Example: get([10, 20, 30], 1) → 20 (positions start at 0)
        
        void set(list<any> array, integer index, any value)
            // Updates the element at the specified position
            // Like replacing an item in a specific slot
            // Example: set([1, 2, 3], 1, 99) → [1, 99, 3]
        
        // Modification operations - changing array contents
        list<any> push(list<any> array, any item)
            // Adds an element to the end of the list
            // Like adding a new item to the end of a list
            // Example: push([1, 2], 3) → [1, 2, 3]
        
        any pop(list<any> array)
            // Removes and returns the last element from the list
            // Like taking the top item off a stack
            // Example: pop([1, 2, 3]) → 3, array becomes [1, 2]
        
        list<any> insert(list<any> array, integer index, any item)
            // Inserts an element at a specific position
            // Like squeezing a new item into the middle of a line
            // Example: insert([1, 3], 1, 2) → [1, 2, 3]
        
        any remove(list<any> array, integer index)
            // Removes and returns the element at the specified position
            // Like taking out a specific item and closing the gap
            // Example: remove([1, 2, 3], 1) → 2, array becomes [1, 3]
        
        // Search operations - finding elements in lists
        boolean contains(list<any> array, any item)
            // Checks if the list contains the specified item
            // Like looking through a box to see if something is there
            // Example: contains([1, 2, 3], 2) → true
        
        integer indexOf(list<any> array, any item)
            // Finds the first position of the item in the list
            // Like finding where something is located in a list
            // Example: indexOf([10, 20, 30], 20) → 1
        
        integer lastIndexOf(list<any> array, any item)
            // Finds the last position of the item in the list
            // Useful when the same item appears multiple times
            // Example: lastIndexOf([1, 2, 1, 3], 1) → 2
        
        // List transformation operations - creating new lists
        list<any> slice(list<any> array, integer start, integer end)
            // Creates a new array containing elements from start to end position
            // Like cutting out a section of the original array
            // Example: slice([1, 2, 3, 4, 5], 1, 4) → [2, 3, 4]
        
        list<any> concat(list<any> array1, list<any> array2)
            // Combines two lists into a single new array
            // Like joining two lists together
            // Example: concat([1, 2], [3, 4]) → [1, 2, 3, 4]
        
        list<any> reverse(list<any> array)
            // Creates a new array with elements in reverse order
            // Like flipping the list upside down
            // Example: reverse([1, 2, 3]) → [3, 2, 1]
        
        list<any> sort(list<any> array)
            // Creates a new array with elements sorted in ascending order
            // Like organizing items from smallest to largest
            // Example: sort([3, 1, 4, 2]) → [1, 2, 3, 4]
        
        // Functional programming operations - advanced array processing
        list<any> map(list<any> array, function callback)
            // Creates a new array by applying a function to each element
            // Like transforming every item in the list using a rule
            // Example: map([1, 2, 3], x => x * 2) → [2, 4, 6]
        
        list<any> filter(list<any> array, function callback)
            // Creates a new array containing only elements that pass a test
            // Like keeping only the items that meet certain criteria
            // Example: filter([1, 2, 3, 4], x => x > 2) → [3, 4]
        
        any reduce(list<any> array, function callback, any initialValue)
            // Reduces the list to a single value by applying a function
            // Like combining all elements into one result
            // Example: reduce([1, 2, 3, 4], (sum, x) => sum + x, 0) → 10
        
        void forEach(list<any> array, function callback)
            // Executes a function for each element in the list
            // Like doing something with every item in the list
            // Example: forEach([1, 2, 3], x => print(x)) → prints 1, 2, 3
        
        // Utility operations - helpful array functions
        boolean isEmpty(list<any> array)
            // Checks if the list has no elements
            // Like checking if a box is completely empty
            // Example: isEmpty([]) → true, isEmpty([1]) → false
        
        boolean isNotEmpty(list<any> array)
            // Checks if the list has at least one element
            // Opposite of isEmpty - checks if there's something there
            // Example: isNotEmpty([1, 2]) → true
        
        any first(list<any> array)
            // Gets the first element of the list
            // Like looking at the item at the front of the line
            // Example: first([10, 20, 30]) → 10
        
        any last(list<any> array)
            // Gets the last element of the list
            // Like looking at the item at the back of the line
            // Example: last([10, 20, 30]) → 30
        
        string join(list<string> array, string separator)
            // Combines all array elements into a single string with separators
            // Like gluing text pieces together with a connector
            // Example: join(["apple", "banana", "orange"], ", ") → "apple, banana, orange"
        
        // List creation helpers - building new lists
        list<any> fill(integer size, any value)
            // Creates a new array of specified size filled with the same value
            // Like making multiple copies of the same item
            // Example: fill(3, "hello") → ["hello", "hello", "hello"]
        
        list<integer> range(integer start, integer end)
            // Creates an array of numbers from start to end
            // Like counting from one number to another
            // Example: range(1, 5) → [1, 2, 3, 4, 5]

// Usage Examples - Real-world array processing scenarios
functions:
    void start()
        // Basic array operations
        list<integer> numbers = [1, 2, 3]
        integer size = list.size(numbers)           // 3
        integer first = list.get(numbers, 0)          // 1
        list.set(numbers, 1, 99)                      // [1, 99, 3]
        
        // Building and modifying lists
        list<string> fruits = ["apple", "banana"]
        fruits = list.add(fruits, "orange")          // ["apple", "banana", "orange"]
        string lastFruit = list.remove(fruits, 2)           // "orange", fruits becomes ["apple", "banana"]
        
        // Searching through data
        list<integer> scores = [85, 92, 78, 96, 88]
        boolean hasHighScore = list.contains(scores, 96)     // true
        integer position = list.indexOf(scores, 92)          // 1
        
        // Data processing and transformation
        list<integer> data = [1, 2, 3, 4, 5]
        list<integer> doubled = list.map(data, x => x * 2)  // [2, 4, 6, 8, 10]
        list<integer> evens = list.filter(data, x => x % 2 == 0)  // [2, 4]
        integer sum = list.reduce(data, (total, x) => total + x, 0)  // 15
        
        // List manipulation
        list<string> names1 = ["Alice", "Bob"]
        list<string> names2 = ["Charlie", "Diana"]
        list<string> allNames = list.concat(names1, names2)  // ["Alice", "Bob", "Charlie", "Diana"]
        list<string> reversed = list.reverse(allNames)       // ["Diana", "Charlie", "Bob", "Alice"]
        
        // Working with sections of lists
        list<integer> bigList = [10, 20, 30, 40, 50]
        list<integer> middle = list.slice(bigList, 1, 4)     // [20, 30, 40]
        
        // Text processing with lists
        list<string> words = ["hello", "world", "from", "Clean"]
        string sentence = list.join(words, " ")               // "hello world from Clean"
        
        // Creating lists programmatically
        list<string> greetings = list.fill(3, "Hello")       // ["Hello", "Hello", "Hello"]
        list<integer> countdown = list.range(5, 1)           // [5, 4, 3, 2, 1]
        
        // Validation and utility
        boolean isEmpty = list.isEmpty([])                    // true
        string firstWord = list.first(words)                  // "hello"
        string lastWord = list.last(words)                    // "Clean"
```

### File Module

The file module makes working with files simple and straightforward. Whether you need to read configuration files, save user data, or process text documents, file has you covered with easy-to-use methods.

```clean
// Basic I/O
file.read(path), file.write(path, content), file.exists(path)
```
    functions:
        // Reading files
        string read(string path)
            // Reads the entire file content as a single string
            // Perfect for small to medium-sized files
        
        list<string> lines(string path)
            // Reads the file and returns each line as a separate string
            // Great for processing text files line by line
        
        // Writing files
        void write(string path, string content)
            // Writes text to a file, replacing any existing content
            // Creates the file if it doesn't exist
        
        void append(string path, string content)
            // Adds text to the end of an existing file
            // Creates the file if it doesn't exist
        
        // File management
        boolean exists(string path)
            // Checks if a file exists at the given path
            // Returns true if found, false otherwise
        
        void delete(string path)
            // Removes a file from the filesystem
            // Does nothing if the file doesn't exist

// Usage Examples
functions:
    void start()
        // Read a configuration file
        string config = file.read("settings.txt")
        
        // Process a log file line by line
        list<string> logLines = file.lines("app.log")
        
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

```clean
// Core requests
http.get(url), http.post(url, body)
```
    functions:
        // GET - Retrieve data from a server
        string get(string url)
            // Sends a GET request to fetch data
            // Returns the response body as a string
        
        // POST - Send new data to a server
        string post(string url, string body)
            // Sends a POST request with data in the body
            // Returns the server's response as a string
        
        // PUT - Update existing data on a server
        string put(string url, string body)
            // Sends a PUT request to update a resource
            // Returns the server's response as a string
        
        // PATCH - Partially update data on a server
        string patch(string url, string body)
            // Sends a PATCH request for partial updates
            // Returns the server's response as a string
        
        // DELETE - Remove data from a server
        string delete(string url)
            // Sends a DELETE request to remove a resource
            // Returns the server's response as a string

// Usage Examples
functions:
    void start()
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

```clean
functions:
    // Parse JSON text into Clean Language data
    any textToData(string jsonText)
        // Parses a JSON string and returns the corresponding Clean value
        // Throws an error if the JSON is invalid
        // JSON types map to Clean types:
        //   - JSON object → pairs<string, any>
        //   - JSON array → list<any>
        //   - JSON string → string
        //   - JSON number → number
        //   - JSON boolean → boolean
        //   - JSON null → null

    any tryTextToData(string jsonText)
        // Attempts to parse JSON, returns null on failure
        // Useful when you want to handle invalid JSON gracefully
        // Does not throw errors for malformed JSON
```

#### Accessing JSON Data

The `any` type returned by `json.textToData()` supports bracket notation for accessing nested data. This allows direct field access on JSON objects and index access on JSON arrays.

```clean
// String key access for JSON objects
any data = json.textToData(jsonString)
any fieldValue = data["fieldName"]      // Access field by string key

// Integer index access for JSON arrays
any arrayData = json.textToData(arrayJson)
any element = arrayData[0]              // Access element by integer index

// Chained access for nested structures
any nested = data["user"]["profile"]["name"]
any item = data["items"][0]["id"]
```

**Bracket Notation Rules:**
- **String keys** (`data["key"]`): Access fields on JSON objects, returns `any`
- **Integer indices** (`data[0]`): Access elements in JSON arrays, returns `any`
- **Chained access**: Both forms can be chained for nested structures
- **Missing fields**: Returns `null` when field doesn't exist or index is out of bounds

```clean
start()
    string jsonText = '{"name": "Alice", "scores": [85, 92, 78]}'
    any data = json.textToData(jsonText)

    // Object field access
    any name = data["name"]           // Returns "Alice"
    any missing = data["unknown"]     // Returns null

    // Array index access
    any scores = data["scores"]
    any first = scores[0]             // Returns 85
    any outOfBounds = scores[100]     // Returns null

    // Use default operator for fallback values
    string userName = data["name"] default "Guest"
    integer firstScore = data["scores"][0] default 0
```

#### Serializing to JSON

```clean
functions:
    // Convert Clean Language data to JSON text
    string dataToText(any data)
        // Converts a Clean value to a compact JSON string
        // Supports: strings, numbers, booleans, null, lists, pairs
        // Example output: {"name":"Alice","age":25}

    string prettyDataToText(any data)
        // Converts a Clean value to a formatted, readable JSON string
        // Adds indentation and line breaks for human readability
        // Example output:
        // {
        //   "name": "Alice",
        //   "age": 25
        // }
```

#### Usage Examples

```clean
start()
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

#### Error Handling

```clean
start()
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

Clean Language uses method-style syntax as the primary pattern for object operations. This makes your code more readable and intuitive by allowing you to call functions directly on values.

### Primary Pattern

Method-style syntax is the preferred way to work with objects and values:

```clean
// Method-style syntax (preferred)
integer textLength = myText.length()
string upperText = myText.toUpperCase()
list.add(myList, item)
value.toString()
```

### Namespace Functions

For utility functions, use lowercase namespace calls:

```clean
// Namespace functions (lowercase)
math.sqrt(16)
string.concat("a", "b")
list.sort(myList)
```

### Method-Style Syntax Examples

Method-style syntax is the preferred way to work with objects and values:

```clean
// Text operations
string text = "Hello World"
integer length = text.length()
string upper = text.toUpperCase()
string lower = text.toLowerCase()
string trimmed = text.trim()

// List operations
list<integer> numbers = [1, 2, 3]
integer size = numbers.length()
boolean empty = numbers.isEmpty()
numbers.add(4)

// Value conversions
integer age = 25
string ageText = age.toString()
number decimal = age.toNumber()

// Object properties
user.name
user.age
user.toString()
```

### Namespace Functions

For utility functions, use lowercase namespace calls:

```clean
// Math operations
math.sqrt(16)
math.max(10, 20)
math.pi()

// String operations
string.concat("Hello", "World")
string.split("a,b,c", ",")
string.trim("  text  ")

// List operations
list.sort(myList)
list.reverse(myList)
list.join(myList, ", ")
```

### When to Use Each Style

**Use Method Style When:**
- Working with a specific value (like `text.length()`)
- Accessing object properties (like `user.name`)
- Converting values (like `value.toString()`)

**Use Namespace Functions When:**
- Calling utility functions (like `math.sqrt()`)
- Working with multiple parameters (like `string.concat()`)
- Using library functions (like `list.sort()`)

## Modules and Imports

Clean Language supports multi-file programs through a module system. Each `.cln` file is a module that can import and use code from other modules.

**Important:** The `import:` statement is **exclusively for Clean Language modules** (`.cln` files). Plugins are NOT imported - they are declared at the project level in `configuration.cln`. See the [Plugin System](#plugin-system) section for details.

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

start()
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

### Multi-File Compilation

The compiler automatically discovers and compiles all imported modules. The `build` command is the recommended way to compile multi-file projects:

```bash
# Build a multi-file project (resolves all imports)
cln build main.cln

# Build with custom output path
cln build main.cln -o app.wasm

# Build with library search paths
cln build main.cln -L ./lib -L ./modules

# Build with optimization level
cln build main.cln -O3
```

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

start()
    number pi = math.pi
    string upper = string.toUpperCase("hello")
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

start()
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

start()
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

## Package Management (Future Feature)

**Note:** Package management is planned for future releases of Clean Language. Currently, Clean Language focuses on core language features and WebAssembly compilation. Package management capabilities will be added in subsequent versions to enable code sharing and dependency management.

## Asynchronous Programming

Clean uses start and later for simple asynchronous execution.
start begins a task in the background.
later declares that the result will be available in the future.
The value blocks only when accessed.
Use background to run a task without keeping the result.
You can also mark a function as background to always run it asynchronously and ignore its result.

later data = start fetchData("url")
print "Working..."
print data          # blocks here only

background logAction("login")    # runs and ignores result

function syncCache() background
    sendUpdateToServer()
    clearLocalTemp()
    
syncCache()    # runs in background automatically

## Memory Management

Clean uses Automatic Reference Counting (ARC) for memory management.

## Plugin System

The Clean Language Plugin System allows you to extend the language with custom Domain-Specific Language (DSL) blocks. Plugins transform DSL syntax into standard Clean Language code before compilation.

### Important: Plugins vs Imports

**Plugins are NOT imported in source files.** The `import:` statement is exclusively for importing Clean Language modules (other `.cln` files).

Plugins are declared at the **project level** in `configuration.cln`:

```clean
// configuration.cln - Project configuration file
plugins:
    frame.web
    frame.ui
    frame.data
```

If a plugin provides runtime helper functions, those helpers may be imported using `import:`, but the plugin itself is never imported:

```clean
// ✅ CORRECT - Import runtime helpers provided by a plugin
import:
    frame.web.request    // Runtime helpers from the web plugin

// ❌ WRONG - Never import plugins directly
import:
    frame.web            // This is NOT how plugins work!
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
