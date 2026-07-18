# Clean Language Specification - Detailed Coverage Matrix

**Generated:** 2025-12-02
**Total Features:** 187
**Covered:** ~120
**Coverage:** ~64%

---

## Legend

- ✅ **COVERED** - Feature has dedicated test(s)
- ⚠️ **PARTIAL** - Feature partially covered or needs verification
- ❌ **MISSING** - No test exists for this feature
- 🔍 **VERIFY** - Test exists but needs manual verification

---

## 1. Lexical Structure (15 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 1.1.1 | Single-line comments (`//`) | ✅ | `lexical/comments_spec.cln` | Lines 4, 11, 22 |
| 1.1.2 | Multi-line comments (`/* */`) | ✅ | `lexical/comments_spec.cln` | Lines 6-8, 14-15, 31-34 |
| 1.2.1 | Tab-based indentation | ❌ | - | Need enforcement test |
| 1.2.2 | Spaces for alignment | ⚠️ | Various | Implicit in all tests |
| 1.2.3 | Mixed tab/space error | ❌ | - | Need error detection test |
| 1.3.1 | Valid identifiers | ✅ | `lexical/identifiers_spec.cln` | |
| 1.3.2 | CamelCase convention | ⚠️ | - | Convention not enforced |
| 1.3.3 | Invalid identifier errors | ✅ | `lexical/identifiers_spec.cln` | Should test errors |
| 1.4 | All 29 keywords | ✅ | `lexical/keywords_spec.cln` | |
| 1.5.1 | Integer literals (decimal) | ✅ | `lexical/integer_literals_spec.cln` | |
| 1.5.2 | Negative integers | ✅ | `lexical/integer_literals_spec.cln` | |
| 1.5.3 | Hex/binary/octal | ✅ | `lexical/numeric_bases_spec.cln` | |
| 1.5.4 | Float literals | ✅ | `lexical/number_literals_spec.cln` | |
| 1.5.5 | Scientific notation | ✅ | `lexical/number_literals_spec.cln` | |
| 1.5.6 | Leading zero optional | ⚠️ | - | Need `.5` test |
| 1.5.7 | String literals | ✅ | `lexical/string_literals_spec.cln` | |
| 1.5.8 | Escape sequences | 🔍 | `lexical/escape_sequences_spec.cln` | Verify coverage |
| 1.5.9 | String interpolation | ✅ | `lexical/string_interpolation_spec.cln` | |
| 1.5.10 | Boolean literals | ✅ | `lexical/boolean_literals_spec.cln` | |
| 1.5.11 | List literals | ✅ | `lexical/list_literals_spec.cln` | |
| 1.5.12 | Empty list | ✅ | `lexical/list_literals_spec.cln` | |
| 1.5.13 | Matrix literals | ✅ | `lexical/matrix_literals_spec.cln` | |

**Section Coverage: 12/15 (80%)**

---

## 2. Type System (32 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 2.1.1 | `boolean` type | ✅ | `types/boolean_type_spec.cln` | |
| 2.1.2 | `integer` type (32-bit) | ✅ | `types/integer_type_spec.cln` | |
| 2.1.3 | `number` type (64-bit) | ✅ | `types/number_type_spec.cln` | |
| 2.1.4 | `string` type | ✅ | `types/string_type_spec.cln` | |
| 2.1.5 | `void` type | ✅ | `types/void_type_spec.cln` | |
| 2.2.1 | `integer:8` (signed) | ✅ | `types/integer_precision_spec.cln` | |
| 2.2.2 | `integer:16` (signed) | ✅ | `types/integer_precision_spec.cln` | |
| 2.2.3 | `integer:32` (signed) | ✅ | `types/integer_precision_spec.cln` | |
| 2.2.4 | `integer:64` (signed) | ✅ | `types/integer_precision_spec.cln` | |
| 2.2.5 | `integer:8u` (unsigned) | ⚠️ | `types/integer_precision_spec.cln` | Verify unsigned |
| 2.2.6 | `integer:16u` (unsigned) | ⚠️ | `types/integer_precision_spec.cln` | Verify unsigned |
| 2.2.7 | `integer:32u` (unsigned) | ⚠️ | `types/integer_precision_spec.cln` | Verify unsigned |
| 2.2.8 | `integer:64u` (unsigned) | ⚠️ | `types/integer_precision_spec.cln` | Verify unsigned |
| 2.3.1 | `number:32` (single) | ✅ | `types/number_precision_spec.cln` | |
| 2.3.2 | `number:64` (double) | ✅ | `types/number_precision_spec.cln` | |
| 2.4.1 | `list<any>` generic | ✅ | `types/list_type_spec.cln` | |
| 2.4.2 | `matrix<any>` 2D | ✅ | `types/matrix_type_spec.cln` | |
| 2.4.3 | `pairs<any, any>` | ⚠️ | `types/pairs_basic_spec.cln` | Verify implementation |
| 2.4.4 | `any` generic type | ❌ | - | Need comprehensive test |
| 2.5.1 | "default" behavior | ✅ | `types/list_behaviors_spec.cln` | |
| 2.5.2 | "line" (FIFO) | ✅ | `types/list_behaviors_spec.cln` | |
| 2.5.3 | "pile" (LIFO) | ✅ | `types/list_behaviors_spec.cln` | |
| 2.5.4 | "unique" (set) | ✅ | `types/list_behaviors_spec.cln` | |
| 2.5.5 | "line-unique" | ❌ | - | Need combination test |
| 2.5.6 | "pile-unique" | ❌ | - | Need combination test |
| 2.6.1 | Type-first declarations | ✅ | `types/core_types_spec.cln` | |
| 2.6.2 | Uninitialized variables | ❌ | - | Need declaration test |
| 2.6.3 | Type widening (int→num) | ✅ | `types/type_widening_spec.cln` | |
| 2.7.1 | `.toInteger` | ❌ | - | Need conversion test |
| 2.7.2 | `.toNumber` | ❌ | - | Need conversion test |
| 2.7.3 | `.toString()` | ✅ | `types/type_conversions_spec.cln` | |
| 2.7.4 | `.toBoolean` | ❌ | - | Need conversion test |

**Section Coverage: 25/32 (78%)**

---

## 3. Apply-Blocks (8 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 3.1.1 | `println:` block | ✅ | `apply_blocks/function_apply_spec.cln` | |
| 3.1.2 | `print:` block | ✅ | `apply_blocks/function_apply_spec.cln` | |
| 3.1.3 | Method apply (`list.push:`) | ✅ | `apply_blocks/method_apply_spec.cln` | |
| 3.2.1 | `integer:` block | ✅ | `apply_blocks/variable_blocks_spec.cln` | |
| 3.2.2 | `string:` block | ✅ | `apply_blocks/variable_blocks_spec.cln` | |
| 3.2.3 | `number:` block | ⚠️ | - | Verify in existing test |
| 3.2.4 | `boolean:` block | ⚠️ | - | Verify in existing test |
| 3.3.1 | `constant:` block | ❌ | - | Need constant block test |

**Section Coverage: 6/8 (75%)**

---

## 4. Expressions (28 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 4.1.1 | Primary precedence | ✅ | `expressions/operator_precedence_spec.cln` | |
| 4.1.2 | Unary precedence | ✅ | `expressions/operator_precedence_spec.cln` | |
| 4.1.3 | Exponentiation (^) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.1.4 | Multiplicative | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.1.5 | Additive | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.1.6 | Comparison | ✅ | `expressions/comparison_operators_spec.cln` | |
| 4.1.7 | Equality | ✅ | `expressions/comparison_operators_spec.cln` | |
| 4.1.8 | Logical AND | ✅ | `expressions/logical_operators_spec.cln` | |
| 4.1.9 | Logical OR | ✅ | `expressions/logical_operators_spec.cln` | |
| 4.1.10 | Assignment | ✅ | `statements/assignment_spec.cln` | |
| 4.2.1 | Parentheses required | ✅ | `core/basics/multiline_expressions_spec.cln` | |
| 4.2.2 | Balanced parsing | ✅ | `core/basics/multiline_expressions_spec.cln` | |
| 4.2.3 | Nested multi-line | ❌ | - | Need complex test |
| 4.2.4 | Unbalanced error | ❌ | - | Need error test |
| 4.3.1 | Addition (+) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.3.2 | Subtraction (-) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.3.3 | Multiplication (*) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.3.4 | Division (/) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.3.5 | Modulo (%) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.3.6 | Exponentiation (^) | ✅ | `expressions/arithmetic_operators_spec.cln` | |
| 4.4 | All comparison ops | ✅ | `expressions/comparison_operators_spec.cln` | |
| 4.4.7 | Identity (`is`) | ❌ | - | Need identity test |
| 4.4.8 | Negated (`not`) | ❌ | - | Need identity test |
| 4.5 | Logical operators | ✅ | `expressions/logical_operators_spec.cln` | |
| 4.6.1 | Matrix multiply | ⚠️ | `core/types/matrix_operations_comprehensive.cln` | Verify |
| 4.6.2 | Matrix add/subtract | ⚠️ | `core/types/matrix_operations_comprehensive.cln` | Verify |
| 4.6.3 | `.transpose()` | ❌ | - | Need method test |
| 4.6.4 | `.inverse()` | ❌ | - | Need method test |
| 4.6.5 | `.determinant()` | ❌ | - | Need method test |
| 4.7.1 | Method calls | ✅ | `expressions/method_calls_spec.cln` | |
| 4.7.2 | Property access | ✅ | `expressions/method_calls_spec.cln` | |
| 4.7.3 | Method with args | ✅ | `expressions/method_calls_spec.cln` | |
| 4.7.4 | Literal methods | ❌ | - | Need literal test |
| 4.7.5 | List indexing | ❌ | - | Need indexing test |

**Section Coverage: 18/28 (64%)**

---

## 5. Statements (12 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 5.1.1 | Type-first declaration | ✅ | `statements/variable_declaration_spec.cln` | |
| 5.1.2 | Without initialization | ⚠️ | - | Verify in existing |
| 5.1.3 | List element assignment | ❌ | - | Need assignment test |
| 5.1.4 | Property assignment | ❌ | - | Need assignment test |
| 5.2.1 | `print "text"` (no newline) | ⚠️ | `stdlib/print_functions_spec.cln` | Verify |
| 5.2.2 | `print(expr) +` (newline) | ⚠️ | `stdlib/print_functions_spec.cln` | Verify |
| 5.2.3 | Auto toString conversion | ✅ | `types/type_conversions_spec.cln` | |
| 5.2.4 | Block syntax | ⚠️ | - | Verify in apply blocks |
| 5.3.1 | `input("prompt")` | ❌ | - | Need input test |
| 5.3.2 | `input.integer()` | ❌ | - | Need input test |
| 5.3.3 | `input.number()` | ❌ | - | Need input test |
| 5.3.4 | `input.yesNo()` | ❌ | - | Need input test |
| 5.4.1 | `return` (void) | ✅ | `functions/return_types_spec.cln` | |
| 5.4.2 | `return value` | ✅ | `functions/return_types_spec.cln` | |
| 5.4.3 | `return expression` | ✅ | `functions/return_types_spec.cln` | |

**Section Coverage: 6/12 (50%)**

---

## 6. Functions (18 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 6.1.1 | Standalone `start()` | ✅ | `functions/function_declaration_spec.cln` | |
| 6.1.2 | `start()` in `functions:` | ✅ | `functions/function_declaration_spec.cln` | |
| 6.2.1 | `functions:` block | ✅ | `functions/functions_block_spec.cln` | |
| 6.2.2 | Return type | ✅ | `functions/return_types_spec.cln` | |
| 6.2.3 | Parameters | ✅ | `functions/function_parameters_spec.cln` | |
| 6.2.4 | `void` return | ✅ | `functions/return_types_spec.cln` | |
| 6.3.1 | `any` generic type | ❌ | - | Need generic test |
| 6.3.2 | Generic return types | ❌ | - | Need generic test |
| 6.3.3 | Generic parameters | ❌ | - | Need generic test |
| 6.3.4 | Type inference | ❌ | - | Need inference test |
| 6.4.1 | `description` annotation | ❌ | - | Need annotation test |
| 6.4.2 | `input` block | ❌ | - | Need input block test |
| 6.4.3 | Default parameter values | ⚠️ | `language/functions/` | Verify coverage |
| 6.4.4 | Input block defaults | ❌ | - | Need defaults test |
| 6.4.5 | Expression defaults | ❌ | - | Need expression test |
| 6.5.1 | Parentheses required | ⚠️ | - | Implicit in all tests |
| 6.5.2 | Error on missing parens | ❌ | - | Need error test |
| 6.6.1 | Implicit return | ❌ | - | Need auto return test |

**Section Coverage: 10/18 (56%)**

---

## 7. Testing Framework (10 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 7.1.1 | `tests:` block | ✅ | `testing/tests_block_spec.cln` | |
| 7.1.2 | Named tests | ✅ | `testing/named_tests_spec.cln` | |
| 7.1.3 | Anonymous tests | ✅ | `testing/anonymous_tests_spec.cln` | |
| 7.2.1 | Function call tests | ❌ | - | Need function test |
| 7.2.2 | Method call tests | ❌ | - | Need method test |
| 7.2.3 | Complex expression tests | ❌ | - | Need complex test |
| 7.2.4 | Object creation tests | ❌ | - | Need object test |
| 7.3.1 | Error test syntax | ❌ | - | Need error test |
| 7.4.1 | Pass/fail reporting | ⚠️ | - | Runtime feature |
| 7.4.2 | Expected vs actual | ⚠️ | - | Runtime feature |

**Section Coverage: 4/10 (40%)**

---

## 8. Control Flow (12 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 8.1.1 | Basic `if` | ✅ | `control_flow/conditionals_spec.cln` | |
| 8.1.2 | `if`-`else` | ✅ | `control_flow/if_else_spec.cln` | |
| 8.1.3 | `if`-`else if` | ✅ | `control_flow/conditionals_spec.cln` | |
| 8.1.4 | Full chain | ✅ | `control_flow/conditionals_spec.cln` | |
| 8.2.1 | `iterate item in list` | ✅ | `control_flow/iterate_collection_spec.cln` | |
| 8.2.2 | `iterate char in string` | ✅ | `control_flow/iterate_collection_spec.cln` | |
| 8.2.3 | `iterate i in x to y` | ✅ | `control_flow/iterate_range_spec.cln` | |
| 8.2.4 | `iterate` with step | ✅ | `control_flow/iterate_range_spec.cln` | |
| 8.2.5 | Negative step | ❌ | - | Need negative test |
| 8.2.6 | Nested loops | ✅ | `control_flow/nested_loops_spec.cln` | |
| 8.3.1 | Range `from x to y` | ⚠️ | - | Verify syntax |
| 8.3.2 | Range with step | ⚠️ | - | Verify syntax |

**Section Coverage: 10/12 (83%)**

---

## 9. Error Handling (6 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 9.1.1 | `error("message")` | ✅ | `error_handling/error_statement_spec.cln` | |
| 9.2.1 | `expr onError default` | ✅ | `error_handling/on_error_spec.cln` | |
| 9.2.2 | `expr onError block` | ✅ | `error_handling/on_error_block_spec.cln` | |
| 9.2.3 | Access `error` variable | ✅ | `error_handling/error_variable_spec.cln` | |
| 9.3.1 | Error bubbling | ❌ | - | Need propagation test |
| 9.3.2 | Call stack propagation | ❌ | - | Need stack test |

**Section Coverage: 4/6 (67%)**

---

## 10. Classes and Objects (22 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 10.1.1 | `class` declaration | ✅ | `classes/class_definition_spec.cln` | |
| 10.1.2 | Class fields | ✅ | `classes/class_fields_spec.cln` | |
| 10.1.3 | Constructor | ✅ | `classes/constructor_spec.cln` | |
| 10.1.4 | Auto-storage | ❌ | - | Need auto-store test |
| 10.1.5 | `functions:` in class | ❌ | - | Verify requirement |
| 10.2.1 | `any` field type | ❌ | - | Need generic test |
| 10.2.2 | `any` method params | ❌ | - | Need generic test |
| 10.2.3 | `any` return types | ❌ | - | Need generic test |
| 10.3.1 | `class Child is Parent` | ✅ | `classes/inheritance_spec.cln` | |
| 10.3.2 | `base(args)` call | ❌ | - | Need base() test |
| 10.3.3 | Field inheritance | ✅ | `classes/inheritance_spec.cln` | |
| 10.3.4 | Method inheritance | ✅ | `classes/inheritance_spec.cln` | |
| 10.3.5 | Method overriding | ⚠️ | `classes/polymorphism_spec.cln` | Verify override |
| 10.4.1 | Direct field access | ❌ | - | Need context test |
| 10.4.2 | Name conflict detection | ❌ | - | Need error test |
| 10.5.1 | Object instantiation | ✅ | `classes/class_definition_spec.cln` | |
| 10.5.2 | Method calls | ✅ | `classes/methods_spec.cln` | |
| 10.5.3 | Property access | ✅ | `classes/class_fields_spec.cln` | |
| 10.6.1 | `ClassName.method()` | ✅ | `classes/static_methods_spec.cln` | |
| 10.6.2 | Static restrictions | ❌ | - | Need restriction test |

**Section Coverage: 12/22 (55%)**

---

## 11. Standard Library (32 features)

### Math Module (10 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 11.1.1 | Core functions | ✅ | `stdlib/math_functions_spec.cln` | sqrt/abs/max/min |
| 11.1.2 | Rounding | ⚠️ | `stdlib/math_functions_spec.cln` | floor/ceil/round |
| 11.1.3 | Trig functions | ✅ | `stdlib/math_trig_spec.cln` | sin/cos/tan |
| 11.1.4 | Inverse trig | ⚠️ | - | asin/acos/atan |
| 11.1.5 | Logarithms | ⚠️ | - | ln/log10/log2 |
| 11.1.6 | Exponentials | ⚠️ | - | exp/exp2 |
| 11.1.7 | Constants | ⚠️ | - | pi/e/tau |
| 11.1.8 | Hyperbolic | ❌ | - | sinh/cosh/tanh |
| 11.1.9 | Sign/trunc | ❌ | - | sign/trunc |
| 11.1.10 | atan2 | ❌ | - | Two-arg arctan |

### String Module (12 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 11.2.1 | Basic ops | ✅ | `stdlib/string_functions_spec.cln` | length/concat/substring |
| 11.2.2 | Case conversion | ✅ | `stdlib/string_upper_lower_spec.cln` | toUpperCase/toLowerCase |
| 11.2.3 | Search ops | ⚠️ | `stdlib/string_functions_spec.cln` | contains/indexOf/lastIndexOf |
| 11.2.4 | Prefix/suffix | ⚠️ | - | startsWith/endsWith |
| 11.2.5 | Trim ops | ✅ | `stdlib/string_trim_spec.cln` | trim/trimStart/trimEnd |
| 11.2.6 | Replace ops | ⚠️ | - | replace/replaceAll |
| 11.2.7 | Split/join | ✅ | `stdlib/string_split_spec.cln` | |
| 11.2.8 | Validation | ❌ | - | isEmpty/isBlank |
| 11.2.9 | Padding | ❌ | - | padStart/padEnd |
| 11.2.10 | Character ops | ❌ | - | charAt/charCodeAt |
| 11.2.11 | Substring | ✅ | `stdlib/string_substring_spec.cln` | |
| 11.2.12 | Concat | ✅ | `stdlib/string_concat_spec.cln` | |

### List Module (9 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 11.3.1 | Basic access | ✅ | `stdlib/list_operations_spec.cln` | size/get/set |
| 11.3.2 | Modification | ✅ | `stdlib/list_operations_spec.cln` | push/pop |
| 11.3.3 | Insertion/removal | ⚠️ | - | insert/remove |
| 11.3.4 | Search ops | ✅ | `stdlib/list_advanced_search_spec.cln` | contains/indexOf |
| 11.3.5 | Slice/concat | ✅ | `stdlib/list_slice_spec.cln`, `list_concat_spec.cln` | |
| 11.3.6 | Reverse/sort | ✅ | `stdlib/list_reverse_spec.cln` | |
| 11.3.7 | Functional ops | ❌ | - | map/filter/reduce |
| 11.3.8 | Utility ops | ⚠️ | - | isEmpty/first/last |
| 11.3.9 | Creation ops | ❌ | - | fill/range |
| 11.3.10 | Join | ✅ | `stdlib/list_join_spec.cln` | |

### File Module (6 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 11.4.1 | `file.read` | ✅ | `stdlib/file_read_spec.cln` | |
| 11.4.2 | `file.lines` | ❌ | - | Read lines |
| 11.4.3 | `file.write` | ✅ | `stdlib/file_write_spec.cln` | |
| 11.4.4 | `file.append` | ❌ | - | Append content |
| 11.4.5 | `file.exists` | ✅ | `stdlib/file_exists_spec.cln` | |
| 11.4.6 | `file.delete` | ❌ | - | Delete file |

### HTTP Module (5 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 11.5.1 | `http.get` | ❌ | - | GET request |
| 11.5.2 | `http.post` | ❌ | - | POST request |
| 11.5.3 | `http.put` | ❌ | - | PUT request |
| 11.5.4 | `http.patch` | ❌ | - | PATCH request |
| 11.5.5 | `http.delete` | ❌ | - | DELETE request |

**Section Coverage: 19/32 (59%)**

---

## 12. Modules and Imports (6 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 12.1.1 | Public by default | ⚠️ | - | Convention test |
| 12.1.2 | `private:` block | ❌ | - | Need private test |
| 12.2.1 | `import: Module` | ⚠️ | `advanced/modules/` | Verify |
| 12.2.2 | `import: module.symbol` | ⚠️ | - | Verify |
| 12.2.3 | `import: Module as alias` | ❌ | - | Need alias test |
| 12.2.4 | `import: symbol as alias` | ❌ | - | Need alias test |

**Section Coverage: 2/6 (33%)**

---

## 13. Asynchronous Programming (4 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 13.1.1 | `later` keyword | ⚠️ | `advanced/async/` | Verify |
| 13.1.2 | `start` keyword | ⚠️ | `advanced/async/` | Verify |
| 13.1.3 | `background` keyword | ⚠️ | - | Verify |
| 13.1.4 | Function as `background` | ❌ | - | Need function test |

**Section Coverage: 2/4 (50%)**

---

## 14. Plugin System (8 features)

| # | Feature | Status | Test File | Notes |
|---|---------|--------|-----------|-------|
| 14.1.1 | Custom DSL blocks | ✅ | `plugins/framework_blocks_spec.cln` | |
| 14.1.2 | Block attributes | ✅ | `plugins/plugin_attributes_spec.cln` | |
| 14.2.1 | `endpoints:` block | ✅ | `plugins/endpoints_spec.cln` | |
| 14.2.2 | Route definitions | ⚠️ | - | Verify in endpoints |
| 14.2.3 | Path parameters | ❌ | - | Need params test |
| 14.3.1 | IDE autocomplete | ❌ | - | Manual test |
| 14.3.2 | Hover docs | ❌ | - | Manual test |
| 14.3.3 | Syntax highlighting | ❌ | - | Manual test |

**Section Coverage: 3/8 (38%)**

---

## Overall Summary

| Tier | Categories | Features | Covered | Coverage | Target |
|------|------------|----------|---------|----------|--------|
| **Tier 1** | 5 core | 112 | 75 | 67% | 100% |
| **Tier 2** | 5 standard | 73 | 47 | 64% | 95% |
| **Tier 3** | 4 advanced | 28 | 11 | 39% | 90% |
| **TOTAL** | **14** | **187** | **~120** | **64%** | **95%** |

---

## Critical Gaps Summary

**Immediate Priority (20 tests):**
1. Type conversions (toInteger, toNumber, toBoolean)
2. Generic `any` type tests
3. Input functions (input, input.integer, input.number, input.yesNo)
4. Function input blocks and defaults
5. Class generic fields and methods
6. Identity operators (is, not)
7. Matrix methods (transpose, inverse, determinant)

**High Priority (25 tests):**
8. List functional operations (map, filter, reduce)
9. HTTP module (all methods)
10. File operations (lines, append, delete)
11. Error propagation tests
12. Testing framework enhancements
13. Apply-block completion (constant blocks)
14. Implicit return and auto-storage

**Medium Priority (15 tests):**
15. String advanced (charAt, padding, validation)
16. Math advanced (hyperbolic, atan2)
17. List utilities (fill, range, isEmpty)
18. Multi-line expression edge cases
19. Static method restrictions

---

**Report End - Use this matrix to track progress feature-by-feature**
