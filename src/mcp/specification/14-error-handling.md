# 14. Error Handling

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


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

---

[← Previous: Control Flow](./13-control-flow.md) | [Index](./README.md) | [Next: Classes and Objects](./15-classes-and-objects.md) →
