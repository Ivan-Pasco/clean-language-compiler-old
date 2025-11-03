# Session 2025-10-27: Field Inheritance Analysis

## Problem

Field inheritance is not working in the compiler. When child classes reference inherited fields from parent classes, the compiler throws "Field 'name' not found in class" errors.

## Test Case

**File**: `tests/cln/language/classes/15_classes_inheritance.cln`

```clean
class Animal
    string name
    integer age
    // ...

class Dog is Animal
    string breed

    functions:
        string makeSound()
            return name + " barks"  // ERROR: Field 'name' not found in class 'Dog'
```

## Root Cause

The field lookup code only searches fields directly in the current class, without traversing the inheritance chain to parent classes.

**Location**: `src/semantic/mod.rs:3130-3156` (PropertyAssignment handler)

```rust
Type::Object(class_name) => {
    if let Some(class) = self.class_table.get(&class_name).cloned() {
        // BUG: Only loops through fields defined directly in this class
        for field in &class.fields {
            if field.name == *property {
                // Found field
                return Ok(Type::Void);
            }
        }
        // ERROR: Field not found (doesn't check parent class)
        Err(CompilerError::type_error(
            &format!("Field '{}' not found in class '{}'", property, class_name),
            ...
        ))
    }
}
```

## Class Structure

From `src/ast/mod.rs:664-674`:

```rust
pub struct Class {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub description: Option<String>,
    pub base_class: Option<String>,  // ← Parent class name
    pub base_class_type_args: Vec<Type>,
    pub fields: Vec<Field>,          // ← Only direct fields
    pub methods: Vec<Function>,
    pub constructor: Option<Constructor>,
    pub location: Option<SourceLocation>,
}
```

## Solution Design

Create a helper method in `SemanticAnalyzer` to collect all fields including inherited ones:

```rust
impl SemanticAnalyzer {
    /// Collect all fields from a class including inherited fields from parent classes
    fn get_all_fields(&self, class_name: &str) -> Result<Vec<Field>, CompilerError> {
        let mut all_fields = Vec::new();
        let mut visited = HashSet::new();  // Prevent infinite loops in case of circular inheritance
        let mut current_class = Some(class_name.to_string());

        while let Some(name) = current_class {
            if visited.contains(&name) {
                return Err(CompilerError::type_error(
                    &format!("Circular inheritance detected involving class '{}'", name),
                    None,
                    None
                ));
            }
            visited.insert(name.clone());

            if let Some(class) = self.class_table.get(&name) {
                // Add fields from current class (in reverse order so parent fields come first)
                all_fields.extend(class.fields.iter().cloned());

                // Move to parent class
                current_class = class.base_class.clone();
            } else {
                break;
            }
        }

        // Reverse to get parent fields first, then child fields
        all_fields.reverse();
        Ok(all_fields)
    }
}
```

## Affected Locations

Need to replace `class.fields` with `self.get_all_fields(&class_name)?` in:

1. **`src/semantic/mod.rs:3134`** - PropertyAssignment field lookup
2. **Property Access** - Likely similar pattern for reading field values
3. **Type checking** - Any other location that validates field access
4. **MIR builder** - May need similar fixes in `src/mir/mir_builder.rs`
5. **Codegen** - May need similar fixes in `src/codegen/mod.rs`

## Search Results

Files containing "Field .* not found in class":
- `/src/mir/mir_builder.rs`
- `/src/typechecker/type_inference.rs`
- `/src/codegen/mod.rs`
- `/src/semantic/mod.rs`

All of these locations need to be updated to use the inheritance-aware field lookup.

## Implementation Steps

1. Add `get_all_fields()` helper method to `SemanticAnalyzer`
2. Replace all `class.fields` iterations with `get_all_fields()` calls
3. Test with `15_classes_inheritance.cln`
4. Run full class test suite to verify no regressions
5. Check for similar issues with method inheritance

## Testing

**Verification command**:
```bash
./target/release/clean-language-compiler compile \
  -i tests/cln/language/classes/15_classes_inheritance.cln \
  -o /tmp/test_inheritance.wasm \
  && wasm-validate /tmp/test_inheritance.wasm \
  && echo "✅ PASS"
```

## Related Issues

- Similar pattern may exist for method inheritance
- Constructor `base()` calls may also need inheritance chain awareness
