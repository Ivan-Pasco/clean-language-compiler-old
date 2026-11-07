# 02. Language Design Rules

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


These are core design rules that define how Clean Language code must be structured:

1. **Functions must be in a `functions:` block**
   - No standalone `function name(...)` at top level
   - Use `functions:` for top-level and class functions
   - Entry point uses `start:` block (not a function)
   ```clean
   // ❌ Invalid
   function myFunc()
       return 42

   // ✅ Valid
   functions:
       integer myFunc()
           return 42
   ```

2. **Helper methods require parentheses**
   - ✅ `x.toString()`
   - ❌ `x.toString`
   ```clean
   value = 42
   text = value.toString()  // ✅ Correct
   ```

3. **Use `any` when the type cannot be known at compile time**
   - `any` is a **compile-time escape hatch** — the compiler trusts that the developer knows the type is correct, similar to TypeScript's `any`.
   - Use `any` when the type genuinely cannot be known at compile time (plugin returns, JSON parsing, external data).
   - There is no runtime boxing or type tag — the compiler simply skips type checking for that value.
   - For type-safe collections, use explicit type parameters: `list<integer>`, `list<string>`, etc.
   ```clean
   functions:
       any identity(any value)
           return value
   ```

4. **Use `functions:` inside `class`**
   - All class methods go inside a `functions:` block
   ```clean
   class MyClass
       integer value
       
       functions:
           void setValue(integer newValue)
               value = newValue
   ```

5. **Use lowercase namespace functions**
   - Use `math.sqrt()`, `string.concat()`, `list.concat()` — not `Math.sqrt()`, `String.concat()`
   - Uppercase namespace names are not valid in Clean Language

6. **Generic containers use angle-bracket syntax; user-defined generics are not allowed**
   - ✅ `list<integer>`, `matrix<number>`, `pairs<string, any>` — the built-in generic containers take a type parameter in angle brackets.
   - ❌ You cannot declare your own generic classes or functions. Angle brackets in user code are reserved for the built-in generic containers; there is no `class MyBox<T>` form.
   - Use `any` (see Rule 7) when you need to hold a value of unknown type.

7. **`any` is a compile-time generic escape hatch**
   - `any` tells the compiler to skip type checking for that value — the developer takes responsibility for correctness.
   - Use `any` only when the type genuinely cannot be known at compile time (plugin returns, JSON parsing, external data).
   - There is no runtime boxing or type tag involved — `any` is purely a compile-time concept.
   - For type-safe collections, use `list<integer>`, `list<string>`, etc. — use `list<any>` only when you genuinely need a heterogeneous collection.

8. **One way to do things**
   - Basic math: Use operators (`a + b`, `a * b`)
   - Comparisons: Use operators (`a == b`, `a > b`, `a >= b`)
   - Logic: Use operators (`a and b`, `a or b`, `not a`)
   - Advanced math: Use functions (`math.sqrt()`, `math.sin()`)
   - Object operations: Use method-style (`text.length()`, `value.toString()`, `items.add(x)`)
   - Utility functions: Use namespace calls (`string.concat()`, `list.concat()`, `list.range()`)

---

[← Previous: Overview](./01-overview.md) | [Index](./README.md) | [Next: Lexical Structure](./03-lexical-structure.md) →
