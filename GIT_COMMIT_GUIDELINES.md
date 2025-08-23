# Git Commit Guidelines for Clean Language Compiler

## Mandatory Pre-Commit Cleanup Procedure

**IMPORTANT**: Before every git commit, you MUST run the cleanup procedure to maintain repository hygiene and prevent unnecessary files from being committed.

### Required Steps (in order):

#### 1. Run Pre-Commit Cleanup Script
```bash
./scripts/pre-commit-cleanup.sh
```

This automated script performs all necessary cleanup steps:
- Runs `cargo clean` to remove all build artifacts
- Deletes all `.cln` and `.wasm` files from project root
- Removes all temporary test and debug files
- Verifies repository is clean and commit-ready

#### 2. Manual Alternative (if script unavailable):
```bash
# Step 1: Clean build artifacts
cargo clean

# Step 2: Remove root-level test files
find . -maxdepth 1 -name "*.cln" -type f -delete
find . -maxdepth 1 -name "*.wasm" -type f -delete

# Step 3: Remove temporary/debug files
find . -maxdepth 1 \( -name "test_*" -o -name "debug_*" -o -name "qa_*" -o -name "simple_*" \) -delete
rm -rf tmp/
find . -maxdepth 1 \( -name "*.log" -o -name "compilation_*.txt" -o -name "qa_*.txt" \) -delete
```

#### 3. Verify Cleanup Success
After cleanup, verify the repository state:
```bash
du -sh .  # Should be ~2-3GB, not 30+ GB
ls *.cln *.wasm 2>/dev/null || echo "Good: No test files in root"
```

### Commit Workflow

#### Standard Commit Process:
```bash
# 1. MANDATORY: Run cleanup
./scripts/pre-commit-cleanup.sh

# 2. Stage your changes
git add <modified-files>

# 3. Create commit with descriptive message
git commit -m "feat: your descriptive commit message"

# 4. Push to remote
git push origin main
```

#### Release Commit Process:
```bash
# 1. MANDATORY: Run cleanup
./scripts/pre-commit-cleanup.sh

# 2. Update version in Cargo.toml
# 3. Stage changes
git add Cargo.toml <other-files>

# 4. Create release commit
git commit -m "feat: v0.x.y - release description"

# 5. Create annotated tag
git tag -a v0.x.y -m "v0.x.y: Release description with key features"

# 6. Push commit and tag
git push origin main
git push origin v0.x.y
```

## Files That Should NOT Be Committed

### Always Excluded:
- `target/` directory (build artifacts)
- `*.cln` files in project root (temporary test files)
- `*.wasm` files in project root (temporary output files) 
- `test_*` files in project root (temporary test files)
- `debug_*` files in project root (temporary debug files)
- `qa_*` files in project root (temporary QA files)
- `simple_*` files in project root (temporary test files)
- `tmp/` directory (temporary files)
- `*.log` files (compilation/test logs)
- Temporary text files like `compilation_*.txt`, `qa_*.txt`

### Allowed Test Files:
- `tests/clean_files/*.cln` (official test suite)
- `tests/clean_files/*.wasm` (expected test outputs)
- `examples/*.cln` (example programs)
- Documentation files (`.md` files)

## Repository Size Management

### Expected Repository Sizes:
- **After cleanup**: ~2-3GB (source code, docs, official tests)
- **Before cleanup**: Can grow to 30GB+ with build artifacts
- **Target size**: Keep under 5GB for optimal Git performance

### Warning Signs:
- Repository over 10GB = cleanup needed immediately
- `du -sh .` shows 30GB+ = major cleanup required
- Slow git operations = likely size/file count issue

## Commit Message Standards

### Format:
```
<type>: <description>

<optional body>

<optional footer>
```

### Types:
- `feat:` New features or enhancements
- `fix:` Bug fixes
- `docs:` Documentation updates
- `refactor:` Code refactoring without behavior change
- `test:` Test-related changes
- `ci:` CI/CD pipeline changes

### Examples:
```bash
git commit -m "feat: add enhanced math.pow function with edge case handling"
git commit -m "fix: resolve file.write boolean return type issue"
git commit -m "docs: add comprehensive API documentation"
```

## Pre-Commit Hook (Optional)

To automate cleanup, add this to `.git/hooks/pre-commit`:
```bash
#!/bin/bash
./scripts/pre-commit-cleanup.sh
if [ $? -ne 0 ]; then
    echo "❌ Pre-commit cleanup failed. Commit aborted."
    exit 1
fi
```

## Emergency Cleanup

If repository is severely polluted:
```bash
# Nuclear option - clean everything
git clean -fdx
cargo clean
./scripts/pre-commit-cleanup.sh

# Verify critical files still exist
ls Cargo.toml src/ tests/ README.md
```

## Best Practices

1. **Always run cleanup before commits** - No exceptions
2. **Test your changes** after cleanup to ensure nothing critical was removed
3. **Keep commits focused** - One logical change per commit  
4. **Write descriptive commit messages** - Future you will thank you
5. **Tag releases properly** - Use semantic versioning (v0.x.y)
6. **Monitor repository size** regularly with `du -sh .`

---

**Remember**: A clean repository is a happy repository! 🧹✨