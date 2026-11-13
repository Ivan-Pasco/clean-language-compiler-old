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

What follows `onError` on the same line decides what happens:

- a value (`onError 0`) is the fallback: the expression's result becomes that value;
- a statement — `print(...)`, `return`, `break`, `continue` or `error(...)` — runs instead, and the variable being set keeps its zero value (`data` above is `""` after a failed read);
- `onError:` followed by an indented block runs the block.

### Errors Raised by the Language

Some operations raise an error themselves. These are ordinary errors: `onError` catches them, and the message is available as `error`.

| Operation | When it raises | Message |
|-----------|----------------|---------|
| Integer `/` and `%` (any integer size) | the divisor is zero | `division by zero` |
| Integer `+`, `-`, `*`, unary `-` (and `MIN / -1`) | the result is outside `integer`, `integer:64`, `integer:32u` or `integer:64u` (8- and 16-bit results raise the "does not fit" error when stored) | `integer overflow` |
| Storing into an integer type (declaration, assignment, argument, return, field, list/pairs element, conversion) | the value does not fit the type | e.g. `300 does not fit in integer:8` |
| `text.toBoolean()` | the text is not `"true"`, `"false"`, `"1"` or `"0"` | e.g. `'yes' is not a valid boolean` |
| `text.toInteger()` | the text is not a whole number, or is outside the `integer` range | e.g. `'x1' is not a valid integer` |
| `text.toNumber()` | the text is not a decimal number, or is beyond the `number` range | e.g. `'1e999' is too large for number` |

```clean
integer q = total / count onError 0
integer n = "abc".toInteger() onError -1
integer m = text.toInteger() onError:
    print("caught: " + error)
```

When the value is a constant, overflow and a value that does not fit are compile-time errors instead.

Number (decimal) arithmetic never raises: it follows IEEE 754, so `1.0 / 0.0` is infinity and `0.0 / 0.0` is NaN.

---

[← Previous: Control Flow](./13-control-flow.md) | [Index](./README.md) | [Next: Classes and Objects](./15-classes-and-objects.md) →
