# Parser Bug: List Namespace Statement Ambiguity

## Bug ID
`PARSER-001-LIST-NAMESPACE`

## Severity
🟡 Medium - Affects valid syntax, but workaround exists

## Description
Namespace function calls starting with `list.` fail when used as statements (without variable assignment) because the parser incorrectly interprets `list` as the start of a type declaration instead of a namespace identifier.

## Reproduction

### Failing Code:
```clean
start()
    list<integer> numbers = [1, 2, 3]
    list.add(numbers, 6)  // ❌ FAILS with "Expected variable name after type"
    print("done")
```

### Error Message:
```
Syntax error: Expected variable name after type
```

### Working Workaround:
```clean
start()
    list<integer> numbers = [1, 2, 3]
    void result = list.add(numbers, 6)  // ✅ WORKS
    print("done")
```

## Root Cause

**Parser Ambiguity**: When the parser encounters `list` at the start of a statement, it must decide between:

1. **Type Declaration**: `list<T> varname = ...`
2. **Namespace Function Call**: `list.methodName(...)`

The parser currently always chooses option 1, causing valid namespace calls to be mis-parsed.

## Affected Patterns

### ❌ Fails:
```clean
list.add(numbers, 6)           // Statement without assignment
list.remove(numbers, 0)         // Statement without assignment
```

### ✅ Works:
```clean
void v = list.add(numbers, 6)   // With assignment
integer x = list.remove(numbers, 0)  // With return value
string.toUpperCase("hello")     // Other namespaces work fine
math.sqrt(16)                   // Other namespaces work fine
```

## Why Other Namespaces Work

The issue is **specific to `list`** because `list` is both:
- A namespace identifier (`list.add`, `list.remove`)
- A type keyword (`list<T>`)

Other namespaces like `string`, `math`, `http` don't conflict with type keywords, so they parse correctly even without assignment.

## Impact

- **Files Affected**: 1 confirmed (`32_comprehensive_stdlib.cln`)
- **Workaround Severity**: Low (simple to apply)
- **User Impact**: Medium (confusing error for valid code)

## Fix Applied

**Test File**: `tests/cln/stdlib/32_comprehensive_stdlib.cln`

Changed from:
```clean
list.add(numbers, 6)
```

To:
```clean
// Note: Assigning to void to work around parser bug with list namespace statements
void addResult = list.add(numbers, 6)
```

**Result**: File now compiles successfully ✅

## Proper Fix (Future)

### Parser Changes Needed:

1. **Lookahead Enhancement**: When seeing `list` at statement start, parser should lookahead to determine intent:
   - If followed by `.methodName(`, treat as namespace call
   - If followed by `<type>`, treat as type declaration

2. **Grammar Modification** (`src/parser/grammar.pest`):
   ```pest
   statement = {
       // ... other statements ...
       | namespace_call_statement  // Add before type_declaration
       | variable_declaration
   }
   
   namespace_call_statement = {
       namespace_identifier ~ "." ~ identifier ~ argument_list
   }
   ```

3. **Parser Logic** (`src/parser/statement_parser.rs`):
   - Check for `.` after `list` token
   - Route to namespace call parser instead of type declaration parser

### Estimated Effort: 2-3 hours

## Testing

### Test Cases to Add:
```clean
// All these should compile successfully:
list.add(nums, 1)
list.remove(nums, 0)  
list.clear(nums)
```

## Status
- **Discovered**: 2025-10-22
- **Workaround Applied**: Yes (removed after proper fix)
- **Proper Fix**: ✅ **COMPLETED** - 2025-10-22
- **Priority**: ~~Medium~~ → **RESOLVED**
- **Fix Document**: `session_2025-10-22_type_keyword_parser_fix.md`

## Related Files
- Test file: `tests/cln/stdlib/32_comprehensive_stdlib.cln`
- Parser: `src/parser/statement_parser.rs`
- Grammar: `src/parser/grammar.pest`
