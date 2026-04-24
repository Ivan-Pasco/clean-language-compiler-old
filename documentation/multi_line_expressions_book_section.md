# Multi-Line Expressions — Chapter 2 Insertion

## Placement

Insert as a new numbered subsection **4. Multi-Line Expressions** inside Chapter 2's Explanation block, immediately after **3. Reassignment** and before the **Code Example** heading.

Also add one entry to Chapter 2's existing **Common Mistakes** section (shown at the bottom of this file).

---

## New subsection to add

### 4. Multi-Line Expressions

When an expression is too long for one line, wrap it in parentheses and continue on the next line. The compiler treats everything inside the matching parentheses as a single expression, so you can break the line wherever reads best.

```clean
// Single line — no parentheses needed
integer total = price + tax + shipping

// Multi-line — parentheses required
integer total = (price + tax +
                 shipping + handling)
```

You can break after any operator, inside a function call, or around a nested expression. Every `(` must have a matching `)` — unmatched parentheses produce a compilation error.

---

## Addition to the existing "Common Mistakes" section

Append this entry alongside the other Common Mistakes already in Chapter 2:

**Forgetting parentheses on a multi-line expression**

```clean
// ❌ The second line is read as a new statement
integer total = price + tax +
                shipping

// ✅ Wrap the whole expression in parentheses
integer total = (price + tax +
                 shipping)
```

---

## Notes for the editor

The subsection is intentionally short to match the rhythm of the other numbered items in the Explanation block (each is roughly one short paragraph plus a code block). The "why" is left implicit — if you want a slightly fuller treatment, a one-sentence Deep Dive entry would fit naturally:

> **Deep Dive — Multi-Line Parsing.** Clean treats each line as a complete statement by default. Parentheses tell the compiler "keep reading until this is balanced," which is how a multi-line expression stays a single expression instead of two broken ones.

Chapter 3 later uses `(first + " " + last).toUpperCase()` — parentheses for grouping before a method call, not the multi-line syntax. The two uses don't conflict, but worth being aware of when editing.
