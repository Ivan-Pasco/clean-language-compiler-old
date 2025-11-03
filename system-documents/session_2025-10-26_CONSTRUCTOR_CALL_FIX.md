# Session 2025-10-26: Constructor Call Fix

## Root Cause Identified

**Location**: `src/mir/mir_builder.rs:1386-1434` (FunctionCall handler)

**Problem**: When `Animal("Fluffy", 5)` is compiled, the WASM call only passes 2 arguments, but the constructor expects 3 (this, name, age).

**WASM Evidence**:
```wasm
000472: 20 05          | local.get 5      <- "Fluffy"
000474: 20 0b          | local.get 11     <- age=5
000476: 10 2b          | call 43          <- Constructor expects (i32, i32, i32)
```

The missing argument is the instance pointer (`this`).

## Solution

Modify the FunctionCall handler to:

1. **Detect constructor calls**:
```rust
// Check if this is a constructor call
let symbol = self.symbol_table.get_symbol(function_symbol_id)?;
let is_constructor = matches!(symbol.kind, SymbolKind::Constructor { .. });
```

2. **For constructors, allocate instance memory**:
```rust
if let SymbolKind::Constructor { class_id, .. } = symbol.kind {
    // Get class to determine instance size
    let class_symbol = self.symbol_table.get_symbol(class_id)?;
    if let SymbolKind::Class { fields, .. } = &class_symbol.kind {
        let instance_size = fields.len() * 4; // 4 bytes per i32 field

        // Generate call to mem_alloc
        let alloc_result = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        self.register_temp_local(context, alloc_result, MirType::I32, expression.location.clone());

        let alloc_call = MirInstruction {
            dest: Some(alloc_result),
            operation: MirOperation::Call {
                function: MirOperand::BuiltinFunction("mem_alloc".to_string()),
                arguments: vec![MirOperand::Const(instance_size as i64)],
            },
            location: expression.location.clone(),
        };

        self.add_instruction(context, alloc_call);

        // Prepend instance pointer to arguments
        mir_arguments.insert(0, MirOperand::Value(alloc_result));
    }
}
```

3. **Call the constructor with instance pointer**:
   - The rest of the code remains the same
   - Constructor is called with (instance_ptr, user_arg1, user_arg2, ...)
   - Returns instance_ptr

## Expected Result

After fix, WASM should generate:
```wasm
i32.const 8          <- Allocate 8 bytes (2 fields)
call mem_alloc       <- Returns instance pointer
local.set temp       <- Store instance pointer
local.get temp       <- Push instance pointer (arg 0)
local.get 5          <- Push "Fluffy" (arg 1)
local.get 11         <- Push 5 (arg 2)
call 43              <- Constructor(this, name, age)
```

## Files to Modify

- `src/mir/mir_builder.rs:1401-1428` - Add constructor detection and instance allocation

## Testing

Test with:
```bash
./target/release/clean-language-compiler compile -i tests/cln/language/classes/07_class_definitions.cln -o /tmp/test_class.wasm
wasm-validate /tmp/test_class.wasm
```

Should produce valid WASM with no type mismatches.
