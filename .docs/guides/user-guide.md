# Clean Language User Guide

## Introduction

Clean is a modern, lightweight programming language designed to be compiled to WebAssembly. It features a simple, clear syntax with strong typing and a focus on readability. This guide will help you get started with the Clean Language and understand its core features.

## Installation

To use the Clean Language, you'll need to install the Clean Language Compiler:

```bash
# Clone the repository
git clone https://github.com/your-username/clean-language-compiler.git
cd clean-language-compiler

# Build the compiler
cargo build --release
```

After building, the compiler will be available at `build/release/cleanc`.

## Basic Syntax

### Hello World

Here's a simple "Hello World" program in Clean:

```
start()
    print("Hello, World!")
```

Every Clean program must have a `start()` function, which serves as the entry point.

### Variables

Variables in Clean are declared using the `let` keyword (optional) or by direct assignment:

```
start()
    // With let keyword and explicit type annotation
    let x: int = 10
    
    // Without let keyword (type is inferred)
    y = 20
    
    // String variable
    message = "Hello, Clean!"
    
    // Print variables
    print(x)
    print(y)
    print(message)
```

### Data Types

Clean supports several basic data types:

- `int`: 32-bit signed integer
- `long`: 64-bit signed integer
- `float`: 64-bit floating-point number
- `boolean`: true or false
- `string`: Text string
- `list`: Collection of values
- `matrix`: Two-dimensional list

Example:

```
start()
    // Integer
    i = 42
    
    // Float
    f = 3.14
    
    // Boolean
    b = true
    
    // String
    s = "Hello, Clean!"
    
    // List
    a = [1, 2, 3, 4, 5]
    
    // Matrix
    m = [[1, 2], [3, 4]]
```

### Operators

Clean supports the standard arithmetic, comparison, and logical operators:

- Arithmetic: `+`, `-`, `*`, `/`, `%`
- Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=`
- Logical: `and`, `or`, `not`

Example:

```
start()
    a = 10
    b = 5
    
    // Arithmetic
    sum = a + b        // 15
    difference = a - b // 5
    product = a * b    // 50
    quotient = a / b   // 2
    
    // Comparison
    isEqual = a == b   // false
    isGreater = a > b  // true
    
    // Logical
    result = (a > 5) and (b < 10) // true
```

### Control Flow

#### Conditionals

Clean supports `if` and `else` statements:

```
start()
    x = 10
    
    if x > 5
        print("x is greater than 5")
    else
        print("x is less than or equal to 5")
```

#### Loops

Clean provides several loop constructs:

- `iterate` for simple loops
- `iterate from-to` for numerical ranges
- `while` for conditional loops

Examples:

```
start()
    // Simple iterate with list
    arr = [1, 2, 3, 4, 5]
    iterate element in arr
        print(element)
    
    // Iterate from-to
    iterate i from 1 to 5
        print(i)
    
    // While loop
    counter = 0
    while counter < 5
        print(counter)
        counter = counter + 1
```

### Functions

Functions in Clean are defined using the `function` keyword:

```
// Function with parameters
function add(a, b)
    return a + b

// Function with explicit return type
function multiply(a: int, b: int): int
    return a * b

start()
    result1 = add(5, 3)      // 8
    result2 = multiply(4, 2) // 8
    
    print(result1)
    print(result2)
```

### Error Handling

Clean provides error handling using the `onError` syntax:

```
start()
    // Try to divide by zero
    result = divide(10, 0) onError:
        print("Division by zero error")
        return 0
    
    print(result)

function divide(a, b)
    if b == 0
        error("Cannot divide by zero")
    return a / b
```

## Standard Library

Clean comes with a standard library providing basic functionality:

### String Operations

```
start()
    s1 = "Hello"
    s2 = "World"
    
    // Concatenation
    combined = s1 + ", " + s2 + "!"  // "Hello, World!"
    
    // Length
    length = string.length(combined)  // 13
    
    // Comparison
    are_equal = string.compare(s1, s1)  // 0 (equal)
```

### List Operations

```
start()
    arr = [1, 2, 3, 4, 5]
    
    // Get length
    length = list.length(arr)  // 5
    
    // Get element
    element = list.get(arr, 2)  // 3 (zero-indexed)
    
    // Set element
    list.set(arr, 1, 10)  // arr becomes [1, 10, 3, 4, 5]
    
    // Iterate
    list.iterate(arr, printElement)
    
    // Map
    doubled = list.map(arr, double)  // [2, 20, 6, 8, 10]

function printElement(element)
    print(element)

function double(element)
    return element * 2
```

## Building and Running Clean Programs

To compile a Clean program to WebAssembly:

```bash
./build/release/cleanc compile path/to/your/program.cl path/to/output.wasm
```

To compile and run a Clean program directly:

```bash
./build/release/cleanc run path/to/your/program.cl
```

## Examples

### Factorial Calculation

```
function factorial(n)
    if n <= 1
        return 1
    return n * factorial(n - 1)

start()
    print("Factorial of 5 is: " + factorial(5))  // 120
```

### Fibonacci Sequence

```
function fibonacci(n)
    if n <= 0
        return 0
    if n == 1
        return 1
    return fibonacci(n - 1) + fibonacci(n - 2)

start()
    iterate i from 0 to 10
        print("Fibonacci(" + i + ") = " + fibonacci(i))
```

## Advanced Features

### Classes and Objects

Clean supports basic object-oriented programming with classes:

```
class Point
    integer x
    integer y
    
    constructor(integer xParam, integer yParam)
        x = xParam      // Implicit context - no 'this' needed
        y = yParam
    
    functions:
        float distance(Point other)
            integer dx = x - other.x    // Direct field access
            integer dy = y - other.y
            return Math.sqrt(dx * dx + dy * dy)

functions:
    void start()
        Point p1 = Point(0, 0)
        Point p2 = Point(3, 4)
        
        float dist = p1.distance(p2)  // 5.0
        print("Distance: " + dist.toString())
```

### Modules and Imports

Clean supports modular code organization:

```
// math.cl
export function square(x)
    return x * x

export function cube(x)
    return x * x * x

// main.cl
import square, cube from "math"

start()
    print("Square of 5: " + square(5))  // 25
    print("Cube of 3: " + cube(3))      // 27
```

## Best Practices

1. Keep your code clean and readable.
2. Use descriptive variable and function names.
3. Add comments for complex logic.
4. Break down large functions into smaller, reusable ones.
5. Handle errors gracefully with `onError` blocks.
6. Test your code thoroughly.

## Troubleshooting

### Common Errors

1. **Syntax Errors**: Check indentation and missing semicolons.
2. **Type Errors**: Ensure your variables are of the correct type.
3. **Missing start function**: Every Clean program must have a `start()` function.
4. **Runtime Errors**: Use onError blocks to handle potential runtime errors.

### Getting Help

If you encounter issues, check the following resources:

- [GitHub Issues](https://github.com/your-username/clean-language-compiler/issues)
- [Language Reference](docs/language-reference.md)
- [Standard Library Documentation](docs/stdlib.md) 