#!/usr/bin/env python3
"""Require a regression pin under tests/cln/bugfixes/ before closing a bug.

Usage
-----
    scripts/require_pin.py <ERROR_CODE_OR_FINGERPRINT> [--version <version>]

Exit codes
----------
    0    A matching pin was found.
    1    No matching pin found (fails the CI/skill step; user must add a pin).
    2    Usage error.

Called by
---------
    * The /resolve-fix skill BEFORE its Step 5 (server resolve-batch call). If
      this script exits non-zero, /resolve-fix must not post `resolved` to the
      server — the fix is incomplete without a durable regression pin.
    * A developer, manually, to check whether a bug already has a pin.

Matching rules
--------------
    A file under tests/cln/bugfixes/ (searched recursively) matches when its
    first ~40 lines (the header block) contain ANY of:

        // Regression: <token>
        // Fingerprint: <token>
        // Tracking: <token>
        // Test: bugfixes/<token>

    where <token> case-insensitively contains the argument. This is permissive
    by design — the goal is to catch the many header conventions used across
    the existing bugfix pins (`// Regression: CMP-XYZ`, dashboard fingerprints
    like `1a20405b`, tracking IDs like `SEM-COMPARE-01`, etc.).

    If --version is given, at least one matching file must also carry a
    `// Fixed in: compiler <version>` line whose version prefix matches. This
    catches the case where a pin exists for an older reoccurrence but the
    current fix has not been paired with an updated pin.

Design notes
------------
    This script is a linter for a policy, not a test runner. It does not
    compile or execute anything — that is the job of scripts/check_regressions.py.
    Keeping the two concerns separate lets /resolve-fix call this cheaply.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BUGFIXES_DIR = REPO_ROOT / "tests" / "cln" / "bugfixes"
HEADER_LINE_BUDGET = 40  # scan only the top of each file

_HEADER_KEYS = (
    "regression",
    "fingerprint",
    "tracking",
    "test",  # matches `// Test: bugfixes/<name>`
)

_FIXED_IN_RE = re.compile(
    r"^//\s*Fixed in:\s*compiler\s+([0-9]+\.[0-9]+\.[0-9]+)",
    re.IGNORECASE,
)


def _extract_header(path: Path) -> list[str]:
    try:
        with path.open("r", encoding="utf-8", errors="replace") as f:
            return [next(f, "") for _ in range(HEADER_LINE_BUDGET)]
    except OSError:
        return []


def _line_mentions_key_and_token(line: str, token_lower: str) -> bool:
    stripped = line.strip()
    if not stripped.startswith("//"):
        return False
    body = stripped[2:].lstrip()
    lower = body.lower()
    if not any(lower.startswith(k) for k in _HEADER_KEYS):
        return False
    return token_lower in lower


def find_pins(token: str) -> list[tuple[Path, list[str]]]:
    """Return every pin whose header mentions `token`."""
    token_lower = token.lower()
    hits: list[tuple[Path, list[str]]] = []
    if not BUGFIXES_DIR.is_dir():
        return hits
    for cln in sorted(BUGFIXES_DIR.rglob("*.cln")):
        header = _extract_header(cln)
        for line in header:
            if _line_mentions_key_and_token(line, token_lower):
                hits.append((cln, header))
                break
    return hits


def _pin_matches_version(header: list[str], version: str) -> bool:
    for line in header:
        m = _FIXED_IN_RE.match(line.strip())
        if m and m.group(1).startswith(version):
            return True
    return False


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="require_pin.py",
        description="Verify a regression pin exists in tests/cln/bugfixes/",
    )
    p.add_argument(
        "token",
        help="Error code, fingerprint, or tracking ID to look up",
    )
    p.add_argument(
        "--version",
        help="If set, at least one pin must also carry `// Fixed in: compiler <version>`",
        default=None,
    )
    p.add_argument(
        "--quiet",
        action="store_true",
        help="Print only the pin paths (or exit code)",
    )
    args = p.parse_args(argv)

    hits = find_pins(args.token)

    if not hits:
        if not args.quiet:
            print(
                f"require_pin: no pin found for '{args.token}' under {BUGFIXES_DIR.relative_to(REPO_ROOT)}",
                file=sys.stderr,
            )
            print(
                "\nAdd a `.cln` file under tests/cln/bugfixes/ whose header",
                "includes one of:\n"
                f"    // Regression: {args.token}\n"
                f"    // Fingerprint: {args.token}\n"
                f"    // Tracking: {args.token}\n"
                "and a `// Fixed in: compiler <version>` line.\n"
                "The file must produce exact `// Expected output:` when run.",
                file=sys.stderr,
            )
        return 1

    if args.version:
        version_hits = [(f, h) for f, h in hits if _pin_matches_version(h, args.version)]
        if not version_hits:
            if not args.quiet:
                print(
                    f"require_pin: found {len(hits)} pin(s) mentioning '{args.token}',",
                    f"but none carry `// Fixed in: compiler {args.version}`:",
                    file=sys.stderr,
                )
                for f, _ in hits:
                    print(f"    {f.relative_to(REPO_ROOT)}", file=sys.stderr)
                print(
                    f"\nUpdate one of these pins with `// Fixed in: compiler {args.version}`",
                    "so /resolve-fix can prove the regression is guarded in this release.",
                    file=sys.stderr,
                )
            return 1
        hits = version_hits

    for path, _ in hits:
        print(path.relative_to(REPO_ROOT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
