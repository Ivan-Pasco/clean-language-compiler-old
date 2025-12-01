# Pre-Commit Workflow

## Trigger
Before each commit

## Steps

### 1. Quick Checks (< 30 seconds)
```bash
# Format check (informational - project allows flexibility)
cargo fmt --check || echo "Consider running cargo fmt"

# Quick compilation check
cargo check
```

### 2. Fast Tests (< 2 minutes)
```bash
# Run Rust unit tests
cargo test --lib

# Quick smoke test
./scripts/test_smoke.sh
```

### 3. WASM Spot Check
```bash
# Validate a sample of WASM files
for wasm in $(ls tests/output/*.wasm | head -20); do
    wasm-validate "$wasm" || echo "WARN: $wasm invalid"
done
```

## Blocking Criteria
- Compilation errors → BLOCK
- Rust test failures → BLOCK
- Critical WASM validation failures → BLOCK

## Non-Blocking Warnings
- Format differences → WARN
- Minor WASM issues → WARN

## Quick Command
```bash
# All-in-one pre-commit check
cargo check && cargo test --lib && ./scripts/test_smoke.sh
```
