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

Tests are always part of the compiled program. There is no flag to add them or to leave them out: every `cln compile`, `cln build` and `cln run` of a file includes its `tests:` block.

The tests run right after `start:` finishes. `cln test` compiles the files, runs each one and reports the tests:

```bash
# Run the tests of one file
cln test myprogram.cln

# Run the tests of every .cln file in a folder
cln test tests/
```

`cln test` exits with code 0 when every test passes and 1 when any test fails.

### Test Output Format

The test runner prints one line per test: `PASS:` or `FAIL:` followed by the test's description. An anonymous test is named by its position (`test_2` is the third test in the block):

```clean
start:
    print(add(1, 1))

functions:
    integer add(integer a, integer b)
        return a + b

tests:
    "adds numbers": add(2, 3) = 5
    "wrong on purpose": add(2, 2) = 5
    add(1, 1) = 2
```

```
PASS: adds numbers
FAIL: wrong on purpose
PASS: test_2
❌ 1 test(s) failed.
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

A test calls a method from outside the class, so the method must be under `public:`:

```clean
class Calculator
    integer value

    constructor(integer initialValue)
        value = initialValue

    functions:
        public:
            integer add(integer x)
                value = value + x
                return value

tests:
    "calculator addition": Calculator(10).add(5) = 15
    "calculator from zero": Calculator(0).add(3) = 3
```

#### Testing List and String Operations

```clean
tests:
    "list operations": [1, 2, 3].length() = 3
    "list contains": [1, 2, 3].contains(2) = true
    "string operations": "hello".toUpperCase() = "HELLO"
    "string indexing": "world".indexOf("r") = 2
```

#### Endpoint Tests (`test "name"`)

A program that uses the frame.server plugin can describe HTTP checks inside `tests:` with the `test "name"` form. The indented lines are a request (`GET`, `POST`, `PUT`, `PATCH` or `DELETE` and a path) followed by checks on the response:

```clean
tests:
    test "health check"
        GET "/api/health"
        status = 200
        json.status = "ok"
```

The compiler checks the form of these tests but does not run them; they are for frame.server's test tools. A file whose `tests:` block holds only endpoint tests has no tests for `cln test` to run, so `cln test` reports "no tests found" for it instead of a failure.

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
