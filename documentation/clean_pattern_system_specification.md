# Clean Pattern System – Specification

## 1. Goals and Overview

The **Clean Pattern System** is a readable, explicit way to describe text patterns in Clean Language. It is designed to be:

- **Simple and friendly** – no cryptic symbols like `^`, `$`, `\w`, `\d{2,4}`.
- **Clean-style** – uses indentation, words, and blocks instead of dense punctuation.
- **Safe and predictable** – patterns are explicit and easy to review.
- **Composable** – patterns can be reused, combined, and tested.

Internally, patterns are compiled into an efficient pattern program (for example, a regex or NFA engine), but this is hidden from the developer.

The pattern system is part of the **standard library and compiler front-end**, not part of the core expression syntax. It is expressed with a dedicated top-level block: `patterns:`.

---

## 2. Top-Level Syntax

### 2.1 `patterns:` Block

Patterns are declared inside a top-level `patterns:` block, similar to `functions:` and `tests:`.

```clean
patterns:
    pattern Email
        // pattern body lines here

    pattern PhoneNumber
        // another pattern
```

A file can contain multiple blocks (`import:`, `functions:`, `patterns:`, `tests:`, etc.). Order is not semantically important, but typical style is:

```clean
import:
    string
    pattern

patterns:
    pattern Email
        // ...

functions:
    boolean isValidEmail(string value)
        return Email.matches(value)
```

### 2.2 Pattern Declaration

Each pattern inside `patterns:` is declared with:

```clean
pattern PatternName
    pattern-commands...
```

- `PatternName` must start with an uppercase letter and follow normal Clean identifier rules.
- The body is a **sequence** of pattern commands. They are matched in order.

A minimal pattern:

```clean
patterns:
    pattern Hello
        text "hello"
```

---

## 3. Pattern Command Language

A pattern body is a sequence of **pattern commands**. Each command describes a piece of text to match, or a structure (optional/choice/repeat/capture).

Patterns always match from **left to right** over the input.

### 3.1 Literal Text

```clean
text "hello"
text ":"
text "@"
```

- Matches the exact text (case-sensitive) given between quotes.
- Quotes use the same rules as Clean string literals.

### 3.2 Character Classes

#### 3.2.1 Single-Character Classes

```clean
digit
letter
wordChar
whitespace
anyChar
startOfLine
endOfLine
```

- `digit` – one decimal digit `0`–`9`.
- `letter` – one alphabetic character (locale-agnostic; typically `A`–`Z`, `a`–`z`).
- `wordChar` – alphanumeric or underscore.
- `whitespace` – spaces, tabs, or other whitespace.
- `anyChar` – any single character (except line break, unless specified by implementation).
- `startOfLine` – anchor at the beginning of the input (does not consume a character).
- `endOfLine` – anchor at the end of the input (does not consume a character).

Example:

```clean
pattern HexPrefix
    text "0x"
```

#### 3.2.2 Repeated Character Classes (shortcuts)

For convenience, Clean Patterns define short commands for repeated classes:

```clean
digits 3
digits 2 to 4
letters 1 to 10
whitespaces 1 to 3
```

- `digits N` – match exactly `N` digits.
- `digits A to B` – match between `A` and `B` digits.
- `letters N` / `letters A to B` – same rule for letters.
- `whitespaces N` / `whitespaces A to B` – same for whitespace.

These are syntactic sugar that desugar to `repeat` blocks over `digit`, `letter`, or `whitespace`.

### 3.3 Character Sets and Ranges

```clean
charIn "abc123"
charNotIn "aeiou"
charRange "a" to "z"
```

- `charIn` – match any **one** character from the given set.
- `charNotIn` – match any **one** character that is **not** in the set.
- `charRange` – match any character in the inclusive range.

Example:

```clean
pattern LowercaseLetter
    charRange "a" to "z"
```

### 3.4 Sequences

A pattern that lists several commands in order creates a **sequence**. All commands must match in order.

```clean
pattern PhoneNumberSimple
    digits 3
    text "-"
    digits 4
```

This matches text like `123-4567`.

### 3.5 Optional Blocks

Optional parts are declared with an `optional:` block.

```clean
optional:
    whitespace
    text "ext"
    whitespace
    digits 1 to 4
```

Semantics:
- The entire block can be present **zero or one** times.
- If the block does not match, the engine continues after the optional block.

Example pattern with optional extension:

```clean
pattern PhoneNumberWithExt
    digits 3
    text "-"
    digits 4

    optional:
        whitespace
        text "ext"
        whitespace
        digits 1 to 4
```

### 3.6 Repetition Blocks

For repeating a sub-pattern, use `repeat` blocks.

```clean
repeat exactly 3:
    digit

repeat 2 to 4:
    letter

repeat 1 to many:
    wordChar
```

- `repeat exactly N:` – repeat the inner block exactly `N` times.
- `repeat A to B:` – repeat between `A` and `B` times.
- `repeat 0 to many:` – zero or more times.
- `repeat 1 to many:` – one or more times.

Example:

```clean
pattern Word
    repeat 1 to many:
        wordChar
```

### 3.7 Choice Blocks (Alternation)

Use `choice:` to express alternatives.

```clean
choice:
    option:
        text "yes"
    option:
        text "no"
```

Semantics:
- The pattern engine tries each `option` in order.
- The first option that matches is chosen.

Example:

```clean
pattern YesOrNo
    startOfLine

    choice:
        option:
            text "yes"
        option:
            text "no"

    endOfLine
```

### 3.8 Named Captures

Use `capture` to name parts of the match.

```clean
capture "areaCode":
    digits 3
```

Syntax:

```clean
capture "name":
    pattern-commands...
```

Example:

```clean
pattern PhoneNumberParts
    capture "areaCode":
        digits 3

    text "-"

    capture "local":
        digits 4
```

At runtime, the match result can expose `areaCode` and `local` as captured strings.

### 3.9 Pattern References

A pattern can reuse another pattern with `use`.

```clean
use EmailLocalPart
use EmailDomain
```

Example:

```clean
pattern EmailLocalPart
    repeat 1 to many:
        wordChar

pattern EmailDomain
    repeat 1 to many:
        wordChar
    text "."
    letters 2 to 4

pattern Email
    use EmailLocalPart
    text "@"
    use EmailDomain
```

- `use PatternName` inlines the referenced pattern as part of the current sequence.
- Recursive patterns are allowed but must be handled with care by the implementation.

### 3.10 Comments in Patterns

You can use line comments inside pattern bodies:

```clean
pattern Price
    text "$"     // currency
    digits 1 to 6 // amount
```

---

## 4. Runtime API

Each pattern declared in a `patterns:` block becomes a **Pattern object** accessible by name in the same file.

The pattern runtime API is available through a `pattern` module (or similar standard library namespace).

### 4.1 Core Types

Suggested core types:

```clean
// Conceptual type names, not syntax
Pattern
PatternMatch
PatternMatchList
```

### 4.2 Methods on Patterns

For a pattern named `Email`:

```clean
Email.matches(text: string) -> boolean
Email.firstMatch(text: string) -> PatternMatch
Email.allMatches(text: string) -> PatternMatchList
Email.replace(text: string, replacement: string) -> string
Email.split(text: string) -> list<string>
```

Example usage:

```clean
functions:
    boolean isValidEmail(string value)
        return Email.matches(value)

    string extractFirstEmail(string value)
        PatternMatch match = Email.firstMatch(value)
        if match.found
            return match.full
        else
            return ""
```

### 4.3 `PatternMatch` Type

`PatternMatch` provides information about one successful match.

Fields (conceptual):

```clean
boolean found      // true if a match exists
string full        // full matched text
integer startIndex // start position in original text
integer endIndex   // end position in original text

// capture access
boolean hasCapture(string name)
string capture(string name)
```

Example:

```clean
functions:
    string getAreaCode(string value)
        PatternMatch match = PhoneNumberParts.firstMatch(value)

        if match.found and match.hasCapture("areaCode")
            return match.capture("areaCode")
        else
            return ""
```

### 4.4 `PatternMatchList` Type

A list-like wrapper over multiple matches.

Conceptual API:

```clean
integer size()
PatternMatch at(integer index)
```

Or simply **aliased as** `list<PatternMatch>` depending on standard library design.

---

## 5. Integration with Clean Language

### 5.1 Where Patterns Live

- Patterns are part of the **front-end** and standard library.
- `patterns:` is a top-level block parsed by the compiler, similar to `functions:` and `tests:`.
- The compiler transforms pattern definitions into:
  - A pattern AST, then
  - A compiled pattern program stored in constant data, and
  - A `Pattern` value accessible by name.

### 5.2 Compilation Phases

1. **Parse Phase**
   - Recognizes `patterns:` blocks.
   - Builds a **Pattern AST** for each `pattern` declaration.

2. **Pattern Lowering Phase**
   - Converts the Pattern AST into a **PatternProgram** (an internal representation for the pattern engine).
   - Generates a constant `Pattern` object for each pattern.

3. **Type Checking**
   - Registers each `PatternName` as a value of type `Pattern` in the module scope.
   - Ensures pattern usage in functions matches the expected API.

4. **Code Generation**
   - Emits the PatternProgram as data in the compiled module.
   - Compiles calls like `Email.matches(value)` into calls to the pattern runtime.

---

## 6. Pattern AST Design

The Pattern AST represents pattern commands in a structured form.

### 6.1 High-Level Nodes

At the highest level, each `pattern` has:

```text
PatternDecl
    name: Identifier
    body: PatternExpr
```

`PatternExpr` covers all constructs.

### 6.2 `PatternExpr` Variants

Conceptually, the AST can be expressed as:

```text
PatternExpr
    = Sequence(list<PatternExpr>)
    | Choice(list<PatternExpr>)         // list of alternative branches
    | Repeat(RepeatKind, PatternExpr)   // quantifiers
    | Optional(PatternExpr)
    | Capture(name: string, PatternExpr)
    | Reference(name: string)           // use PatternName
    | Atom(AtomKind)
```

### 6.3 `AtomKind` Variants

```text
AtomKind
    = Text(value: string)
    | Digit
    | Letter
    | WordChar
    | Whitespace
    | AnyChar
    | StartOfLine
    | EndOfLine
    | CharIn(set: string)
    | CharNotIn(set: string)
    | CharRange(from: char, to: char)
```

### 6.4 `RepeatKind`

```text
RepeatKind
    = Exact(count: integer)
    | Range(min: integer, max: integer)
    | AtLeast(min: integer)      // used for "1 to many"
    | Any                        // used for "0 to many"
```

### 6.5 Desugaring Rules

Several surface constructs are syntactic sugar:

- `digits N` → `Repeat(Exact(N), Atom(Digit))`
- `digits A to B` → `Repeat(Range(A, B), Atom(Digit))`
- `letters N` → `Repeat(Exact(N), Atom(Letter))`
- `whitespaces N` → `Repeat(Exact(N), Atom(Whitespace))`

- `optional:` block → `Optional(Sequence(...))`
- `repeat exactly N:` block → `Repeat(Exact(N), Sequence(...))`
- `repeat A to B:` block → `Repeat(Range(A, B), Sequence(...))`
- `repeat 1 to many:` block → `Repeat(AtLeast(1), Sequence(...))`
- `repeat 0 to many:` block → `Repeat(Any, Sequence(...))`

- `choice:` block with options → `Choice(list<Sequence(...)>)`

This design keeps the internal representation minimal and expressive.

---

## 7. Pattern Engine Architecture

The pattern engine is the runtime component that executes compiled patterns.

### 7.1 Components

- **Pattern AST** – direct representation of `patterns:` source.
- **PatternProgram** – compiled representation used at runtime.
- **PatternRuntime** – functions that run a PatternProgram against input text.

### 7.2 Compilation Pipeline

1. Build `PatternAST` from pattern declarations.
2. Validate patterns:
   - Report errors for impossible ranges, empty ranges, invalid references.
   - Detect simple left recursion, if recursive patterns are allowed.
3. Lower `PatternAST` to `PatternProgram`:
   - Typically an NFA, DFA, or a bytecode-like instruction list.
4. Store `PatternProgram` in a constant `Pattern` object.

### 7.3 Runtime Execution

Given a `Pattern` and an input text:

- `Pattern.matches(text)` runs a full-match check.
- `Pattern.firstMatch(text)` runs a search and returns the first `PatternMatch`.
- `Pattern.allMatches(text)` iterates over input and collects matches.
- `Pattern.replace(text, replacement)` replaces matches with the given string (implementation may support capture substitution later).

Performance details (DFA/NFA, backtracking) are implementation-specific and not part of the surface specification.

---

## 8. Errors and Diagnostics

The compiler should provide clear, friendly error messages for pattern issues.

Examples:

- Unknown pattern reference:

  ```text
  Error: Unknown pattern "EmailDomain" used in pattern "Email".
  ```

- Invalid range:

  ```text
  Error: Invalid digits range "digits 5 to 2" in pattern "Code". Min must be <= max.
  ```

- Conflicting names:

  ```text
  Error: Duplicate pattern name "Email" in patterns block.
  ```

- Capture name mismatch:

  ```text
  Warning: Capture "areaCode" is declared but never used.
  ```

---

## 9. Style Guidelines

To keep patterns easy to read:

- Use one logical step per line.
- Use `optional:` and `choice:` instead of clever tricks.
- Prefer named captures for parts that will be used in code.
- Avoid deeply nested patterns; factor them into named patterns and `use` them.

Example of a clear pattern:

```clean
patterns:
    pattern Price
        startOfLine
        text "$"
        capture "amount":
            digits 1 to 6
        optional:
            text "."
            digits 1 to 2
        endOfLine
```

---

## 10. Summary

- The Clean Pattern System provides a high-level, **explicit** way to specify text patterns.
- It uses friendly commands (`text`, `digit`, `optional`, `choice`, `capture`) instead of dense regex syntax.
- Patterns live in a dedicated `patterns:` block and compile down to a compact PatternProgram.
- At runtime, patterns expose a small, powerful API for matching and extraction.
- The AST and architecture keep the system simple, extensible, and aligned with Clean Language design principles.

