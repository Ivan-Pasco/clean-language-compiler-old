# 🚀 Testing Protocol from Claude Code

## Quick Commands for Claude Code

Use these simple commands to fire the testing protocol directly:

### **🎯 Most Common Commands**

```bash
# Quick test suite (30 seconds) - Use during development
./qa/run_tests.sh quick

# Test failure protocol - Use when ANY test fails
./qa/run_tests.sh fix

# Full quality gate - Use before commits
./qa/run_tests.sh full
```

### **⚡ Slash Commands in Claude Code**

When you type `@test` in Claude Code, these commands should appear:

**Primary Commands:**
- `@test` - 🚀 Quick test suite (most common)
- `@test-fix` - 🔧 Test failure protocol (when ANY test fails)
- `@test-full` - 🎯 Full quality gate  
- `@test-all` - 🎯 Complete test suite (everything)
- `@tests` - 🎯 Full quality gate (alias)

**Specialized Commands:**
- `@test-quick` - 🚀 Quick test suite (explicit)
- `@test-validate` - 📚 Validate against spec
- `@test-unit` - 🧪 Unit tests only
- `@test-benchmark` - ⚡ Performance tests
- `@test-coverage` - 📊 Coverage analysis
- `@test-property` - 🎲 Property-based tests
- `@test-help` - ❓ Show all options

### **📋 All Available Commands**

| Command | Description | When to Use |
|---------|-------------|-------------|
| `./qa/run_tests.sh quick` | Fast parser + basic tests | During active development |
| `./qa/run_tests.sh fix` | Test failure protocol checker | **When ANY test fails** |
| `./qa/run_tests.sh full` | Complete quality gate | Before committing |
| `./qa/run_tests.sh validate` | Check tests against spec | Before fixing implementation |
| `./qa/run_tests.sh unit` | Unit tests only | Focused testing |
| `./qa/run_tests.sh benchmark` | Performance testing | Performance validation |
| `./qa/run_tests.sh coverage` | Coverage analysis | Quality assessment |
| `./qa/run_tests.sh property` | Property-based tests | Edge case testing |
| `./qa/run_tests.sh all` | Complete test suite | Comprehensive testing |

### **🔥 CRITICAL: When Tests Fail**

**⚠️  QUALITY STANDARD**: We require **100% compilation rate AND 100% execution rate**

**ALWAYS run this command first:**
```bash
./qa/run_tests.sh fix
```

This will remind you of the **mandatory protocol**:
1. Review `documentation/Clean_Language_Specification.md`
2. Validate test correctness against specification
3. Fix test if wrong
4. Fix implementation ONLY if test is correct

### **⚡ Quick Reference**

```bash
# Development workflow
./qa/run_tests.sh quick        # Fast feedback
./qa/run_tests.sh fix          # When tests fail
./qa/run_tests.sh full         # Before commit

# Show all options
./qa/run_tests.sh help
```

### **🛠️ Integration with Claude Code**

1. **Testing Configuration**: `.claude-testing.json` contains all command definitions
2. **Simple Interface**: One script handles all testing scenarios  
3. **Clear Output**: Color-coded results with pass/fail status
4. **Protocol Enforcement**: Built-in reminders for test failure workflow

### **📁 Key Files**

- `./run_tests.sh` - Main testing script
- `.claude-testing.json` - Configuration for Claude Code
- `documentation/Clean_Language_Specification.md` - Authority for test validation
- `qa/TESTING_GUIDE.md` - Detailed testing documentation

### **🎯 Examples**

```bash
# Quick development check
./qa/run_tests.sh quick

# Test failed - need protocol
./qa/run_tests.sh fix

# Ready to commit
./qa/run_tests.sh full

# Check specific functionality
./qa/run_tests.sh unit
./qa/run_tests.sh benchmark
```

The testing protocol is now **one command away** from Claude Code!