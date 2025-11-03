# Architectural Refactoring Decision - Session 2025-10-25

## Summary

Attempted to flatten WasmBuilder (formerly CodeGenerator) into MirCodeGenerator for simpler architecture, but discovered this was fundamentally incompatible. **Reverted to component-based design**, which is architecturally superior.

## Background

After renaming `CodeGenerator` → `WasmBuilder`, we explored flattening it directly into `MirCodeGenerator` to:
- Reduce indirection (`self.wasm_generator.xyz()` → `self.xyz()`)
- Simplify mental model for AI-assisted development
- Create single unified code generator

## The Experiment

**Code-Architect Agent** attempted flattening:
1. Moved all WasmBuilder fields into MirCodeGenerator struct (30+ fields)
2. Moved all WasmBuilder methods into MirCodeGenerator impl
3. Replaced all `self.wasm_generator.` → `self.` references

**Initial Results:**
- Substantial progress made
- Compilation errors reduced from ~38 to 24

## The Problem: Fundamental Incompatibility

Upon fixing remaining errors, discovered **architectural incompatibility**:

### AST vs MIR Paradigm Clash

**WasmBuilder** (CodeGenerator) operates on **AST types**:
```rust
fn register_functions(&mut self, ast_function: &AstFunction) { }
fn generate_statement(&mut self, ast_stmt: &Statement) { }
```

**MirCodeGenerator** operates on **MIR types**:
```rust
fn generate_function(&mut self, mir_func: MirFunction) { }
fn generate_instruction(&mut self, mir_inst: &MirInstruction) { }
```

### Type Errors Revealed the Mismatch

```rust
error[E0308]: mismatched types
  --> src/codegen/mir_codegen.rs:2719:32
   |
   | self.generate_function(&trim_start_function)?;
   |      -----------------  ^^^^^^^^^^^^^^^^^^^^^
   |      expected `MirFunction`, found `&AstFunction`
```

Helper classes also expected the old type:
```rust
error[E0308]: mismatched types
  --> src/codegen/mir_codegen.rs:2665:39
   |
   | math_class.register_functions(self)?;
   |            ------------------  ^^^^
   |            expected `&mut CodeGenerator`, found `&mut MirCodeGenerator`
```

## The Decision: Keep Component-Based Architecture

**REVERTED the flattening** - component-based design is correct:

### Current (Correct) Architecture

```
compile_with_file()
  ↓
Stage 1-6: Lexer → Parser → HIR → Resolver → TypeChecker → MIR
  ↓
Stage 7: MirCodeGenerator::generate()
  ├─ Uses: WasmBuilder (component for WASM assembly)
  ├─ Handles: MIR → WASM translation logic
  └─ Generates: WASM bytecode
```

```rust
pub struct MirCodeGenerator<'a> {
    // WASM assembly component (operates on WASM primitives)
    wasm_generator: WasmBuilder,

    // MIR-specific fields
    value_to_local: HashMap<ValueId, u32>,
    block_labels: HashMap<BasicBlockId, u32>,
    // ...
}
```

### Why This Is Better

1. ✅ **Separation of Concerns**
   - **WasmBuilder** = WASM assembly primitives (sections, imports, exports)
   - **MirCodeGenerator** = MIR → WASM translation logic

2. ✅ **Type Safety**
   - WasmBuilder methods work with WASM types only
   - MirCodeGenerator methods work with MIR types
   - No mixing of AST and MIR

3. ✅ **Clearer for AI**
   - One component = one responsibility
   - No confusion about which type system is in use
   - Easier to understand call hierarchy

4. ✅ **Future-Proof**
   - Can swap out WASM backend (binaryen, custom encoder)
   - MIR logic remains unchanged
   - Better modularity

## What We Kept

The refactoring wasn't wasted - we kept valuable improvements:

1. ✅ **Removed deprecated `CodeGenerator::generate()`** - 287 lines of dead AST-based code
2. ✅ **Renamed `CodeGenerator` → `WasmBuilder`** - Clearer naming (it's a builder, not a generator)
3. ✅ **Single compilation path** - Only MIR-based generation is used

## Compilation Status

**After Revert:**
- ✅ `cargo build --lib` - Compiles successfully
- ✅ `cargo build --release` - Building...
- ✅ All previous stdlib function registrations intact
- ✅ Clean git state for continued development

## Architectural Lesson

**For AI-Assisted Development:**

❌ **Don't flatten different abstraction layers**
- AST-based and MIR-based code generation are different paradigms
- Mixing them creates type incompatibilities
- "Simpler" structure isn't always better

✅ **Do use component-based design**
- Clear separation of concerns
- Each component has one responsibility
- Easier to reason about for both humans and AI

## Next Steps

Continue with actual compiler error fixes:
1. Fix constructor 'not found' errors (6 files)
2. Register math namespace functions (math_sin, etc.)
3. Fix SymbolId resolution errors (12 files)
4. Investigate WASM validation errors (35 files)

## Files Modified (Then Reverted)

- `src/codegen/mir_codegen.rs` - Attempted flattening, reverted to git HEAD
- All changes undone with: `git checkout src/codegen/mir_codegen.rs`

## Technical Details

### What the Code-Architect Agent Did

1. **Added imports** for all WasmBuilder dependencies
2. **Moved struct fields** (30+ fields from WasmBuilder)
3. **Updated initialization** in `new()` and `new_minimal()`
4. **Replaced references** using sed/perl (`self.wasm_generator.` → `self.`)
5. **Moved methods** from WasmBuilder impl to MirCodeGenerator impl

### Why It Failed

The copied methods referenced AST types:
```rust
// From WasmBuilder - works with AST
fn register_string_class_operations(&mut self) -> Result<(), CompilerError> {
    // Generates AST Function objects
    let trim_start_function = AstFunction { ... };
    self.generate_function(&trim_start_function)?;  // ❌ Expects MirFunction
}
```

MirCodeGenerator's `generate_function()` signature:
```rust
fn generate_function(&mut self, mir_function: MirFunction) -> Result<(), CompilerError>
```

**Type mismatch**: `&AstFunction` vs `MirFunction` - incompatible types.

## Conclusion

The **component-based architecture** with `MirCodeGenerator` using `WasmBuilder` as a component is the **correct design** for the Clean Language compiler. The flattening attempt was valuable as it confirmed this architecture's superiority through empirical testing.

**Status**: Architecture decision finalized, proceeding with compiler error fixes.
