# Nightly Test Workflow

## Schedule
Daily comprehensive testing

## Trigger
Manual or scheduled

## Steps

### 1. Environment Setup
```bash
cd /Users/earcandy/Documents/Dev/Clean\ Language/clean-language-compiler
git pull origin main
cargo build --release
```

### 2. Comprehensive Testing
Execute using existing infrastructure:
```bash
# Run comprehensive tests
cargo run --bin cln comprehensive-test 2>&1 | tee logs/nightly_$(date +%Y%m%d).log

# Or use existing script
./scripts/comprehensive_test_runner.sh
```

### 3. WASM Validation
```bash
# Validate all compiled output
for wasm in tests/output/*.wasm; do
    wasm-validate "$wasm" 2>&1 || echo "INVALID: $wasm"
done
```

### 4. Regression Check
Use regression-guard agent to compare with previous baseline.

### 5. Report Generation
```bash
# Generate summary
echo "=== Nightly Test Report $(date) ===" > logs/nightly_report.md
echo "" >> logs/nightly_report.md
grep -E "Success Rate|PASS|FAIL|Error" logs/nightly_*.log >> logs/nightly_report.md
```

## Success Criteria
- All tests compile successfully
- WASM validation passes
- No regressions from previous night
- Success rate maintained or improved

## Artifacts
- `logs/nightly_YYYYMMDD.log`
- `logs/nightly_report.md`
