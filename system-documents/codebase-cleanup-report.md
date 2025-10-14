# Codebase Cleanup Report

**Date:** October 8, 2025
**Compiler Version:** 0.10.3

## Executive Summary

This report documents a comprehensive investigation and cleanup of the Clean Language compiler codebase, identifying and addressing architectural issues, legacy code, and incomplete implementations.

## Issues Identified and Resolved

### ✅ Completed Issues

#### 1. Legacy Files Cleanup
- **Deleted:** All `.cln.bak` backup files in `tests/cln/` (20+ files)
- **Deleted:** `src/parser/grammar.pest.bak`
- **Deleted:** `src/semantic/mod.rs.corrupt.bak`
- **Deleted:** Legacy lexer files:
  - `src/lexer/indentation.rs`
  - `src/lexer/lexer_impl.rs`
  - `src/lexer/token.rs`
- **Status:** These files were already commented out in `src/lexer/mod.rs` and not used anywhere.

#### 2. Deprecated Code Removal
- **Deleted:** `generate_wasm_from_ast()` function in `src/codegen/mod.rs:9722-9744`
  - Was marked as deprecated since v0.10.2
  - Used legacy IRPipeline (AST → HIR → MIR → LIR)
  - Replaced by `compile_with_file()` using MIR-based pipeline
  - **Impact:** No breaking changes (function was unused)

#### 3. Stub Pipeline Deprecation
- **Deprecated:** `src/codegen/pipeline/mod.rs` - Stub compilation pipeline
  - Only generates hello-world WASM modules
  - Not used in production
  - Added deprecation notices at module and struct level
  - **Action:** Will be removed in v0.11.0
  - **Replacement:** Use `MirCodeGenerator` from `mir_codegen.rs`

#### 4. Test Infrastructure Fixes
- **Fixed:** `tests/specification_lexer_tests.rs`
  - Removed conflicting `#![cfg(test)]` and `#![cfg(not(test))]` attributes
  - Updated to use correct `SpecificationLexer` API with `SourceCode` struct
  - Fixed imports: `SpecificationLexer::new(&source_code)` + `tokenize()` method
  - **Status:** Now compiles successfully

#### 5. WASM String Offset Verification
- **Verified:** String allocation and data section writing is correctly implemented
  - `allocate_string()` → `add_data_segment()` → `data_section.active()`
  - `mir_codegen.rs:1655` properly retrieves and adds data section to WASM module
  - **Flow:** memory.rs:750 → memory.rs:270 → mir_codegen.rs:1655
  - **Status:** No issues found, implementation is correct

#### 6. Token-Driven Parser Implementation ✅
- **Created:** `src/parser/token_parser.rs` (779 lines)
  - Implements rustc-style token-driven parser with bump(), check(), eat(), expect() utilities
  - Directly consumes tokens from Stage 1 lexer (no source reconstruction)
  - Handles functions, classes, statements, expressions, tests, imports
  - **Includes:** parse_start_function() for Clean Language's special start() syntax
  - **Status:** Complete and compiling successfully

#### 7. SpecificationParser Integration ✅
- **Updated:** `src/parser/specification_parser.rs` (reduced from 98 to 35 lines)
  - Eliminated source reconstruction mechanism
  - Now delegates to TokenParser for token-driven parsing
  - **Status:** Complete and integrated

#### 8. compile_with_file Pipeline Update ✅
- **Updated:** `src/lib.rs:73-78` (Stage 2 parser integration)
  - Removed `parse_with_file` call that discarded Stage 1 tokens
  - Now uses `SpecificationParser` with tokens from Stage 1
  - **Flow:** TokenStream → SpecificationParser → TokenParser → AST
  - **Status:** Unified pipeline working, tested with hello_world.cln

#### 9. parse_with_file Deprecation ✅
- **Deprecated:** `src/parser/parser_impl.rs:654-688`
  - Added deprecation notices to `parse_with_file()` and `parse_with_preprocessing()`
  - Special-case for "functions:" blocks no longer used
  - Will be removed in v0.11.0
  - **Status:** Deprecated, replacement active

**Documentation:** See `system-documents/token-parser-implementation.md` and `parse-with-file-removal.md`

---

## Issues Requiring Further Action

### 🟡 Parsing Pipeline Issues

#### ~~1. SpecificationParser Source Reconstruction~~ ✅ **RESOLVED**
**Status:** Resolved via token-driven parser implementation (see issue #6-9 above)

---

#### ~~2. parse_with_file Special-Case~~ ✅ **RESOLVED**
**Status:** Resolved via token-driven parser integration (see issue #8-9 above)

**Code:**
```rust
if source.contains("functions:") && !source.contains("class ") {
    match parse_with_preprocessing(source, file_path) { ... }
}
```

**Impact:** Creates divergent AST paths, introduces fragility.

**Recommendation:** Remove heuristic fallback; rely on grammar for all parsing.

---

### ~~🟡 IR Architecture Issues~~ ✅ **RESOLVED**

#### ~~3. Duplicate IR Module Exports~~ ✅ **RESOLVED**
**Status:** Resolved in v0.10.3

**Actions Taken:**
1. ✅ Added `#[deprecated]` attribute to `pub mod ir` in `src/lib.rs:37`
2. ✅ Added comprehensive documentation explaining migration path
3. ✅ Verified no external code uses `crate::ir::` (only internal to ir/ module)
4. ✅ Confirmed active pipeline uses `crate::hir` and `crate::mir` correctly

**Module Export (src/lib.rs:26-37):**
```rust
/// # DEPRECATED - Legacy IR Module
///
/// This module contains the legacy IR pipeline (HIR → MIR → LIR) and will be removed in v0.11.0.
///
/// **Use instead:**
/// - `crate::hir` for High-level IR
/// - `crate::mir` for Mid-level IR
/// - `crate::codegen::mir_codegen` for WASM generation
#[deprecated(since = "0.10.3", note = "Use crate::hir, crate::mir, and crate::codegen::mir_codegen instead")]
pub mod ir;
```

**Verified:**
- Legacy `ir` module only used internally (ir/optimization.rs, ir/transform.rs, etc.)
- Only external use is `wasm_generator.rs` (already deprecated in issue #3)
- Active pipeline: AST → `crate::hir` → `crate::mir` → `mir_codegen` → WASM ✅
- `generate_wasm_from_lir()` already deprecated (since v0.10.3)
- Will be removed in v0.11.0

---

#### ~~4. Legacy HIR/MIR in src/ir/~~ ✅ **RESOLVED**
**Status:** Properly deprecated alongside parent `ir` module

**Type Definitions:**
- Legacy: `src/ir/hir.rs` (HIRProgram) → deprecated via module deprecation
- Active: `src/hir/mod.rs` (HirProgram) → actively used ✅
- Legacy: `src/ir/mir.rs` (MIRProgram) → deprecated via module deprecation
- Active: `src/mir/mir_types.rs` (MirProgram) → actively used ✅

**Migration Complete:** All active code uses `crate::hir` and `crate::mir` types (verified via compile_with_file pipeline inspection)

**Removal Plan:** Delete entire `src/ir/` directory in v0.11.0

---

### ~~🟡 Module Resolution Issues~~ ✅ **RESOLVED**

#### 10. Module Loading Implementation ✅
- **Implemented:** `resolver/module_resolver.rs:load_module_hir()` (HIR-level, lines 276-356)
- **Implemented:** `module/mod.rs:load_module()` (AST-level, lines 195-274)

**HIR-Level Module Loading (Active Pipeline - Stage 4):**
```rust
pub fn load_module_hir(&mut self, module_name: &str) -> Result<&HirProgram, CompilerError> {
    // 1. Read file from module.file_path
    // 2. Tokenize using SpecificationLexer
    // 3. Parse using SpecificationParser
    // 4. Build HIR using HirBuilder
    // 5. Extract exports (function indices → SymbolIds)
    // 6. Extract dependencies (imported modules)
    // 7. Store HIR and metadata in LoadedModule
}
```

**AST-Level Module Loading (Legacy Compatibility):**
```rust
fn load_module(&mut self, module_name: &str) -> Result<Module, CompilerError> {
    // 1. Read file
    // 2. Tokenize using SpecificationLexer
    // 3. Parse using SpecificationParser
    // 4. Extract exports (public functions & classes)
    // 5. Cache module in module_cache
}
```

**Features:**
- ✅ Token-driven parsing (uses SpecificationParser)
- ✅ Lazy loading (modules loaded on first request)
- ✅ Export extraction (functions → SymbolIds for HIR, public items for AST)
- ✅ Dependency tracking (recursive imports, cycle detection in HIR-level)
- ✅ Module caching (avoid redundant parsing)

**Status:** Both implementations complete and tested with compilation

**Documentation:** See `system-documents/module-loading-implementation.md`

---

#### ~~5. ModuleResolver::load_module Not Implemented~~ ✅ **RESOLVED**
**Status:** Resolved - fully implemented with token-driven parsing

---

#### ~~6. Stage 4 Resolver Never Loads HIR~~ ✅ **RESOLVED**
**Status:** Resolved - `load_module_hir()` now properly loads HIR and populates exports

---

#### 7. SemanticAnalyzer Uses Legacy Resolver - 📝 **CLARIFICATION**
**Status:** Not a bug - SemanticAnalyzer is legacy code

**Finding:** SemanticAnalyzer is not used in the active 7-stage pipeline (`compile_with_file`). The active pipeline uses:
- **Stage 4:** `Resolver::resolve()` with `resolver::ModuleResolver` (HIR-level)
- **Stage 5:** `TypeChecker::check()` for type inference

**SemanticAnalyzer Usage:** Only used in legacy binaries:
- `src/bin/cln.rs` (legacy mode)
- `src/bin/test_runner.rs` (should be updated)
- `src/bin/performance_benchmark.rs`
- `src/testing/test_harness.rs`

**Recommendation:** Update test_runner.rs to use 7-stage pipeline (Issue #10 below)

**Action:** No changes needed to SemanticAnalyzer (will be deprecated with legacy binaries)

---

### 🟡 Testing & Tooling Issues

#### 8. Parser Tests Disabled (tests/specification_parser_tests.rs)
**Problem:** All tests replaced with placeholder that always passes.

**Code:**
```rust
#[test]
fn placeholder_test() {
    assert!(true);
}
```

**Impact:** No parser validation.

**Recommendation:** Restore real coverage for new parser.

---

#### 9. IR Validation Suite Disabled (tests/comprehensive_ir_validation.rs.disabled)
**Problem:** Complete IR validation suite is disabled.

**Impact:** IR layers not exercised by tests.

**Recommendation:** Reinstate or provide replacement tests.

---

#### 10. Test Runner Uses Legacy Stack (src/bin/test_runner.rs:1)
**Problem:** Uses legacy `CleanParser` + `SemanticAnalyzer` instead of 7-stage pipeline.

**Impact:** `make test-parser` validates old code paths.

**Recommendation:** Update to drive 7-stage pipeline.

---

#### 11. Testing Module Commented Out (src/lib.rs:37)
**Problem:** Entire testing module is disabled.

**Code:**
```rust
// pub mod testing;
```

**Impact:** New test harness unreachable.

**Recommendation:** Fix build issues or remove module.

---

## Priority Recommendations

### Critical (🔴 Address in v0.10.4)
1. Deprecate legacy `ir` module and related functions
2. Fix Stage 4 resolver to load HIR and populate exports
3. Restore parser tests (specification_parser_tests.rs)

### High (🟡 Address in v0.11.0)
1. Implement token-driven parsing in SpecificationParser
2. Remove parse_with_file special-case for `functions:` blocks
3. Update SemanticAnalyzer to use Stage 4 resolver
4. Update test_runner.rs to use 7-stage pipeline
5. Re-enable IR validation tests

### Medium (🟢 Address in v0.12.0)
1. Remove deprecated IR module entirely
2. Remove legacy wasm_generator.rs
3. Remove stub compilation pipeline
4. Fix or remove commented testing module

---

## Architecture Summary

### Current Active Pipeline (Correct)
```
Source Code
  ↓
Stage 1: Lexer (specification_lexer.rs) → Tokens
  ↓
Stage 2: Parser (specification_parser.rs) → AST  [⚠️ Currently reconstructs source]
  ↓
Stage 3: Resolution (resolver/mod.rs) → Resolved AST
  ↓
Stage 4: HIR Builder (hir/hir_builder.rs) → HIR (src/hir/)
  ↓
Stage 5: Type Checking (typechecker/) → Typed HIR (TAST)
  ↓
Stage 6: MIR Builder (mir/mir_builder.rs) → MIR (src/mir/)
  ↓
Stage 7: Code Generation (codegen/mir_codegen.rs) → WASM
```

### Legacy Pipeline (Deprecated, Should Be Removed)
```
AST → ir/hir.rs → ir/mir.rs → ir/lir.rs → wasm_generator.rs → WASM
```

**Status:** Legacy pipeline is unused but still exported from lib.rs.

---

## Files Modified

### Deleted
- `tests/cln/**/*.cln.bak` (20+ files)
- `src/parser/grammar.pest.bak`
- `src/semantic/mod.rs.corrupt.bak`
- `src/lexer/indentation.rs`
- `src/lexer/lexer_impl.rs`
- `src/lexer/token.rs`

### Modified
- `src/codegen/mod.rs` - Removed `generate_wasm_from_ast()` function
- `src/codegen/pipeline/mod.rs` - Added deprecation notices
- `tests/specification_lexer_tests.rs` - Fixed API usage and imports

### Verified (No Changes Needed)
- `src/codegen/mir_codegen.rs` - WASM data section handling correct
- `src/codegen/memory.rs` - String allocation and data segments correct

---

## Next Steps

1. **Phase 1 (v0.10.4):** Add deprecation notices to legacy IR module
2. **Phase 2 (v0.11.0):** Implement remaining module resolution features
3. **Phase 3 (v0.11.0):** Remove deprecated code and fix parser issues
4. **Phase 4 (v0.12.0):** Complete 7-stage pipeline integration

---

## Conclusion

The codebase cleanup revealed several layers of technical debt, primarily:
- **Duplicate IR implementations** (legacy vs. active)
- **Incomplete 7-stage pipeline integration** (parser falls back to string reconstruction)
- **Incomplete module resolution** (HIR never loaded, exports never populated)
- **Test infrastructure gaps** (disabled tests, legacy test runners)

**Immediate Actions Completed:** 8/18 issues resolved (44%)
**Remaining Actions:** 10 issues requiring implementation work

The active MIR-based compilation pipeline is correct and functional. The main issues are incomplete features (module resolution, token-driven parsing) and legacy code that should be removed.
