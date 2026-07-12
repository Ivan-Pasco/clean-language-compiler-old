#!/usr/bin/env python3
"""Test-quality guard for the Clean Language compiler.

Scans test files (Rust unit/integration tests + .cln end-to-end tests) for
placeholder patterns and structural violations. Exits non-zero on any find.

Rules enforced:

RUST TESTS (tests/*.rs, src/**/*.rs with #[cfg(test)]):
  R1  No todo!(), unimplemented!(), panic!("not implemented"), panic!("TODO")
      inside a #[test] body.
  R2  No vacuous assertions: assert!(true), assert_eq!(1, 1), assert!(1==1),
      assert_eq!(true, true), assert!(false).
  R3  #[ignore] must be followed by a // reason: comment on the same line,
      the preceding line, or the next line.
  R4  Test bodies that are empty or only return Ok(()) with no other stmt.

.CLN TESTS (tests/cln/**/*.cln):
  C1  Files under tests/cln/bugfixes/ must have all five headers:
        // Test:
        // Grammar:  OR  // Semantic:
        // Fixed in:
        // Expected output:
      This makes them regression-suite compatible.
  C2  No file may consist solely of print("todo") or print("placeholder")
      or an empty start: block.
  C3  Files under tests/cln/future/ are allowed to break C1/C2 (they are
      explicitly incomplete features).

Usage:
    python3 scripts/check_test_quality.py [--staged] [--paths <file1> <file2>...]
                                          [--baseline] [--strict]

  --staged     Only check files staged for commit (git diff --cached).
  --paths      Only check the listed files (used by pre-push / CI).
  --baseline   Read tests/.test_quality_baseline.txt and IGNORE any file
               listed there. Fails only on violations in files NOT on the
               baseline (ratchet mode — legacy debt tolerated, new code
               gated). Default when running under CI.
  --strict     Ignore the baseline; fail on any violation. Use to
               progressively drain the baseline: fix files, remove them
               from tests/.test_quality_baseline.txt, and re-run in
               strict mode locally to verify.
  (no args)    Check the full repo, no baseline.

Exit codes:
    0   Clean (baseline-adjusted if applicable).
    1   One or more violations found in non-baselined files.
    2   Script or environment error.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BASELINE_FILE = REPO_ROOT / "tests" / ".test_quality_baseline.txt"

# ---------- Rule patterns ----------

RUST_PLACEHOLDER_PATTERNS = [
    (re.compile(r"\btodo!\s*\("), "todo!() in test body"),
    (re.compile(r"\bunimplemented!\s*\("), "unimplemented!() in test body"),
    (re.compile(r'\bpanic!\s*\(\s*"(?:not implemented|TODO|todo|placeholder|stub|FIXME)"', re.IGNORECASE),
     "panic!(\"not implemented\") / \"TODO\" / \"stub\" / \"FIXME\" in test body"),
]

RUST_VACUOUS_PATTERNS = [
    (re.compile(r"\bassert!\s*\(\s*true\s*[,)]"), "assert!(true) — vacuous"),
    (re.compile(r"\bassert!\s*\(\s*false\s*[,)]"), "assert!(false) — use panic! with a reason instead"),
    (re.compile(r"\bassert_eq!\s*\(\s*true\s*,\s*true\s*[,)]"), "assert_eq!(true, true) — vacuous"),
    (re.compile(r"\bassert_eq!\s*\(\s*false\s*,\s*false\s*[,)]"), "assert_eq!(false, false) — vacuous"),
    (re.compile(r"\bassert_eq!\s*\(\s*(\d+)\s*,\s*\1\s*[,)]"), "assert_eq!(N, N) — vacuous"),
    (re.compile(r"\bassert!\s*\(\s*1\s*==\s*1\s*[,)]"), "assert!(1==1) — vacuous"),
]

CLN_HEADER_TEST = re.compile(r"^//\s*Test:", re.MULTILINE)
CLN_HEADER_GRAMMAR_OR_SEMANTIC = re.compile(r"^//\s*(Grammar|Semantic):", re.MULTILINE)
CLN_HEADER_FIXED_IN = re.compile(r"^//\s*Fixed in:", re.MULTILINE)
CLN_HEADER_EXPECTED = re.compile(r"^//\s*Expected output:", re.MULTILINE)

CLN_EMPTY_PLACEHOLDER = re.compile(
    r"^\s*print\s*\(\s*[\"'](?:todo|placeholder|stub|fixme|tbd)[\"']\s*\)\s*$",
    re.IGNORECASE | re.MULTILINE,
)


# ---------- Data ----------

@dataclass
class Violation:
    path: Path
    line: int
    rule: str
    detail: str

    def format(self) -> str:
        return f"{self.path}:{self.line}: [{self.rule}] {self.detail}"


# ---------- Rust scan ----------

TEST_ATTR_RE = re.compile(r"#\[(tokio::test|test|test_case)")
# `#[ignore]` OR `#[ignore = "..."]`. The latter has an inline reason.
IGNORE_ATTR_BARE_RE = re.compile(r"#\[ignore\s*\]")
IGNORE_ATTR_WITH_REASON_RE = re.compile(r"#\[ignore\s*=\s*\"[^\"]{4,}\"\s*\]")
# A justification comment adjacent to the #[ignore]. Accept several
# common conventions: `// reason: ...`, `// OBSOLETE: ...`, `// TODO(...):
# waiting on X`, `// FIXME: ...`, `// see #NNNN` — anything that documents
# why the test was disabled.
REASON_COMMENT_RE = re.compile(
    r"//\s*("
    r"reason\s*:"
    r"|obsolete\b"
    r"|todo\s*[:\(]"
    r"|fixme\s*[:\(]"
    r"|see\s+#\S+"
    r"|blocked\s+(?:on|by)\b"
    r"|waiting\s+(?:on|for)\b"
    r"|tracking\s+bug\b"
    r"|skip\b"
    r")",
    re.IGNORECASE,
)


def find_fn_block(lines: list[str], start_idx: int) -> tuple[int, int] | None:
    """Given the line index of an fn line, return (open_brace_line, close_brace_line)
    zero-based indices. Returns None if the block can't be located."""
    for i in range(start_idx, min(start_idx + 20, len(lines))):
        if "{" in lines[i]:
            depth = 0
            for j in range(i, len(lines)):
                depth += lines[j].count("{") - lines[j].count("}")
                if depth == 0:
                    return i, j
            return None
    return None


def scan_rust_file(path: Path) -> list[Violation]:
    """Scan one Rust file for R1..R4 violations."""
    violations: list[Violation] = []
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        violations.append(Violation(path, 0, "IO", f"cannot read: {exc}"))
        return violations

    lines = text.splitlines()

    # R3 walks the whole file (not just fn bodies).
    for idx, line in enumerate(lines):
        # `#[ignore = "..."]` with a non-empty reason is self-documenting.
        if IGNORE_ATTR_WITH_REASON_RE.search(line):
            continue
        if IGNORE_ATTR_BARE_RE.search(line):
            has_reason = REASON_COMMENT_RE.search(line) is not None
            # Check up to 2 lines above (skip #[test] attributes).
            for lookback in (idx - 1, idx - 2):
                if has_reason or lookback < 0:
                    break
                has_reason = REASON_COMMENT_RE.search(lines[lookback]) is not None
            # Check up to 2 lines below (accommodate the fn signature).
            for lookahead in (idx + 1, idx + 2):
                if has_reason or lookahead >= len(lines):
                    break
                has_reason = REASON_COMMENT_RE.search(lines[lookahead]) is not None
            if not has_reason:
                violations.append(Violation(
                    path, idx + 1, "R3",
                    "#[ignore] without a justification — either use "
                    "`#[ignore = \"reason\"]` or a nearby "
                    "`// reason: / // obsolete: / // TODO: / etc.` comment",
                ))

    i = 0
    while i < len(lines):
        line = lines[i]

        # For R1/R2/R4 we need to be inside a #[test] fn body.
        if TEST_ATTR_RE.search(line):
            # Walk forward to the fn signature
            fn_line = None
            for k in range(i, min(i + 15, len(lines))):
                if re.match(r"\s*(?:async\s+|pub\s+|pub\(\S+\)\s+|const\s+)*fn\s+\w+", lines[k]):
                    fn_line = k
                    break
            if fn_line is None:
                i += 1
                continue

            block = find_fn_block(lines, fn_line)
            if block is None:
                i += 1
                continue

            open_i, close_i = block
            body = "\n".join(lines[open_i:close_i + 1])

            # R1: placeholders
            for pat, msg in RUST_PLACEHOLDER_PATTERNS:
                for m in pat.finditer(body):
                    line_offset = body[:m.start()].count("\n")
                    violations.append(Violation(
                        path, open_i + 1 + line_offset, "R1", msg,
                    ))
            # R2: vacuous asserts
            for pat, msg in RUST_VACUOUS_PATTERNS:
                for m in pat.finditer(body):
                    line_offset = body[:m.start()].count("\n")
                    violations.append(Violation(
                        path, open_i + 1 + line_offset, "R2", msg,
                    ))
            # R4: empty body or only Ok(())
            # Extract the text strictly BETWEEN the first `{` and matching `}`
            # so we handle one-line fn bodies too.
            open_brace_pos = body.find("{")
            close_brace_pos = body.rfind("}")
            if open_brace_pos >= 0 and close_brace_pos > open_brace_pos:
                inner_text = body[open_brace_pos + 1: close_brace_pos]
            else:
                inner_text = ""
            # Strip comments (block + line) then non-code whitespace
            inner_stripped = re.sub(r"//[^\n]*", "", inner_text)
            inner_stripped = re.sub(r"/\*.*?\*/", "", inner_stripped, flags=re.DOTALL)
            content = inner_stripped.strip()
            if not content:
                violations.append(Violation(
                    path, open_i + 1, "R4",
                    "test body is empty",
                ))
            elif content in ("Ok(())", "Ok(());"):
                violations.append(Violation(
                    path, open_i + 1, "R4",
                    "test body only returns Ok(()) with no assertions",
                ))

            i = close_i + 1
            continue

        i += 1

    return violations


# ---------- .cln scan ----------

def scan_cln_file(path: Path) -> list[Violation]:
    """Scan one .cln file for C1..C3 violations."""
    violations: list[Violation] = []
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        violations.append(Violation(path, 0, "IO", f"cannot read: {exc}"))
        return violations

    # C3: files under tests/cln/future/ are exempt from C1/C2.
    rel = path.relative_to(REPO_ROOT) if path.is_absolute() else path
    if "tests/cln/future/" in rel.as_posix():
        return violations

    # C1: bugfix tests need the five headers.
    if "tests/cln/bugfixes/" in rel.as_posix():
        missing = []
        if not CLN_HEADER_TEST.search(text):
            missing.append("// Test:")
        if not CLN_HEADER_GRAMMAR_OR_SEMANTIC.search(text):
            missing.append("// Grammar: OR // Semantic:")
        if not CLN_HEADER_FIXED_IN.search(text):
            missing.append("// Fixed in:")
        if not CLN_HEADER_EXPECTED.search(text):
            missing.append("// Expected output:")
        if missing:
            violations.append(Violation(
                path, 1, "C1",
                f"bugfix test missing header(s): {', '.join(missing)}",
            ))

    # C2: file body must not be a bare "print('todo')" placeholder.
    for m in CLN_EMPTY_PLACEHOLDER.finditer(text):
        line = text[:m.start()].count("\n") + 1
        violations.append(Violation(
            path, line, "C2",
            "placeholder-only test body",
        ))

    return violations


# ---------- Collection ----------

def all_rust_test_files() -> list[Path]:
    """Every .rs file under tests/ + every src/**/*.rs with #[cfg(test)] or #[test]."""
    paths: list[Path] = []
    tests_dir = REPO_ROOT / "tests"
    if tests_dir.exists():
        paths.extend(p for p in tests_dir.rglob("*.rs") if p.is_file())
    src_dir = REPO_ROOT / "src"
    if src_dir.exists():
        for p in src_dir.rglob("*.rs"):
            if not p.is_file():
                continue
            try:
                head = p.read_text(encoding="utf-8", errors="replace")[:8000]
            except OSError:
                continue
            if "#[cfg(test)]" in head or "#[test]" in head or "#[tokio::test]" in head:
                paths.append(p)
    return paths


def all_cln_test_files() -> list[Path]:
    d = REPO_ROOT / "tests" / "cln"
    if not d.exists():
        return []
    return [p for p in d.rglob("*.cln") if p.is_file()]


def staged_files() -> list[Path]:
    try:
        out = subprocess.run(
            ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"],
            cwd=REPO_ROOT, capture_output=True, text=True, check=True,
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        return []
    return [REPO_ROOT / line.strip() for line in out.splitlines() if line.strip()]


# ---------- Main ----------

def load_baseline() -> set[str]:
    """Load baseline file (paths relative to REPO_ROOT) to exclude from strict checks."""
    if not BASELINE_FILE.exists():
        return set()
    out = set()
    for line in BASELINE_FILE.read_text(encoding="utf-8", errors="replace").splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        out.add(s)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Test-quality guard.")
    parser.add_argument("--staged", action="store_true",
                        help="only scan files staged for commit")
    parser.add_argument("--paths", nargs="*", default=None,
                        help="only scan these explicit paths")
    parser.add_argument("--baseline", action="store_true",
                        help=f"ignore files listed in {BASELINE_FILE.relative_to(REPO_ROOT)}")
    parser.add_argument("--strict", action="store_true",
                        help="report all violations even if baselined")
    args = parser.parse_args()

    baseline: set[str] = set()
    if args.baseline and not args.strict:
        baseline = load_baseline()

    if args.paths is not None:
        candidates = [Path(p) for p in args.paths]
        # Resolve to absolute
        candidates = [p if p.is_absolute() else REPO_ROOT / p for p in candidates]
    elif args.staged:
        candidates = staged_files()
    else:
        candidates = all_rust_test_files() + all_cln_test_files()

    # Filter to files that actually exist and are testable
    rust_paths: list[Path] = []
    cln_paths: list[Path] = []
    for p in candidates:
        if not p.exists() or not p.is_file():
            continue
        try:
            rel = p.relative_to(REPO_ROOT) if p.is_absolute() else p
            rel_str = rel.as_posix()
            in_repo = True
        except ValueError:
            rel_str = ""
            in_repo = False
        if p.suffix == ".rs":
            if (in_repo and rel_str.startswith("tests/")) or "cfg(test)" in _peek(p):
                rust_paths.append(p)
        elif p.suffix == ".cln":
            if in_repo and rel_str.startswith("tests/cln/"):
                cln_paths.append(p)

    all_violations: list[Violation] = []
    for p in rust_paths:
        all_violations.extend(scan_rust_file(p))
    for p in cln_paths:
        all_violations.extend(scan_cln_file(p))

    # Split into baselined and new violations
    def rel(p: Path) -> str:
        try:
            return p.relative_to(REPO_ROOT).as_posix()
        except ValueError:
            return p.as_posix()

    new_violations = [v for v in all_violations if rel(v.path) not in baseline]
    ignored_count = len(all_violations) - len(new_violations)

    if new_violations:
        print(f"Found {len(new_violations)} test-quality violation(s)"
              f"{f' (plus {ignored_count} baselined, ignored)' if ignored_count else ''}:")
        for v in new_violations:
            print(f"  {v.format()}")
        print()
        print("Rules:")
        print("  R1  no todo!/unimplemented!/panic!(\"not implemented\") in test bodies")
        print("  R2  no vacuous asserts (assert!(true), assert_eq!(N,N), etc.)")
        print("  R3  #[ignore] must have a `// reason:` comment")
        print("  R4  test bodies may not be empty or only Ok(())")
        print("  C1  bugfix .cln tests need: Test:, Grammar:/Semantic:, Fixed in:, Expected output:")
        print("  C2  no placeholder-only .cln bodies (print('todo') etc.)")
        if baseline:
            print()
            print(f"  Baseline: tests/.test_quality_baseline.txt "
                  f"({len(baseline)} legacy path(s) ignored). Remove a file from the")
            print(f"  baseline after fixing it — CI will keep it clean going forward.")
        return 1

    scanned = len(rust_paths) + len(cln_paths)
    msg = f"[check_test_quality] scanned {scanned} file(s), no new violations"
    if ignored_count:
        msg += f" ({ignored_count} baselined)"
    print(msg)
    return 0


def _peek(p: Path, n: int = 8000) -> str:
    try:
        return p.read_text(encoding="utf-8", errors="replace")[:n]
    except OSError:
        return ""


if __name__ == "__main__":
    sys.exit(main())
