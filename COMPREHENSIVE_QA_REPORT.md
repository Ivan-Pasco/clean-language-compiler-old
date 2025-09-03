# Comprehensive QA Analysis Report

## Executive Summary

- **Total Failed Tests**: 64
- **Success Rate**: 79.93% (255/319 tests passing)
- **Primary Issue Categories**: 8

## Error Categories by Impact

### 1. Missing Method
**Impact**: 17 tests affected
**Tests**: 33_complex_integration, 51_http_method_syntax, 72_default_parameters_comprehensive, 94_stdlib_string_comprehensive, test_chained_minimal, test_chained_property_method, test_complex_onerror, test_conditional_simple, test_different_property_chain, test_explicit_method_chained...

### 2. Codegen Error
**Impact**: 17 tests affected
**Tests**: 64_default_parameters_spec, 69_string_interpolation_comprehensive, 74_file_module_comprehensive, 76_math_module_comprehensive, 79_http_module_comprehensive, 82_matrix_operations_comprehensive, 93_stdlib_math_comprehensive, debug_list_push, debug_listlength_conflict, debug_method_2...

### 3. Type Mismatch
**Impact**: 8 tests affected
**Tests**: test_debug_spacing, test_exponent_pattern, test_grammar_debug_3, test_if_else_boundary_debug, test_minimal_default, test_multiple_functions_debug, test_no_return_type, test_simple_default_params

### 4. Invalid Method Call
**Impact**: 7 tests affected
**Tests**: 34_list_behaviors, 48_method_style_syntax_fixed, 68_list_behaviors_comprehensive, 77_string_module_comprehensive, debug_multifunction_complex, test_list_type, test_while_concat

### 5. Missing Namespace Function
**Impact**: 5 tests affected
**Tests**: 78_list_module_comprehensive, debug_args, test_args_comprehensive, test_http_basic, test_simple_chained_property

### 6. Undefined Variable
**Impact**: 4 tests affected
**Tests**: 52_async_keywords, debug_default_long, debug_default_param, debug_default_simple

### 7. Invalid Condition Type
**Impact**: 3 tests affected
**Tests**: 48_method_style_syntax, 54_integration_test, 97_testing_framework_comprehensive

### 8. Missing Return Value
**Impact**: 3 tests affected
**Tests**: 59_default_parameters_working, test_return_issue, test_var_arith_return

## Critical Missing Standard Library Functions

### Most Critical Missing Methods
- `size`: 6 tests blocked
- `getName`: 1 tests blocked
- `mustBeTrue`: 1 tests blocked
- `integer`: 1 tests blocked

### Most Critical Missing Namespace Functions
- `http.get`: 3 tests blocked
- `compare.integer.toString`: 3 tests blocked
- `obj.prop.toString`: 2 tests blocked
- `list.insert`: 1 tests blocked
- `string.isBlank`: 1 tests blocked
- `http.post`: 1 tests blocked
- `obj.prop.greaterThan`: 1 tests blocked
- `compare.integer.greaterThan`: 1 tests blocked
- `compare.equal`: 1 tests blocked

## Top 5 Critical Gaps by Impact

### 1. Missing List/Collection Methods
**Impact**: ~14 tests affected
**Examples**: size, add, remove
**Priority**: 🔴 CRITICAL

### 2. Missing String Methods
**Impact**: ~39 tests affected
**Examples**: isEmpty, contains, concat
**Priority**: 🔴 CRITICAL

### 3. Missing HTTP Module
**Impact**: ~6 tests affected
**Examples**: http.get, http.post
**Priority**: 🟡 HIGH

### 4. Missing Object Methods
**Impact**: ~17 tests affected
**Examples**: getName, getArea, toString
**Priority**: 🔴 CRITICAL

### 5. Type System Issues
**Impact**: ~64 tests affected
**Examples**: Boolean conditions, Method chaining
**Priority**: 🔴 CRITICAL

## Specific Implementation Priorities

### List/Collection Standard Library
**Task**: Implement size(), add(), remove(), isEmpty() methods
**Priority**: 🔴 CRITICAL

### String Standard Library
**Task**: Implement isEmpty(), contains(), concat() methods
**Priority**: 🔴 CRITICAL

### HTTP Module
**Task**: Implement http.get(), http.post() namespace functions
**Priority**: 🟡 HIGH

### Object Method Resolution
**Task**: Fix method lookup for custom object types
**Priority**: 🟡 HIGH

### Type System Improvements
**Task**: Fix boolean condition type checking
**Priority**: 🟡 HIGH

## Progress Since Recent Improvements

✅ **Apply-blocks**: Working correctly (recent implementation successful)
✅ **Math constants**: pi, e, tau now working
🟡 **String functions**: Partially implemented, missing key methods
❌ **List behaviors**: Critical missing methods blocking multiple tests
❌ **HTTP module**: Not implemented
❌ **Testing framework**: Missing core functionality

## Recommendations for Next Steps

1. **Immediate Priority**: Implement missing list methods (size, add, remove)
2. **High Priority**: Complete string standard library implementation
3. **Medium Priority**: Implement HTTP module for networking tests
4. **Ongoing**: Fix type system issues with boolean conditions
5. **Testing**: Re-run comprehensive tests after each major implementation

## Detailed Error Breakdown

### 33_complex_integration
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/33_complex_integration.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/33_complex_integration.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/33_complex_integration.cln to /Users/earcan...
```

### 34_list_behaviors
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/34_list_behaviors.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/34_list_behaviors.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/34_list_behaviors.cln to /Users/earcandy/Documents/De...
```

### 48_method_style_syntax
**Category**: INVALID_CONDITION_TYPE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/48_method_style_syntax.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/48_method_style_syntax.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/48_method_style_syntax.cln to /Users/earcan...
```

### 48_method_style_syntax_fixed
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/48_method_style_syntax_fixed.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/48_method_style_syntax_fixed.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/48_method_style_syntax_fixed.cl...
```

### 51_http_method_syntax
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/51_http_method_syntax.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/51_http_method_syntax.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/51_http_method_syntax.cln to /Users/earcandy/...
```

### 52_async_keywords
**Category**: UNDEFINED_VARIABLE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/52_async_keywords.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/52_async_keywords.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/52_async_keywords.cln to /Users/earcandy/Documents/De...
```

### 54_integration_test
**Category**: INVALID_CONDITION_TYPE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/54_integration_test.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/54_integration_test.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/54_integration_test.cln to /Users/earcandy/Docume...
```

### 59_default_parameters_working
**Category**: MISSING_RETURN_VALUE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/59_default_parameters_working.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/59_default_parameters_working.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/59_default_parameters_working...
```

### 64_default_parameters_spec
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/64_default_parameters_spec.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/64_default_parameters_spec.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/64_default_parameters_spec.cln to /...
```

### 68_list_behaviors_comprehensive
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/68_list_behaviors_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/68_list_behaviors_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/68_list_behaviors_compreh...
```

### 69_string_interpolation_comprehensive
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/69_string_interpolation_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/69_string_interpolation_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/69_string_int...
```

### 72_default_parameters_comprehensive
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/72_default_parameters_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/72_default_parameters_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/72_default_parame...
```

### 74_file_module_comprehensive
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/74_file_module_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/74_file_module_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/74_file_module_comprehensive.cl...
```

### 76_math_module_comprehensive
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/76_math_module_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/76_math_module_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/76_math_module_comprehensive.cl...
```

### 77_string_module_comprehensive
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/77_string_module_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/77_string_module_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/77_string_module_comprehens...
```

### 78_list_module_comprehensive
**Category**: MISSING_NAMESPACE_FUNCTION
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/78_list_module_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/78_list_module_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/78_list_module_comprehensive.cl...
```

### 79_http_module_comprehensive
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/79_http_module_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/79_http_module_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/79_http_module_comprehensive.cl...
```

### 82_matrix_operations_comprehensive
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/82_matrix_operations_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/82_matrix_operations_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/82_matrix_operation...
```

### 93_stdlib_math_comprehensive
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/93_stdlib_math_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/93_stdlib_math_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/93_stdlib_math_comprehensive.cl...
```

### 94_stdlib_string_comprehensive
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.16s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/94_stdlib_string_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/94_stdlib_string_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/94_stdlib_string_comprehens...
```

### 97_testing_framework_comprehensive
**Category**: INVALID_CONDITION_TYPE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/97_testing_framework_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/97_testing_framework_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/97_testing_framewor...
```

### debug_args
**Category**: MISSING_NAMESPACE_FUNCTION
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_args.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_args.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_args.cln to /Users/earcandy/Documents/Dev/Clean Language/clea...
```

### debug_default_long
**Category**: UNDEFINED_VARIABLE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_default_long.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_default_long.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_default_long.cln to /Users/earcandy/Documents...
```

### debug_default_param
**Category**: UNDEFINED_VARIABLE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_default_param.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_default_param.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_default_param.cln to /Users/earcandy/Docume...
```

### debug_default_simple
**Category**: UNDEFINED_VARIABLE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_default_simple.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_default_simple.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_default_simple.cln to /Users/earcandy/Doc...
```

### debug_list_push
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.17s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_list_push.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_list_push.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_list_push.cln to /Users/earcandy/Documents/Dev/Clea...
```

### debug_listlength_conflict
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_listlength_conflict.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_listlength_conflict.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_listlength_conflict.cln to /Use...
```

### debug_method_2
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_method_2.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_method_2.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_method_2.cln to /Users/earcandy/Documents/Dev/Clean L...
```

### debug_method_call_statement
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_method_call_statement.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_method_call_statement.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_method_call_statement.cln t...
```

### debug_multifunction_complex
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.16s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_multifunction_complex.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_multifunction_complex.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_multifunction_complex.cln t...
```

### debug_namespace_parsing
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_namespace_parsing.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_namespace_parsing.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_namespace_parsing.cln to /Users/ear...
```

### debug_second_function
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_second_function.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_second_function.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_second_function.cln to /Users/earcandy/...
```

### debug_with_comments
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_with_comments.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/debug_with_comments.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/debug_with_comments.cln to /Users/earcandy/Docume...
```

### test_args_comprehensive
**Category**: MISSING_NAMESPACE_FUNCTION
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_args_comprehensive.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_args_comprehensive.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_args_comprehensive.cln to /Users/ear...
```

### test_boolean_assignment
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_boolean_assignment.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_boolean_assignment.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_boolean_assignment.cln to /Users/ear...
```

### test_chained_minimal
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_chained_minimal.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_chained_minimal.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_chained_minimal.cln to /Users/earcandy/Doc...
```

### test_chained_property_method
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_chained_property_method.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_chained_property_method.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_chained_property_method.cl...
```

### test_complex_onerror
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_complex_onerror.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_complex_onerror.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_complex_onerror.cln to /Users/earcandy/Doc...
```

### test_conditional_simple
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_conditional_simple.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_conditional_simple.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_conditional_simple.cln to /Users/ear...
```

### test_debug_property_method
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_debug_property_method.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_debug_property_method.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_debug_property_method.cln to /...
```

### test_debug_spacing
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_debug_spacing.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_debug_spacing.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_debug_spacing.cln to /Users/earcandy/Documents...
```

### test_different_property_chain
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_different_property_chain.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_different_property_chain.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_different_property_chain...
```

### test_explicit_method_chained
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_explicit_method_chained.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_explicit_method_chained.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_explicit_method_chained.cl...
```

### test_exponent_pattern
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.12s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_exponent_pattern.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_exponent_pattern.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_exponent_pattern.cln to /Users/earcandy/...
```

### test_grammar_debug
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_grammar_debug.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_grammar_debug.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_grammar_debug.cln to /Users/earcandy/Documents...
```

### test_grammar_debug_3
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_grammar_debug_3.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_grammar_debug_3.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_grammar_debug_3.cln to /Users/earcandy/Doc...
```

### test_http_basic
**Category**: MISSING_NAMESPACE_FUNCTION
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_http_basic.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_http_basic.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_http_basic.cln to /Users/earcandy/Documents/Dev/Clea...
```

### test_if_else_boundary_debug
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_if_else_boundary_debug.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_if_else_boundary_debug.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_if_else_boundary_debug.cln t...
```

### test_inheritance_minimal
**Category**: CODEGEN_ERROR
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_inheritance_minimal.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_inheritance_minimal.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_inheritance_minimal.cln to /Users/...
```

### test_list_type
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_list_type.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_list_type.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_list_type.cln to /Users/earcandy/Documents/Dev/Clean L...
```

### test_minimal_default
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_minimal_default.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_minimal_default.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_minimal_default.cln to /Users/earcandy/Doc...
```

### test_multiple_functions_debug
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_multiple_functions_debug.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_multiple_functions_debug.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_multiple_functions_debug...
```

### test_no_return_type
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_no_return_type.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_no_return_type.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_no_return_type.cln to /Users/earcandy/Docume...
```

### test_property_method_debug
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_property_method_debug.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_property_method_debug.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_property_method_debug.cln to /...
```

### test_property_method_no_args
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_property_method_no_args.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_property_method_no_args.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_property_method_no_args.cl...
```

### test_regular_method_multiarg
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.15s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_regular_method_multiarg.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_regular_method_multiarg.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_regular_method_multiarg.cl...
```

### test_return_issue
**Category**: MISSING_RETURN_VALUE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_return_issue.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_return_issue.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_return_issue.cln to /Users/earcandy/Documents/De...
```

### test_simple_chain
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_chain.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_simple_chain.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_chain.cln to /Users/earcandy/Documents/De...
```

### test_simple_chained_property
**Category**: MISSING_NAMESPACE_FUNCTION
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_chained_property.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_simple_chained_property.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_chained_property.cl...
```

### test_simple_default_params
**Category**: TYPE_MISMATCH
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.14s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_default_params.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_simple_default_params.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_default_params.cln to /...
```

### test_simple_method_multiarg
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_method_multiarg.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_simple_method_multiarg.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_method_multiarg.cln t...
```

### test_simple_property_method
**Category**: MISSING_METHOD
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.13s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_property_method.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_simple_property_method.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_simple_property_method.cln t...
```

### test_var_arith_return
**Category**: MISSING_RETURN_VALUE
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.16s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_var_arith_return.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_var_arith_return.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_var_arith_return.cln to /Users/earcandy/...
```

### test_while_concat
**Category**: INVALID_METHOD_CALL
```
Finished `dev` profile [optimized + debuginfo] target(s) in 0.15s
     Running `target/debug/clean-language-compiler compile -i '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_while_concat.cln' -o '/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output/test_while_concat.wasm'`
Compiling /Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/test_while_concat.cln to /Users/earcandy/Documents/De...
```

