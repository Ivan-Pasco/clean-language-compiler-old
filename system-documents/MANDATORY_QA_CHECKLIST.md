# MANDATORY QUALITY ASSURANCE CHECKLIST

## READ THIS BEFORE EVERY WORK SESSION

This document is MANDATORY. No shortcuts. No optimism. No assumptions.

---

## ⚠️ CRITICAL RULES - NO EXCEPTIONS

### Rule 1: NEVER Claim "Ready" Without 100% Test Pass Rate
- ❌ FORBIDDEN: "The compiler is ready"
- ❌ FORBIDDEN: "This feature works"
- ❌ FORBIDDEN: "All tests pass"
- ✅ REQUIRED: "X/284 tests pass (Y%)" with actual numbers

### Rule 2: ALWAYS Run Full Test Suite Before Status Updates
- ❌ FORBIDDEN: Testing 3 examples and claiming success
- ❌ FORBIDDEN: Testing random samples
- ❌ FORBIDDEN: Assuming old test results are still valid
- ✅ REQUIRED: Run `python3 scripts/run_full_test_suite.py` EVERY TIME
- ✅ REQUIRED: Show actual test results with numbers

### Rule 3: NEVER Use Optimistic Language Without Data
- ❌ FORBIDDEN: "Should work", "Probably works", "Might work"
- ❌ FORBIDDEN: "Comprehensive tests" for anything less than 100% coverage
- ❌ FORBIDDEN: "All done" when failures remain
- ✅ REQUIRED: "Tested X files, Y passed, Z failed"
- ✅ REQUIRED: Show evidence for every claim

### Rule 4: ALWAYS Document What Was Actually Tested
- ❌ FORBIDDEN: Vague claims like "tested the compiler"
- ❌ FORBIDDEN: "Ran tests" without specifying which tests
- ✅ REQUIRED: List exact files/features tested
- ✅ REQUIRED: Show test commands used
- ✅ REQUIRED: Save test results to `tests/results/`

### Rule 5: NEVER Skip Validation Steps
- ❌ FORBIDDEN: Assuming code compiles without building
- ❌ FORBIDDEN: Assuming WASM is valid without `wasm-validate`
- ❌ FORBIDDEN: Assuming code runs without execution test
- ✅ REQUIRED: Build → Compile → Validate → Execute → Verify Output

---

## 🎯 BEFORE STARTING ANY WORK

### Checklist (Check ALL boxes before proceeding):

- [ ] Read this entire document
- [ ] Read `system-documents/TESTING_OVERHAUL.md`
- [ ] Understand that optimistic language is FORBIDDEN
- [ ] Understand that claims require evidence
- [ ] Understand that 100% = "ready", anything less = "not ready"

---

## 📋 MANDATORY WORKFLOW FOR EVERY TASK

### Phase 1: Understand Current State (MANDATORY)

```bash
# Step 1: Run FULL test suite to get baseline
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
python3 scripts/run_full_test_suite.py > baseline_results.txt

# Step 2: Read the results
cat baseline_results.txt

# Step 3: Document baseline
# Record: X/284 tests passing (Y%)
```

**DO NOT PROCEED until you know the exact baseline numbers.**

### Phase 2: Make Changes (CAREFUL)

```bash
# Step 1: Make focused changes
# Step 2: Build to verify compilation
cargo build --release

# Step 3: Test the specific feature you changed
cargo run --bin clean-language-compiler compile -i test_file.cln -o test.wasm

# Step 4: Validate WASM
wasm-validate test.wasm
```

**DO NOT PROCEED until your specific change compiles and validates.**

### Phase 3: Verify Impact (MANDATORY)

```bash
# Step 1: Run FULL test suite again
python3 scripts/run_full_test_suite.py > after_changes_results.txt

# Step 2: Compare results
diff baseline_results.txt after_changes_results.txt

# Step 3: Verify improvement or at least no regression
# - Pass rate must stay same or increase
# - New failures are NOT acceptable unless fixing other issues
```

**DO NOT PROCEED until you verify no regression occurred.**

### Phase 4: Document Results (MANDATORY)

```bash
# Step 1: Save test results
cp after_changes_results.txt tests/results/session_$(date +%Y%m%d_%H%M%S).txt

# Step 2: Update progress tracking
# Document exact numbers: "Was X/284, now Y/284"
```

**DO NOT CLAIM completion until results are documented.**

---

## 🚨 WHEN REPORTING STATUS

### ALWAYS Include These Numbers:

1. **Test Pass Rate**: "X/284 tests pass (Y%)"
2. **Category Breakdown**: Show pass rates by category
3. **Specific Failures**: List failing test files
4. **Baseline Comparison**: "Was X%, now Y%"
5. **Evidence**: Link to test results file

### Example CORRECT Status Report:

```
STATUS REPORT - 2025-01-06

Full Test Suite Results:
✅ PASSED: 189/284 (66.5%)
❌ FAILED: 95/284 (33.5%)

Category Breakdown:
  Core basics: 45/50 (90%)
  Functions: 23/40 (57.5%)
  Classes: 15/35 (42.9%)
  Control flow: 30/32 (93.8%)

Top Failures:
  - Default parameters (15 files)
  - Nested methods (8 files)
  - Complex inheritance (7 files)

Baseline Comparison:
  Previous: 175/284 (61.6%)
  Current: 189/284 (66.5%)
  Improvement: +14 tests (+4.9%)

HONEST ASSESSMENT:
❌ Compiler is NOT ready for production
⚠️ 95 failures remain
🎯 Target: 284/284 (100%)

Evidence: tests/results/session_20250106_143022.txt
```

### Example INCORRECT Status Report (FORBIDDEN):

```
STATUS REPORT - 2025-01-06

✅ All tests pass!
✅ Compiler is ready!
🎉 Everything works!

[NO NUMBERS, NO EVIDENCE, NO DETAILS = FORBIDDEN]
```

---

## 🔍 VERIFICATION CHECKLIST BEFORE CLAIMING ANYTHING

Before saying ANY feature works, check ALL of these:

### For Compilation:
- [ ] Runs `cargo build` without errors
- [ ] Source code compiles to WASM
- [ ] WASM passes `wasm-validate`
- [ ] No warning messages about missing features
- [ ] File size is reasonable (not 0 bytes)

### For Features:
- [ ] Tested with multiple examples (minimum 5)
- [ ] Tested edge cases (empty, null, invalid input)
- [ ] Tested integration with other features
- [ ] All related tests in test suite pass
- [ ] No regressions in other features

### For "Ready" Claims:
- [ ] Full test suite: 284/284 pass (100%)
- [ ] All categories show 100% pass rate
- [ ] No TODO comments in code for this feature
- [ ] No warnings during compilation
- [ ] No errors during execution
- [ ] Documentation matches implementation
- [ ] Examples all work correctly

**If ANY checkbox is unchecked, you CANNOT claim "ready".**

---

## 📊 QUALITY GATES

### Gate 1: Basic Sanity (Minimum Bar)
- 3/3 basic sanity tests pass
- Compiler builds without errors
- Can compile trivial programs

**Status**: Entry-level functionality

### Gate 2: Partial Functionality
- ≥50% of full test suite passes
- Core features work
- Basic programs compile

**Status**: Development in progress

### Gate 3: Nearly Ready
- ≥95% of full test suite passes
- Most features work
- Only edge cases fail

**Status**: Close to production-ready

### Gate 4: Production Ready
- 100% of full test suite passes
- All features work
- All tests pass
- No known issues

**Status**: Ready for release

**Current compiler status MUST be assessed against these gates with actual numbers.**

---

## 🎓 LESSONS FROM PAST MISTAKES

### Mistake 1: Trusting Test Names
- ❌ "comprehensive-test" only tested 3 files
- ✅ Always verify what tests actually do
- ✅ Read test code before trusting results

### Mistake 2: Sample Testing
- ❌ Testing 10 random files and extrapolating
- ✅ Test ALL files, every time
- ✅ No shortcuts, no assumptions

### Mistake 3: Optimistic Language
- ❌ "All tests pass!" when only 3 passed
- ✅ "3/3 basic tests pass, full suite unknown"
- ✅ Be brutally honest

### Mistake 4: Assuming Success
- ❌ Code compiles → "it works"
- ✅ Code compiles → validate → execute → verify output → "it works"
- ✅ Multiple verification steps required

### Mistake 5: Not Documenting Evidence
- ❌ "I tested it" with no proof
- ✅ Save test results to files
- ✅ Show exact commands used
- ✅ Provide reproducible steps

---

## ⚡ QUICK REFERENCE

### Before Every Session:
1. Read this document
2. Run full test suite
3. Know baseline numbers

### During Work:
1. Make small changes
2. Test immediately
3. Verify no regressions

### Before Reporting:
1. Run full test suite again
2. Show actual numbers
3. Provide evidence
4. Be honest about gaps

### Never Say:
- "All tests pass" (unless 284/284)
- "It works" (without proof)
- "Ready" (without 100%)
- "Comprehensive" (unless testing everything)

### Always Say:
- "X/284 tests pass (Y%)"
- "Tested with these specific files: ..."
- "Evidence: [file path]"
- "Still have Z failures remaining"

---

## 🎯 SUCCESS CRITERIA

The compiler is "READY" ONLY when:

- [ ] Full test suite: 284/284 (100%)
- [ ] All categories: 100% pass rate
- [ ] All WASM validates with `wasm-validate`
- [ ] All compiled programs execute correctly
- [ ] All outputs match expected results
- [ ] No TODO/FIXME comments for core features
- [ ] Documentation is accurate
- [ ] This checklist is fully satisfied

**Until ALL criteria are met: Status = "NOT READY"**

---

## 📝 MANDATORY DOCUMENTATION

Every work session MUST create:

1. **Test Results File**: `tests/results/session_YYYYMMDD_HHMMSS.txt`
2. **Change Log**: What was changed and why
3. **Impact Assessment**: How many tests affected
4. **Next Steps**: What still needs fixing

**No documentation = work didn't happen**

---

## ⚠️ FINAL WARNING

This is not optional. This is not negotiable.

- NO optimistic language without evidence
- NO claims without test results
- NO "ready" without 100% pass rate
- NO shortcuts in verification

**Quality over speed. Truth over optimism. Evidence over assumptions.**

---

*Last Updated: 2025-01-06*
*Read this document at the start of EVERY session*
*Violating these rules wastes everyone's time*
