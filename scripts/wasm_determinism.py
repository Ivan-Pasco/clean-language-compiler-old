#!/usr/bin/env python3
"""WASM determinism check.

Compile a curated corpus of `.cln` files TWICE with the same compiler binary,
optimization level, and target, then diff the SHA-256 of each output. Any
difference is a reproducibility bug — a byte-for-byte identical compiler
run must produce a byte-for-byte identical WASM.

Usage
-----
    scripts/wasm_determinism.py                  # default corpus, opt-level 2
    scripts/wasm_determinism.py --opt-level 3
    scripts/wasm_determinism.py --cln target/release/cln
    scripts/wasm_determinism.py --corpus custom.txt

The default corpus lives at [scripts/wasm_determinism_corpus.txt] and lists
one relative `.cln` path per line (empty lines and `# comments` ignored).

Exit codes
----------
    0    All corpus files produce identical output on both compiles.
    1    At least one file has non-deterministic output.
    2    Environment or usage error.

Why this exists
---------------
    During the stage-snapshot rollout (Steps 1-3 of the testing strategy) we
    discovered the compiler emits WASM `export` entries in HashMap iteration
    order — a genuine non-determinism today. The snapshot harness normalizes
    around it, but the underlying issue should be tracked. This check makes
    the drift visible: run it in nightly CI, and any regression to
    determinism (or the introduction of new sources of noise) breaks the
    build.

    When the compiler is fixed, this check becomes a permanent guard.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = Path(__file__).resolve().parent / "wasm_determinism_corpus.txt"
DEFAULT_ALLOWLIST = Path(__file__).resolve().parent / "wasm_determinism_allowlist.txt"


def load_corpus(path: Path) -> list[Path]:
    entries: list[Path] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        entries.append(REPO_ROOT / line)
    return entries


def load_allowlist(path: Path) -> set[str]:
    """Load an allowlist of known-drifting corpus entries. Each line is one
    relative path (matching the corpus format). Files listed here still get
    checked, but their drift is downgraded from a hard failure to a warning."""
    if not path.is_file():
        return set()
    entries: set[str] = set()
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            entries.add(line)
    return entries


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def compile_one(cln: Path, source: Path, out: Path, opt: int) -> tuple[bool, str]:
    result = subprocess.run(
        [str(cln), "compile", str(source), "--output", str(out),
         "--opt-level", str(opt), "--quiet"],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip()
    return True, ""


def check_file(cln: Path, source: Path, opt: int) -> tuple[str, str, str | None]:
    """Return (status, detail, diff_hint). status ∈ {'ok','diff','skip','fail'}."""
    with tempfile.TemporaryDirectory() as td:
        td_path = Path(td)
        a = td_path / "a.wasm"
        b = td_path / "b.wasm"

        ok_a, err_a = compile_one(cln, source, a, opt)
        if not ok_a:
            return "skip", f"first compile failed: {err_a[:120]}", None

        ok_b, err_b = compile_one(cln, source, b, opt)
        if not ok_b:
            return "fail", f"second compile failed after first succeeded: {err_b[:120]}", None

        sha_a = sha256(a)
        sha_b = sha256(b)
        if sha_a == sha_b:
            return "ok", sha_a[:12], None
        return "diff", f"a={sha_a[:12]} b={sha_b[:12]}", (
            f"     A: {a}\n     B: {b}"
        )


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="wasm_determinism.py")
    p.add_argument(
        "--cln",
        default="target/release/cln",
        help="Path to the cln binary (default: target/release/cln)",
    )
    p.add_argument(
        "--corpus",
        type=Path,
        default=DEFAULT_CORPUS,
        help=f"Corpus file, one relative .cln path per line (default: {DEFAULT_CORPUS.relative_to(REPO_ROOT)})",
    )
    p.add_argument(
        "--opt-level",
        type=int,
        default=2,
        help="Optimization level to test at (default: 2)",
    )
    p.add_argument(
        "--all-opt-levels",
        action="store_true",
        help="Test each corpus file at opt levels 0, 1, 2, 3",
    )
    p.add_argument(
        "--allowlist",
        type=Path,
        default=DEFAULT_ALLOWLIST,
        help=f"Files with known drift, downgraded to warning "
             f"(default: {DEFAULT_ALLOWLIST.relative_to(REPO_ROOT)})",
    )
    p.add_argument(
        "--strict",
        action="store_true",
        help="Ignore the allowlist and treat all drift as hard failure",
    )
    args = p.parse_args(argv)

    cln = Path(args.cln)
    if not cln.is_absolute():
        cln = REPO_ROOT / cln
    if not cln.is_file():
        print(f"wasm_determinism: cln binary not found at {cln}", file=sys.stderr)
        print("hint: cargo build --release --bin cln", file=sys.stderr)
        return 2

    if not args.corpus.is_file():
        print(f"wasm_determinism: corpus file not found: {args.corpus}", file=sys.stderr)
        return 2

    corpus = load_corpus(args.corpus)
    if not corpus:
        print(f"wasm_determinism: corpus {args.corpus} is empty", file=sys.stderr)
        return 2

    opt_levels = [0, 1, 2, 3] if args.all_opt_levels else [args.opt_level]

    allowlist = set() if args.strict else load_allowlist(args.allowlist)

    total = 0
    ok = 0
    diffs: list[tuple[Path, int, str, str]] = []
    warned: list[tuple[Path, int, str]] = []
    skips: list[tuple[Path, int, str]] = []
    fails: list[tuple[Path, int, str]] = []

    for opt in opt_levels:
        for source in corpus:
            total += 1
            rel = source.relative_to(REPO_ROOT) if source.is_relative_to(REPO_ROOT) else source
            if not source.is_file():
                skips.append((source, opt, "source not found"))
                print(f"  SKIP  opt={opt}  {rel}  (not found)")
                continue
            status, detail, hint = check_file(cln, source, opt)
            if status == "ok":
                ok += 1
                print(f"  ok    opt={opt}  {rel}  sha={detail}")
            elif status == "diff":
                rel_str = str(rel)
                if rel_str in allowlist:
                    warned.append((source, opt, detail))
                    print(f"  WARN  opt={opt}  {rel}  {detail}  (allowlisted)")
                else:
                    diffs.append((source, opt, detail, hint or ""))
                    print(f"  DIFF  opt={opt}  {rel}  {detail}")
                    if hint:
                        print(hint)
            elif status == "skip":
                skips.append((source, opt, detail))
                print(f"  SKIP  opt={opt}  {rel}  {detail}")
            else:
                fails.append((source, opt, detail))
                print(f"  FAIL  opt={opt}  {rel}  {detail}")

    print()
    print(f"total={total}  ok={ok}  diff={len(diffs)}  warn={len(warned)}  "
          f"skip={len(skips)}  fail={len(fails)}")

    if warned:
        print()
        print(f"Allowlisted drift (still tracked, does not fail the build):")
        for src, opt, detail in warned:
            rel = src.relative_to(REPO_ROOT) if src.is_relative_to(REPO_ROOT) else src
            print(f"  - {rel} @ opt={opt}: {detail}")
        print(f"  → Remove entries from {args.allowlist.relative_to(REPO_ROOT)} as they get fixed.")

    if diffs or fails:
        print()
        if diffs:
            print("Non-deterministic outputs (compiler produced different WASM on identical input):")
            for src, opt, detail, _ in diffs:
                rel = src.relative_to(REPO_ROOT) if src.is_relative_to(REPO_ROOT) else src
                print(f"  - {rel} @ opt={opt}: {detail}")
            print()
            print(f"  → If drift here is genuinely known and being worked on, add it")
            print(f"    to {args.allowlist.relative_to(REPO_ROOT)}. Otherwise, fix the")
            print(f"    non-determinism at its source.")
        if fails:
            print("Second-compile failures (transient? or genuine flaky compile?):")
            for src, opt, detail in fails:
                rel = src.relative_to(REPO_ROOT) if src.is_relative_to(REPO_ROOT) else src
                print(f"  - {rel} @ opt={opt}: {detail}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
