# PROJECT RULES - TESTING ORGANIZATION

## MANDATORY TESTING REQUIREMENTS

### Rule 1: Test File Location
- **ALL test files (.cln) MUST be located within `tests/cln/` directory**
- **NO .cln files are permitted in project root or any other location**
- **NO exceptions to this rule - all testing MUST follow this structure**

### Rule 2: Test Directory Structure
Tests MUST be organized in the following hierarchy:
```
tests/
├── cln/                    # All Clean Language test files
│   ├── core/              # Core language features
│   │   ├── basics/        # Basic syntax and hello world
│   │   ├── variables/     # Variable declarations and types
│   │   ├── operators/     # Arithmetic, logical, comparison
│   │   └── control-flow/  # If/else, loops, conditionals
│   ├── functions/         # Function-related tests
│   │   ├── declarations/  # Function definitions
│   │   └── calls/        # Function and method calls
│   ├── oop/              # Object-oriented programming
│   │   ├── classes/      # Class definitions
│   │   └── inheritance/  # Inheritance and polymorphism
│   ├── data-structures/  # Complex data types
│   │   ├── arrays/       # List operations
│   │   └── matrices/     # Matrix operations
│   └── integration/      # Integration tests
│       └── comprehensive/ # Full feature combinations
├── qa/                    # Quality assurance and analysis
│   ├── scripts/          # QA-specific scripts
│   ├── reports/          # QA analysis reports
│   └── results/          # QA test results
└── output/               # Compiled test outputs (.wasm files)
```

### Rule 3: Test File Naming Convention
- **Format**: `NN_descriptive_name.cln`
- **NN**: Two-digit number for ordering (01-99)
- **descriptive_name**: Clear description using underscores
- **Examples**: `01_hello_world.cln`, `05_arithmetic_operations.cln`

### Rule 4: Test File Header Requirements
Every test file MUST include this header:
```clean
// Test Description: Brief description of what is tested
// Category: category name (core, functions, oop, etc.)
// Dependencies: list of dependencies or "none"
// Expected: PASS or FAIL
```

### Rule 5: Test Duplication Prevention
- **Before creating new tests**: Compare against ALL existing tests
- **Check for similar functionality**: Ensure no duplication occurs
- **Extend existing tests**: When possible, add to existing rather than create new
- **Document test coverage**: Maintain awareness of what's already tested

### Rule 6: New Test Creation Process
1. **Analyze existing tests** in the target category
2. **Identify gaps** in test coverage
3. **Choose appropriate category** and numbering
4. **Follow naming convention** and header requirements
5. **Place in correct directory** within tests/cln structure
6. **Verify no duplication** with existing functionality

### Rule 7: Test Categories Defined
- **core/basics**: Basic syntax, hello world, simple statements
- **core/variables**: Variable declarations, types, assignments
- **core/operators**: Arithmetic, logical, comparison operations
- **core/control-flow**: If/else, while, iterate loops
- **functions/declarations**: Function definitions and syntax
- **functions/calls**: Function calls, method calls, chaining
- **oop/classes**: Class definitions, constructors, methods
- **oop/inheritance**: Inheritance, polymorphism, base calls
- **data-structures/arrays**: List operations, array access
- **data-structures/matrices**: Matrix operations and syntax
- **integration/comprehensive**: Multi-feature integration tests

### Rule 8: Enforcement
- **Code reviews MUST verify** test placement compliance
- **CI/CD checks MUST validate** no .cln files outside tests/cln
- **Any violations MUST be corrected** before merge approval
- **Project maintainers MUST enforce** these rules consistently

### Rule 9: Test Maintenance
- **Regular cleanup** of outdated or redundant tests
- **Update tests** when language specification changes
- **Maintain test documentation** and coverage tracking
- **Remove temporary tests** after debugging sessions

### Rule 10: QA and Testing Infrastructure
- **QA folder MUST be located at `tests/qa/`** - no exceptions
- **All QA scripts, reports, and analysis** MUST be within tests/qa structure
- **Test compilation outputs** MUST go to `tests/output/` directory only
- **NO .wasm files** are permitted in project root or other locations

### Rule 11: Scripts Organization
- **ALL shell scripts (.sh) MUST be located in `scripts/` directory**
- **NO script files** are permitted in project root or other locations
- **QA-specific scripts** may be placed in `tests/qa/scripts/` for QA workflow
- **Project-wide scripts** MUST be in main `scripts/` directory

### Rule 12: Compilation Output Management
- **Test compilation outputs** MUST be saved to `tests/output/` only
- **NO .wasm files** in project root, tests/cln/, or any other location
- **Clean compilation outputs** regularly to prevent repository bloat
- **All temporary outputs** MUST be cleaned before commits

### Rule 13: Exception Handling
- **NO exceptions** to the tests/cln directory requirement
- **NO exceptions** to the scripts/ directory requirement
- **NO exceptions** to the tests/qa/ directory requirement
- **NO exceptions** to the tests/output/ compilation output requirement
- **Temporary debugging files** MUST be cleaned up immediately
- **Development tests** MUST follow the same organization rules
- **All test files** are subject to these mandatory requirements