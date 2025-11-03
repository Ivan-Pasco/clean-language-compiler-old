# Apply Block Codegen Bug Investigation

## Date: 2025-10-23

## Summary
After achieving 100% compilation success, investigated WASM validation failures (69.7%, 207/297 valid). Discovered critical bug in apply block code generation.

## The Bug

**Test Case** (`test_single_boolean.cln`):
```clean
functions:
	test()
		println:
			"test message"

start()
	test()
```

**WASM Output** (disassembled):
```
000420 func[40]:   // Generated for apply block
 000421: 01 7f     | local[0] type=i32
 000423: 0f        | return
 000424: 0b        | end

000426 func[41]:   // test() function
 000427: 01 7f     | local[0] type=i32
 000429: 10 28     | call 40           // Call func[40]
 00042b: 21 00     | local.set 0       // ERROR: Try to store void return value
 00042d: 0f        | return

000430 func[42] <_start>:
 000431: 10 29     | call 41           // Call test()
```

**Problem**:
1. The `println: "test message"` apply block generates an EMPTY function (func[40])
2. No actual print calls are being generated
3. `test()` calls this empty function and tries to store its (non-existent) return value
4. This causes "type mismatch in local.set, expected [i32] but got []"

## Root Cause

The apply block content is **not being executed during code generation**. The generated function (func[40]) is just a stub with a local variable and immediate return - no print statement execution.

**Evidence**:
- No calls to print functions (indices 0-2) found in entire WASM
- Only calls to func[40] and func[41]
- func[40] body contains no logic

## Code Paths Investigated

### `src/codegen/mod.rs`
Lines 9012-9036: `generate_function_apply_block_statement()`
```rust
for expression in expressions {
    if function_name == "print" || function_name == "println" || function_name == "printl" {
        self.generate_print_statement(expression, false, instructions)?;
    } else {
        let call_expr = Expression::Call(...);
        self.generate_expression(&call_expr, instructions)?;
        // Drop result if not void
    }
}
```

This code LOOKS correct but the print statement is never being generated.

### Attempted Fix #1: `generate_expression_statement()`
Modified to check for `WasmType::Unit` before dropping:
```rust
let expr_type = self.generate_expression(expr, instructions)?;
if expr_type != WasmType::Unit {
    instructions.push(Instruction::Drop);
}
```

**Result**: Made things WORSE - validation dropped from 69.7% to 68.7%
**Status**: REVERTED

## Key Findings

1. **Not in HIR/MIR**: FunctionApplyBlock stays as AST all the way to codegen
2. **Print functions registered**: Functions 0-2 are print imports
3. **Apply block code never executes**: The actual content (println) is not being converted to print calls
4. **Empty wrapper function**: Something creates func[40] as an empty stub

## Next Steps

1. **Add debug logging** to `generate_function_apply_block_statement()` to see if it's being called
2. **Trace code path** from AST statement to WASM generation
3. **Check if** there's an early return or condition that skips println generation
4. **Investigate** why a wrapper function is created instead of inline execution
5. **Compare** with working print statement cases (like simple `print("hello")`)

## Hypothesis

The apply block might be going through a different code path that:
1. Creates a wrapper function (func[40])
2. Should populate that function with print calls
3. But the population step is being skipped or failing silently
4. Then the parent function calls the empty wrapper and tries to use its result

## Files Involved

- `src/codegen/mod.rs` - Main codegen with `generate_function_apply_block_statement()`
- `src/codegen/statement_generator.rs` - Newer statement generator (possibly unused)
- `src/codegen/function_generator.rs` - Function body generation
- `src/ast/mod.rs` - FunctionApplyBlock AST definition

## Status

- **Compilation**: 100% success ✅
- **WASM Validation**: 69.7% (207/297)
- **local.set errors**: 31-40 files (most common error type)
- **This bug**: Blocks progress on ~30-40 test files

## Recommendation

Before proceeding further, need to:
1. Add comprehensive debug logging
2. Create minimal reproduction case
3. Trace exact code path from parser → AST → codegen → WASM
4. Understand why apply block content is lost during transformation
