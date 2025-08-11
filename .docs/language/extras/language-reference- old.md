# Clean Language Reference

This document provides a comprehensive reference for the Clean programming language syntax, semantics, and standard library.

## Table of Contents

1. [Lexical Structure](#lexical-structure)
2. [Data Types](#data-types)
3. [Variables](#variables)
4. [Expressions](#expressions)
5. [Statements](#statements)
6. [Functions](#functions)
7. [Control Flow](#control-flow)
8. [Error Handling](#error-handling)
9. [Classes and Objects](#classes-and-objects)
10. [Modules and Imports](#modules-and-imports)
11. [Standard Library](#standard-library)
12. [Grammar Specification](#grammar-specification)

## Lexical Structure

### Comments

```
// Single line comment

/* 
   Multi-line
   comment
*/
```

### Identifiers

Identifiers in Clean must start with a letter or underscore and may contain letters, numbers, and underscores.

Valid: `x`, `count`, `myVariable`, `_private`, `value1`
Invalid: `1value`, `my-var`, `$name`

### Keywords

The following are reserved keywords in Clean:

```
and        class       else        error       export      
false      for         from        function    if          
import     in          iterate     let         not         
onError    or          print       printl      return      
start      test        this        to          true        
while
```

### Literals

#### Integers
```
10      // Decimal
0xff    // Hexadecimal
0b1010  // Binary
0o777   // Octal
```

#### Floating-point
```
3.14
.5
6.02e23
```

#### Strings
```
"Hello, world!"
"Line 1\nLine 2"
```

#### Booleans
```
true
false
```

#### Lists
```
[1, 2, 3, 4]
["a", "b", "c"]
[]  // Empty list
```

#### Matrices
```
[[1, 2], [3, 4]]
```

## Data Types

### Primitive Types

| Type     | Description           | Example Literals      |
|----------|-----------------------|-----------------------|
| `int`    | 32-bit signed integer | `42`, `-7`            |
| `long`   | 64-bit signed integer | `9223372036854775807` |
| `float`  | 64-bit float          | `3.14`, `6.02e23`     |
| `boolean`| Boolean true/false    | `true`, `false`       |
| `string` | Text string           | `"Hello"`             |

### Composite Types

| Type     | Description           | Example                |
|----------|-----------------------|------------------------|
| `list`  | List of values        | `[1, 2, 3]`            |
| `matrix` | 2D list              | `[[1, 2], [3, 4]]`     |
| `object` | Instance of a class   | `Point(2, 3)`          |

### Type Annotations

Type annotations are optional in Clean. When omitted, types are inferred:

```
let x: int = 10       // Explicit type
let y = 20            // Type inferred as int
let z: float = 3.14   // Explicit type
```

## Variables

### Declaration and Assignment

```
// Declaration with let
let x: int = 10

// Declaration with type inference
let y = 20

// Direct assignment (also declaration)
z = 30
```

### Scope

Variables in Clean are block-scoped:

```
start()
    x = 10  // Outer scope
    
    if x > 5
        y = 20  // Inner scope, only available within this block
        print(x)  // Can access outer scope
    
    print(y)  // Error: y is not defined in this scope
```

## Expressions

### Arithmetic Operators

| Operator | Description    | Example     |
|----------|----------------|-------------|
| `+`      | Addition       | `a + b`     |
| `-`      | Subtraction    | `a - b`     |
| `*`      | Multiplication | `a * b`     |
| `/`      | Division       | `a / b`     |
| `%`      | Modulo         | `a % b`     |

### Comparison Operators

| Operator | Description       | Example     |
|----------|-------------------|-------------|
| `==`     | Equal             | `a == b`    |
| `!=`     | Not equal         | `a != b`    |
| `<`      | Less than         | `a < b`     |
| `>`      | Greater than      | `a > b`     |
| `<=`     | Less or equal     | `a <= b`    |
| `>=`     | Greater or equal  | `a >= b`    |

### Logical Operators

| Operator | Description  | Example          |
|----------|--------------|------------------|
| `and`    | Logical AND  | `a > 0 and b < 10` |
| `or`     | Logical OR   | `a > 0 or b > 0`   |
| `not`    | Logical NOT  | `not (a == b)`     |

### Precedence and Associativity

Operators in Clean follow standard precedence rules:

1. Parentheses `()`
2. Unary operators (`not`, unary `-`)
3. Multiplicative (`*`, `/`, `%`)
4. Additive (`+`, `-`)
5. Comparison (`<`, `>`, `<=`, `>=`)
6. Equality (`==`, `!=`)
7. Logical AND (`and`)
8. Logical OR (`or`)

All binary operators are left-associative except for assignment.

## Statements

### Variable Declaration

```
let x: int = 10
x = 20
```

### Print Statement

```
print(expression)  // Print value
printl(expression) // Print value with newline
```

### Return Statement

```
return expression
```

### Assignment

```
variable = expression
```

## Functions

### Definition

```
function name(param1, param2, ...) 
    // Function body
    return expression
```

### With Type Annotations

```
function add(a: int, b: int): int
    return a + b
```

### Function Calls

```
result = add(5, 3)
```

### Anonymous Functions

```
square = function(x)
    return x * x
```

## Control Flow

### Conditional Statements

```
if condition
    // Statements executed if condition is true
else
    // Statements executed if condition is false
```

If-else chains:

```
if condition1
    // Statements
else if condition2
    // Statements
else
    // Statements
```

### Loops

#### Iterate Loop

```
iterate item in collection
    // Body
```

#### Iterate From-To Loop

```
iterate i from start to end
    // Body
```

#### While Loop

```
while condition
    // Body
```

## Error Handling

### Raising Errors

```
function divide(a, b)
    if b == 0
        error("Cannot divide by zero")
    return a / b
```

### Handling Errors

```
result = divide(10, 0) onError:
    print("Division error occurred")
    return 0
```

## Classes and Objects

### Class Definition

```
class ClassName
    // Properties
    property1: Type
    property2: Type
    
    // Constructor
    constructor(param1, param2)
        this.property1 = param1
        this.property2 = param2
    
    // Methods
    function method1(param) 
        return expression
```

### Object Creation and Use

```
obj = ClassName(arg1, arg2)
result = obj.method1(arg)
```

## Modules and Imports

### Exporting

```
// File: math.cl
export function add(a, b)
    return a + b
```

### Importing

```
// File: main.cl
import add from "math"

start()
    result = add(5, 3)
    print(result)
```

### Multiple Imports

```
import function1, function2 from "module"
```

## Standard Library

### String Operations

| Function | Description | Example |
|----------|-------------|---------|
| `string.length(s)` | Returns string length | `string.length("hello")` |
| `string.concat(s1, s2)` | Concatenates strings | `string.concat("a", "b")` |
| `string.compare(s1, s2)` | Compares strings | `string.compare("a", "b")` |

### List Operations

| Function | Description | Example |
|----------|-------------|---------|
| `list.length(arr)` | Returns list length | `list.length([1,2,3])` |
| `list.get(arr, index)` | Gets element at index | `list.get(arr, 0)` |
| `list.set(arr, index, value)` | Sets element at index | `list.set(arr, 0, 42)` |
| `list.iterate(arr, callback)` | Iterates over elements | `list.iterate(arr, print)` |
| `list.map(arr, callback)` | Maps elements | `list.map(arr, double)` |

### Math Operations

| Function | Description | Example |
|----------|-------------|---------|
| `math.sqrt(x)` | Square root | `math.sqrt(16)` |
| `x ^ y` | Exponentiation | `2 ^ 3` |
| `math.abs(x)` | Absolute value | `math.abs(-5)` |
| `math.floor(x)` | Floor | `math.floor(3.7)` |
| `math.ceil(x)` | Ceiling | `math.ceil(3.2)` |
| `math.round(x)` | Round | `math.round(3.5)` |

## Grammar Specification

The Clean language grammar is defined in Extended Backus-Naur Form (EBNF). Here's a simplified version:

```
program = { function_decl | class_decl | start_function } ;

start_function = "start" "(" ")" block ;

function_decl = "function" identifier "(" [ parameter_list ] ")" [ ":" type ] block ;

class_decl = "class" identifier "{" { property_decl | constructor | method_decl } "}" ;

property_decl = identifier ":" type ";" ;

constructor = "constructor" "(" [ parameter_list ] ")" block ;

method_decl = "function" identifier "(" [ parameter_list ] ")" [ ":" type ] block ;

parameter_list = parameter { "," parameter } ;

parameter = identifier [ ":" type ] ;

block = "{" { statement } "}" | indent { statement } dedent ;

statement = variable_decl | assignment | if_stmt | iterate_stmt | 
            while_stmt | return_stmt | function_call | print_stmt ;

variable_decl = [ "let" ] identifier [ ":" type ] [ "=" expression ] ";" ;

assignment = identifier "=" expression ";" ;

if_stmt = "if" expression block [ "else" block ] ;

iterate_stmt = "iterate" identifier "in" expression block |
                "iterate" identifier "from" expression "to" expression block ;

while_stmt = "while" expression block ;

return_stmt = "return" [ expression ] ";" ;

function_call = identifier "(" [ argument_list ] ")" [ "onError" ":" block ] ;

print_stmt = ( "print" | "printl" ) "(" expression ")" ";" ;

argument_list = expression { "," expression } ;

expression = logical_expr ;

logical_expr = comparison_expr { ("and" | "or") comparison_expr } ;

comparison_expr = additive_expr { ("==" | "!=" | "<" | ">" | "<=" | ">=") additive_expr } ;

additive_expr = multiplicative_expr { ("+" | "-") multiplicative_expr } ;

multiplicative_expr = unary_expr { ("*" | "/" | "%") unary_expr } ;

unary_expr = [ "not" | "-" ] primary_expr ;

primary_expr = literal | identifier | function_call | "(" expression ")" ;

literal = integer | float | string | boolean | list | matrix ;

type = "int" | "long" | "float" | "boolean" | "string" | "list" | "matrix" | identifier ;
```

Note: This grammar is simplified and not complete. The actual parser implements additional rules and error handling. 