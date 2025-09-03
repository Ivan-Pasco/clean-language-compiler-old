# QA Checklist - Quick Reference

## Pre-QA Setup ✅

- [ ] Ensure project builds without compilation errors: `cargo build`
- [ ] Verify git status is clean or changes are intentional
- [ ] QA scripts are executable: `chmod +x tests/qa_scripts/*.sh`
- [ ] Python 3 is available for analysis scripts

## Phase 1: Baseline Assessment 📊

- [ ] **Run comprehensive test**:
  ```bash
  ./tests/qa_scripts/run_comprehensive_test.sh
  ```
- [ ] **Record baseline metrics**:
  - Success Rate: ___% 
  - Tests Passing: ___/___
  - Timestamp: ___________

- [ ] **Identify blocker issues**:
  - [ ] Compilation errors (CRITICAL 🔴)
  - [ ] Non-exhaustive patterns (CRITICAL 🔴)  
  - [ ] Core AST handling failures (CRITICAL 🔴)

## Phase 2: Error Analysis 🔍

- [ ] **Run error categorization**:
  ```bash
  # Save test output to file first
  timeout 120s cargo run --bin clean-language-compiler comprehensive-test > test_output.log 2>&1
  python3 tests/qa_scripts/categorize_errors.py test_output.log error_analysis.md
  ```

- [ ] **Categorize by priority**:
  - [ ] CRITICAL (🔴): __ errors affecting __ tests
  - [ ] HIGH (🟡): __ errors affecting __ tests  
  - [ ] MEDIUM (🟢): __ errors affecting __ tests

- [ ] **Document in TASKS.md**:
  - [ ] Add critical issues with 🔴 priority
  - [ ] Add high-impact issues with 🟡 priority
  - [ ] Include specific file paths and line numbers

## Phase 3: Systematic Fixes 🔧

### Critical Issues First (🔴)
- [ ] **Fix compilation blockers**:
  - [ ] Non-exhaustive pattern matches in `src/codegen/`
  - [ ] Missing AST variant handlers
  - [ ] Undefined function implementations

- [ ] **Verify fix**: Run test after each critical fix
- [ ] **No regressions**: Ensure previously passing tests still pass

### High Priority Issues (🟡)  
- [ ] **Address by test count impact**:
  - [ ] Undefined variables/namespaces (affects __ tests)
  - [ ] Unimplemented features (affects __ tests)
  - [ ] Missing code generation (affects __ tests)

- [ ] **Implement production-grade solutions**:
  - [ ] No placeholder implementations (`todo!`, `unimplemented!`)
  - [ ] Complete feature implementation
  - [ ] Proper error handling

### Medium Priority Issues (🟢)
- [ ] **Feature completions**:
  - [ ] Edge case handling
  - [ ] Syntax improvements  
  - [ ] Performance optimizations

## Phase 4: Validation & Progress 📈

- [ ] **Re-run comprehensive test**:
  ```bash
  ./tests/qa_scripts/run_comprehensive_test.sh
  ```

- [ ] **Generate progress report**:
  ```bash
  ./tests/qa_scripts/generate_progress_report.sh
  ```

- [ ] **Measure improvement**:
  - Previous Success Rate: ___%
  - Current Success Rate: ___%  
  - Improvement: +___% (minimum +10% required)
  - Tests gained: +___ tests

- [ ] **Quality gates check**:
  - [ ] No CRITICAL errors remaining
  - [ ] Success rate improved by minimum 10%
  - [ ] No regressions in previously passing tests
  - [ ] All fixes documented in TASKS.md

## Phase 5: Syntax Compliance 📝

- [ ] **Validate test syntax**:
  ```bash
  python3 tests/qa_scripts/validate_syntax_compliance.py tests/clean_files/ syntax_report.md
  ```

- [ ] **Fix non-compliant tests**:
  - [ ] Indentation (tabs vs spaces)
  - [ ] Variable declaration syntax
  - [ ] Function definition format
  - [ ] String literal format (double quotes)

- [ ] **Re-test fixed files**:
  ```bash
  cargo run --bin clean-language-compiler compile -i fixed_test.cln
  ```

## Production Readiness Gates 🎯

### Development Quality (50%+)
- [ ] Success rate ≥ 50%
- [ ] All CRITICAL errors resolved
- [ ] Core language features working

### High Quality (85%+)  
- [ ] Success rate ≥ 85%
- [ ] Most HIGH priority issues resolved
- [ ] Advanced features implemented

### Production Ready (95%+)
- [ ] Success rate ≥ 95%
- [ ] All major language features complete
- [ ] Comprehensive error handling
- [ ] Performance optimized

## Completion Checklist ✅

- [ ] **Final test run**: Success rate ≥ target
- [ ] **Documentation updated**:
  - [ ] TASKS.md reflects current state
  - [ ] Language-Specification.md updated if needed
  - [ ] All fixes documented

- [ ] **Code quality verified**:
  - [ ] No placeholder implementations
  - [ ] All functions fully implemented
  - [ ] Production-grade standards met

- [ ] **Regression testing**:
  - [ ] Previously working features still work
  - [ ] New implementations don't break existing code
  - [ ] Test suite stability verified

## Emergency Procedures 🚨

### If Tests Won't Run
1. Check compiler compilation: `cargo build`
2. Fix Rust compilation errors first
3. Check for missing dependencies
4. Verify file permissions on test files

### If Success Rate Drops
1. Run `git diff` to see recent changes
2. Identify which change introduced regressions
3. Fix or revert the problematic change
4. Re-run baseline assessment

### If Fixes Don't Work
1. Use Serena MCP for deeper analysis:
   ```bash
   serena find_symbol "problematic_function"
   serena get_symbols_overview "src/problem_module.rs"
   ```
2. Use Context7 MCP for external examples
3. Search web for similar compiler patterns
4. Break down complex fixes into smaller steps

## Quick Commands Reference 🚀

```bash
# Full QA run
./tests/qa_scripts/run_comprehensive_test.sh

# Error analysis
python3 tests/qa_scripts/categorize_errors.py - < test_output.log

# Progress tracking  
./tests/qa_scripts/generate_progress_report.sh

# Syntax validation
python3 tests/qa_scripts/validate_syntax_compliance.py tests/clean_files/

# Individual test
cargo run --bin clean-language-compiler compile -i test.cln -o test.wasm

# Debug compilation
RUST_LOG=debug cargo run --bin clean-language-compiler compile -i test.cln
```

---

**📚 Detailed Methodology**: [`tests/COMPREHENSIVE_QA_PROCEDURE.md`](COMPREHENSIVE_QA_PROCEDURE.md)  
**🔧 QA Scripts**: [`tests/qa_scripts/`](qa_scripts/)  
**📋 Task Tracking**: [`TASKS.md`](../TASKS.md)