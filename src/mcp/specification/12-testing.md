# 12. Testing

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

---

[← Previous: Contracts](./11-contracts.md) | [Index](./README.md) | [Next: Control Flow](./13-control-flow.md) →
