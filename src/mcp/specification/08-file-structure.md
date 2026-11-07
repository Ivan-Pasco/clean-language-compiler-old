# 08. File Structure

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

---

[← Previous: Statements](./07-statements.md) | [Index](./README.md) | [Next: Build Configuration](./09-build-configuration.md) →
