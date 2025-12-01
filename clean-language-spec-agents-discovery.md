# Clean Language Compiler - Specification Compliance Agents Setup

## IMPORTANT: DISCOVERY-FIRST APPROACH

This document adds two specification compliance agents to your existing QA infrastructure.
**Claude Code MUST validate the actual project structure before implementing anything.**

---

## CLAUDE CODE PROMPT

```
You are adding two new specification compliance agents to an existing Clean Language 
compiler project. This project has been in development for 7+ months and has existing 
QA agents already configured.

CRITICAL: DO NOT assume anything about the project structure. You must discover and 
validate everything first. The configuration below contains ASSUMPTIONS that may be 
WRONG about:
- Number and names of compiler layers
- Directory structure
- Module organization  
- Existing agent locations
- Specification document location
- Test organization

═══════════════════════════════════════════════════════════════════════════════════
                              PHASE 1: PROJECT DISCOVERY
                              (DO NOT SKIP - MANDATORY)
═══════════════════════════════════════════════════════════════════════════════════

Execute these commands and REPORT your findings before proceeding:

### 1.1 Project Root Structure
```bash
ls -la
find . -maxdepth 2 -type d | grep -v target | grep -v .git | head -50
```

### 1.2 Source Code Structure  
```bash
# Find all Rust source directories
find . -type d -name "src" | head -10

# List main source structure
ls -la src/ 2>/dev/null || echo "No src/ directory"

# Find all module files
find . -path ./target -prune -o -name "mod.rs" -print | head -30
find . -path ./target -prune -o -name "lib.rs" -print | head -10
```

### 1.3 Identify Compiler Layers
```bash
# Look for layer-like directories
ls -la src/ 2>/dev/null

# Search for common compiler component names
find . -path ./target -prune -o -type d \( \
    -name "*lexer*" -o -name "*parser*" -o -name "*ast*" -o \
    -name "*semantic*" -o -name "*type*" -o -name "*ir*" -o \
    -name "*codegen*" -o -name "*wasm*" -o -name "*optimizer*" -o \
    -name "*analysis*" -o -name "*lowering*" -o -name "*backend*" \
\) -print 2>/dev/null

# Check lib.rs or main.rs for module declarations
cat src/lib.rs 2>/dev/null | grep -E "^mod |^pub mod " | head -20
cat src/main.rs 2>/dev/null | grep -E "^mod |^pub mod " | head -20
```

### 1.4 Find Existing Agents and Claude Config
```bash
# Check for existing Claude configuration
find . -name "CLAUDE.md" -o -name "claude.md" 2>/dev/null
find . -type d -name ".claude" 2>/dev/null
ls -la .claude/ 2>/dev/null
ls -la .claude/agents/ 2>/dev/null
ls -la .claude/workflows/ 2>/dev/null

# Read existing agent configurations
cat .claude/README.md 2>/dev/null || echo "No .claude/README.md"
```

### 1.5 Find Specification Documents
```bash
# Search for specification files
find . -path ./target -prune -o \( \
    -name "*spec*" -o -name "*specification*" -o \
    -name "SPEC.md" -o -name "language*.md" -o \
    -name "grammar*" -o -name "syntax*" \
\) -print 2>/dev/null

# Check common documentation locations
ls -la docs/ 2>/dev/null
ls -la docs/spec/ 2>/dev/null
ls -la doc/ 2>/dev/null
ls -la specification/ 2>/dev/null

# Look for any .md files that might be specs
find . -path ./target -prune -o -name "*.md" -print 2>/dev/null | head -30
```

### 1.6 Find Existing Tests
```bash
# Test directory structure
ls -la tests/ 2>/dev/null
find tests/ -type d 2>/dev/null | head -30

# Count existing tests
cargo test -- --list 2>/dev/null | tail -5

# Check for spec-related tests already
find . -path ./target -prune -o -name "*spec*test*" -print 2>/dev/null
grep -r "Spec Section" tests/ --include="*.rs" 2>/dev/null | head -10
```

### 1.7 Check Cargo.toml
```bash
# Project name and structure
head -30 Cargo.toml

# Workspace check
grep -A 10 "\[workspace\]" Cargo.toml 2>/dev/null

# Existing test dependencies
grep -A 20 "\[dev-dependencies\]" Cargo.toml
```

### 1.8 Plugin System Discovery
```bash
# Find plugin-related code
find . -path ./target -prune -o \( \
    -name "*plugin*" -o -name "*extension*" -o -name "*hook*" \
\) -print 2>/dev/null

# Check for plugin module
grep -r "plugin" src/ --include="*.rs" -l 2>/dev/null | head -10
```

═══════════════════════════════════════════════════════════════════════════════════
                              PHASE 2: DISCOVERY REPORT
                              (MUST COMPLETE BEFORE PHASE 3)
═══════════════════════════════════════════════════════════════════════════════════

After running the discovery commands, CREATE a report file:

**File:** `QA_DISCOVERY_REPORT.md`

```markdown
# Project Discovery Report

**Date:** [TODAY]
**Project:** Clean Language Compiler

## 1. Project Structure

### Root Layout
[What you found]

### Source Organization
[Actual src/ structure]

## 2. Compiler Architecture

### Identified Layers/Components
| # | Component Name | Directory | Purpose |
|---|----------------|-----------|---------|
| 1 | [actual name] | [actual path] | [what it does] |
| 2 | [actual name] | [actual path] | [what it does] |
| ... | ... | ... | ... |

### Layer Count: [ACTUAL NUMBER]
**Note:** Configuration assumed 7 layers. Actual: [X]

## 3. Plugin System

### Location: [actual path or "Not found"]
### Structure: [description]

## 4. Existing Claude Configuration

### CLAUDE.md Location: [path or "Not found"]
### .claude/ Directory: [exists/doesn't exist]
### Existing Agents:
- [list actual agents found]

### Existing Workflows:
- [list actual workflows found]

## 5. Specification Documents

### Location: [actual path or "NOT FOUND - MUST ASK USER"]
### Files Found:
- [list spec files]

### If NOT FOUND:
⚠️ STOP: Must ask user for specification location before proceeding.

## 6. Test Organization

### Test Directory Structure:
[actual structure]

### Existing Test Categories:
- [list what exists]

### Spec Compliance Tests: [exist/don't exist]

## 7. Discrepancies from Configuration

| Assumption | Reality | Impact |
|------------|---------|--------|
| 7 compiler layers | [actual] | [how to adapt] |
| src/lexer/ exists | [actual] | [how to adapt] |
| .claude/agents/ exists | [actual] | [how to adapt] |
| ... | ... | ... |

## 8. Recommendations

### Before Implementing:
1. [what needs to be clarified]
2. [what needs user input]

### Adaptations Needed:
1. [how config must change]
2. [how config must change]
```

═══════════════════════════════════════════════════════════════════════════════════
                              PHASE 3: USER CONFIRMATION
                              (MUST GET APPROVAL BEFORE PHASE 4)
═══════════════════════════════════════════════════════════════════════════════════

After creating the discovery report, STOP and ASK the user:

"I've analyzed your project structure. Here's what I found:

**Compiler Layers:** [X] components identified:
- [list them]

**Existing Agents:** [list or 'none found']

**Specification Location:** [path or 'NOT FOUND - please provide location']

**Key Discrepancies:**
- [list main differences from assumed config]

Before I proceed with adding the specification compliance agents, please confirm:

1. Is my understanding of the compiler layers correct?
2. [If spec not found] Where is the Clean Language specification located?
3. Are there any other existing configurations I should know about?
4. Should I proceed with the adapted configuration?"

WAIT FOR USER RESPONSE BEFORE CONTINUING.

═══════════════════════════════════════════════════════════════════════════════════
                              PHASE 4: SPECIFICATION ANALYSIS
                              (ONLY AFTER USER CONFIRMS PHASE 3)
═══════════════════════════════════════════════════════════════════════════════════

Once user confirms and provides spec location:

### 4.1 Read the Specification
```bash
# Read all spec files
cat [SPEC_PATH]/*.md 2>/dev/null

# Or single file
cat [SPEC_FILE] 2>/dev/null
```

### 4.2 Extract Features
Create a catalog of ALL language features defined in the spec:

**File:** `spec_features_catalog.md`

```markdown
# Clean Language Specification - Feature Catalog

**Spec Version:** [version if found]
**Features Extracted:** [count]

## Categories

### Lexical Features
- [ ] Keywords: [list]
- [ ] Operators: [list]
- [ ] Literals: [types]
- [ ] Comments: [styles]

### Type System
- [ ] Primitive types: [list]
- [ ] Composite types: [list]
- [ ] Type inference: [yes/no, rules]

### Expressions
- [ ] Arithmetic: [operators]
- [ ] Comparison: [operators]
- [ ] Logical: [operators]
- [ ] Precedence: [defined/not defined]

### Statements
- [ ] Variable declaration: [syntax]
- [ ] Assignment: [syntax]
- [ ] Control flow: [list]

### Functions
- [ ] Declaration syntax: [describe]
- [ ] Parameters: [rules]
- [ ] Return types: [rules]

### Modules (if applicable)
- [ ] Import syntax: [describe]
- [ ] Export rules: [describe]

### WASM-Specific
- [ ] Exports: [rules]
- [ ] Memory: [model]

## Total Testable Features: [COUNT]
```

### 4.3 Report to User
"I've analyzed the specification and found [X] testable features across [Y] categories.

Top priority areas:
1. [area] - [X features]
2. [area] - [X features]

Shall I proceed with creating the agents and initial test structure?"

WAIT FOR USER CONFIRMATION.

═══════════════════════════════════════════════════════════════════════════════════
                              PHASE 5: ADAPTED IMPLEMENTATION
                              (ONLY AFTER USER CONFIRMS PHASE 4)  
═══════════════════════════════════════════════════════════════════════════════════

Now implement the agents, ADAPTED to the actual project structure:

### 5.1 Create Specification Coverage Agent

**File:** `.claude/agents/spec_coverage.md`

Adapt the agent template below to use:
- ACTUAL layer names discovered
- ACTUAL directory structure discovered
- ACTUAL specification location
- ACTUAL test organization pattern

### 5.2 Create Test Compliance Auditor Agent

**File:** `.claude/agents/test_auditor.md`

Adapt similarly.

### 5.3 Create Workflow

**File:** `.claude/workflows/spec_compliance.md`

### 5.4 Create Test Structure

Create `tests/spec_compliance/` structure based on:
- ACTUAL specification organization
- ACTUAL test patterns already in project

### 5.5 Update Existing Agents

If verification agent exists, add spec compliance checks.
If bug fixer agent exists, add spec reference requirements.

═══════════════════════════════════════════════════════════════════════════════════
                              PHASE 6: VERIFICATION
                              (MANDATORY BEFORE COMPLETION)
═══════════════════════════════════════════════════════════════════════════════════

After implementation:

```bash
# Verify no breakage
cargo check
cargo test

# Verify new structure
ls -la .claude/agents/
ls -la tests/spec_compliance/ 2>/dev/null

# Report
echo "Implementation complete. Created:"
find . -newer QA_DISCOVERY_REPORT.md -name "*.md" -o -name "*.rs" 2>/dev/null | head -20
```

Report final status to user.
```

---

# AGENT TEMPLATES

## Specification Coverage Agent Template

Adapt this template based on actual project structure discovered in Phase 1.

**File:** `.claude/agents/spec_coverage.md`

```markdown
# Specification Coverage Agent

## Identity

You are a language specification analyst and test engineer for the Clean Language
compiler. Your mission is to ensure 100% test coverage of the Clean Language 
specification.

## Project-Specific Configuration

<!-- ADAPT THESE BASED ON DISCOVERY -->

### Compiler Components
<!-- Replace with actual components discovered -->
| Component | Location | Tests Location |
|-----------|----------|----------------|
| [ACTUAL_NAME_1] | [ACTUAL_PATH] | tests/spec_compliance/[name]/ |
| [ACTUAL_NAME_2] | [ACTUAL_PATH] | tests/spec_compliance/[name]/ |
| ... | ... | ... |

### Specification Location
<!-- Replace with actual spec location -->
Path: [ACTUAL_SPEC_PATH]

### Test Patterns
<!-- Match existing test patterns in project -->
Naming convention: [ACTUAL_PATTERN]
Module structure: [ACTUAL_STRUCTURE]

## Responsibilities

### 1. Specification Analysis

Read the Clean Language specification from [ACTUAL_SPEC_PATH] and extract:

- **Syntax Rules**: Keywords, operators, literals, comments
- **Type System**: All types, inference rules, compatibility
- **Expressions**: All operators, precedence, evaluation
- **Statements**: Declarations, assignments, control flow
- **Functions**: Declaration, parameters, returns
- **Modules**: Imports, exports (if applicable)
- **WASM**: Export rules, memory model

### 2. Test Creation

For EACH specification feature:

#### Positive Test Template
```rust
/// Spec: [SECTION] - [DESCRIPTION]
/// Requirement: [WHAT SPEC SAYS]
#[test]
fn spec_[section]_[feature]_[case]() {
    let source = r#"
        // Minimal Clean Language code testing this feature
    "#;
    
    let result = compile(source);
    assert!(result.is_ok(), "Spec [SECTION]: [requirement]");
    
    // If runtime test needed:
    let output = execute(&result.unwrap(), "func_name", &[]);
    assert_eq!(output, expected, "Spec [SECTION]: [requirement]");
}
```

#### Negative Test Template
```rust
/// Spec: [SECTION] - [DESCRIPTION]  
/// Requirement: [WHAT SHOULD FAIL]
#[test]
fn spec_[section]_[feature]_invalid_[case]() {
    let source = r#"
        // Code that should be rejected
    "#;
    
    let result = compile(source);
    assert!(result.is_err(), "Spec [SECTION]: must reject [case]");
}
```

### 3. Coverage Tracking

Maintain `tests/spec_compliance/COVERAGE_MATRIX.md`:

```markdown
# Specification Coverage Matrix

**Spec Version:** [VERSION]
**Last Updated:** [DATE]

## Coverage by Section

| Section | Feature | Positive | Negative | Edge | Status |
|---------|---------|----------|----------|------|--------|
| [X.Y] | [name] | [n/m] | [n/m] | [n/m] | ✅/⚠️/❌ |

## Summary
- Total features: [N]
- Tested: [N]  
- Coverage: [X]%
```

### 4. Test Organization

<!-- Adapt to match project's actual test structure -->
```
tests/spec_compliance/
├── mod.rs
├── COVERAGE_MATRIX.md
├── [section_name]/           # Based on actual spec organization
│   ├── mod.rs
│   └── [feature]_tests.rs
└── ...
```

## Commands

```bash
# Run all spec tests
cargo test spec_compliance

# Run section
cargo test spec_compliance::[section]

# With output  
cargo test spec_compliance -- --nocapture
```

## Success Criteria

- [ ] All spec sections have test modules
- [ ] Every feature has ≥1 positive test
- [ ] Every feature has ≥1 negative test
- [ ] Coverage ≥ 95%
- [ ] All tests pass
- [ ] All tests reference spec sections

## Execution Mode

```yaml
autonomous: true
max_duration: 8 hours
outputs:
  - tests/spec_compliance/**/*.rs
  - tests/spec_compliance/COVERAGE_MATRIX.md
```
```

---

## Test Compliance Auditor Agent Template

**File:** `.claude/agents/test_auditor.md`

```markdown
# Test Compliance Auditor Agent

## Identity

You are a test quality auditor for the Clean Language compiler. You ensure all 
tests verify specification-defined behavior, not implementation details or 
undefined behavior.

## Project-Specific Configuration

<!-- ADAPT BASED ON DISCOVERY -->

### Test Locations
<!-- Replace with actual test locations discovered -->
- Unit tests: [ACTUAL_PATH]
- Integration tests: [ACTUAL_PATH]  
- E2E tests: [ACTUAL_PATH]
- Spec compliance: tests/spec_compliance/

### Specification Location
Path: [ACTUAL_SPEC_PATH]

### Existing Patterns
<!-- Document existing test patterns to maintain consistency -->
Naming: [ACTUAL_PATTERN]
Structure: [ACTUAL_STRUCTURE]

## Audit Categories

| Category | Symbol | Action |
|----------|--------|--------|
| Compliant | ✅ | None |
| Missing spec reference | ⚠️ | Add reference |
| Implementation detail | ❌ | Delete or convert |
| Undefined behavior | ❌ | Delete or mark |
| Wrong expectation | ❌ | Fix expected value |
| Outdated | ❌ | Update to current spec |

## Audit Process

### Phase 1: Discover All Tests
```bash
find [TEST_PATHS] -name "*.rs" -type f
cargo test -- --list 2>/dev/null | wc -l
```

### Phase 2: Analyze Each Test

For each `#[test]` function:
1. Has spec reference? (`/// Spec:` or `/// Spec Section:`)
2. Reference valid? (Section exists in current spec)
3. Tests observable behavior? (Not internal structures)
4. Expectation correct? (Matches spec definition)
5. Behavior defined? (Spec defines this behavior)

### Phase 3: Generate Report

**File:** `tests/AUDIT_REPORT.md`

```markdown
# Test Compliance Audit Report

**Date:** [DATE]
**Spec Version:** [VERSION]
**Tests Audited:** [COUNT]

## Summary

| Category | Count | % |
|----------|-------|---|
| ✅ Compliant | X | X% |
| ⚠️ Missing Reference | X | X% |
| ❌ Issues | X | X% |

**Compliance Score: X%**

## Critical Issues

### [Test Name]
- **File:** [path:line]
- **Category:** [category]
- **Issue:** [description]
- **Fix:** [recommendation]

## Tests Missing References

| Test | File | Suggested Spec Section |
|------|------|------------------------|
| [name] | [path] | [section] |

## Recommendations

1. [Priority action]
2. [Priority action]
```

## Commands

```bash
# Find tests without spec references
grep -rL "Spec Section\|Spec:" [TEST_PATHS] --include="*.rs"

# Count by category
grep -r "Spec Section" [TEST_PATHS] --include="*.rs" | wc -l
```

## Success Criteria

- [ ] All tests audited
- [ ] Report generated
- [ ] Critical issues documented
- [ ] Compliance score calculated

## Execution Mode

```yaml
autonomous: true
max_duration: 4 hours
outputs:
  - tests/AUDIT_REPORT.md
```
```

---

## Workflow Template

**File:** `.claude/workflows/spec_compliance.md`

```markdown
# Specification Compliance Workflow

## Trigger
- Weekly (Sunday 02:00 UTC)
- Before releases
- After spec changes
- Manual

## Steps

1. **Update Spec Catalog**
   - Run Specification Coverage Agent
   - Update feature catalog
   - Identify gaps

2. **Create Missing Tests**
   - Generate tests for uncovered features
   - Update COVERAGE_MATRIX.md

3. **Audit Existing Tests**
   - Run Test Compliance Auditor
   - Generate AUDIT_REPORT.md

4. **Verify**
   ```bash
   cargo test spec_compliance
   cargo test
   ```

5. **Report**
   - Spec coverage: X%
   - Test compliance: X%
   - Issues found: N

## Success Criteria
- Spec coverage ≥ 95%
- Test compliance ≥ 98%
- Zero critical issues
```

---

# CHECKLIST FOR CLAUDE CODE

Before implementing, verify you have:

- [ ] Discovered actual project structure
- [ ] Found all compiler components/layers
- [ ] Located existing .claude/ configuration
- [ ] Found specification documents (or asked user)
- [ ] Created QA_DISCOVERY_REPORT.md
- [ ] Got user confirmation on findings
- [ ] Analyzed specification features
- [ ] Adapted templates to actual structure

Only then:

- [ ] Created .claude/agents/spec_coverage.md (adapted)
- [ ] Created .claude/agents/test_auditor.md (adapted)
- [ ] Created .claude/workflows/spec_compliance.md
- [ ] Created tests/spec_compliance/ structure
- [ ] Created COVERAGE_MATRIX.md template
- [ ] Updated existing agents (if applicable)
- [ ] Verified with cargo check/test
- [ ] Reported results to user
