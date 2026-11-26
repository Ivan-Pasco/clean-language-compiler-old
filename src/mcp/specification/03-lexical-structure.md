# 03. Lexical Structure

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


### Comments

```clean
// Single line comment

/* 
   Multi-line
   comment
*/
```

### Whitespace and Indentation

Clean Language uses **tab-based indentation** for code structure:

- **Indentation**: Uses tabs only. Each tab represents one block level
- **Spaces**: May be used within expressions for alignment and formatting, but not for indentation
- **Block Structure**: Indentation defines code blocks (no braces `{}`)
- **Whitespace**: Includes spaces, tabs, carriage returns, and newlines

**Example:**
```clean
start:
⇥⇥⇥⇥integer x = 5    // Tab indentation
⇥⇥⇥⇥if x > 0
⇥⇥⇥⇥⇥⇥⇥⇥print("positive")    // Nested tab indentation
⇥⇥⇥⇥else
⇥⇥⇥⇥⇥⇥⇥⇥print("zero or negative")
```

**Indentation Rules:**
- Each indentation level must use exactly one tab character
- Mixing tabs and spaces for indentation is not allowed
- Spaces within expressions are permitted for readability:
  ```clean
  result = function(arg1,  arg2,  arg3)    // Spaces for alignment
  value  = x + y                           // Spaces around operators
  ```

### Identifiers

Identifiers must:
- Start with a letter (`A-Z`, `a-z`) or an underscore (`_`)
- Contain only letters, digits, and underscores
- Follow camelCase conventions (e.g. `myVariable`, `calculateSum`)

**Valid Examples:**
```clean
x
count
myVariable
value1
calculateSum
_count
```

A leading underscore is allowed but has no special meaning: `_count` is an ordinary name.

**Invalid Examples:**
```clean
1value      // Cannot start with digit
my-var      // Hyphens not allowed
$name       // Special characters not allowed
```

### Keywords

Clean Language has one keyword list, in two parts: **reserved** keywords and **contextual** keywords (below). It is the list the compiler reserves, and it matches `foundation/spec/grammar/grammar.ebnf` §1.2.

Reserved keywords cannot be used as variable, parameter, function or class names:

```
always      and         background  break       can         class
constant    constructor continue    default     else        ensure
false       for         from        function    functions   if
import      in          is          iterate     later       none
not         null        onError     or          plugins     print
public      require     return      returns     start       tests
this        to          true        while
```

Five of them — `and`, `or`, `not`, `start` and `can` — may also name a function, method or parameter (`counter.start()`, `auth.can("posts.write")`), and `from` and `to` may name a method.

#### Contextual Keywords

The following keywords are **contextual** — they have their keyword meaning only at the start of their own construct (`state:`, `rules:`, `watch count:`, `iterate i in 1 to 9 step 2`, `reset state`, `test "name"`, `error("...")`) and may be used as variable names, function parameters, fields and in expressions everywhere else:

```
build       computed    description error       guard       input
intent      reset       rules       screen      source      spec
state       step        test        unit        watch
```

This allows code like `string rules = ""` while still recognizing `rules:` as a block header. A state variable name must be an ordinary identifier, not a contextual keyword.

The following words are not keywords at all. They have a meaning in one position only and are ordinary names everywhere else: the type names (`integer`, `number`, `string`, `boolean`, `void`, `any`, `list`, `matrix`, `pairs`, `handler`), `then` (in `if … then … else`), `as` (in `import:`), `validate` (a `validate Name:` block), `base` (`base(...)`, `base.method()`), `private`, `external`, `inclusive`, `line`, `unique`, `pile`, `ok`, `check` and `messages`.

#### Keywords as Member Names

After `.`, every keyword — reserved, contextual or not yet used — is an ordinary name. A method, field or plugin function may be named `require`, `type`, `state`, `start` and so on:

```clean
string tenant = auth.tenant.require()
string kind = item.type
counter.start()
```

Outside member access the reserved keywords remain reserved: `integer require = 1` is still an error.

#### Reserved Keywords (not yet used)

The following reserved keywords currently have no meaning in Clean Language, but are reserved so that adding them later does not break existing code:

```
for        this       returns    function
```

- `for` — reserved as an alternate loop keyword. Use `iterate` today (`for` is SYN002).
- `this` — not used: class methods refer to their own fields and methods by bare name. Writing `this` (`this.x`, `this.m()`, or bare `this`) is SYN002.
- `returns` — reserved for possible arrow-style function signatures.
- `function` — functions are declared in a `functions:` block; `function name(...)` is SYN002.

Using any of these names as an identifier is a compile-time error. `unit` has no meaning either, but it is a contextual keyword, so it may be used as a name.

### Literals

#### Numeric Literals

**Integers:**
```clean
42          // Decimal
-17         // Negative decimal
0xff        // Hexadecimal
0b1010      // Binary
0o777       // Octal
```

**Floating-Point:**
```clean
3.14        // Standard decimal
.5          // Leading zero optional
6.02e23     // Scientific notation
-2.5        // Negative number
```

#### String Literals

**Basic Strings:**
```clean
"Hello, World!"
"Line 1\nLine 2"
""          // Empty string
```

**Escape Sequences:**

Clean Language supports standard escape sequences within string literals:

| Sequence | Result | Example |
|----------|--------|---------|
| `\"` | Double quote | `"say \"hi\""` → `say "hi"` |
| `\\` | Backslash | `"path\\file"` → `path\file` |
| `\n` | Newline | `"line1\nline2"` → two lines |
| `\t` | Tab | `"col1\tcol2"` → tab-separated |
| `\r` | Carriage return | `"text\r"` → with CR |
| `\{` | Literal left brace | `"\{not interpolation\}"` → `{not interpolation}` |
| `\}` | Literal right brace | `"\{literal\}"` → `{literal}` |
| `\0` | Null character | `"text\0"` → null-terminated |

**Escape Sequences in JSON Strings:**

Escape sequences are particularly useful when working with JSON:

```clean
// JSON with escaped quotes
string jsonStr = "{\"name\": \"Alice\", \"age\": 25}"

// Parse JSON containing escape sequences
any data = json.textToData("{\"count\": 42}")
integer count = data.count  // Returns 42

// Nested JSON with multiple escape sequences
string nestedJson = "{\"user\": {\"name\": \"Bob\", \"active\": true}}"
any parsed = json.textToData(nestedJson)
string userName = parsed.user.name  // Returns "Bob"
```

**String Interpolation:**
```clean
name = "World"
greeting = "Hello, {name}!"     // Results in "Hello, World!"

// Simple property access allowed
user = User("Alice", 25)
message = "User {user.name} is {user.age} years old"

// Note: Complex method calls in strings are not supported
// ❌ "Hello {user.name}, you have {messages.count()} messages"
```

**Interpolation vs Literal Braces:**

The compiler distinguishes between interpolation and literal braces:
- `"{variable}"` → Interpolation (evaluates `variable`)
- `"{obj.prop}"` → Interpolation (evaluates `obj.prop`)
- `"{(expr)}"` → Interpolation (evaluates expression)
- `"{\"literal\"}"` → NOT interpolation (produces `{"literal"}`)
- `"\{literal\}"` → NOT interpolation (produces `{literal}`)

#### Boolean Literals
```clean
true
false
```

#### None Literal

The `none` value represents the absence of a value. It is distinct from `0`, `false`, or empty string `""`.

```clean
none        // The empty value (standard spelling)
null        // Same value, accepted alias
```

Write `none` for no value. `null` is an accepted alias and means exactly the same thing, so code written with `null` keeps working.

**None Semantics:**
- `none` is a value, not a type name: you cannot declare a variable of type `none`. It can be stored only in a `T?` (or an `any`) — every plain type rejects it (see Type System, Nullable Types)
- `none == none` is `true`
- `none == anything_else` is `false` (except for another none)
- Use the `default` operator to provide a fallback value for none
- Use the `!` operator to assert a value is not none

#### List Literals
```clean
[1, 2, 3, 4]           // Integer list
["a", "b", "c"]        // String list
[]                     // Empty list
[true, false, true]    // Boolean list
```

#### Matrix Literals
```clean
[[1, 2], [3, 4]]                    // 2x2 matrix
[[1, 2, 3], [4, 5, 6], [7, 8, 9]]   // 3x3 matrix
[[]]                                // Empty matrix
```

#### Pairs Literals

A pairs literal lists `key: value` entries inside braces. It makes a `pairs<K, V>` value.

```clean
{"a": 1, "b": 2}       // string keys
{name: "Ada", age: 36} // a bare name is the key text: same as {"name": "Ada", "age": 36}
{1: "one", 2: "two"}   // integer keys
{}                     // empty pairs
```

- A bare name before `:` is the key's text, not a variable: `{name: 1}` has the key `"name"` even when a variable `name` exists.
- Integer keys are stored as text: `{1: "x"}` prints as `{"1":"x"}`.

```clean
start:
	pairs<string, integer> ages = {"ada": 36, "bo": 7}
	print(ages)            // {"ada":36,"bo":7}
```

---

[← Previous: Language Design Rules](./02-language-design-rules.md) | [Index](./README.md) | [Next: Type System](./04-type-system.md) →
