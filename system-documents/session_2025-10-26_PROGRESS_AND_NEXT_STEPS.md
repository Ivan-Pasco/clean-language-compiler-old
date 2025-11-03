# Session 2025-10-26: Class Method Fix Progress and Next Steps

## Date
2025-10-26 (Continuation)

## Fixes Applied

### 1. Fixed PropertyAccess Field Index Lookup
**Location**: `src/mir/mir_builder.rs:1666-1685`

**Problem**: PropertyAccess was using SymbolId values (e.g., 204, 205) as field indices instead of positional indices (0, 1, 2).

**Fix**: Changed to look up actual field position using `.position()`:
```rust
let field_index_value = if let Some(ref class) = context.class_context {
    class.fields.iter()
        .position(|f| f.symbol_id == *property_symbol)
        .ok_or_else(|| vec![CompilerError::validation_error(...)])? as i64
} else {
    return Err(...);
};
```

**Result**: ✅ Field indices now correctly resolve to 0 and 1 instead of 204 and 205.

### 2. Fixed 'this' Variable Handling
**Location**: `src/mir/mir_builder.rs:1098-1110`

**Problem**: When 'this' was referenced, code incorrectly assumed it was `ValueId(0)`.

**Fix**: Changed to get the actual ValueId from the first parameter:
```rust
if name == "this" && context.class_context.is_some() {
    if let Some(first_param) = context.function.parameters.first() {
        return Ok(first_param.value_id);
    } else {
        return Err(...);
    }
}
```

**Result**: ✅ 'this' now correctly uses the actual parameter ValueId.

### 3. Fixed Implicit Field Access
**Location**: `src/mir/mir_builder.rs:1139-1147`

**Problem**: Same issue - assumed 'this' was `ValueId(0)`.

**Fix**: Changed to get 'this' ValueId from first parameter:
```rust
let this_value_id = if let Some(first_param) = context.function.parameters.first() {
    first_param.value_id
} else {
    return Err(...);
};
```

**Result**: ✅ Implicit field access now uses correct 'this' ValueId.

### 4. Added Implicit 'this' Parameter to Class Methods
**Location**: `src/mir/mir_builder.rs:298-317`

**Problem**: Class methods in TAST don't include 'this' as a parameter, causing methods to have empty parameter lists.

**Fix**: Inject 'this' parameter as first parameter when `class_context` is present:
```rust
if let Some(class_ctx) = class_context {
    let this_value_id = ValueId(context.function.next_value_id);
    context.function.next_value_id += 1;

    let this_param = MirParameter {
        value_id: this_value_id,
        name: "this".to_string(),
        param_type: MirType::I32, // Instance pointer
        location: tast_function.location.clone(),
    };

    context.function.parameters.push(this_param);
    current_scope.insert("this".to_string(), this_value_id);
}
```

**Result**: ✅ Methods now have 'this' as their first parameter.

## Current Status

### What Works
- ✅ Field indices resolve correctly (0, 1 instead of 204, 205)
- ✅ 'this' variable references use correct ValueId from parameters
- ✅ Class methods have 'this' parameter injected
- ✅ Constructor generates and validates successfully
- ✅ start function generates and validates successfully

### What Still Fails
- ❌ `getName()` and `getAge()` methods fail during codegen with:
  ```
  ValueId(1) not found in local variable map during load_operand
  ```

### Debug Output Shows
```
DEBUG MIR PROPACCESS: Resolved field index: 0   ✅ Correct!
DEBUG MIR VAR: Processing variable 'this'        ✅ Found in scope!
DEBUG: Successfully generated function 'constructor'  ✅ Works!
DEBUG: ERROR generating function 'getName': ValueId(1) not found  ❌ Fails!
```

## Root Cause Analysis

The MIR building phase works correctly:
1. Methods have 'this' parameter
2. Field indices are correct
3. PropertyAccess expressions build correctly

**The bug is in WASM codegen phase!**

When codegen processes the GetElementPtr instruction, it can't find ValueId(1) in the `value_to_local` map. This suggests:

1. **Possible Issue 1**: The GetElementPtr instruction's result ValueId isn't being properly added to the local_variable_map
2. **Possible Issue 2**: The method's 'this' parameter isn't being added to value_to_local during function setup
3. **Possible Issue 3**: There's a different ValueId being referenced that we're not tracking

## Next Investigation Steps

### Step 1: Add Debug Logging to Codegen
Add logging in `src/codegen/mir_codegen.rs` around line 285-289 to see what parameters are being added:

```rust
// Allocate locals for function parameters
for param in &function.parameters {
    eprintln!("DEBUG CODEGEN: Adding parameter '{}' with ValueId({}) to local {}",
        param.name, param.value_id.0, self.next_local_index);
    let local_index = self.next_local_index;
    self.value_to_local.insert(param.value_id, local_index);
    self.next_local_index += 1;
}
```

### Step 2: Add Debug Logging to GetElementPtr Handler
In `src/codegen/mir_codegen.rs` around line 814-862, add logging:

```rust
MirOperation::GetElementPtr { base, indices } => {
    eprintln!("DEBUG CODEGEN GEP: base={:?}, indices={:?}", base, indices);
    eprintln!("DEBUG CODEGEN GEP: value_to_local map has {} entries",
        self.value_to_local.len());

    // Existing load_operand code...
}
```

### Step 3: Check What ValueIds Are in Methods
Add debug output to show all ValueIds in the method's MIR:

```rust
eprintln!("DEBUG MIR: Method '{}' has {} instructions", function.name, function.blocks.len());
for (block_id, block) in &function.blocks {
    for instr in &block.instructions {
        eprintln!("DEBUG MIR:   Instr dest={:?}, op={:?}",
            instr.dest, std::mem::discriminant(&instr.operation));
    }
}
```

### Step 4: Verify GetElementPtr Creates Load Instruction
Check if GetElementPtr result is supposed to automatically load, or if we need a separate Load instruction after GetElementPtr.

## Success Criteria

1. ✅ All 4 functions (start, constructor, getName, getAge) generate successfully
2. ✅ WASM validates with no errors
3. ✅ 19 class-related test files pass WASM validation
4. ✅ Validation rate improves from 73% to 79%

## Files Modified

- `src/mir/mir_builder.rs:298-317` - Added 'this' parameter injection
- `src/mir/mir_builder.rs:1098-1110` - Fixed 'this' variable handling
- `src/mir/mir_builder.rs:1139-1147` - Fixed implicit field access
- `src/mir/mir_builder.rs:1666-1685` - Fixed PropertyAccess field index lookup

## Key Insight

**The problem has shifted from MIR building to WASM codegen.** The MIR is now being built correctly with proper parameters and field indices, but the codegen phase can't find the ValueIds it needs when generating WASM instructions.

This is likely a mismatch between how MIR creates ValueIds and how codegen tracks them in the `value_to_local` map.
