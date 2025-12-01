# Release Verification Workflow

## Trigger
Before any version release

## Prerequisites
- All development complete
- TASKS.md reviewed
- Version updated in Cargo.toml (semantic versioning: X.Y.Z)

## Steps

### 1. Pre-Release Verification
Execute verifier agent with all checks:
```bash
# Full verification suite
./scripts/test_qa_validation.sh

# Comprehensive tests
cargo run --bin cln comprehensive-test

# Build release binary
cargo build --release
```

### 2. Version Verification
```bash
# Verify version in Cargo.toml follows semver
VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
echo "Release version: $VERSION"

# Ensure version format is X.Y.Z (no suffixes per project rules)
if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "ERROR: Version must be semantic (X.Y.Z)"
    exit 1
fi
```

### 3. Regression Check
Execute regression-guard agent:
- Compare with previous release baseline
- Verify no functionality regressions

### 4. Documentation Check
```bash
# Verify key docs exist
for doc in README.md CLAUDE.md documentation/Clean_Language_Specification.md; do
    if [ -f "$doc" ]; then
        echo "✓ $doc exists"
    else
        echo "✗ $doc missing"
    fi
done
```

### 5. Final Verification Report
Generate report using verifier agent in `system-documents/release_verification_vX.Y.Z.md`

### 6. Release Approval
Required approvals:
- [ ] Verifier agent: APPROVED
- [ ] Human review: APPROVED
- [ ] All tests pass: YES

## Post-Approval Steps
```bash
# Tag release (per project rules: no 'v' prefix)
git tag $VERSION
git push origin $VERSION

# Create GitHub release (if applicable)
```

## Artifacts
- `system-documents/release_verification_vX.Y.Z.md`
- Git tag: `X.Y.Z`
- Release binary: `target/release/cln`
