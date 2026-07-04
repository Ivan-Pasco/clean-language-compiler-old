#!/usr/bin/env python3
"""Regression enforcement for tests/cln/bugfixes/.

For every *.cln file under tests/cln/bugfixes/ that carries an `// Expected output:`
header block, this script:
  1. Compiles the file with cln (release build).
  2. Runs the WASM through wasmtime_runner.
  3. Extracts stdout between the runner's `--- Output ---` markers.
  4. Diffs against the expected block from the header.

Exits non-zero on any mismatch. Intended to be wired into CI to catch silent
regressions on bugs that have already been resolved on errors.cleanlanguage.dev.

Optional companion mode:
  --sync-resolved         pulls bugs marked resolved from the errors API and
                          creates skeleton test files for any that don't yet
                          exist locally (dev-only; requires ERROR_API_KEY).
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BUGFIXES_DIR = REPO_ROOT / "tests" / "cln" / "bugfixes"

# Prefer overrides from env, then fall back to the freshly-built release binaries.
# CI builds cln + wasmtime_runner into target/release before invoking this script.
# Locally, developers may point CLN / WASMTIME_RUNNER at ~/.cleen/bin/cln etc.
CLN_BIN = Path(os.environ.get("CLN") or REPO_ROOT / "target" / "release" / "cln")
RUNNER_BIN = Path(os.environ.get("WASMTIME_RUNNER") or REPO_ROOT / "target" / "release" / "wasmtime_runner")

HEADER_RE = re.compile(
    r"//\s*Expected output:\s*\n((?://[^\n]*\n)+)",
    re.MULTILINE,
)
OUTPUT_MARKER_START = "--- Output ---"
OUTPUT_MARKER_END = "--- End Output ---"


OPT_IN_MARKER = "// Fixed in: compiler"


def parse_expected(source: str) -> list[str] | None:
    # Opt-in: only files that explicitly declare a `// Fixed in: compiler <ver>`
    # line are checked. Existing bugfixes/ files without this marker are treated
    # as legacy fixtures (may need plugins, may not have deterministic output).
    if OPT_IN_MARKER not in source:
        return None
    m = HEADER_RE.search(source)
    if not m:
        return None
    raw_lines: list[str] = []
    for raw in m.group(1).splitlines():
        stripped = raw.lstrip()
        if not stripped.startswith("//"):
            break
        # Drop `//` and exactly one trailing space if present.
        content = stripped[2:]
        if content.startswith(" "):
            content = content[1:]
        # Stop when a new header field starts (e.g. `Fixed in:`, blank comment).
        if content == "" or re.match(r"^[A-Z][A-Za-z ]+:", content):
            break
        raw_lines.append(content.rstrip())
    # Compute common leading whitespace and strip it uniformly so header authors
    # can indent the expected block for readability without breaking the diff.
    non_empty = [l for l in raw_lines if l]
    if non_empty:
        indent = min(len(l) - len(l.lstrip(" ")) for l in non_empty)
        raw_lines = [l[indent:] if len(l) >= indent else l for l in raw_lines]
    while raw_lines and raw_lines[-1] == "":
        raw_lines.pop()
    return raw_lines


def extract_runner_stdout(raw: str) -> list[str]:
    lines = raw.splitlines()
    try:
        start = lines.index(OUTPUT_MARKER_START) + 1
        end = lines.index(OUTPUT_MARKER_END, start)
    except ValueError:
        return []
    return [l.rstrip() for l in lines[start:end]]


def run_one(cln_path: Path) -> tuple[bool, str]:
    source = cln_path.read_text()
    expected = parse_expected(source)
    if expected is None:
        return True, "SKIP (no Expected output header)"

    with tempfile.TemporaryDirectory() as tmp:
        wasm = Path(tmp) / (cln_path.stem + ".wasm")
        compile_proc = subprocess.run(
            [str(CLN_BIN), "compile", str(cln_path), "--output", str(wasm)],
            capture_output=True, text=True, timeout=20,
        )
        if compile_proc.returncode != 0 or not wasm.exists():
            return False, f"COMPILE FAIL:\n{(compile_proc.stderr or compile_proc.stdout)[-2000:]}"
        run_proc = subprocess.run(
            [str(RUNNER_BIN), str(wasm)],
            capture_output=True, text=True, timeout=15,
        )
        actual = extract_runner_stdout(run_proc.stdout)

    if actual == expected:
        return True, f"OK ({len(expected)} lines)"
    return False, (
        f"OUTPUT MISMATCH\n"
        f"  expected ({len(expected)}):\n    " + "\n    ".join(expected) + "\n"
        f"  actual   ({len(actual)}):\n    " + "\n    ".join(actual)
    )


def check() -> int:
    if not CLN_BIN.exists() or not RUNNER_BIN.exists():
        print(f"error: build cln + wasmtime_runner first (missing {CLN_BIN} or {RUNNER_BIN})", file=sys.stderr)
        return 2

    files = sorted(BUGFIXES_DIR.glob("*.cln"))
    if not files:
        print("no .cln files in tests/cln/bugfixes/", file=sys.stderr)
        return 2

    total = len(files)
    checked = 0
    failed: list[tuple[Path, str]] = []
    skipped = 0

    for f in files:
        ok, msg = run_one(f)
        if msg.startswith("SKIP"):
            skipped += 1
            continue
        checked += 1
        prefix = "PASS" if ok else "FAIL"
        print(f"[{prefix}] {f.name}: {msg}")
        if not ok:
            failed.append((f, msg))

    print()
    print(f"Regression summary: {total} total, {checked} checked, {skipped} skipped, {len(failed)} failed")
    return 0 if not failed else 1


def sync_resolved() -> int:
    api_key = os.environ.get("ERROR_API_KEY")
    if not api_key:
        keyfile = Path.home() / ".cleen" / "error-api-key"
        if keyfile.exists():
            api_key = keyfile.read_text().strip()
    if not api_key:
        print("error: ERROR_API_KEY not set and ~/.cleen/error-api-key missing", file=sys.stderr)
        return 2

    import urllib.request
    import json as jsonlib

    req = urllib.request.Request(
        "https://errors.cleanlanguage.dev/api/v1/bugs?component=compiler&status=resolved",
        headers={"X-API-Key": api_key},
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = jsonlib.loads(resp.read())

    existing = {p.name for p in BUGFIXES_DIR.glob("*.cln")}
    added = 0
    for bug in data.get("bugs", []):
        fp_short = bug["fingerprint"][:12]
        code = (bug.get("error_code") or "unknown").lower().replace("-", "_")
        repro = (bug.get("minimal_repro") or "").strip()
        if not repro:
            continue
        candidate = f"{code}_{fp_short}.cln"
        if any(candidate == e or fp_short in e for e in existing):
            continue
        target = BUGFIXES_DIR / candidate
        header = (
            f"// Test: bugfixes/{target.stem}\n"
            f"// Grammar: TODO — cite grammar.ebnf production\n"
            f"// Semantic: {bug.get('error_code','?')} (tracking: {fp_short})\n"
            f"// Fixed in: compiler {bug.get('fixed_in_version','?')}\n"
            "//\n"
            "// Expected output:\n"
            "//   TODO — fill in from bug's expected_behavior field\n"
            "\n"
        )
        target.write_text(header + repro + "\n")
        print(f"created placeholder {target.name} (fill in Expected output before enabling)")
        added += 1

    print(f"Sync summary: {added} new placeholder(s) created; review + fill headers before committing.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--sync-resolved", action="store_true",
                    help="pull resolved bugs from the errors API and create placeholder .cln files")
    args = ap.parse_args()
    if args.sync_resolved:
        return sync_resolved()
    return check()


if __name__ == "__main__":
    sys.exit(main())
