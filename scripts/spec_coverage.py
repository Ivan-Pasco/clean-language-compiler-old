#!/usr/bin/env python3
"""Spec-rule coverage report for the Clean Language compiler.

Reads the numbered rule codes out of `foundation/spec/semantic-rules.md`,
scans every `.cln` file under `tests/cln/` for citations, and reports the
gap. Produces both a human-readable summary and a machine-readable JSON
artifact for CI.

Usage
-----
    scripts/spec_coverage.py                       # print human report
    scripts/spec_coverage.py --json out.json       # write machine artifact
    scripts/spec_coverage.py --min-per-rule N      # fail if any rule < N cites
    scripts/spec_coverage.py --strict-structured   # only count // Semantic: cites

Exit codes
----------
    0    All rules meet the coverage floor.
    1    One or more rules are below the floor (uncovered by default).
    2    Usage or environment error (spec file missing, etc.).

What counts as a citation
-------------------------
    Default (lenient): the rule code appears anywhere in the `.cln` file.
    This catches both the structured header (`// Semantic: SEM007`) and any
    ad-hoc mention in a comment. Rationale: the goal is to know a rule has
    at least one test *anchored* to it, not to police the header format.

    Strict (`--strict-structured`): only `// Semantic: <CODE>` or
    `// Grammar: <production>` header lines count. Use this to measure how
    close we are to the UNIFIED_TESTING_STRATEGY.md ideal.

Why this exists
---------------
    Line coverage (grcov) does not tell us whether a specific numbered
    semantic rule is exercised by a test. A rule can be "well covered" by
    lines while having zero end-to-end tests that specifically anchor to it.
    Spec-rule coverage is the second axis called out in the testing
    strategy plan; this script measures it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# The spec lives one level up in foundation/. Fall back to a search if the
# expected layout ever changes.
_DEFAULT_SPEC = REPO_ROOT.parent / "foundation" / "spec" / "semantic-rules.md"

_TESTS_DIR = REPO_ROOT / "tests" / "cln"

# Section headings in the spec look like `### SEM001 — …` (h3), sometimes `##`.
_RULE_HEADING = re.compile(r"^#{2,4}\s+([A-Z]+[0-9]{3,4})\b")

# Any occurrence of a rule code in a .cln file.
_ANY_CITE = re.compile(r"\b([A-Z]+[0-9]{3,4})\b")

# Structured citation: `// Semantic: SEM001` or `// Semantic: SEM001, IDX002`
_STRUCTURED_CITE = re.compile(
    r"^\s*//\s*(?:Semantic|Grammar):\s*(.*)$",
    re.IGNORECASE,
)


def load_rule_codes(spec_path: Path) -> list[str]:
    codes: list[str] = []
    seen = set()
    try:
        for line in spec_path.read_text(encoding="utf-8").splitlines():
            m = _RULE_HEADING.match(line)
            if m:
                code = m.group(1)
                if code not in seen:
                    seen.add(code)
                    codes.append(code)
    except OSError as e:
        print(f"spec_coverage: cannot read {spec_path}: {e}", file=sys.stderr)
        raise
    return codes


def scan_tests(strict_structured: bool) -> dict[str, list[Path]]:
    """Return {rule_code: [paths that cite it]}."""
    hits: dict[str, list[Path]] = defaultdict(list)
    if not _TESTS_DIR.is_dir():
        return hits

    for cln in sorted(_TESTS_DIR.rglob("*.cln")):
        # Skip files under future/ — those are intentionally uncovered.
        if "future" in cln.parts:
            continue
        try:
            text = cln.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue

        cites: set[str] = set()
        if strict_structured:
            for line in text.splitlines():
                m = _STRUCTURED_CITE.match(line)
                if not m:
                    continue
                # Payload after `// Semantic:` — extract any rule codes.
                for code_m in _ANY_CITE.finditer(m.group(1)):
                    cites.add(code_m.group(1))
        else:
            for code_m in _ANY_CITE.finditer(text):
                cites.add(code_m.group(1))

        for code in cites:
            hits[code].append(cln)

    return hits


def build_report(
    rule_codes: list[str],
    hits: dict[str, list[Path]],
) -> dict:
    """Assemble a JSON-friendly report."""
    entries = []
    covered = 0
    for code in rule_codes:
        paths = hits.get(code, [])
        if paths:
            covered += 1
        entries.append(
            {
                "code": code,
                "citations": len(paths),
                "sample_files": [
                    str(p.relative_to(REPO_ROOT)) for p in paths[:3]
                ],
            }
        )
    total = len(rule_codes)
    return {
        "total_rules": total,
        "covered_rules": covered,
        "uncovered_rules": total - covered,
        "coverage_pct": round(100.0 * covered / total, 2) if total else 0.0,
        "rules": entries,
    }


def print_human(report: dict, min_per_rule: int) -> None:
    print(
        f"Spec-rule coverage: {report['covered_rules']}/{report['total_rules']}"
        f" rules cited ({report['coverage_pct']}%)"
    )
    print()
    print(f"{'CODE':<12} {'CITES':>6}  status")
    print("-" * 50)
    for rule in report["rules"]:
        cites = rule["citations"]
        if cites == 0:
            status = "UNCOVERED"
        elif cites < min_per_rule:
            status = f"below min ({min_per_rule})"
        else:
            status = "ok"
        print(f"{rule['code']:<12} {cites:>6}  {status}")
    print()
    uncovered = [r["code"] for r in report["rules"] if r["citations"] == 0]
    if uncovered:
        print(f"Uncovered ({len(uncovered)}): {', '.join(uncovered)}")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="spec_coverage.py")
    p.add_argument(
        "--spec",
        type=Path,
        default=_DEFAULT_SPEC,
        help=f"Path to semantic-rules.md (default: {_DEFAULT_SPEC})",
    )
    p.add_argument(
        "--json",
        type=Path,
        default=None,
        help="Write machine-readable JSON report to this path",
    )
    p.add_argument(
        "--min-per-rule",
        type=int,
        default=1,
        help="Fail if any rule has fewer than N citing tests (default: 1)",
    )
    p.add_argument(
        "--strict-structured",
        action="store_true",
        help="Only count `// Semantic:` / `// Grammar:` header citations",
    )
    p.add_argument(
        "--allow-uncovered",
        type=str,
        default="",
        help="Comma-separated rule codes exempt from the min-per-rule check "
             "(use sparingly, for rules that are intentionally unimplemented)",
    )
    args = p.parse_args(argv)

    if not args.spec.is_file():
        print(f"spec_coverage: spec file not found: {args.spec}", file=sys.stderr)
        return 2

    rule_codes = load_rule_codes(args.spec)
    if not rule_codes:
        print(f"spec_coverage: no rule codes extracted from {args.spec}", file=sys.stderr)
        return 2

    hits = scan_tests(strict_structured=args.strict_structured)
    report = build_report(rule_codes, hits)

    if args.json:
        args.json.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"wrote {args.json}", file=sys.stderr)

    print_human(report, args.min_per_rule)

    exempt = {c.strip() for c in args.allow_uncovered.split(",") if c.strip()}
    below = [
        r for r in report["rules"]
        if r["citations"] < args.min_per_rule and r["code"] not in exempt
    ]
    if below:
        print(
            f"\nFAIL: {len(below)} rule(s) below the coverage floor "
            f"(--min-per-rule={args.min_per_rule}).",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
