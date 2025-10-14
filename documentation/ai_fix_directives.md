# Clean Language Compiler – AI Fix Directives

This document captures the concrete fixes we need you to implement in the next repair pass. Each section references the current problem spots and outlines the work required to close the gaps.

## Parser Remediation
- **Restore full if/else chains**  
  The token-driven parser drops trailing `else` clauses because `parse_if` returns while a `Dedent` token is still pending. Update `src/parser/token_parser.rs:1053` so that it (a) consumes the `Dedent` that terminates the `then` block when another clause follows, and (b) recognises `else if` by recursively building nested `Statement::If` nodes instead of forcing callers to treat the second `if` as top-level. Adjust `parse_block` to yield the dedent level it consumes so `parse_if` can decide when to look for `else`, and add tests covering `if/else`, `if/elif/else`, and nested chains.

- **Implement `iterate` syntax**  
  `parse_statement` never inspects `TokenKind::Iterate`; everything uses the temporary `for` handler. Add a dedicated `parse_iterate` helper in `src/parser/token_parser.rs` that understands:
  - `iterate item in collection:` (lowering to `Statement::Iterate`)
  - `iterate i in start..end [step stepExpr]:` (lowering to `Statement::RangeIterate`)
  Record iterator names and block contents with location information just like `parse_for`.

- **Add range expression support**  
  When parsing `0..10` or `0..=10` we currently raise syntax errors because no stage recognises `TokenKind::Range` / `RangeInclusive`. Introduce a `parse_range_expression` stage invoked ahead of additive parsing so the AST emits `Expression::Range { start, end, inclusive }`. Ensure both the pest-based and token parsers stay consistent.

- **Enable default parameters**  
  Parameters always return `default_value: None` (`src/parser/token_parser.rs:769`). Extend `parse_parameter` to accept an optional `=` followed by a full expression, capturing it in the `Parameter` node. Follow through in the resolver/type-checker so defaulted parameters are inserted when call sites omit trailing arguments.

## Code Generation Fixes
- **Produce valid `while` loops**  
  `generate_while_statement` (`src/codegen/statement_generator.rs:439`) emits a lone `Loop` with `BrIf(1)`, which invalidates the module because there is no enclosing label. Wrap the loop with an outer `Block`, branch to label `0` to exit, and reset the stack so validation passes.

- **Rework `iterate` lowering**  
  The current implementation (`src/codegen/statement_generator.rs:584`) leaves the iterable value on the stack, assumes the collection is a bare variable, and punts on range loops. Refactor to:
  1. Evaluate the iterable once, stash it in a dedicated local, and use that local for both length queries and element loads.
  2. Support arbitrary collection expressions (method results, literals, etc.) by relying on that local.
  3. Implement `Statement::RangeIterate` in `generate_range_iterate_statement` by materialising a counter loop that respects optional `step`.
  4. Remove the dummy `Expression::Literal(0)` placeholder and ensure the stack is balanced at every branch/loop exit.

- **Fix method chaining emission**  
  `generate_custom_method_call` (`src/codegen/expression_generator.rs:942`) re-evaluates the object, leaves stale values on the stack, and rejects non-variable receivers via `get_object_class`. Update this path to:
  - Evaluate the receiver once, store it in a temporary local, and reuse it after argument evaluation so the call stack matches `(this, args…)`.
  - Extend `get_object_class` to accept `Expression::MethodCall`, `Expression::Call`, and property accesses by looking up the return type via `self.class_table` / method signatures.
  - Return the real WASM type by consulting the located `Function.return_type`, so downstream chaining inherits accurate type information.
  - Expand `infer_expression_type` (`src/codegen/statement_generator.rs:753`) to infer types for method calls using the same metadata, allowing chained calls in loop headers and assignments.

- **Emit range values**  
  Add handling for `Expression::Range` inside `generate_expression` so range literals compile to a structure the loop lowering can iterate over (e.g., allocate a small runtime descriptor or inline the start/end immediates). Ensure inclusive vs. exclusive bounds map to the correct loop condition.

## Testing & Tooling
- **Repair the comprehensive test harness**  
  `scripts/comprehensive_test.sh` (lines 49–70) marks compilation as “success” even if no `.wasm` file is produced, and fails outright when `timeout` is unavailable. Treat a missing output artifact as a compilation failure (including failure counts and JSON status), and fall back to a plain runner invocation when `timeout` is not present.

- **Regression coverage**  
  Introduce focused integration tests under `tests/clean_files/` that exercise: (1) plain `if/else`, (2) chained `else if`, (3) `iterate` over lists and ranges (with and without `step`), and (4) chained method calls returning class instances. Each should assert the generated `.wasm` passes validation via the existing QA scripts.

Delivering the above will unblock the failing control-flow suites, restore method chaining semantics, and eliminate the malformed WebAssembly output that currently breaks validation.
