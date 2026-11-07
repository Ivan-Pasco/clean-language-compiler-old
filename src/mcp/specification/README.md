# Clean Language Specification

**Authority:** This folder is THE single source of truth for the Clean Language. Everything else in `foundation/` complements what is defined here.

**Version:** Comprehensive Specification (see repository history for the current revision — the source file does not carry an explicit version number)

The specification is split into focused sections, one per file. Read in order for the language manual, or jump to a specific topic.

## Table of Contents

1. [Overview](./01-overview.md) — what Clean Language is and its design goals
2. [Language Design Rules](./02-language-design-rules.md) — the "one way to do things" rules
3. [Lexical Structure](./03-lexical-structure.md) — tokens, keywords, whitespace
4. [Type System](./04-type-system.md) — primitive and composite types, conversions
5. [Apply-Blocks](./05-apply-blocks.md) — the apply-block DSL
6. [Expressions](./06-expressions.md) — arithmetic, logical, and comparison expressions
7. [Statements](./07-statements.md) — variable declarations, assignments, blocks
8. [File Structure](./08-file-structure.md) — how a .cln file is organized
9. [Build Configuration](./09-build-configuration.md) — main.cln
10. [Functions](./10-functions.md) — declaration, parameters, return values
11. [Contracts](./11-contracts.md) — `require` and `ensure`
12. [Testing](./12-testing.md) — the `tests:` block
13. [Control Flow](./13-control-flow.md) — if, iterate, while
14. [Error Handling](./14-error-handling.md) — onError and error propagation
15. [Classes and Objects](./15-classes-and-objects.md) — class syntax, inheritance, methods
16. [Standard Library](./16-standard-library.md) — built-in functions
17. [Method-Style Syntax](./17-method-style-syntax.md) — when to use method vs. namespace calls
18. [Modules and Imports](./18-modules-and-imports.md) — file-level imports
19. [Asynchronous Programming](./19-async.md) — start, later, background
20. [Memory Management](./20-memory-management.md) — ARC-based memory model
21. [AI Integration](./21-ai-integration.md) — spec, intent, source: keywords
22. [Plugin System](./22-plugin-system.md) — how plugins extend the language
23. [State Management](./23-state-management.md) — state: blocks

## How to change the specification

Modifying the specification requires developer approval. See `foundation/README.md` for the change protocol.
