# Session 2025-10-26: MIR Fix Investigation

## Date
2025-10-26 (Continuation - MIR Fix Attempt)

## Problem

Methods `getName()` and `getAge()` fail to generate with error:
```
ValueId(1) not found in local variable map during load_operand.
```

## Investigation Steps

### Step 1: Initial Fix Attempt

**Location**: `src/mir/mir_builder.rs:1111-1167`

**Fix Applied**: Modified the `TastExpressionKind::Variable` case to:
1. Check if variable is a class field
2. Generate proper load instructions:
   - Copy `this` (parameter 0)
   - GetElementPtr to get field address
   - Load field value

**Result**: Build succeeded, but **methods still fail with same error**

### Step 2: Added Debug Logging

**Location**: `src/mir/mir_builder.rs:1113-1127`

**Debug Output Added**:
```rust
eprintln!("DEBUG MIR: Looking for field '{}' in class with {} fields", name, class.fields.len());
eprintln!("DEBUG MIR: Field search result: {:?}", result);
eprintln!("DEBUG MIR: Found field '{}' at index {}, generating load instructions", name, field_index);
```

**Result**: **NO DEBUG OUTPUT** - Code path is never executed!

### Step 3: Analysis

**Key Discovery**: The field access code in `TastExpressionKind::Variable` is NOT being executed when processing `name` and `age` in method bodies.

**Possible Reasons**:
1. Variables `name` and `age` ARE found in scope_stack (so my code is never reached)
2. NO class_context exists when processing these methods
3. Expression is handled through a different code path entirely
4. **MOST LIKELY**: The MIR is being built from pre-processed method bodies where fields have already been resolved to incorrect ValueIds

## Root Cause Hypothesis

The issue is likely in **method generation**, not expression handling.

When class methods are generated:
1. The method body needs access to instance fields
2. Fields should be accessible as variables in the method scope
3. BUT: The fields aren't being added to the scope when the method MIR is built
4. OR: Fields are being resolved to incorrect ValueIds earlier in the pipeline

## Files To Investigate

### Priority 1: Method Generation
- `src/mir/mir_builder.rs` - How are class methods being processed?
- Look for where method bodies are converted to MIR
- Check if instance fields are added to method scope
- Search for "class method" or "method generation" comments

### Priority 2: Class Context
- How is `class_context` set up?
- Is it available during method body processing?
- Check if `this` parameter is being added correctly

### Priority 3: Field Resolution
- When does field name resolution happen?
- Is it during type checking (TAST generation)?
- Are fields being resolved to Symbol IDs that later become ValueIds?

## Next Steps

### Option 1: Search For Method Generation Code
```bash
cd src/mir
grep -n "method" mir_builder.rs | grep -i "generate\|build"
```

### Option 2: Add Broader Debug Logging
Add `eprintln!()` at the START of the Variable expression handling to see what variables are being processed:
```rust
TastExpressionKind::Variable { name } => {
    eprintln!("DEBUG MIR: Processing variable '{}'", name);
    eprintln!("DEBUG MIR: Has class_context: {}", context.class_context.is_some());
    eprintln!("DEBUG MIR: Scope stack depth: {}", context.scope_stack.len());
    // ... rest of code
}
```

### Option 3: Dump MIR Structure
Add debug output to see the MIR that's being generated for the methods:
```rust
eprintln!("DEBUG MIR: Function '{}' has {} instructions", function.name, function.body.len());
for (i, instr) in function.body.iter().enumerate() {
    eprintln!("DEBUG MIR:   Instr {}: {:?}", i, instr.operation);
}
```

## Expected Findings

If methods are being built correctly, we should see:
- Instance fields added to method scope OR
- Field access expressions being properly converted to load instructions OR
- Evidence of where the wrong ValueId(1) is being created

## Success Criteria

1. Find where method bodies are being processed in MIR builder
2. Identify why class fields aren't accessible in method scope
3. Fix the root cause (likely in method generation, not expression handling)
4. All 4 functions generate successfully
5. WASM validates
