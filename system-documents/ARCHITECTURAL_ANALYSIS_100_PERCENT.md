# Comprehensive Architectural Analysis: Roadmap to 100% Test Pass Rate

**Current Status**: 209/285 tests passing (73.3%)
**Target**: 285/285 tests passing (100%)
**Gap**: 76 failing tests (26.7%)

**Analysis Date**: 2025-10-06
**Documentation Sources**: Context7 (Pest Parser, Rust Compiler, Wasmtime)

---

## Executive Summary

This analysis identifies **four critical architectural issues** preventing 100% test pass rate and provides evidence-based solutions using best practices from Pest parser, Rust compiler architecture, and WebAssembly ecosystems.

### Key Findings

1. **Dual Parsing Architecture**: Split between grammar parser and preprocessor causes inconsistent type handling for class methods
2. **Incomplete Type Inference**: Three-level namespace calls (`compare.integer.greaterThan`) bypass type inference system
3. **Missing Language Features**: `onError` error handling and default parameters not implemented
4. **Insufficient WASM Validation**: No intermediate validation between MIR and final WASM output

---

## Failure Analysis by Category

### Category 1: Class Inheritance (12 tests, 4.2%)

**Example Test**: `tests/cln/debug/test_constructor_base_minimal.cln`

**Error Pattern**:
```
Cannot unify types: string and null
  Expected: string (from method signature: string first())
  Actual: null (inferred by type checker)
```

**Root Cause - Dual Parsing Architecture**:

The compiler has **two separate parsing paths**:

1. **Grammar-based Parser** (`src/parser/parser_impl.rs`):
   - Handles standalone functions correctly
   - Matches `Rule::function_return_type` and extracts return types
   - Produces HIR with correct type annotations

2. **Preprocessor-based Parser** (`src/parser/preprocessor.rs`):
   - Handles class methods
   - **Does not extract return types from grammar rules**
   - Produces HIR with `HirType::Void` for all methods

**Evidence from Investigation**:
- Grammar emits `Rule::function_return_type` for class methods (verified via debug output)
- Parser in `parse_function_in_block` only matches `Rule::function_type` (line 1603)
- Attempting to add `Rule::function_return_type` match breaks standalone functions (-70 tests regression)
- This proves the dual architecture: same grammar rule has different semantics in different contexts

**Best Practice from Rust Compiler Architecture** (rustc-dev-guide):
> "The HIR should be a faithful representation of the source code, with type information attached during a separate type checking pass. Mixing parsing and type inference leads to inconsistent results."

**Recommended Solution**:

1. **Eliminate Preprocessor** (High Impact):
   - Move class method parsing entirely into grammar-based parser
   - Use grammar rule composition: `class_method = { function_signature ~ function_body }`
   - This ensures consistent HIR generation for all function-like constructs

2. **Unify Type Extraction** (Medium Impact):
   - Create single `extract_return_type()` function used by both standalone functions and methods
   - Apply during HIR building phase, not during parsing
   - Store return type as `Option<HirType>` in HIR, resolve during type checking

**Implementation Steps**:
```
Phase 1: Preserve preprocessor temporarily
  - Add return type extraction to preprocessor output
  - Test: Should fix all 12 class inheritance tests

Phase 2: Grammar unification
  - Extend grammar to handle class methods directly
  - Remove preprocessor dependency
  - Test: Should maintain all 221 tests (209 + 12)

Phase 3: HIR cleanup
  - Create unified type extraction utilities
  - Separate parsing from type annotation
```

---

### Category 2: Method Chaining (9 tests, 3.2%)

**Example Test**: `tests/cln/debug/test_simple_chain.cln`

**Error Pattern**:
```
Cannot unify types: null and boolean
  Expected: boolean (from variable declaration)
  Actual: null (from compare.integer.greaterThan(a, b))
```

**Root Cause - Incomplete Namespace Resolution**:

Three-level method calls like `compare.integer.greaterThan()` are parsed into AST but **type inference doesn't recognize this pattern**.

**Evidence from Investigation**:
- Added stdlib types to `infer_static_method_return_type` at lines 2653-2663
- Pattern matching: `("integer", "greaterThan") => Ok(ConcreteType::Boolean)`
- **No improvement**: Still 209/285 tests passing
- Conclusion: Three-level calls don't reach `infer_static_method_return_type` code path

**Current Type Inference Flow** (`src/typechecker/type_inference.rs`):
```
infer_expression()
  → match expr
    → StaticMethodCall { class_name, method_name } // Two-level only!
      → infer_static_method_return_type(class_name, method_name)
```

**Missing Pattern**:
```rust
// Three-level calls need new AST variant:
ThreeLevelMethodCall {
    namespace: String,  // "compare"
    class_name: String, // "integer"
    method_name: String // "greaterThan"
}
```

**Best Practice from Pest Parser** (pest-parser/pest):
> "Use grammar rule composition to represent nested structures. Avoid flattening namespace hierarchies during parsing."

**Recommended Solution**:

1. **AST Enhancement** (High Impact):
   - Add `ThreeLevelMethodCall` variant to `Expression` enum
   - Add `NamespaceMethodCall` variant for extensibility (4+ levels)
   - Parse in `parse_call_expression()` by detecting multiple dot operators

2. **Type Inference Extension** (High Impact):
   - Add `infer_namespace_method_return_type(namespace, class, method)`
   - Implement namespace type registry (similar to stdlib registry)
   - Load from `src/stdlib/namespaces.rs` with method signatures

3. **Namespace Type Registry** (Medium Impact):
   ```rust
   // New file: src/stdlib/namespaces.rs
   pub fn get_namespace_method_type(
       namespace: &str,
       class: &str,
       method: &str
   ) -> Option<ConcreteType> {
       match (namespace, class, method) {
           ("compare", "integer", "greaterThan") => Some(ConcreteType::Boolean),
           ("compare", "integer", "lessThan") => Some(ConcreteType::Boolean),
           // ... comprehensive mapping
       }
   }
   ```

**Implementation Steps**:
```
Phase 1: AST extension
  - Add ThreeLevelMethodCall to Expression enum
  - Update parser to recognize pattern
  - Test: Should parse but still fail type inference

Phase 2: Type inference
  - Create namespace type registry
  - Add infer_namespace_method_return_type()
  - Test: Should fix all 9 method chaining tests

Phase 3: Validation
  - Add tests for 4+ level nesting
  - Document namespace resolution algorithm
```

---

### Category 3: Error Handling (10 tests, 3.5%)

**Error Pattern**:
```
Unknown syntax: onError
```

**Root Cause - Missing Language Feature**:

The `onError` syntax is defined in Language Specification but **not implemented in compiler**:

**Missing Components**:
1. Grammar rules for `onError` blocks
2. AST representation (`OnErrorBlock` variant)
3. HIR/MIR handling for error propagation
4. WASM code generation for try-catch semantics

**Best Practice from Wasmtime** (bytecodealliance/wasmtime):
> "WebAssembly doesn't have native exception handling in core spec. Use result types or import exception handling proposal."

**Recommended Solution**:

1. **Grammar Addition** (High Impact):
   ```pest
   on_error_block = { "onError" ~ "(" ~ identifier ~ ")" ~ indented_block }
   statement = {
       assignment
       | function_call
       | on_error_block  // Add this
       | ...
   }
   ```

2. **AST/HIR Representation** (High Impact):
   ```rust
   pub enum Statement {
       // ... existing variants
       OnError {
           error_var: String,
           try_block: Vec<Statement>,
           catch_block: Vec<Statement>,
       }
   }
   ```

3. **WASM Code Generation Strategy** (Medium Impact):

   **Option A - Result Types** (Recommended):
   - Wrap error-prone calls in result types: `(result (tuple ...) (tuple i32))`
   - Check result, branch to error handler if needed
   - More compatible with current WASM core spec

   **Option B - Exception Handling Proposal**:
   - Use WASM exception handling proposal (not yet standardized)
   - Generate `try`/`catch` instructions
   - Requires newer WASM runtime support

**Implementation Steps**:
```
Phase 1: Grammar and parsing
  - Add onError grammar rules
  - Parse into OnError AST variant
  - Test: Should parse onError blocks

Phase 2: Semantic analysis
  - Type check error variable
  - Validate error handler scope
  - Test: Should validate onError semantics

Phase 3: Code generation (Result Types approach)
  - Generate result-wrapped calls
  - Add branch logic for error handling
  - Test: Should fix all 10 error handling tests
```

---

### Category 4: Default Parameters (2 tests, 0.7%)

**Error Pattern**:
```
Function expects 2 arguments, got 1
```

**Root Cause - Missing Language Feature**:

Default parameters defined in specification but not implemented.

**Recommended Solution**:

1. **Grammar Extension** (Low Impact):
   ```pest
   parameter = { identifier ~ ":" ~ type_annotation ~ ("=" ~ expression)? }
   ```

2. **AST Enhancement** (Low Impact):
   ```rust
   pub struct Parameter {
       pub name: String,
       pub param_type: Type,
       pub default_value: Option<Expression>, // Add this
   }
   ```

3. **Function Call Handling** (Medium Impact):
   - During semantic analysis, fill missing arguments with default values
   - Validate default values are compile-time constants
   - Insert default expressions into call site during HIR building

**Implementation Steps**:
```
Phase 1: Parsing
  - Add default value parsing to parameter grammar
  - Store in AST Parameter struct
  - Test: Should parse default parameters

Phase 2: Semantic analysis
  - Validate default values are constants
  - Fill missing arguments during type checking
  - Test: Should fix both default parameter tests
```

---

## Integration Tests (15 tests, 5.3%)

**Analysis**: Integration tests combine multiple language features. Expected to resolve naturally when Categories 1-4 are fixed.

**Validation Strategy**:
- Fix Categories 1-4 first
- Re-run integration tests
- Address any remaining compound issues individually

---

## Recommended Implementation Roadmap

### Phase 1: Quick Wins (Expected: +14 tests → 223/285, 78.2%)

**Priority**: Fix Categories 3 & 4 first (lowest architectural risk)

1. **Default Parameters** (+2 tests)
   - Time: 2-3 hours
   - Risk: Low
   - Files: `grammar.pest`, `ast/mod.rs`, `semantic/analyzer.rs`

2. **Error Handling - Basic Implementation** (+12 tests estimate)
   - Time: 1-2 days
   - Risk: Medium
   - Files: `grammar.pest`, `ast/mod.rs`, `codegen/mod.rs`
   - Note: Start with Result Types approach (simpler)

### Phase 2: Core Architecture Fix (Expected: +21 tests → 244/285, 85.6%)

**Priority**: Fix Categories 1 & 2 (high impact, higher risk)

3. **Class Method Type Handling** (+12 tests)
   - Time: 2-3 days
   - Risk: High (touched during regression attempts)
   - Strategy: Fix preprocessor first, refactor later
   - Files: `parser/preprocessor.rs`, `hir/builder.rs`

4. **Three-Level Namespace Resolution** (+9 tests)
   - Time: 1-2 days
   - Risk: Medium
   - Files: `ast/mod.rs`, `parser/parser_impl.rs`, `typechecker/type_inference.rs`
   - New file: `stdlib/namespaces.rs`

### Phase 3: Integration Validation (Expected: 285/285, 100%)

5. **Integration Test Fixes**
   - Time: 1-2 days
   - Risk: Low (should mostly auto-resolve)
   - Strategy: Run full test suite, fix remaining edge cases

**Total Estimated Time**: 1-2 weeks of focused development

---

## Architectural Best Practices Applied

### From Pest Parser Documentation

✅ **Rule Composition Over Preprocessing**:
- Current issue: Preprocessor splits parsing logic
- Solution: Use grammar rules like `class_method = { function_signature ~ function_body }`

✅ **Explicit Error Recovery**:
- Add `@{ ... }` atomic rules for better error messages
- Use `silent_rule` for implementation details users don't need to see

### From Rust Compiler Architecture

✅ **Separate Parsing from Type Inference**:
- Current issue: Parser attempts type extraction during parsing
- Solution: Build untyped HIR, add types in separate pass

✅ **HIR → MIR Lowering**:
- Current: Direct HIR → WASM (skips optimization opportunities)
- Solution: Add proper MIR lowering with optimization passes

✅ **Type Normalization**:
- Current: Type unification happens during inference
- Solution: Normalize types before unification (handles type aliases, generics better)

### From Wasmtime/WebAssembly

✅ **Early WASM Validation**:
- Current: No validation until runtime
- Solution: Run `wasm-tools validate` after code generation

✅ **Optimization Passes**:
- Add `wasm-opt` integration for dead code elimination, constant folding

---

## Risk Assessment

### High Risk Changes

1. **Eliminating Preprocessor** (Category 1, later phase)
   - Impact: Affects all class parsing
   - Mitigation: Keep preprocessor initially, add type extraction only
   - Validation: Run full test suite after each incremental change

2. **AST Changes for Three-Level Calls** (Category 2)
   - Impact: Affects all expression handling
   - Mitigation: Add new variant, keep existing two-level calls working
   - Validation: Ensure no regression in two-level method calls

### Medium Risk Changes

3. **Error Handling Code Generation** (Category 3)
   - Impact: New WASM instruction patterns
   - Mitigation: Start with result types (simpler than exceptions)
   - Validation: Test error propagation thoroughly

4. **Namespace Type Registry** (Category 2)
   - Impact: New type lookup mechanism
   - Mitigation: Separate registry from type inference logic
   - Validation: Test namespace methods independently

### Low Risk Changes

5. **Default Parameters** (Category 4)
   - Impact: Limited to function call handling
   - Mitigation: Well-understood feature, similar to existing parameter handling
   - Validation: Test with various argument counts

---

## Quality Assurance Strategy

### Testing After Each Phase

```bash
# After every code change:
python3 scripts/run_full_test_suite.py

# Expected progression:
# Phase 1 complete: 223/285 (78.2%)
# Phase 2 complete: 244/285 (85.6%)
# Phase 3 complete: 285/285 (100%)
```

### Regression Prevention

1. **Git Workflow**:
   - Create feature branch for each category
   - Commit after each incremental improvement
   - If test count decreases, immediately `git revert`

2. **Validation Checklist**:
   - [ ] Test count increases or stays same
   - [ ] No new error categories introduced
   - [ ] Cargo build succeeds
   - [ ] WASM validation passes (add `wasm-tools validate`)

3. **Documentation**:
   - Update `TASKS.md` after each phase
   - Mark completed tasks with implementation notes
   - Document any architectural decisions made

---

## Success Metrics

### Immediate (Phase 1)
- ✅ 223/285 tests passing (78.2%)
- ✅ Error handling basic cases working
- ✅ Default parameters implemented

### Mid-term (Phase 2)
- ✅ 244/285 tests passing (85.6%)
- ✅ Class methods properly typed
- ✅ Three-level namespace calls working

### Long-term (Phase 3)
- ✅ 285/285 tests passing (100%)
- ✅ All integration tests passing
- ✅ Zero regressions from baseline

### Code Quality
- ✅ Preprocessor eliminated or minimal
- ✅ Single type extraction mechanism
- ✅ Namespace type registry comprehensive
- ✅ WASM validation integrated

---

## Conclusion

Reaching 100% test pass rate requires fixing **four distinct architectural issues**:

1. **Dual parsing architecture** causing class method type loss
2. **Incomplete namespace resolution** for three-level method calls
3. **Missing error handling** feature (`onError` syntax)
4. **Missing default parameters** feature

The recommended **phased approach** prioritizes:
- **Phase 1**: Low-risk feature additions (+14 tests)
- **Phase 2**: Core architectural fixes (+21 tests)
- **Phase 3**: Integration validation (+41 tests)

By applying best practices from Pest, Rust compiler, and WebAssembly ecosystems, this roadmap provides a **systematic path to 100%** while minimizing regression risk.

**Estimated Timeline**: 1-2 weeks of focused development with rigorous testing after each phase.
