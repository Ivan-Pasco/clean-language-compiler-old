# Clean Language Compiler - QA Infrastructure

This directory contains quality assurance automation and scripts for the Clean Language compiler.

## Directory Structure

```
tests/qa/
├── README.md           # This file
└── scripts/            # QA automation scripts
    ├── run_comprehensive_test.sh
    ├── validate_all_wasm.sh
    ├── generate_progress_report.sh
    └── categorize_errors.py
```

## Integration with Existing Infrastructure

This QA directory complements:
- `tests/cln/` - Test source files (179+ .cln files)
- `tests/output/` - Compiled WASM output
- `scripts/` - Main project scripts (comprehensive_test_runner.sh, etc.)
- `.claude/agents/` - Agent configurations for testing

## Usage

### Comprehensive Testing
```bash
# Use existing infrastructure
./scripts/comprehensive_test_runner.sh

# Or the CLI command
cargo run --bin cln comprehensive-test
```

### QA-Specific Validation
```bash
# Validate all WASM output
./tests/qa/scripts/validate_all_wasm.sh

# Generate progress report
./tests/qa/scripts/generate_progress_report.sh
```

## Agent Integration

The QA infrastructure works with these agents:
- **clean-language-qa-engineer**: Comprehensive quality assurance
- **regression-guard**: Regression prevention
- **verifier**: Final quality gate
- **compiler-debugger**: Systematic debugging
- **error-fixer**: Error resolution

## Quality Standards

Per project mandate (CLAUDE.md):
- 100% compilation rate required
- 100% execution rate required
- No placeholder implementations
- No todo!() macros
- Production-grade code only
