# ROOT CAUSE FOUND: Apply Block Not Lowered to HIR

## Date: 2025-10-23 (continued debugging)

## CRITICAL DISCOVERY

### The Problem
FunctionApplyBlock statements like `println: "message"` generate empty wrapper functions instead of executing their content.

### The Root Cause
**FunctionApplyBlock is NOT being lowered to HIR!**

### Evidence

1. **Debug Logging**: Added logging to `generate_function_apply_block_statement()` in codegen
   - Result: NO output - function never called!

2. **HIR Statement Enum** (`src/hir/mod.rs:155-220`):
   - Has: `Print`, `Expression`, `If`, `While`, `For`, `LaterAssignment`
   - MISSING: FunctionApplyBlock, MethodApplyBlock, TypeApplyBlock

3. **HIR Builder**: Searched `src/hir/` for FunctionApplyBlock
   - Result: ZERO matches - not handled at all!

4. **MIR**: Searched for FunctionApplyBlock
   - Result: Not present in MIR either

5. **Compilation Pipeline** (`src/lib.rs:67-178`):
   ```
   AST → HIR → Resolver → TypeChecker → TAST → MIR → WASM
   ```

### What's Happening

1. Parser creates AST with `Statement::FunctionApplyBlock`
2. Semantic analyzer validates it (sees it in AST)
3. HIR builder is supposed to lower it to HIR but...
4. **HIR builder doesn't handle FunctionApplyBlock!**
5. FunctionApplyBlock gets lost/skipped
6. Empty function stub generated somehow
7. Parent function calls empty stub
8. Tries to store non-existent return value → validation error

### The Fix

Need to add lowering logic in HIR builder to transform:

**AST**:
```rust
Statement::FunctionApplyBlock {
    function_name: "println",
    expressions: [Literal("test message")],
    ...
}
```

**HIR** (should become):
```rust
HirStatement::Print {
    expression: HirExpression::Literal("test message"),
    newline: true,
    location: ...
}
```

For non-print functions, should become multiple `HirStatement::Expression` with function calls.

### Files to Modify

1. **`src/hir/hir_builder.rs`**:
   - Add `lower_function_apply_block()` method
   - Handle print/println/printl specially → `HirStatement::Print`
   - Handle other functions → multiple `HirStatement::Expression` with calls

2. **Similar for**:
   - `MethodApplyBlock` → multiple method calls
   - `TypeApplyBlock` → multiple variable declarations
   - `ConstantApplyBlock` → multiple constant declarations

### Why This Wasn't Obvious

1. Semantic analyzer validates AST directly (works fine)
2. Codegen has handlers for apply blocks (never reached!)
3. No error thrown - just silently skipped
4. Something generates empty wrapper function (need to investigate where)

### Next Steps

1. ✅ Add debug logging (DONE - found no calls)
2. ✅ Trace compilation pipeline (DONE - found HIR gap)
3. **→ Add HIR lowering for apply blocks**
4. Test and verify WASM generation improves
5. Handle other apply block types if needed

### Expected Impact

Once FunctionApplyBlock is properly lowered to HIR:
- Print statements will generate in WASM
- No more empty wrapper functions
- local.set errors should disappear
- Expect validation rate to jump from 69.7% to ~85%+

## This Is The Bug!

The apply blocks work in the old direct-to-codegen path but fail in the modern 7-stage pipeline because they're not being lowered from AST to HIR.
