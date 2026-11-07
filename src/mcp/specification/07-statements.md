# 07. Statements

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

Clean Language provides an intuitive and clean print syntax that distinguishes between output with and without newlines. The parenthesized form `print(value)` prints the value followed by a newline. The continuation form `print(value) +` and the bare form `print value` print without a newline, so the next output continues on the same line.

#### Simple Syntax
The print statement uses two distinct forms based on whether you want a newline:

**Print with newline (default):**
```clean
print("Hello")          // Prints "Hello" and adds newline
print(variable)         // Prints variable content and adds newline
print(expression)       // Prints expression result and adds newline
print(42)               // Prints "42" and adds newline
```

**Print without newline (continuation):**
```clean
print("Hello") +        // Prints "Hello" (no newline; next output continues the line)
print "Hello"           // Bare form: prints "Hello" (no newline)
print variable          // Prints variable content (no newline)
```

**Key Design Principles:**
- **Intuitive**: The common case — one value per line — is the plain `print(value)` call; the trailing `+` marks output that continues on the same line
- **Clean**: Simple distinction between the two behaviors
- **Readable**: The trailing `+` visually joins the next output onto the current line
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

Each line in the block is equivalent to a `print(expr)` call. The above is exactly the same as:

```clean
print("User: " + username)
print("Score: " + score)
print("Status: active")
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

---

[← Previous: Expressions](./06-expressions.md) | [Index](./README.md) | [Next: File Structure](./08-file-structure.md) →
