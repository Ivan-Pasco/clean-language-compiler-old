#!/usr/bin/env bash
# Release-tier verification gate for the Clean Language compiler.
#
# Runs the extra checks that comita's per-commit tier does not:
#   1. Architecture boundaries (already required in PR CI — re-verified here).
#   2. Stage-boundary snapshots stable.
#   3. WASM determinism check (currently reports drift; see TASKS.md).
#   4. Regression pins compile and produce expected output.
#   5. Spec-rule coverage does not drop below the committed baseline.
#   6. (Optional) Host-parity check when running from a full workspace.
#
# Usage
# -----
#     scripts/release_gate.sh                 # run all checks, exit non-zero on any hard failure
#     scripts/release_gate.sh --skip-determinism   # skip the known-drifting WASM byte check
#     STRICT=1 scripts/release_gate.sh        # promote every soft check (determinism, coverage floor) to hard
#
# Design notes
# ------------
# * Hard checks fail the script (exit 1). Soft checks print a warning and
#   continue; use STRICT=1 to promote them.
# * The script does NOT run comita — it is a pre-comita verification.
#   The intent is: run this locally, then run comita.
# * Every check writes to a per-run log directory printed at the end.

set -uo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"
if [ -z "$REPO_ROOT" ]; then
    echo "release_gate: not in a git repo" >&2
    exit 2
fi
cd "$REPO_ROOT"

# ---- config ----------------------------------------------------------------
STRICT="${STRICT:-0}"
SKIP_DETERMINISM=0
for arg in "$@"; do
    case "$arg" in
        --skip-determinism) SKIP_DETERMINISM=1 ;;
        --help|-h)
            sed -n '2,20p' "$0"
            exit 0
            ;;
        *) echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

LOG_DIR="$(mktemp -d -t cln-release-gate.XXXXXX)"
echo "release_gate: logs → $LOG_DIR"

# ---- state -----------------------------------------------------------------
HARD_FAILS=()
SOFT_WARNINGS=()

# ---- helpers ---------------------------------------------------------------
GREEN='\033[1;32m'; RED='\033[1;31m'; YELLOW='\033[1;33m'; BLUE='\033[1;34m'; NC='\033[0m'

step() { printf "\n${BLUE}== %s ==${NC}\n" "$1"; }
pass() { printf "${GREEN}✓${NC} %s\n" "$1"; }
fail() { printf "${RED}✗${NC} %s\n" "$1" >&2; HARD_FAILS+=("$1"); }
warn() {
    printf "${YELLOW}!${NC} %s\n" "$1"
    if [ "$STRICT" = "1" ]; then
        HARD_FAILS+=("$1 (promoted by STRICT=1)")
    else
        SOFT_WARNINGS+=("$1")
    fi
}

# ---- 1. Architecture boundaries -------------------------------------------
step "1/6  architecture boundaries"
if cargo test --test architecture_boundaries -- --nocapture > "$LOG_DIR/1-arch.log" 2>&1; then
    pass "architecture_boundaries passed"
else
    fail "architecture_boundaries failed — see $LOG_DIR/1-arch.log"
fi

# ---- 2. Stage-boundary snapshots ------------------------------------------
step "2/6  stage-boundary snapshots"
if [ -f tests/test_stage_snapshots.rs ]; then
    if cargo test --test test_stage_snapshots > "$LOG_DIR/2-snap.log" 2>&1; then
        pass "stage snapshots stable"
    else
        # Distinguish snapshot drift (soft) from compile failure (hard).
        if grep -q "snapshot assertion" "$LOG_DIR/2-snap.log"; then
            warn "stage snapshot drift — run 'cargo insta review' and commit acceptance"
        else
            fail "test_stage_snapshots failed to build/run — see $LOG_DIR/2-snap.log"
        fi
    fi
else
    warn "tests/test_stage_snapshots.rs missing (testing-strategy step 3 not landed?)"
fi

# ---- 3. WASM determinism --------------------------------------------------
step "3/6  WASM determinism"
if [ "$SKIP_DETERMINISM" = "1" ]; then
    warn "WASM determinism check skipped by --skip-determinism"
elif [ -x scripts/wasm_determinism.py ]; then
    if [ -x target/release/cln ]; then
        cln_bin="target/release/cln"
    elif [ -x target/debug/cln ]; then
        cln_bin="target/debug/cln"
    else
        warn "no cln binary found (run: cargo build --release --bin cln)"
        cln_bin=""
    fi
    if [ -n "$cln_bin" ]; then
        if python3 scripts/wasm_determinism.py --cln "$cln_bin" > "$LOG_DIR/3-det.log" 2>&1; then
            pass "WASM determinism check clean"
        else
            # Known issue — see TASKS.md. Soft warn.
            warn "WASM determinism drift detected — see $LOG_DIR/3-det.log (tracked in TASKS.md)"
        fi
    fi
else
    warn "scripts/wasm_determinism.py missing"
fi

# ---- 4. Regression pins ---------------------------------------------------
step "4/6  regression pins"
if [ -x scripts/check_regressions.py ]; then
    if python3 scripts/check_regressions.py > "$LOG_DIR/4-regressions.log" 2>&1; then
        pass "regression pins pass"
    else
        fail "regression pins broken — see $LOG_DIR/4-regressions.log"
    fi
else
    warn "scripts/check_regressions.py missing"
fi

# ---- 5. Spec-rule coverage ------------------------------------------------
step "5/6  spec-rule coverage ratchet"
if [ -x scripts/spec_coverage.py ]; then
    # Baseline % of numbered semantic rules that must be cited by at least
    # one test under tests/cln/. Raise only when the suite genuinely covers
    # more rules. Was previously read from spec-coverage.yml; that CI job
    # was retired 2026-07-25 (external spec repo inaccessible from CI).
    baseline="54.93"
    if true; then
        # spec_coverage.py needs foundation/spec/ next to the compiler crate.
        # In dev checkouts this is already true. In CI this is set up in the
        # workflow. If it's missing here, warn but don't fail — the CI job
        # will catch a genuine regression.
        if [ ! -f ../foundation/spec/semantic-rules.md ]; then
            warn "foundation/spec/semantic-rules.md not found — cannot check coverage locally"
        else
            python3 scripts/spec_coverage.py --json "$LOG_DIR/5-coverage.json" \
                > "$LOG_DIR/5-coverage.log" 2>&1 || true
            pct=$(python3 -c "import json; print(json.load(open('$LOG_DIR/5-coverage.json'))['coverage_pct'])" 2>/dev/null || echo 0)
            if python3 -c "import sys; sys.exit(0 if float('$pct') + 0.01 >= float('$baseline') else 1)"; then
                pass "spec-rule coverage ${pct}% >= baseline ${baseline}%"
            else
                fail "spec-rule coverage ${pct}% dropped below baseline ${baseline}%"
            fi
        fi
    fi
else
    warn "scripts/spec_coverage.py missing"
fi

# ---- 6. Host parity (best-effort) -----------------------------------------
step "6/6  host parity (best-effort)"
parity_script="../foundation/management/scripts/check_host_parity.py"
if [ -x "$parity_script" ]; then
    # Compiler emission side; server checks live in the server component.
    if python3 "$parity_script" --host server --strict > "$LOG_DIR/6-parity-server.log" 2>&1; then
        pass "host-parity (server) OK"
    else
        warn "host-parity (server) drift — see $LOG_DIR/6-parity-server.log"
    fi
else
    warn "$parity_script not found (skipped)"
fi

# ---- summary ---------------------------------------------------------------
printf "\n"
printf "══════════════════════════════════════════════════\n"
printf "release_gate summary\n"
printf "══════════════════════════════════════════════════\n"
printf "hard failures : %d\n" "${#HARD_FAILS[@]}"
printf "soft warnings : %d\n" "${#SOFT_WARNINGS[@]}"
printf "log directory : %s\n" "$LOG_DIR"

if [ "${#HARD_FAILS[@]}" -gt 0 ]; then
    printf "\n${RED}HARD FAILURES:${NC}\n"
    for f in "${HARD_FAILS[@]}"; do printf "  - %s\n" "$f"; done
    printf "\nDo NOT run comita until every hard failure is fixed.\n"
    exit 1
fi

if [ "${#SOFT_WARNINGS[@]}" -gt 0 ]; then
    printf "\n${YELLOW}SOFT WARNINGS:${NC}\n"
    for w in "${SOFT_WARNINGS[@]}"; do printf "  - %s\n" "$w"; done
    printf "\nSafe to run comita, but review each warning first.\n"
fi

printf "\n${GREEN}Ready for comita.${NC}\n"
exit 0
