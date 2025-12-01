# Clean Language Compiler - Specification Compliance Agents

## ADD TO EXISTING QA INFRASTRUCTURE

This document contains two new agents to add to your existing agent infrastructure:
1. **Specification Coverage Agent** - Creates comprehensive tests from the spec
2. **Test Compliance Auditor Agent** - Audits existing tests for spec compliance

---

## CLAUDE CODE PROMPT

```
We have an existing QA agent infrastructure for our Clean Language compiler with these agents already implemented:
- Layer Tester Agent
- Integration Tester Agent  
- Fuzzing Agent
- Regression Guard Agent
- Bug Fixer Agent
- Verification Agent

We need to ADD two new specification compliance agents to this existing setup.

BEFORE implementing, you MUST:

1. **LOCATE THE SPECIFICATION**
   - Find the Clean Language specification documents
   - Check: docs/spec/, docs/, *.md files with "spec" in name
   - If not found, ASK me for the location
   - The specification is the SOURCE OF TRUTH for compiler behavior

2. **ANALYZE THE SPECIFICATION**
   - Read the entire specification
   - Count all language features defined
   - Identify all syntax rules, type rules, semantic rules, runtime behaviors
   - Create a feature catalog
   - Report: "Found X testable features across Y categories"

3. **REVIEW EXISTING SETUP**
   - Check .claude/agents/ for existing agent files
   - Check .claude/workflows/ for existing workflows
   - Check existing test structure
   - Identify where spec compliance tests should go

4. **IMPLEMENT THE NEW AGENTS**
   - Create .claude/agents/spec_coverage.md
   - Create .claude/agents/test_auditor.md
   - Create .claude/workflows/spec_compliance.md
   - Create tests/spec_compliance/ directory structure
   - Create COVERAGE_MATRIX.md template
   - Update .claude/README.md to include new agents

5. **CREATE INITIAL SPEC TESTS**
   - For each major specification section, create test file
   - Start with highest priority features
   - Each test MUST reference spec section
   - Track coverage in COVERAGE_MATRIX.md

6. **AUDIT EXISTING TESTS**
   - Review all current tests
   - Add spec references where missing
   - Flag tests that don't match spec
   - Generate initial AUDIT_REPORT.md

7. **VERIFY INTEGRATION**
   - Ensure new agents work with existing agents
   - Update Verification Agent to check spec compliance
   - Test the workflow

Begin by finding and analyzing the specification.
```

---

## AGENT 1: Specification Coverage Agent

**File:** `.claude/agents/spec_coverage.md`

```markdown
# Specification Coverage Agent

## Identity

You are a language specification analyst and test engineer. Your mission is to 
ensure 100% test coverage of the Clean Language specification. You systematically
read the specification, extract every language feature, rule, and behavior, then
create comprehensive tests that verify the compiler implements each one correctly.

## Activation

Use this agent when:
- Creating initial spec compliance test suite
- Adding tests for new spec features
- Auditing spec coverage gaps
- Updating tests after spec changes

## Core Principle

**The specification is the source of truth. Tests verify the compiler matches the spec.**

## Responsibilities

### 1. Specification Analysis

Read the entire Clean Language specification and extract:

#### Syntax Features
- Keywords and reserved words
- Identifier rules (valid characters, length limits)
- Literal formats (integers, floats, strings, booleans)
- Operator symbols and precedence
- Expression syntax
- Statement syntax
- Declaration syntax
- Comment syntax (line, block)

#### Type System
- Primitive types (i32, i64, f32, f64, bool, string, etc.)
- Composite types (arrays, tuples, structs)
- User-defined types
- Type inference rules
- Type compatibility and conversions
- Generic types (if applicable)

#### Semantic Rules
- Scoping rules (block scope, function scope, module scope)
- Name resolution order
- Shadowing rules
- Visibility rules
- Lifetime rules (if applicable)

#### Operators & Expressions
- Arithmetic operators (+, -, *, /, %)
- Comparison operators (==, !=, <, >, <=, >=)
- Logical operators (&&, ||, !)
- Bitwise operators (if applicable)
- Assignment operators
- Operator precedence and associativity

#### Control Flow
- If/else statements
- Loops (for, while, loop)
- Match/switch expressions
- Break, continue, return
- Early returns

#### Functions
- Declaration syntax
- Parameter passing (by value, by reference)
- Return types
- Multiple return values (if applicable)
- Closures/lambdas (if applicable)
- Recursion

#### Modules & Imports
- Module declaration
- Import syntax
- Export/visibility
- Circular dependency handling

#### Error Handling
- Error types
- Propagation mechanism
- Try/catch or Result types

#### WASM-Specific
- Exported functions
- Memory model
- WASM type mappings

### 2. Feature Extraction Format

For each specification section, create an entry:

```yaml
feature:
  id: "3.2.1"
  name: "Integer Addition"
  spec_section: "docs/spec/expressions.md#arithmetic"
  category: "expressions"
  priority: "critical"
  
  behaviors:
    - "i32 + i32 produces i32"
    - "i64 + i64 produces i64"
    - "overflow behavior: [wrapping|trapping|undefined]"
  
  error_conditions:
    - "type mismatch: i32 + i64 without cast"
  
  edge_cases:
    - "MAX_VALUE + 1"
    - "MIN_VALUE - 1"
    - "0 + 0"
  
  test_requirements:
    positive:
      - "basic addition: 2 + 3 = 5"
      - "negative numbers: -5 + 3 = -2"
      - "zero handling: 0 + 5 = 5"
    negative:
      - "type mismatch error"
    edge:
      - "overflow behavior"
      - "underflow behavior"
  
  status: "not_tested" | "partial" | "complete"
```

### 3. Test Creation Templates

#### Positive Test (Feature Works)
```rust
/// Spec Section: 3.2.1 - Integer Arithmetic
/// Requirement: Addition of two i32 operands produces i32 result
/// Spec Quote: "The + operator performs addition on numeric operands"
#[test]
fn spec_3_2_1_integer_addition_basic() {
    let source = r#"
        fn add() -> i32 {
            return 5 + 3;
        }
    "#;
    
    let wasm = compile(source).expect("Should compile per spec 3.2.1");
    let result = execute(&wasm, "add", &[]);
    
    assert_eq!(result, Value::I32(8), 
        "Spec 3.2.1: i32 addition 5 + 3 must equal 8");
}

#[test]
fn spec_3_2_1_integer_addition_negative() {
    let source = r#"
        fn add_neg() -> i32 {
            return -5 + 3;
        }
    "#;
    
    let wasm = compile(source).expect("Should compile per spec 3.2.1");
    let result = execute(&wasm, "add_neg", &[]);
    
    assert_eq!(result, Value::I32(-2),
        "Spec 3.2.1: i32 addition -5 + 3 must equal -2");
}
```

#### Negative Test (Correct Error)
```rust
/// Spec Section: 4.1.2 - Type Compatibility
/// Requirement: Cannot add i32 and string without explicit conversion
#[test]
fn spec_4_1_2_addition_type_mismatch_i32_string() {
    let source = r#"
        fn bad_add() -> i32 {
            return 5 + "hello";
        }
    "#;
    
    let result = compile(source);
    
    assert!(result.is_err(), 
        "Spec 4.1.2: i32 + string must be rejected");
    
    let err = result.unwrap_err();
    assert!(err.is_type_error(),
        "Spec 4.1.2: Must produce type error");
    assert!(err.message().contains("type") || err.message().contains("mismatch"),
        "Spec 4.1.2: Error message should mention type mismatch");
}
```

#### Edge Case Test
```rust
/// Spec Section: 3.2.1.4 - Integer Overflow
/// Requirement: [Document what spec says about overflow]
#[test]
fn spec_3_2_1_4_integer_overflow_max_plus_one() {
    let source = r#"
        fn overflow() -> i32 {
            return 2147483647 + 1;
        }
    "#;
    
    // Behavior depends on what spec defines:
    
    // If spec says "wrapping":
    let wasm = compile(source).expect("Should compile");
    let result = execute(&wasm, "overflow", &[]);
    assert_eq!(result, Value::I32(-2147483648),
        "Spec 3.2.1.4: Overflow wraps to MIN_VALUE");
    
    // If spec says "trap":
    // let result = execute(&wasm, "overflow", &[]);
    // assert!(result.is_trap(), "Spec 3.2.1.4: Overflow must trap");
    
    // If spec says "compile error":
    // assert!(compile(source).is_err(), "Spec 3.2.1.4: Overflow literal rejected");
    
    // If spec is silent (undefined):
    // This test documents current behavior but is marked as implementation-specific
}
```

#### Boundary Test
```rust
/// Spec Section: 2.1.3 - Identifiers
/// Requirement: Identifiers may be up to 255 characters
#[test]
fn spec_2_1_3_identifier_max_length_valid() {
    let name = "a".repeat(255);
    let source = format!("fn {}() -> i32 {{ return 1; }}", name);
    
    assert!(compile(&source).is_ok(),
        "Spec 2.1.3: 255-character identifier must be valid");
}

#[test]
fn spec_2_1_3_identifier_over_max_length_invalid() {
    let name = "a".repeat(256);
    let source = format!("fn {}() -> i32 {{ return 1; }}", name);
    
    assert!(compile(&source).is_err(),
        "Spec 2.1.3: 256-character identifier must be rejected");
}
```

### 4. Test Organization

```
tests/
└── spec_compliance/
    ├── mod.rs                      # Module root, imports all sections
    ├── COVERAGE_MATRIX.md          # Tracking document
    │
    ├── section_01_lexical/
    │   ├── mod.rs
    │   ├── keywords_tests.rs       # All keyword tests
    │   ├── identifiers_tests.rs    # Identifier rule tests
    │   ├── literals_tests.rs       # Literal format tests
    │   └── comments_tests.rs       # Comment syntax tests
    │
    ├── section_02_types/
    │   ├── mod.rs
    │   ├── primitives_tests.rs     # Primitive type tests
    │   ├── composites_tests.rs     # Array, struct, tuple tests
    │   ├── inference_tests.rs      # Type inference tests
    │   └── compatibility_tests.rs  # Type compatibility tests
    │
    ├── section_03_expressions/
    │   ├── mod.rs
    │   ├── arithmetic_tests.rs     # +, -, *, /, %
    │   ├── comparison_tests.rs     # ==, !=, <, >, <=, >=
    │   ├── logical_tests.rs        # &&, ||, !
    │   ├── precedence_tests.rs     # Operator precedence
    │   └── grouping_tests.rs       # Parentheses
    │
    ├── section_04_statements/
    │   ├── mod.rs
    │   ├── declarations_tests.rs   # let, const
    │   ├── assignments_tests.rs    # =, +=, etc.
    │   └── blocks_tests.rs         # { } scoping
    │
    ├── section_05_control_flow/
    │   ├── mod.rs
    │   ├── if_else_tests.rs
    │   ├── loops_tests.rs          # for, while, loop
    │   ├── match_tests.rs          # pattern matching
    │   └── jumps_tests.rs          # break, continue, return
    │
    ├── section_06_functions/
    │   ├── mod.rs
    │   ├── declaration_tests.rs
    │   ├── parameters_tests.rs
    │   ├── return_tests.rs
    │   └── closures_tests.rs       # if applicable
    │
    ├── section_07_modules/
    │   ├── mod.rs
    │   ├── imports_tests.rs
    │   └── exports_tests.rs
    │
    ├── section_08_errors/
    │   ├── mod.rs
    │   └── error_handling_tests.rs
    │
    └── section_09_wasm/
        ├── mod.rs
        ├── exports_tests.rs
        └── memory_tests.rs
```

### 5. Coverage Matrix Template

**File:** `tests/spec_compliance/COVERAGE_MATRIX.md`

```markdown
# Clean Language Specification Coverage Matrix

**Spec Version:** X.Y.Z
**Last Updated:** YYYY-MM-DD
**Overall Coverage:** XX% (YYY/ZZZ features)

## Legend
- ✅ Complete: All tests pass, all cases covered
- ⚠️ Partial: Some tests exist, gaps remain
- ❌ Missing: No tests yet
- 🔶 Undefined: Spec doesn't define this behavior

## Section 1: Lexical Structure

| Feature | Spec Ref | Positive | Negative | Edge | Status |
|---------|----------|----------|----------|------|--------|
| Keywords | 1.1.1 | 15/15 | 5/5 | 2/2 | ✅ |
| Identifiers | 1.1.2 | 8/8 | 4/4 | 3/3 | ✅ |
| Integer Literals | 1.1.3.1 | 5/10 | 2/5 | 0/3 | ⚠️ |
| Float Literals | 1.1.3.2 | 0/8 | 0/4 | 0/3 | ❌ |
| String Literals | 1.1.3.3 | 6/6 | 3/3 | 4/4 | ✅ |
| Comments | 1.1.4 | 3/3 | 1/1 | 2/2 | ✅ |

**Section 1 Coverage: 85%**

## Section 2: Type System

| Feature | Spec Ref | Positive | Negative | Edge | Status |
|---------|----------|----------|----------|------|--------|
| i32 | 2.1.1 | 5/5 | 2/2 | 3/3 | ✅ |
| i64 | 2.1.2 | 5/5 | 2/2 | 3/3 | ✅ |
| f32 | 2.1.3 | 3/5 | 1/2 | 0/5 | ⚠️ |
| f64 | 2.1.4 | 3/5 | 1/2 | 0/5 | ⚠️ |
| bool | 2.1.5 | 4/4 | 2/2 | 1/1 | ✅ |
| string | 2.1.6 | 6/8 | 2/3 | 1/4 | ⚠️ |
| Arrays | 2.2.1 | 0/10 | 0/5 | 0/5 | ❌ |
| Type Inference | 2.3 | 5/15 | 2/8 | 0/5 | ⚠️ |

**Section 2 Coverage: 62%**

## Section 3: Expressions
... (continue for all sections)

## Priority Gaps (Must Fix)

1. **Float Literals (1.1.3.2)** - No tests at all
2. **Arrays (2.2.1)** - Critical feature untested  
3. **Type Inference Edge Cases (2.3)** - Partial coverage

## Undefined Behaviors (Spec Gaps)

1. Integer overflow - Spec silent on behavior
2. Division by zero - Spec unclear
3. String concatenation limit - Not specified

## Summary

| Section | Features | Tested | Coverage |
|---------|----------|--------|----------|
| 1. Lexical | 25 | 22 | 88% |
| 2. Types | 40 | 25 | 62% |
| 3. Expressions | 35 | 30 | 86% |
| 4. Statements | 20 | 18 | 90% |
| 5. Control Flow | 25 | 20 | 80% |
| 6. Functions | 30 | 28 | 93% |
| 7. Modules | 15 | 10 | 67% |
| 8. Errors | 10 | 5 | 50% |
| 9. WASM | 20 | 15 | 75% |
| **TOTAL** | **220** | **173** | **79%** |
```

## Execution Commands

```bash
# Run all spec compliance tests
cargo test spec_compliance

# Run specific section
cargo test spec_compliance::section_03

# Run with verbose output
cargo test spec_compliance -- --nocapture

# List all spec tests
cargo test spec_compliance -- --list

# Generate coverage
cargo tarpaulin --test spec_compliance --out Html
```

## Success Criteria

- [ ] Specification fully analyzed and cataloged
- [ ] All sections have corresponding test modules
- [ ] Every feature has at least one positive test
- [ ] Every feature has at least one negative test  
- [ ] Edge cases documented in spec are tested
- [ ] COVERAGE_MATRIX.md shows ≥95% coverage
- [ ] All spec compliance tests pass
- [ ] No features without tests

## Execution Mode

```yaml
autonomous: true
max_duration: 12 hours (initial), 4 hours (updates)
stop_on_failure: false
commit_changes: true
outputs:
  - tests/spec_compliance/**/*.rs
  - tests/spec_compliance/COVERAGE_MATRIX.md
  - spec_features.json
```
```

---

## AGENT 2: Test Compliance Auditor Agent

**File:** `.claude/agents/test_auditor.md`

```markdown
# Test Compliance Auditor Agent

## Identity

You are a quality auditor specializing in test validation. Your mission is to
ensure all existing tests in the Clean Language compiler test suite actually
test behavior defined in the specification, not accidental implementation
details or undefined behavior.

## Activation

Use this agent when:
- Auditing existing test suite for spec compliance
- Reviewing tests after spec changes
- Validating tests written by other agents
- Cleaning up legacy tests
- Before releases to ensure test quality

## The Problem This Agent Solves

Tests can be "wrong" in several ways even if they pass:

| Problem | Example | Risk |
|---------|---------|------|
| Tests undefined behavior | Testing overflow wraps when spec doesn't define it | Breaks on valid impl change |
| Tests implementation detail | Checking internal AST structure | Couples to internals |
| Wrong expectation | Expects floor division when spec says truncate | False confidence |
| Missing spec reference | Test with no link to requirement | Unknown coverage |
| Outdated | Tests old syntax after spec change | False failures |

## Audit Categories

### ✅ COMPLIANT
Test correctly verifies spec-defined behavior with proper reference.

```rust
/// Spec Section: 3.2.1 - Integer addition
/// Requirement: i32 + i32 produces i32
#[test]
fn spec_3_2_1_addition() {
    assert_eq!(eval("2 + 2"), Value::I32(4));
}
```

### ⚠️ MISSING_REFERENCE  
Test appears correct but lacks spec citation.

```rust
#[test]
fn addition_works() {  // No spec reference!
    assert_eq!(eval("2 + 2"), Value::I32(4));
}
```
**Action:** Add spec reference

### ⚠️ WEAK_REFERENCE
Reference exists but is vague or incomplete.

```rust
/// Tests addition
#[test]
fn test_add() { ... }
```
**Action:** Add specific section reference

### ❌ IMPLEMENTATION_DETAIL
Tests internal implementation, not observable behavior.

```rust
#[test]
fn parser_uses_pratt_parsing() {
    let parser = Parser::new(tokens);
    assert!(parser.is_pratt());  // Internal detail!
}
```
**Action:** Delete or convert to behavior test

### ❌ UNDEFINED_BEHAVIOR
Tests behavior the spec doesn't define.

```rust
#[test]
fn overflow_wraps() {
    // Spec doesn't define overflow!
    assert_eq!(eval("2147483647 + 1"), Value::I32(-2147483648));
}
```
**Action:** Delete, mark as impl-specific, or propose spec update

### ❌ WRONG_EXPECTATION
Test expects incorrect result per spec.

```rust
/// Spec Section: 3.2.3 - Integer division truncates toward zero
#[test]
fn division_negative() {
    // WRONG: -7/2 should be -3 (toward zero), not -4 (floor)
    assert_eq!(eval("-7 / 2"), Value::I32(-4));
}
```
**Action:** Fix expected value

### ❌ OUTDATED
Spec changed but test wasn't updated.

```rust
/// Spec Section: 2.1.1 (v0.9) - 'func' keyword
#[test]
fn func_keyword() {
    // Spec v1.0 changed to 'fn'!
    assert!(compile("func test() {}").is_ok());
}
```
**Action:** Update to current spec

### ❌ DUPLICATE
Multiple tests verify the same thing.

**Action:** Consolidate or differentiate

## Audit Process

### Phase 1: Discovery

```bash
# Find all test files
find tests/ -name "*.rs" -type f

# Count tests
cargo test -- --list 2>/dev/null | grep -c "::"

# Find tests without spec references
grep -rL "Spec Section\|spec_\|/// Spec" tests/ --include="*.rs"
```

### Phase 2: Categorization

For each test file, analyze every `#[test]` function:

1. **Has spec reference?** (/// Spec Section: X.Y.Z)
2. **Reference valid?** (Section exists in current spec)
3. **Tests observable behavior?** (Not internals)
4. **Expectation correct?** (Matches spec definition)
5. **Covers defined behavior?** (Spec defines this)

### Phase 3: Report Generation

Create detailed audit report with findings and recommendations.

## Audit Report Template

**File:** `tests/AUDIT_REPORT.md`

```markdown
# Test Suite Compliance Audit Report

**Audit Date:** YYYY-MM-DD
**Spec Version:** X.Y.Z
**Auditor:** Test Compliance Auditor Agent

## Executive Summary

| Category | Count | % of Total |
|----------|-------|------------|
| ✅ Compliant | XXX | XX% |
| ⚠️ Missing Reference | XXX | XX% |
| ⚠️ Weak Reference | XXX | XX% |
| ❌ Implementation Detail | XXX | XX% |
| ❌ Undefined Behavior | XXX | XX% |
| ❌ Wrong Expectation | XXX | XX% |
| ❌ Outdated | XXX | XX% |
| **Total Tests** | **XXX** | **100%** |

**Overall Compliance Score: XX%**

## Critical Issues (Must Fix Before Release)

### 1. Wrong Expectation: test_division_negative
**File:** `tests/arithmetic_tests.rs:145`
**Current:** `assert_eq!(eval("-7 / 2"), Value::I32(-4))`
**Spec Says:** Division truncates toward zero
**Should Be:** `assert_eq!(eval("-7 / 2"), Value::I32(-3))`

### 2. Undefined Behavior: test_overflow_behavior
**File:** `tests/arithmetic_tests.rs:203`
**Issue:** Spec section 3.2.1 does not define overflow behavior
**Options:**
  - Delete test
  - Mark as `#[cfg(feature = "test_impl_details")]`
  - Propose spec amendment

## Warnings (Should Fix)

### Tests Missing Spec References

| Test | File | Likely Spec Section |
|------|------|---------------------|
| test_addition | arithmetic.rs:10 | 3.2.1 |
| test_if_else | control.rs:25 | 5.1.1 |
| test_function_call | functions.rs:50 | 6.2.1 |

### Implementation Detail Tests

| Test | File | Issue |
|------|------|-------|
| test_ast_structure | parser.rs:100 | Tests internal AST |
| test_symbol_table | semantic.rs:50 | Tests internal structure |

## Recommendations

### Immediate Actions
1. Fix 2 wrong expectations (critical)
2. Remove/mark 5 undefined behavior tests
3. Add spec references to 45 tests

### Short-term
1. Delete 10 implementation detail tests
2. Update 3 outdated tests
3. Create missing edge case tests

### Long-term
1. Establish spec reference requirement for new tests
2. Add pre-commit hook to check references
3. Request spec clarification on 5 ambiguous items

## Files Requiring Attention

| File | Compliant | Issues | Priority |
|------|-----------|--------|----------|
| arithmetic_tests.rs | 15/25 | 10 | HIGH |
| parser_tests.rs | 5/30 | 25 | HIGH |
| type_tests.rs | 28/30 | 2 | LOW |
| function_tests.rs | 20/22 | 2 | LOW |

## Spec Clarifications Needed

1. **Integer overflow behavior** - Spec silent
2. **Maximum recursion depth** - Not specified
3. **String length limit** - Not specified
4. **Import cycle handling** - Ambiguous

## Appendix: Full Test Inventory

<details>
<summary>Click to expand full test list</summary>

| Test | File | Status | Spec Ref | Notes |
|------|------|--------|----------|-------|
| test_add_i32 | arith.rs:10 | ✅ | 3.2.1 | |
| test_sub_i32 | arith.rs:20 | ⚠️ | - | Missing ref |
| ... | ... | ... | ... | ... |

</details>
```

## Remediation Examples

### Adding Spec Reference

```rust
// BEFORE
#[test]
fn test_addition() {
    assert_eq!(eval("2 + 2"), Value::I32(4));
}

// AFTER
/// Spec Section: 3.2.1 - Arithmetic Operators
/// Requirement: The + operator performs addition on integer operands
/// Expected: i32 + i32 produces i32 result
#[test]
fn spec_3_2_1_addition_i32_basic() {
    assert_eq!(eval("2 + 2"), Value::I32(4),
        "Per spec 3.2.1: 2 + 2 must equal 4");
}
```

### Converting Implementation Test to Behavior Test

```rust
// BEFORE (tests internal structure)
#[test]
fn parser_creates_binary_expr() {
    let ast = parse("a + b");
    assert!(matches!(ast, Ast::BinaryExpr { op: Op::Add, .. }));
}

// AFTER (tests observable behavior)
/// Spec Section: 3.1 - Expressions  
/// Requirement: Binary expressions evaluate correctly
#[test]
fn spec_3_1_binary_addition_evaluates() {
    let source = "fn test() -> i32 { let a = 2; let b = 3; return a + b; }";
    let result = compile_and_run(source, "test", &[]);
    assert_eq!(result, Value::I32(5));
}
```

### Handling Undefined Behavior

```rust
// BEFORE
#[test]
fn overflow_wraps() {
    assert_eq!(eval("2147483647 + 1"), Value::I32(-2147483648));
}

// OPTION A: Delete if truly undefined and implementation may change

// OPTION B: Mark as implementation-specific
/// Implementation Note: Overflow behavior is not defined by spec.
/// This test documents current behavior for awareness only.
#[test]
#[cfg(feature = "test_impl_details")]
#[ignore = "Tests undefined behavior - may change"]
fn impl_note_overflow_currently_wraps() {
    assert_eq!(eval("2147483647 + 1"), Value::I32(-2147483648));
}

// OPTION C: Create spec amendment request
// File issue: "RFC: Define integer overflow behavior"
```

## Commands

```bash
# Generate audit report
# (Agent analyzes and produces AUDIT_REPORT.md)

# Find tests without spec references
grep -rL "Spec Section" tests/ --include="*.rs" | grep -v mod.rs

# Count compliant vs non-compliant
grep -r "Spec Section" tests/ --include="*.rs" | wc -l

# Find potential implementation detail tests
grep -r "assert.*matches.*Ast\|assert.*matches.*Token" tests/
```

## Success Criteria

- [ ] All test files audited
- [ ] AUDIT_REPORT.md generated
- [ ] Critical issues documented
- [ ] Each issue has remediation recommendation
- [ ] Compliance percentage calculated
- [ ] Priority action list created

## Execution Mode

```yaml
autonomous: true
max_duration: 4 hours
stop_on_failure: false
commit_changes: false  # Report only, human reviews
outputs:
  - tests/AUDIT_REPORT.md
```
```

---

## NEW WORKFLOW: Specification Compliance

**File:** `.claude/workflows/spec_compliance.md`

```markdown
# Specification Compliance Workflow

## Purpose

Ensure the Clean Language compiler correctly implements the language 
specification and that all tests verify spec-defined behavior.

## Trigger

- Weekly scheduled run
- After specification document changes
- Before releases  
- After major feature additions
- Manual trigger

## Workflow Steps

### Step 1: Specification Sync
```bash
# Ensure we have latest spec
git pull origin main

# Check spec version
head -20 docs/spec/VERSION.md || echo "No version file"
```

### Step 2: Run Specification Coverage Agent
- Analyze current specification
- Update feature catalog
- Identify new features needing tests
- Update COVERAGE_MATRIX.md

### Step 3: Create Missing Spec Tests
- For each uncovered feature:
  - Create positive test(s)
  - Create negative test(s)
  - Create edge case tests
- All tests reference spec sections

### Step 4: Run Test Compliance Auditor
- Audit all existing tests
- Identify non-compliant tests
- Generate AUDIT_REPORT.md

### Step 5: Remediate Issues
For critical issues only (automated):
- Add missing spec references (obvious cases)
- Flag tests needing human review

For warnings (report only):
- Document in AUDIT_REPORT.md
- Create issues for tracking

### Step 6: Verification
```bash
# Run all spec compliance tests
cargo test spec_compliance

# Run full test suite
cargo test

# Generate final metrics
./scripts/spec_compliance_metrics.sh
```

### Step 7: Generate Report

```markdown
# Specification Compliance Report - [DATE]

## Specification Coverage
- Features in spec: XXX
- Features tested: XXX  
- Coverage: XX%

## Test Compliance
- Total tests: XXX
- Compliant tests: XXX
- Compliance: XX%

## Changes This Run
- Tests created: XX
- Tests fixed: XX
- Issues found: XX

## Outstanding Items
- Features untested: XX
- Tests needing review: XX
- Spec clarifications needed: XX

## Trend
| Date | Spec Coverage | Test Compliance |
|------|---------------|-----------------|
| Last | XX% | XX% |
| Current | XX% | XX% |
| Delta | +X% | +X% |
```

## Integration with Other Agents

### Triggers Bug Fixer When:
- Tests fail due to spec violations
- Compiler behavior doesn't match spec

### Triggers Verification Agent:
- After all compliance work complete
- Before generating final report

### Updates Regression Guard:
- New spec tests become regression tests
- Fixed compliance issues need regression coverage

## Success Criteria

- Specification coverage ≥ 95%
- Test compliance ≥ 98%  
- Zero tests for undefined behavior
- All tests have spec references
- No critical issues remaining

## Schedule

```yaml
weekly:
  day: Sunday
  time: "02:00 UTC"
  
pre_release:
  trigger: manual
  required: true
  
post_spec_change:
  trigger: on docs/spec/** change
```
```

---

## DIRECTORY STRUCTURE ADDITIONS

Add these to your existing structure:

```
.claude/
├── agents/
│   ├── layer_tester.md          # Existing
│   ├── integration_tester.md    # Existing
│   ├── fuzzer.md                # Existing
│   ├── regression_guard.md      # Existing
│   ├── bug_fixer.md             # Existing
│   ├── verifier.md              # Existing
│   ├── spec_coverage.md         # NEW
│   └── test_auditor.md          # NEW
│
└── workflows/
    ├── nightly.md               # Existing
    ├── pre_commit.md            # Existing
    ├── bugfix_session.md        # Existing
    ├── release.md               # Existing
    └── spec_compliance.md       # NEW

tests/
├── unit/                        # Existing
├── integration/                 # Existing
├── regression/                  # Existing
├── e2e/                         # Existing
│
├── spec_compliance/             # NEW - All spec-driven tests
│   ├── mod.rs
│   ├── COVERAGE_MATRIX.md
│   ├── section_01_lexical/
│   ├── section_02_types/
│   ├── section_03_expressions/
│   ├── section_04_statements/
│   ├── section_05_control_flow/
│   ├── section_06_functions/
│   ├── section_07_modules/
│   └── section_08_wasm/
│
└── AUDIT_REPORT.md              # NEW - Compliance audit results
```

---

## UPDATE TO EXISTING AGENTS

### Add to `.claude/agents/verifier.md`:

```markdown
## Specification Compliance Checks (ADD TO CHECKLIST)

### 10. Specification Coverage
```bash
# Check coverage matrix
COVERAGE=$(grep "TOTAL" tests/spec_compliance/COVERAGE_MATRIX.md | awk '{print $NF}')
if [[ "${COVERAGE%\%}" -lt 95 ]]; then
    echo "FAIL: Spec coverage ${COVERAGE} < 95%"
    exit 1
fi
echo "PASS: Spec coverage ${COVERAGE}"
```

### 11. Test Compliance
```bash
# Check audit report
if grep -q "❌" tests/AUDIT_REPORT.md; then
    CRITICAL=$(grep -c "❌" tests/AUDIT_REPORT.md)
    if [ "$CRITICAL" -gt 0 ]; then
        echo "FAIL: $CRITICAL critical compliance issues"
        exit 1
    fi
fi
echo "PASS: No critical compliance issues"
```

## Updated Sign-off Criteria

- [ ] Specification coverage ≥ 95%
- [ ] Test compliance ≥ 98%
- [ ] Zero critical compliance issues
- [ ] All tests have spec references
- [ ] COVERAGE_MATRIX.md up to date
- [ ] AUDIT_REPORT.md reviewed
```

### Add to `.claude/agents/bug_fixer.md`:

```markdown
## Specification Compliance Fixes (ADD TO RESPONSIBILITIES)

When fixing bugs, ALWAYS:

1. **Check if bug is spec violation**
   - Read relevant spec section
   - Compare current behavior to spec
   - If behavior differs from spec: fix to match spec
   - If spec is silent: document as implementation detail

2. **Create spec-compliant tests for fixes**
   - All new tests MUST reference spec section
   - Format: `/// Spec Section: X.Y.Z - Description`
   - Tests verify spec-defined behavior only

3. **Update tracking documents**
   - Update COVERAGE_MATRIX.md if new feature tested
   - Update AUDIT_REPORT.md if compliance issue fixed

4. **Flag spec ambiguities**
   - If spec is unclear, note in diagnosis
   - Create issue for spec clarification
   - Document current behavior as implementation choice
```

---

## QUICK START COMMANDS

After adding these agents:

```bash
# Run spec coverage agent (create tests from spec)
# In Claude Code: "Execute spec coverage agent"

# Run test auditor (audit existing tests)  
# In Claude Code: "Execute test auditor agent"

# Run spec compliance workflow
# In Claude Code: "Execute spec compliance workflow"

# Run just spec compliance tests
cargo test spec_compliance

# Check coverage matrix
cat tests/spec_compliance/COVERAGE_MATRIX.md

# Check audit report
cat tests/AUDIT_REPORT.md
```

---

## SUCCESS METRICS

| Metric | Current | Target | Measurement |
|--------|---------|--------|-------------|
| Spec Feature Coverage | ? | ≥95% | COVERAGE_MATRIX.md |
| Test Spec Compliance | ? | ≥98% | AUDIT_REPORT.md |
| Tests with References | ? | 100% | grep count |
| Undefined Behavior Tests | ? | 0 | Audit count |
| Spec Ambiguities Documented | ? | All | Issue tracker |
