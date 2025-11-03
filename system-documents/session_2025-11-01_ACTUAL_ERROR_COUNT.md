# Session 2025-11-01: Actual Error Count and Regression Analysis

## Current Status

**CRITICAL REGRESSION DISCOVERED**

### Actual Test Results (After Fresh Recompile)
- **Total .cln test files**: 297
- **Successfully compiled AND valid WASM**: 244 (82.2%)
- **Failed to compile**: 50 (16.8%)
- **Compiled but invalid WASM**: 3 (1.0%)

### Previous Session Status (From Summary)
- **Total**: 297
- **Passing**: 278 (93.6%)
- **Failing**: 19

### Regression Analysis
- **Success rate dropped**: 93.6% → 82.2% (**-11.4% regression**)
- **New failures**: 34 additional files now fail
- **Compilation failures**: Increased from ~0 to 50 files

## Root Cause: SymbolId Resolution Failure

**Error**: `Cannot resolve SymbolId(X) to function name during code generation`

**Affected**: 50 files (16.8% of test suite)

**Example** (`02_numeric_literals.cln`):
```
SymbolId(162) for print() NOT FOUND in function_symbol_map
Map only has: {SymbolId(201): "start"}
```

**Cause**: Hardcoded SymbolId mappings don't match actual runtime IDs

**Solution**: Revert conditional operations registration OR fix SymbolId mapping
