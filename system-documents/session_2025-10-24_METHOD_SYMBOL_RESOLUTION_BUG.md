# Session 2025-10-24 Continued: Method Symbol Resolution Bug

## 🔍 Critical Discovery: Root Cause of 42 "local.set empty stack" Errors

**Validation Rate**: 176/295 (59.7%)
**Error Category**: local.set type mismatch (empty stack) - 42 files (52.5% of all WASM validation errors)
**Root Cause Identified**: Instance methods not getting symbol IDs during resolution

---

## Investigation Timeline

### 1. Initial Error Pattern
```
/tmp/minimal_compliance_test.wasm:0000461: error: type mismatch in local.set, expected [i32] but got []
```

### 2. WASM Disassembly Analysis
```wasm
000453: 20 05          | local.get 5      # Get object 'p'
000455: 41 04          | i32.const 4
000457: 6a             | i32.add
000458: 20 05          | local.get 5
00045a: 28 02 00       | i32.load 2 0
00045d: 10 00          | call 0 <env.print>   # BUG: Should call getX(), calling print!
00045f: 21 04          | local.set 4          # Tries to store void result → EMPTY STACK
```

**Discovery**: Method call `p.getX()` is being compiled as `call 0` (print) instead of the actual getX method!

### 3. Debug Logging Results
```
[DEBUG] Call SymbolId(203) → 'constructor' → index 40 ✅
[DEBUG] Call SymbolId(0) → 'print' → index 0      ← getX() becomes print!
[DEBUG] Call SymbolId(0) → 'print' → index 0      ✅
```

**Discovery**: The `getX()` method call has `SymbolId(0)` which resolves to 'print'.

---

## 🐛 The Bug

### Location: `src/resolver/resolver_impl.rs:1202-1204`

```rust
// Regular instance method call
let resolved_receiver = self.resolve_expression(receiver)?;

let mut resolved_arguments = Vec::new();
for arg in arguments {
    resolved_arguments.push(self.resolve_expression(arg)?);
}

// Method resolution is complex and depends on receiver type
// For now, we'll resolve it as None (built-in method)
let method_symbol_id = None;  // ← BUG: Always None for instance methods!

Ok(ResolvedHirExpression::MethodCall {
    receiver: Box::new(resolved_receiver),
    method: method.clone(),
    method_symbol_id,  // ← None becomes SymbolId(0) in type checker
    arguments: resolved_arguments,
    location: location.clone(),
})
```

### Flow Through Compilation Pipeline

1. **Resolver** (`src/resolver/resolver_impl.rs:1204`):
   ```rust
   method_symbol_id = None
   ```

2. **Type Checker** (`src/typechecker/type_inference.rs:1913`):
   ```rust
   method_symbol: method_symbol_id
       .unwrap_or(SymbolId(0))  // None → SymbolId(0)
   ```

3. **MIR Builder** (`src/mir/mir_builder.rs:1659`):
   ```rust
   function: MirOperand::Function(function_symbol)  // SymbolId(0)
   ```

4. **Codegen** (`src/codegen/mir_codegen.rs:785-795`):
   ```rust
   get_function_name_by_symbol(SymbolId(0))  // → "print"
   function_map.get("print")                  // → index 0
   Call(0)                                    // → calls print instead of getX!
   ```

---

## 💥 Impact

### Why This Causes "local.set empty stack" Errors

1. **Source**: `integer value = p.getX()`
2. **Expected**: Call getX() → returns integer → store in local
3. **Actual**: Call print() → returns void → **stack is empty** → local.set fails
4. **WASM Error**: `type mismatch in local.set, expected [i32] but got []`

### Files Affected

**42 files** (52.5% of all WASM validation errors) are caused by this bug, including:
- minimal_compliance_test.cln
- 34_list_behaviors.cln
- 08_class_inheritance.cln
- 48_method_style_syntax.cln
- matrix_operations_comprehensive.cln
- And 37 more...

---

## 🎯 The Architectural Challenge

### Why Methods Can't Be Resolved During Resolution Phase

The resolver runs **before** type inference:
1. **Resolver**: Resolves names to symbol IDs
2. **Type Checker**: Infers types
3. **MIR Builder**: Generates intermediate representation
4. **Codegen**: Generates WASM

**Problem**: To look up a method, we need to know the receiver's type (class). But the resolver doesn't have type information yet!

```clean
Point p = Point(3, 4)
integer value = p.getX()  // Resolver doesn't know 'p' is type 'Point'!
```

---

## 🔧 Possible Solutions

### Option 1: Defer Method Resolution to Type Checker ✅ RECOMMENDED

**Approach**: Keep `method_symbol_id = None` in resolver, but actually resolve it in the type checker once we know the receiver's type.

**Implementation** (`src/typechecker/type_inference.rs:1909-1920`):
```rust
TastExpressionKind::MethodCall {
    receiver: Box::new(tast_receiver),
    method_name: method.clone(),
    method_symbol: method_symbol_id.unwrap_or_else(|| {
        // CRITICAL FIX: Resolve method symbol from receiver type
        if let Some(class_type) = extract_class_type(&tast_receiver.expr_type) {
            if let Some(method_sym) = self.symbol_table.lookup_class_member(class_type, &method) {
                return method_sym;
            }
        }
        SymbolId(0)  // Fallback for built-in methods
    }),
    arguments: tast_arguments,
    type_args: Vec::new(),
}
```

**Pros**:
- Minimal architecture changes
- Type information is available
- Can look up method in class symbol table

**Cons**:
- Type checker becomes more complex
- Mixes resolution and type checking

### Option 2: Two-Pass Resolution

**Approach**: First pass resolves variables, second pass resolves methods after type inference.

**Pros**:
- Clean separation of concerns

**Cons**:
- Significant architectural changes
- More complex overall

### Option 3: Store Receiver Type in HIR

**Approach**: Add type annotations to HIR during parsing, use them in resolver.

**Pros**:
- Resolver has needed information

**Cons**:
- Requires running type inference earlier
- May not work for type inference

---

## 📊 Expected Impact of Fix

### Before Fix
- **Valid WASM**: 176/295 (59.7%)
- **Invalid WASM**: 80 files
  - local.set empty stack: 42 files (52.5%)
  - Other errors: 38 files (47.5%)

### After Fix (Projected)
- **Valid WASM**: ~218/295 (73.9%)
- **Improvement**: +42 files (+14.2%)
- **Invalid WASM**: ~38 files
  - implicit_return: 15 files
  - other errors: 23 files

**This would be a massive improvement!** 42 files is the single largest error category.

---

## 🎯 Recommended Next Steps

1. ✅ **Implement Option 1**: Add method resolution in type checker
2. ✅ **Extract class type helper**: Create utility to get class from ConcreteType
3. ✅ **Look up method symbol**: Use symbol_table.lookup_class_member()
4. ✅ **Test with minimal_compliance_test.cln**: Verify getX() resolves correctly
5. ✅ **Run comprehensive test**: Measure improvement across all 295 files
6. ✅ **Update documentation**: Record the fix and improvement

---

## 📝 Key Learnings

1. **Architecture Matters**: The order of compilation phases (resolution → type checking) creates dependencies
2. **TODOs Are Technical Debt**: The "for now" comment from line 1203 shows unfinished work
3. **Debug Logging Is Essential**: Adding debug output revealed the SymbolId(0) issue immediately
4. **Systematic Investigation**: Following the data flow through all compilation phases pinpointed the exact bug

---

**Session Status**: 🔍 Root cause identified, solution designed, ready for implementation
