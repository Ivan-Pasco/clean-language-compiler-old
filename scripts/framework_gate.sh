#!/usr/bin/env bash
# Framework gate: build real Frame code with the compiler under test.
#
# Plugins are Clean programs compiled by this compiler, so a code generation
# bug can break a plugin that built fine with the previous release (0.33.168
# shipped exactly that). Tests run against plugins built by an older compiler
# hide such regressions. This gate:
#
#   1. copies the framework checkout into a scratch folder (the checkout you
#      pass in is never modified);
#   2. rebuilds every plugin from source with the compiler under test
#      (scripts/build-all-plugins.sh);
#   3. installs them into an isolated ~/.cleen under a scratch HOME, next to
#      the compiler under test and a clean-server binary;
#   4. runs this repo's tests/cln/bugfixes pins again with the plugins
#      installed (pins that need a plugin are skipped where none exist), when
#      --runner is given;
#   5. runs the framework's unit/integration/e2e suites
#      (scripts/run-framework-tests.sh), the plugin helper tests
#      (scripts/test-plugins.sh --no-unit, the suites above already ran the
#      unit tests) and the example smoke tests (scripts/smoke-examples.sh);
#   6. builds the Clean Language website as a canary, when its checkout is
#      found (--website DIR; default: "Web Site Clean" next to this repo).
#      Example apps are small; a real site is not, and 0.33.17x shipped a
#      frame.ui that ran out of plugin memory only on a project that size.
#      The site is copied into the scratch folder, built with its own
#      scripts/build.sh against the toolchain installed above, started in
#      clean-server (no database) and its home page must answer 200.
#      Nothing from the site is stored in this repository.
#
# Usage:
#   scripts/framework_gate.sh --framework DIR --cln PATH --server PATH
#                             [--runner PATH] [--website DIR | --no-website]
#                             [--keep]
#
#   --framework  a cleen-framework-old checkout
#   --cln        compiler under test (e.g. target/release/cln)
#   --server     clean-server binary (old toolchain)
#   --runner     wasmtime_runner built from this repo (enables step 4)
#   --website    website checkout for step 6 (a missing default is skipped
#                with a note; a missing --website DIR is an error)
#   --no-website skip step 6
#   --keep       keep the scratch folder (printed at the end) for debugging
#
# Needs: bash, python3, curl, openssl, wasm-tools (plugin build.sh scripts
# strip unused imports with it). Exit status is non-zero when any step fails.
set -uo pipefail

FRAMEWORK=""
CLN=""
SERVER=""
RUNNER=""
KEEP=0
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WEBSITE="$REPO_ROOT/../Web Site Clean"
WEBSITE_GIVEN=0
while [ $# -gt 0 ]; do
    case "$1" in
        --framework) FRAMEWORK="$2"; shift 2 ;;
        --cln) CLN="$2"; shift 2 ;;
        --server) SERVER="$2"; shift 2 ;;
        --runner) RUNNER="$2"; shift 2 ;;
        --website) WEBSITE="$2"; WEBSITE_GIVEN=1; shift 2 ;;
        --no-website) WEBSITE=""; shift ;;
        --keep) KEEP=1; shift ;;
        -h|--help) sed -n '2,48p' "$0"; exit 0 ;;
        *) echo "framework_gate: unknown argument: $1" >&2; exit 2 ;;
    esac
done

for pair in "framework:$FRAMEWORK" "cln:$CLN" "server:$SERVER"; do
    if [ -z "${pair#*:}" ]; then
        echo "framework_gate: --${pair%%:*} is required" >&2
        exit 2
    fi
done
[ -d "$FRAMEWORK/plugins" ] || { echo "framework_gate: no plugins/ in $FRAMEWORK" >&2; exit 2; }
[ -x "$CLN" ] || { echo "framework_gate: compiler not executable: $CLN" >&2; exit 2; }
[ -x "$SERVER" ] || { echo "framework_gate: server not executable: $SERVER" >&2; exit 2; }
if [ -n "$RUNNER" ]; then
    [ -x "$RUNNER" ] || { echo "framework_gate: runner not executable: $RUNNER" >&2; exit 2; }
    RUNNER="$(cd "$(dirname "$RUNNER")" && pwd)/$(basename "$RUNNER")"
fi
if [ -n "$WEBSITE" ] && [ ! -f "$WEBSITE/scripts/build.sh" ]; then
    if [ "$WEBSITE_GIVEN" = 1 ]; then
        echo "framework_gate: no scripts/build.sh in website checkout: $WEBSITE" >&2
        exit 2
    fi
    echo "framework_gate: website checkout not found ($WEBSITE); skipping the website canary"
    WEBSITE=""
fi
command -v wasm-tools >/dev/null || { echo "framework_gate: wasm-tools is required" >&2; exit 2; }

WORK="$(mktemp -d -t cln-framework-gate.XXXXXX)"
cleanup() { [ "$KEEP" = 1 ] || rm -rf "$WORK"; }
trap cleanup EXIT

GATE_HOME="$WORK/home"
FW="$WORK/framework"
mkdir -p "$GATE_HOME/.cleen/bin" "$GATE_HOME/.cleen/plugins"
cp "$CLN" "$GATE_HOME/.cleen/bin/cln"
cp "$SERVER" "$GATE_HOME/.cleen/bin/clean-server"
# Copy the checkout without its git data and build outputs.
rsync -a --exclude .git --exclude node_modules --exclude 'tests/framework/output' "$FRAMEWORK/" "$FW/"

export HOME="$GATE_HOME"
export CLEEN_CLN="$GATE_HOME/.cleen/bin/cln"
export CLEEN_SERVER="$GATE_HOME/.cleen/bin/clean-server"
export CLEEN_PLUGINS_DIR="$GATE_HOME/.cleen/plugins"
# Errors from this run stay in the scratch dev queue; nothing is published.
export CLEEN_TELEMETRY_FORCE=local

echo "framework_gate: compiler $("$CLEEN_CLN" --version)"
echo "framework_gate: server   $("$CLEEN_SERVER" --version 2>/dev/null | head -1)"
echo "framework_gate: scratch  $WORK"

# Step 6: build the website copy with its own build script and boot it.
website_canary() {
    local site="$WORK/website" port log pid i code
    rsync -a --exclude .git --exclude _backup --exclude dist --exclude '*.wasm' \
        "$WEBSITE/" "$site/" || return 1
    (cd "$site" && CLN_COMPILER="$CLEEN_CLN" bash scripts/build.sh) || return 1
    port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
    log="$WORK/website-server.log"
    (cd "$site" && exec "$CLEEN_SERVER" dist/server.wasm --port "$port" --host 127.0.0.1) > "$log" 2>&1 &
    pid=$!
    for i in $(seq 1 100); do
        grep -qi 'listening' "$log" && break
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    code="$(curl -s -o /dev/null -m 10 -w '%{http_code}' "http://127.0.0.1:$port/")"
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    if [ "$code" != 200 ]; then
        echo "website: GET / -> $code (want 200); server log:"
        tail -20 "$log"
        return 1
    fi
    echo "website: built and served GET / -> 200"
}

FAILED=()
step() {
    local name="$1"; shift
    echo
    echo "== $name =="
    local start=$SECONDS
    if "$@"; then
        echo "-- $name: ok ($((SECONDS - start))s)"
    else
        echo "-- $name: FAILED ($((SECONDS - start))s)"
        FAILED+=("$name")
    fi
}

# Every later step depends on freshly built plugins; stop early when the
# build or install fails instead of testing whatever plugin.wasm is left.
step "rebuild plugins" bash "$FW/scripts/build-all-plugins.sh"
INSTALLED=0
if [ ${#FAILED[@]} -eq 0 ]; then
    step "install plugins" bash "$FW/scripts/install-plugins.sh"
    [ ${#FAILED[@]} -eq 0 ] && INSTALLED=1
fi
if [ ${#FAILED[@]} -eq 0 ] && [ -n "$RUNNER" ]; then
    step "regression pins (with plugins)" \
        env CLN="$CLEEN_CLN" WASMTIME_RUNNER="$RUNNER" python3 "$REPO_ROOT/scripts/check_regressions.py"
fi
if [ ${#FAILED[@]} -eq 0 ]; then
    cd "$FW"
    step "framework suites" bash scripts/run-framework-tests.sh
    step "plugin helper tests" bash scripts/test-plugins.sh --no-unit
    step "example smoke tests" bash scripts/smoke-examples.sh
fi
# The canary needs the installed plugins, not the earlier suites' results.
if [ "$INSTALLED" = 1 ] && [ -n "$WEBSITE" ]; then
    step "website canary" website_canary
fi

echo
if [ ${#FAILED[@]} -gt 0 ]; then
    echo "framework_gate: FAILED: ${FAILED[*]}"
    [ "$KEEP" = 1 ] && echo "framework_gate: scratch kept at $WORK"
    exit 1
fi
echo "framework_gate: all steps passed"
