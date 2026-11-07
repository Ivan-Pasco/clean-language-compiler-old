# 17. Method-Style Syntax

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Clean Language follows the "one way to do things" principle. Every operation has exactly one name and one preferred calling style.

### Method Style — The Primary Pattern

When an operation acts on a specific value, use method-style syntax. The value comes first, followed by a dot and the operation name:

```clean
// String operations
string text = "Hello World"
integer length = text.length()
string upper = text.toUpperCase()
string lower = text.toLowerCase()
string trimmed = text.trim()
boolean found = text.contains("World")
list<string> words = text.split(" ")
string cleaned = text.replace("Hello", "Hi")

// List operations
list<integer> numbers = [1, 2, 3]
integer count = numbers.length()
boolean empty = numbers.isEmpty()
numbers.add(4)
numbers.remove(0)
boolean has = numbers.contains(2)
list<integer> sorted = numbers.sort()

// Value conversions
integer age = 25
string ageText = age.toString()
number decimal = age.toNumber()

// Object properties
user.name
user.age
user.toString()
```

**Test:** Can I read the call as "*subject-verb-object*" where the value is the subject? Then it is method style.
- `text.length()` — "text has length"
- `numbers.add(4)` — "numbers add 4"
- `user.toString()` — "user converts to string"

### Namespace Functions — For Utilities Only

Namespace functions are used only when an operation does not belong to a single value — typically utility functions with multiple independent inputs:

```clean
// Math utilities (no single owner)
math.sqrt(16)
math.max(10, 20)
math.abs(-5)

// Creating new collections
list.concat(listA, listB)
list.range(1, 10)
list.fill(5, 0)

// Joining (utility taking a list + separator)
string.concat("Hello", " World")
list.join(words, ", ")
string.join(parts, "-")
```

**Test:** Do I have to pick a "subject"? If the operation reads naturally as `module.action(inputs)` without any input being privileged, it is namespace style.
- `math.max(10, 20)` — neither number is the "subject"
- `list.range(1, 10)` — creates a new list from two integers, no existing list to be the subject
- `list.concat(a, b)` — combines two peers into a third

### The Rule

- If the operation acts **on a value** → use method style: `value.operation()`
- If the operation **creates something new** from multiple inputs → use namespace: `module.operation(a, b)`
- Every operation has **one name** — no aliases, no shortcuts, no alternate forms

### Why This Rule Exists

**One name, one call site.** Without this rule, a language accumulates duplicates: `concat(a, b)`, `a.concat(b)`, `String.concat(a, b)`, `a + b`. Clean Language picks one form for each operation and forbids the others. This makes code:

- **Uniform to read** — you know how to spell any operation without checking docs.
- **Uniform to search** — grepping for `.length()` finds every length check in the codebase.
- **Uniform for tooling** — the LSP has one signature to autocomplete, not three.

**Exponentiation is `^`, not `math.pow`.** Basic arithmetic uses operators (`+`, `-`, `*`, `/`, `^`, `%`); advanced math uses `math.` functions; there is no `math.add`, `math.multiply`, or `math.pow`. See `foundation/docs/Clean Language Specification.md` §Language Design Rules → Rule 8.

### Quick Reference

| Category | Style | Example |
|----------|-------|---------|
| String operations on a string value | method | `text.length()`, `text.toUpperCase()` |
| List operations on a list value | method | `items.add(x)`, `items.length()` |
| Value conversion | method | `age.toString()`, `text.toNumber()` |
| Object property / method | method | `user.name`, `user.deposit(100)` |
| Math functions | namespace | `math.sqrt(x)`, `math.max(a, b)` |
| Basic arithmetic | operator | `a + b`, `a * b`, `a ^ b` |
| List/string creation utilities | namespace | `list.range(1, 10)`, `list.fill(5, 0)` |
| Joining a list into a string | namespace | `list.join(items, sep)`, `string.join(parts, sep)` |

---

[← Previous: Standard Library](./16-standard-library.md) | [Index](./README.md) | [Next: Modules and Imports](./18-modules-and-imports.md) →
