# 🧪 Clean Language Compiler Test Suite
## Test Organization & Specification Coverage

**Counts as of:** compiler 0.33.189 (2026-11-26)
**`.cln` test files:** 1054 in `tests/cln/` (31 category folders, plus 19 loose `debug_*.cln` files at the top level)
**Rust test files:** 155 in `tests/*.rs`

Methodology and rules: [`UNIFIED_TESTING_STRATEGY.md`](./UNIFIED_TESTING_STRATEGY.md). Scripts: [`../scripts/README.md`](../scripts/README.md).

---

## 📋 Test Organization Structure

### `.cln` programs (`tests/cln/`)

| Folder | Files | What it holds |
|--------|------:|---------------|
| `bugfixes/` | 441 | One regression pin per fixed bug. Each carries the four required headers and an `// Expected output:` (or `// Expected error:`) block checked by `scripts/check_regressions.py`. |
| `spec_compliance/` | 174 | One folder per spec area (`apply_blocks`, `async`, `classes`, `control_flow`, `error_handling`, `expressions`, `functions`, `generics`, `imports`, `io`, `lexical`, `plugins`, `scope`, `statements`, `stdlib`, `testing`, `types`). See `COVERAGE_MATRIX.md` there. |
| `stdlib/` | 66 | Standard library: `console`, `http`, `io`, `json`, `list`, `math`, `memory`, `string`, plus general files |
| `language/` | 62 | `async`, `classes`, `control_flow`, `error_handling`, `expressions`, `functions`, `operators`, `state_management`, `strings`, `testing`, `validation` |
| `core/` | 61 | `basics`, `capabilities`, `collections`, `operators`, `types`, `variables` |
| `ci/` | 52 | Files compiled by the CI corpus job |
| `plugins/` | 27 | Programs that use plugins (skipped when the plugin is not installed) |
| `control/` | 20 | Control flow |
| `canaries/` | 20 | Cross-component canaries (`scripts/canary_*.py`) |
| `codegen/` | 11 | Code generation cases |
| `examples/` | 10 | Complete example programs |
| `testing/`, `imports/` | 9 each | `tests:` blocks; multi-file imports |
| `parser_compliance/`, `future/`, `contracts/` | 8, 7, 7 | Parser edge cases; unimplemented features (exempt from header rules); `require`/`ensure`/`always:` |
| `bridge/`, `advanced/` | 6 each | Bridge imports; advanced features |
| `validation/`, `regression/`, `loops/`, `debug/` | 5 each | |
| `functions/`, `ai/` | 4 each | |
| `integration/`, `strings/`, `external/` | 3, 2, 2 | |
| `fail/`, `error_statement/`, `dsl/`, `docs/` | 1 each | |

Regenerate the counts with:

```bash
for d in tests/cln/*/; do echo "$(find "$d" -name '*.cln' | wc -l) $d"; done | sort -rn
```

### Rust tests (`tests/*.rs`)

155 integration-test files, one per area or per bug (`test_<topic>.rs`, `specification_parser_tests.rs`, `specification_lexer_tests.rs`, ...). Each is its own Cargo test target: `cargo test --test <file-stem>`. Unit tests live next to the code in `src/` and run with `cargo test --lib`.

### Other folders

| Folder | Purpose |
|--------|---------|
| `tests/output/` | Compiled WASM from `tests/cln/` (never commit hand-made files here) |
| `tests/qa/` | QA helper scripts (see `tests/qa/README.md`) |
| `tests/docs-features/` | Fixtures for the doc-code synchronisation checks |
| `tests/snapshots/`, `tests/results/` | Snapshot data and historical result logs |

---

## 📊 Clean Language Specification Coverage

Coverage is measured, not listed here: `python3 scripts/spec_coverage.py` reports which grammar productions and semantic rules are cited by a test header, and `tests/cln/spec_compliance/COVERAGE_MATRIX.md` maps spec areas to files. Known gaps between the specification and the compiler are listed in `system-documents/doc-implementation-parity-2026-11-25.md`.

---

## 🎯 Quality Assurance Standards

### **Production Quality Requirements**
- **100% Compilation Success**: All organized tests must compile successfully
- **Zero Tolerance**: No partial implementations or placeholders allowed
- **Full Coverage**: Every language feature documented in specification must be tested
- **WebAssembly Compatibility**: All generated code must be valid WASM

### **Test Organization Benefits**
1. **Logical Structure**: Easy navigation by feature area
2. **Specification Alignment**: Direct mapping to Clean Language Specification sections
3. **Maintainability**: Clear separation of concerns for easy maintenance
4. **Development Support**: Organized debug files for compiler development

### **Usage Guidelines**
- **Core Tests**: Must always pass for production releases
- **Language Tests**: Required for feature validation
- **Advanced Tests**: For modern language feature validation
- **Stdlib Tests**: For standard library API validation
- **Debug Tests**: For compiler development and debugging only

---

## 🚀 Running Tests

### **Test File Organization Standards**
- **Source Files**: All test files MUST be located in `tests/cln/` organized by feature category
- **Output Files**: All compiled WASM files MUST be placed in `tests/output/` directory
- **No Root Files**: Test files should NEVER be created in the project root directory

### **Individual Test Compilation**
```bash
# Compile a specific test - ALWAYS use tests/output/ directory
cargo run --bin cln -- compile tests/cln/core/basics/00_minimal.cln -o tests/output/00_minimal.wasm

# Run it with the compiler's own host bridge
./target/release/wasmtime_runner tests/output/00_minimal.wasm

# Parse only (lexer + parser)
cargo run --bin cln -- parse tests/cln/language/classes/14_classes_basic.cln --ast-json

# Debug with AST display
cargo run --bin cln -- debug tests/cln/examples/28_complex_example.cln --show-ast

# Run a file's tests: block
cargo run --bin cln -- test tests/cln/testing/
```

### **Batch Testing**
```bash
# Every file under tests/cln/ by category, stops on the first failure
./scripts/comprehensive_test_runner.sh

# Compile and execute every file
./scripts/comprehensive_test.sh

# Pass/fail counts for everything except future/
./scripts/test_all.sh

# Regression pins (tests/cln/bugfixes/)
python3 scripts/check_regressions.py

# Rust tests
cargo test
```

### **🎯 Comprehensive QA Testing Methodology**

**Complete Test Directive**: For comprehensive quality assurance, ALL `.cln` files in the `tests/cln/` folder structure must be tested systematically:

#### **Testing Process**
1. **Iterate Through All Files**: Test every `.cln` file in `tests/cln/` directory structure
2. **Compile Each File**: Compile to `tests/output/` directory with proper naming
3. **Execute Each WASM**: Run the compiled WASM file with `wasmtime_runner` to verify functionality
4. **Fix on Error**: When encountering errors:
   - Stop and analyze the error thoroughly
   - Fix the underlying issue in the compiler
   - Resume testing from where the error occurred

#### **Problem Resolution Strategy**
- **Quick Issues**: Fix immediately and continue
- **Complex Problems**: Think hard about the solution, research documentation
- **Very Complex Problems**: Think harder, use Context7 MCP and internet search for solutions
- **Specification Issues**: Consult Clean Language Specification for compliance requirements

#### **Quality Standards**
- **Zero Tolerance**: Every test file must compile and execute successfully
- **Complete Coverage**: No skipping of files or partial implementations
- **Production Ready**: All fixes must be production-quality, no placeholders
- **Documentation**: Update specification when new language features are discovered

#### **Execution Command Pattern**
```bash
# Standard compilation pattern for QA
cargo run --bin cln -- compile tests/cln/[category]/[file].cln -o tests/output/[file].wasm

# Execution verification (provides the host bridge functions)
./target/release/wasmtime_runner tests/output/[file].wasm
```

### **CI/CD Integration**

GitHub workflows in `.github/workflows/`: `ci.yml` (build and tests), `architecture-guards.yml` (test-quality and boundary guards), `regressions.yml` (regression pins), `cln-corpus.yml` (`.cln` corpus), `coverage.yml`, `framework-gate.yml` (framework plugins build with this compiler), `json-property.yml`, `release.yml` (tag `v*`). `fuzz.yml` and `nightly.yml` are disabled. Git hooks in `.githooks/` run `check_test_quality.py`, `check_regressions.py` and `cargo test --lib` (install with `scripts/install-hooks.sh`).

---

## 🔄 Maintenance & Updates

### **When Adding New Tests**
1. **Identify Category**: Determine appropriate subdirectory based on feature
2. **Follow Naming**: Use descriptive names with number prefixes for ordering
3. **Update Documentation**: Add to this README when adding significant test coverage
4. **Verify Coverage**: Ensure new tests align with Clean Language Specification

### **When Removing Tests**
1. **Validate Redundancy**: Ensure removal doesn't create coverage gaps
2. **Preserve Examples**: Keep representative examples of each feature
3. **Update Counts**: Update file counts in this documentation

### **Continuous Integration**
- All organized tests should be part of CI/CD pipeline
- Debug tests can be excluded from production validation
- Specification coverage should be monitored and maintained
