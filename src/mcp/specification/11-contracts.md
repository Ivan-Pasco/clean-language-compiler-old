# 11. Contracts

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

---

[← Previous: Functions](./10-functions.md) | [Index](./README.md) | [Next: Testing](./12-testing.md) →
