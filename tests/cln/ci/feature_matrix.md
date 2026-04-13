# Feature Matrix — CI Test Coverage

Generated: 2026-04-12

## Summary

| Tier | Tests | Compile Pass | Compile Fail | Status |
|------|-------|-------------|-------------|--------|
| Tier 1 — Fundamentals | 15 | 15 | 0 | All pass |
| Tier 2 — Core Features | 20 | 20 | 0 | All pass |
| Tier 3 — Advanced Features | 10 | 10 | 0 | All pass |
| Tier 4 — Known Incomplete | 5 | 3 | 2 | Tracked |
| **Total** | **50** | **48** | **2** | **96%** |

## Runtime Execution Note

The standalone `wasmtime_runner` binary cannot execute these WASM files because it does not
provide the full host bridge (missing `_res_redirect`, `_session_store`, and related imports).
Execution requires the full clean-server stack. Compile-pass status is the authoritative CI gate.

Expected output files in `expected/` document the correct stdout each program would produce
when executed with the full runtime. These serve as correctness specification for future
integration with a headless test runner.

## Feature Coverage by Grammar Area

| Feature Area | Grammar Rules Exercised | Test File | Tier | Compile |
|---|---|---|---|---|
| Empty start block | start: with single statement | t1_empty_start | 1 | PASS |
| String literal print | print(string_literal) | t1_print_string | 1 | PASS |
| Integer literal print | integer.toString() | t1_print_integer | 1 | PASS |
| Number literal print | number.toString() | t1_print_number | 1 | PASS |
| Boolean literal print | true.toString(), false.toString() | t1_print_boolean | 1 | PASS |
| Integer variable declaration | integer name = value | t1_variable_integer | 1 | PASS |
| String variable declaration | string name = value | t1_variable_string | 1 | PASS |
| Number variable declaration | number name = value | t1_variable_number | 1 | PASS |
| Boolean variable declaration | boolean name = value | t1_variable_boolean | 1 | PASS |
| Arithmetic operators | +, -, *, / on integers | t1_arithmetic | 1 | PASS |
| String concatenation | string + string | t1_string_concat | 1 | PASS |
| Integer comparison | ==, !=, <, >, <=, >= | t1_comparison_int | 1 | PASS |
| String comparison | ==, != on strings | t1_comparison_string | 1 | PASS |
| Logical operators | and, or, not | t1_logical_ops | 1 | PASS |
| Variable reassignment | name = new_value | t1_reassignment | 1 | PASS |
| If/else conditional | if cond / else | t2_if_else | 2 | PASS |
| If/else-if/else chain | if / else if / else | t2_if_elseif | 2 | PASS |
| While loop | while condition: body | t2_while_loop | 2 | PASS |
| Iterate range | iterate i in N to M | t2_iterate_range | 2 | PASS |
| Iterate with step | iterate i in N to M step K | t2_iterate_step | 2 | PASS |
| Break and continue | break, continue in while | t2_break_continue | 2 | PASS |
| Void function | void fn() declaration + call | t2_function_void | 2 | PASS |
| Integer-returning function | integer fn() + return | t2_function_return | 2 | PASS |
| Function with parameters | fn(type param, ...) | t2_function_params | 2 | PASS |
| String-returning function | string fn() + return | t2_function_string_return | 2 | PASS |
| Multiple functions | several functions, cross-calls | t2_multiple_functions | 2 | PASS |
| Recursive function | fn calls itself, base case | t2_recursive_function | 2 | PASS |
| Nested if/else | if inside if | t2_nested_if | 2 | PASS |
| Nested iterate loops | iterate inside iterate | t2_nested_loops | 2 | PASS |
| String interpolation | "text {expr}" | t2_string_interpolation | 2 | PASS |
| Number to string conversion | number.toString() | t2_number_to_string | 2 | PASS |
| Integer to string conversion | integer.toString() | t2_integer_to_string | 2 | PASS |
| Modulo operator | integer % integer | t2_modulo | 2 | PASS |
| Unary minus | -literal, -variable | t2_unary_minus | 2 | PASS |
| Mixed arithmetic | integer.toNumber() + number | t2_mixed_arithmetic | 2 | PASS |
| Class fields and constructor | class, constructor() | t3_class_basic | 3 | PASS |
| Class methods | functions: block in class | t3_class_methods | 3 | PASS |
| Class inheritance | class X is Y, base() | t3_class_inheritance | 3 | PASS |
| List creation and access | list<T> = [...], [i], .length() | t3_list_basic | 3 | PASS |
| Error handling (onError) | expr onError fallback, error() | t3_onError | 3 | PASS |
| Constant declaration | integer CONST = value | t3_constant | 3 | PASS |
| Multi-step expressions | intermediate variables | t3_multiline_expr | 3 | PASS |
| String built-in methods | .length(), .toUpperCase(), .contains() | t3_string_methods | 3 | PASS |
| Math built-in functions | math.abs(), math.floor(), math.sqrt() | t3_math_functions | 3 | PASS |
| Nested function calls | fn(fn(...)) as arguments | t3_nested_function_calls | 3 | PASS |
| Async function / later | async fn + later keyword | t4_async_basic | 4 | FAIL |
| Module import | import ModuleName | t4_import_module | 4 | FAIL |
| Generic list iteration | iterate item in list<T> | t4_generic_list | 4 | PASS |
| Matrix 2D access | matrix<T> = [[...]], [r][c] | t4_matrix_basic | 4 | PASS |
| Pairs type | pairs<K,V>, key assignment, lookup | t4_pairs_basic | 4 | PASS |

## Tier 4 Known Failure Details

### t4_async_basic — COMPILE_FAIL
- **Error**: `E001: Expected LeftParen, found Identifier("fetchGreeting")`
- **Root cause**: The parser does not support `async` function modifier or `later` call keyword
- **Impact**: Any async programming patterns fail at parse stage
- **Spec reference**: async/later keywords exist in the language spec but are not yet implemented

### t4_import_module — COMPILE_FAIL
- **Error**: `Module 'MathUtils' not found`
- **Root cause**: Module resolution only searches the file's own directory; no module registry
- **Impact**: Cross-file imports are not functional
- **Spec reference**: `import ModuleName` is specified but resolution is incomplete

## Section Ordering Rule (compiler enforcement)

The compiler enforces a strict section ordering rule:
```
import:  →  start:  →  state:  →  class  →  functions:
```
All CI tests follow this ordering. The `start:` block appears first, class definitions and
`functions:` blocks appear after it. Tests that violate this ordering produce:
`error[E001]: 'start:' section is out of order — it must appear before 'class'`
