---
name: spec-compliance
description: Workflow for ensuring specification compliance across the Clean Language compiler
trigger: manual, before-release, after-spec-change
---

# Specification Compliance Workflow

Ensures the Clean Language compiler fully implements and tests all features defined in the specification.

## Trigger Events

- **Manual**: User requests spec compliance check
- **Before Release**: Part of release verification
- **After Spec Change**: When specification is modified
- **Weekly**: Sunday 02:00 UTC (optional)

## Workflow Steps

### Step 1: Update Specification Catalog

**Agent:** spec-coverage

```bash
# Read current spec
cat documentation/Clean_Language_Specification.md | head -100

# Check feature catalog exists and is current
if [ -f system-documents/spec_features_catalog.md ]; then
    echo "Catalog exists, checking last modified..."
    ls -la system-documents/spec_features_catalog.md
else
    echo "Creating feature catalog..."
fi
```

**Actions:**
1. Read specification document
2. Extract all testable features (187 identified)
3. Update `system-documents/spec_features_catalog.md`
4. Identify any new or changed features

### Step 2: Analyze Test Coverage

**Agent:** spec-coverage

```bash
# Count existing tests
find tests/cln -name "*.cln" | wc -l

# Check spec compliance directory
ls -la tests/cln/spec_compliance/ 2>/dev/null || echo "Creating spec_compliance directory..."

# Map tests to spec sections
for section in lexical types expressions functions control_flow classes; do
    echo "$section: $(find tests/cln -name "*${section}*" -o -name "*spec*" | wc -l) tests"
done
```

**Actions:**
1. Scan all test files in `tests/cln/`
2. Map tests to specification sections
3. Calculate coverage per section
4. Identify gaps in coverage

### Step 3: Create Missing Tests

**Agent:** spec-coverage

For each uncovered feature:
1. Create test file in `tests/cln/spec_compliance/{category}/`
2. Include spec section reference
3. Test positive case (should work)
4. Test negative case (should fail) if applicable
5. Test edge cases

**Test Template:**
```clean
// Spec Section: X.Y - Feature Name
// Tests the [feature] as defined in specification

start()
    // Positive test case
    [valid code that should work]

    // Edge case test
    [boundary condition test]

    print("Spec X.Y tests complete")
```

### Step 4: Audit Existing Tests

**Agent:** test-auditor

```bash
# Find tests without spec references
grep -rL "Spec Section\|Spec:" tests/cln/ --include="*.cln" | wc -l

# Check for deprecated syntax patterns
grep -r "function [a-z]" tests/cln/ --include="*.cln" | grep -v "functions:" | wc -l
```

**Actions:**
1. Scan all tests for spec compliance
2. Identify tests using deprecated syntax
3. Identify tests with wrong expectations
4. Generate `tests/AUDIT_REPORT.md`

### Step 5: Compile All Tests

**Agent:** clean-language-qa-engineer

```bash
# Compile all test files
for f in tests/cln/**/*.cln; do
    output="tests/output/$(basename $f .cln).wasm"
    ./target/release/cln compile "$f" -o "$output" 2>/dev/null
done

# Count results
echo "Total: $(find tests/cln -name '*.cln' | wc -l)"
echo "Compiled: $(find tests/output -name '*.wasm' | wc -l)"
```

### Step 6: Validate WASM Output

**Agent:** verifier

```bash
# Validate all WASM files
VALID=0
TOTAL=0
for wasm in tests/output/*.wasm; do
    TOTAL=$((TOTAL + 1))
    if wasm-validate "$wasm" 2>/dev/null; then
        VALID=$((VALID + 1))
    fi
done
echo "WASM Validation: $VALID/$TOTAL"
```

### Step 7: Execute Tests

**Agent:** clean-language-qa-engineer

```bash
# Run executable tests
for wasm in tests/output/*.wasm; do
    timeout 5 ./target/release/wasmtime_runner "$wasm" 2>/dev/null && echo "PASS: $(basename $wasm)" || echo "FAIL: $(basename $wasm)"
done
```

### Step 8: Generate Compliance Report

**Output:** `system-documents/spec_compliance_report_YYYYMMDD.md`

```markdown
# Specification Compliance Report

**Date:** {date}
**Spec Version:** 0.14.0
**Compiler Version:** {version}

## Summary

| Metric | Value | Target |
|--------|-------|--------|
| Spec Features | 187 | - |
| Features Tested | X | 187 |
| Coverage | X% | 100% |
| Tests Passing | X/Y | 100% |
| WASM Valid | X/Y | 100% |

## Coverage by Section

| # | Section | Features | Tested | Coverage |
|---|---------|----------|--------|----------|
| 1 | Lexical | 15 | ? | ?% |
| 2 | Types | 32 | ? | ?% |
| ... | ... | ... | ... | ... |

## Issues Found

### Critical (Must Fix)
- [issue]

### High Priority
- [issue]

### Medium Priority
- [issue]

## Recommendations

1. [recommendation]
2. [recommendation]

## Next Steps

1. [ ] Fix critical issues
2. [ ] Add missing tests
3. [ ] Update deprecated tests
4. [ ] Re-run compliance check
```

## Success Criteria

| Criteria | Target | Status |
|----------|--------|--------|
| Spec coverage | 100% | [ ] |
| Test audit compliance | 100% | [ ] |
| All tests compile | 100% | [ ] |
| All WASM valid | 100% | [ ] |
| All tests execute | 100% | [ ] |
| Zero critical issues | 0 | [ ] |

## Execution Order

```
1. spec-coverage (catalog update) ──┐
                                    ├──> 3. spec-coverage (create tests)
2. test-auditor (audit existing) ───┘
                                          │
                                          v
                              4. qa-engineer (compile all)
                                          │
                                          v
                              5. verifier (validate WASM)
                                          │
                                          v
                              6. qa-engineer (execute tests)
                                          │
                                          v
                              7. Generate compliance report
```

## Integration Points

- **Pre-release**: Run before any version release
- **CI/CD**: Can be triggered by spec changes
- **Weekly**: Optional scheduled run
- **Manual**: On-demand for feature work

## Files Created/Updated

| File | Purpose |
|------|---------|
| `system-documents/spec_features_catalog.md` | Feature inventory |
| `system-documents/spec_compliance_report_*.md` | Compliance reports |
| `tests/cln/spec_compliance/**/*.cln` | New spec tests |
| `tests/cln/spec_compliance/COVERAGE_MATRIX.md` | Coverage tracking |
| `tests/AUDIT_REPORT.md` | Test audit results |
