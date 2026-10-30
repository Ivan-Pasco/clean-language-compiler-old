# Clean Language Compiler Scripts

Utility scripts for testing, release gating and CI. Run them from the
compiler crate root (the scripts `cd` there themselves where needed).

## Test runners

- **test.sh**: Entry point for the "test" workflow; runs `comprehensive_test_runner.sh`.
- **comprehensive_test_runner.sh**: Compiles every file under `tests/cln/` by category with `cln compile`, writes WASM to `tests/output/`, and stops on the first failure with a detailed error log. See `tests/UNIFIED_TESTING_STRATEGY.md`.
- **comprehensive_test.sh**: Compiles and executes every `.cln` file with `wasmtime_runner`.
- **test_all.sh**: Compiles every `tests/cln/` file (except `future/`) and reports pass/fail counts.
- **test_smoke.sh**: Fast smoke run over a representative subset.

## Guards (used by git hooks and CI)

- **check_regressions.py**: Enforces that every `tests/cln/bugfixes/` pin still compiles and prints its expected output.
- **check_test_quality.py**: Rejects vacuous or placeholder tests (rules R1–R4, C1–C3).
- **require_pin.py**: Requires a regression pin under `tests/cln/bugfixes/` before a bug is closed.
- **spec_coverage.py**: Reports which spec rules have test coverage.
- **wasm_determinism.py**: Checks that compiling the corpus in `wasm_determinism_corpus.txt` twice yields identical WASM (`wasm_determinism_allowlist.txt` lists known exceptions).
- **release_gate.sh**: Release-tier verification gate.
- **canary_applicability.py**, **canary_matrix.py**: Build the cross-component canary manifest and summary.

## Setup

- **install-hooks.sh**: Points git at the repo-tracked hooks in `.githooks/`.

## Workflows

- **CI_CD_WORKFLOW.md**: The `comita` release workflow.
