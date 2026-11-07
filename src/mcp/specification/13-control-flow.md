# 13. Control Flow

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

---

[← Previous: Testing](./12-testing.md) | [Index](./README.md) | [Next: Error Handling](./14-error-handling.md) →
