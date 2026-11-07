# 05. Apply-Blocks

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


**Apply-blocks are a core language feature where `identifier:` applies that identifier to each indented item.**

### Function Calls

Apply-blocks work with any function or method that takes a single argument. Note that `print:` — while it looks like an apply-block — is a **separate syntactic construct** with its own semantic rule (see [Semantic Rules SYN008](../../spec/semantic-rules.md) and the [Standard Library — Console I/O](../../spec/stdlib-reference.md) section). A `print:` block prints each indented value one per line; it is not a general apply-block target. Example:

```clean
items.add:
	item1
	item2
	item3
// Equivalent to: items.add(item1), items.add(item2), items.add(item3)
```

For printing multiple values, use individual `print()` calls (each prints its own line):

```clean
print("First line")
print(variable_name)
print(result.toString())
```

### Variable Declarations
```clean
integer:
    count = 0
    maxSize = 100
    currentIndex = -1
// Equivalent to: integer count = 0, integer maxSize = 100, integer currentIndex = -1

string:
    name = "Alice"
    version = "1.0"
// Equivalent to: string name = "Alice", string version = "1.0"
```

### Constants
```clean
constant:
    integer MAX_SIZE = 100
    number PI = 3.14159
    string VERSION = "1.0.0"
```

---

[← Previous: Type System](./04-type-system.md) | [Index](./README.md) | [Next: Expressions](./06-expressions.md) →
