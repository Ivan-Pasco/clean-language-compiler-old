# For Loop ValueId Tracking Fix - November 2, 2025

## Problem Identified

**Error**: "ValueId(2) not found in local variable map during store_to_local"

**Affected Files** (minimum 3 confirmed):
1. `18_control_flow_loops.cln`
2. `16_classes_polymorphism.cln`
3. `20_async_parallel.cln`

## Root Cause

The `TastStatement::For` handler in `src/mir/mir_builder.rs` (lines 1059-1269) was creating **four unregistered ValueIds**:

### 1. Loop Index Counter (index_value_id)
**Line**: 1075-1076
```rust
let index_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;
// NO register_temp_local() call!
```

**Purpose**: Tracks the current iteration index (starts at 0, increments each loop)

### 2. Array/Range Length (length_value_id)
**Line**: 1134-1135
```rust
let length_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;
// NO register_temp_local() call!
```

**Purpose**: Stores the length of the iterable for bounds checking

### 3. Loop Condition Result (condition_value_id)
**Line**: 1149-1150
```rust
let condition_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;
// NO register_temp_local() call!
```

**Purpose**: Stores the result of `index < length` comparison

### 4. Incremented Index Value (incremented_value_id)
**Line**: 1229-1230
```rust
let incremented_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;
// NO register_temp_local() call!
```

**Purpose**: Stores `index + 1` for the next iteration

## Solution Implemented

Added `register_temp_local()` calls for all four ValueIds:

### Fix 1: Register Loop Index (lines 1078-1084)
```rust
// Create iterator index variable (starts at 0)
let index_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;

// CRITICAL FIX: Register index ValueId as a local
self.register_temp_local(
    context,
    index_value_id,
    MirType::I32,
    location.clone(),
);
```

### Fix 2: Register Array Length (lines 1145-1151)
```rust
// Get array length (for bounds checking)
let length_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;

// CRITICAL FIX: Register length ValueId as a local
self.register_temp_local(
    context,
    length_value_id,
    MirType::I32,
    location.clone(),
);
```

### Fix 3: Register Loop Condition (lines 1168-1174)
```rust
// Compare index < length
let condition_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;

// CRITICAL FIX: Register condition ValueId as a local
self.register_temp_local(
    context,
    condition_value_id,
    MirType::I32, // Boolean represented as I32
    location.clone(),
);
```

### Fix 4: Register Incremented Index (lines 1232-1238)
```rust
// Increment index: index = index + 1
let incremented_value_id = ValueId(context.function.next_value_id);
context.function.next_value_id += 1;

// CRITICAL FIX: Register incremented ValueId as a local
self.register_temp_local(
    context,
    incremented_value_id,
    MirType::I32,
    location.clone(),
);
```

## Loop Structure Affected

This fix applies to both types of `iterate` loops in Clean Language:

### Range-Based Iteration
```clean
iterate i in 0 to 3
    print(i)
```

Compiled to:
- `index_value_id` = 0 (starting index)
- `length_value_id` = 3 (end of range)
- Loop while `index < length`

### Collection Iteration
```clean
list<integer> numbers = [1, 2, 3, 4, 5]
iterate num in numbers
    print(num)
```

Compiled to:
- `index_value_id` = 0 (array index)
- `length_value_id` = 5 (array length)
- `iterator_value_id` = numbers[index] (current element)
- Loop while `index < length`

## Expected Impact

**Minimum 3 files will be fixed**:
- `18_control_flow_loops.cln` - Has both range and collection iteration
- `16_classes_polymorphism.cln` - Uses loops in test functions
- `20_async_parallel.cln` - Uses loops with async operations

**Could fix additional files** that use `iterate` loops but currently have different ValueId errors.

## Technical Details

### Why These Were Missed

The For loop handler has proper registration for the `iterator_value_id` (the loop variable visible to user code):

```rust
// Create local for iterator variable
let iterator_local = MirLocal {
    name: Some(iterator_name.clone()),
    local_type: MirType::I32,
    is_mutable: false,
    location: location.clone(),
};
context.function.locals.insert(iterator_value_id, iterator_local);
```

But it was missing registration for the **internal loop control variables** (index, length, condition, incremented).

### Comparison to ArrayAccess Fix

Similar issue to the ArrayAccess fix from earlier:
- **ArrayAccess**: Pointer and loaded value not registered
- **For loops**: Index, length, condition, and increment not registered

Both cases involved **intermediate values** used internally by MIR operations but not exposed to user code.

## Files Modified

**src/mir/mir_builder.rs**:
- Lines 1078-1084: Added index_value_id registration
- Lines 1145-1151: Added length_value_id registration
- Lines 1168-1174: Added condition_value_id registration
- Lines 1232-1238: Added incremented_value_id registration

## Next Steps

1. Build compiler with fixes
2. Test `18_control_flow_loops.cln` to verify fix
3. Run comprehensive test to measure improvement
4. Investigate remaining ValueId errors (ValueId(4), ValueId(23), ValueId(50))

---

**Status**: Fix implemented, awaiting build and test results
