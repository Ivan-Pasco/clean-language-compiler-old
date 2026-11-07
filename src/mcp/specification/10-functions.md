# 10. Functions

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

**Important:** The `start:` entry block is different from the `start` keyword used for background expressions. See [Asynchronous Programming](./19-async.md) for details on `start` as an expression.

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

---

[← Previous: Build Configuration](./09-build-configuration.md) | [Index](./README.md) | [Next: Contracts](./11-contracts.md) →
