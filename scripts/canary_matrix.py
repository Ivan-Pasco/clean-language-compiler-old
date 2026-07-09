#!/usr/bin/env python3
"""Render the cross-component canary matrix into a markdown summary.

Called by the ``cross-component-canaries`` job in ``.github/workflows/
nightly.yml`` after each host repo's canary workflow has completed (or
timed out). Reads:

    * ``applicability.json``    — namespace → applicable hosts, produced by
                                  ``canary_applicability.py``.
    * ``<host>-report.json``    — one file per host, downloaded from that
                                  host's ``canary-report`` artifact. Each
                                  reports pass/fail per canary the host ran.
    * ``<host>-run-url.txt``    — the GitHub Actions run URL for the host's
                                  dispatched workflow, so red cells can link
                                  directly to the responsible job log.
    * ``<host>-status.txt``     — one of ``success``, ``failure``, ``timeout``,
                                  ``dispatch_error``. If not ``success`` or
                                  ``failure``, every cell for that host is
                                  rendered as the status word rather than
                                  ✓/✗.

Renders a matrix table to stdout (or ``--out``) with:

    rows = canary namespace
    cols = server, node-server, browser
    cells = ✓ (pass), ✗ (fail), skip (not applicable), timeout, dispatch_error

Exit code is always 0 — the matrix is a report, not a gate. Per the umbrella
prompt "Not a real-time gate. This is nightly. If it catches drift Tuesday
morning, the fix is a normal PR, not a rollback."
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HOSTS: tuple[str, ...] = ("server", "node-server", "browser")

# Symbols used in the matrix. Kept short so a wide matrix fits in a
# GitHub Step Summary column without wrapping.
CELL_PASS = "✓"
CELL_FAIL = "✗"
CELL_SKIP = "skip"


def load_json(path: Path) -> dict:
    """Load a JSON file, returning ``{}`` on missing/invalid input.

    Missing per-host reports happen legitimately (host workflow timed out
    or errored before it could upload). Treat as "no data" rather than
    crashing the matrix render — a downstream host failure should not
    hide the rest of the results.
    """
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def load_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def canary_cell(
    host: str,
    canary: str,
    applicable_hosts: list[str],
    report: dict,
    host_status: str,
    run_url: str,
) -> str:
    """Return the markdown cell for one (canary, host) intersection."""
    if host not in applicable_hosts:
        return CELL_SKIP

    if host_status in {"timeout", "dispatch_error"}:
        return host_status

    results = report.get("results", {})
    entry = results.get(canary)
    if entry is None:
        # Host was supposed to run this canary but the report doesn't
        # mention it. Treat as fail — either the host runner crashed
        # before this canary, or the applicability manifest is out of
        # sync with what the runner actually attempted. Both are red
        # cells in the matrix.
        if run_url:
            return f"[{CELL_FAIL}]({run_url})"
        return CELL_FAIL

    passed = bool(entry.get("passed", False))
    if passed:
        return CELL_PASS

    # Fail — link to the responsible host's run log if we have one, so
    # a red cell in the nightly summary jumps straight to the job that
    # produced it (umbrella DoD).
    if run_url:
        return f"[{CELL_FAIL}]({run_url})"
    return CELL_FAIL


def render(inputs_dir: Path) -> str:
    """Render the full matrix as a markdown string.

    Layout mirrors the umbrella prompt's success criterion:
    rows = canary, cols = host, cell = ✓ / ✗ / skip. A silent addition of
    ``_i18n_*`` to clean-server without matching updates elsewhere produces
    a red cell in the server column of every affected canary.
    """
    applicability_path = inputs_dir / "applicability.json"
    applicability = load_json(applicability_path).get("canaries", {})

    per_host_reports = {
        host: load_json(inputs_dir / f"{host}-report.json") for host in HOSTS
    }
    per_host_status = {
        host: load_text(inputs_dir / f"{host}-status.txt") or "success"
        for host in HOSTS
    }
    per_host_run_url = {
        host: load_text(inputs_dir / f"{host}-run-url.txt") for host in HOSTS
    }

    if not applicability:
        return (
            "## Cross-component canary matrix\n\n"
            "_No applicability manifest found — nothing to render._\n"
        )

    lines = ["## Cross-component canary matrix", ""]

    # Per-host status banner: quick-read summary of whether each host's
    # workflow completed cleanly. `success` means the host workflow ran to
    # completion — its individual canaries may still be red inside the
    # matrix; that's a different signal.
    lines.append(
        "| Host | Workflow status | Run |"
    )
    lines.append("|------|-----------------|-----|")
    for host in HOSTS:
        status = per_host_status[host]
        url = per_host_run_url[host]
        run_cell = f"[log]({url})" if url else "—"
        lines.append(f"| `{host}` | {status} | {run_cell} |")
    lines.append("")

    # Main matrix. One row per canary sorted alphabetically (stable output
    # across runs so diffs surface only real changes).
    header = "| Canary | " + " | ".join(f"`{h}`" for h in HOSTS) + " |"
    sep = "|--------|" + "|".join(["----"] * len(HOSTS)) + "|"
    lines.append(header)
    lines.append(sep)

    for canary in sorted(applicability.keys()):
        applicable = applicability[canary].get("hosts", [])
        row = [f"`{canary}`"]
        for host in HOSTS:
            row.append(
                canary_cell(
                    host,
                    canary,
                    applicable,
                    per_host_reports[host],
                    per_host_status[host],
                    per_host_run_url[host],
                )
            )
        lines.append("| " + " | ".join(row) + " |")

    # Legend so a first-time reader knows what each symbol means. Keep it
    # short — the nightly summary is scanned for red cells, not read
    # top-to-bottom.
    lines.extend(
        [
            "",
            "**Legend:** "
            "✓ pass, ✗ fail (linked to host job log), "
            "`skip` not applicable to this host, "
            "`timeout` host workflow did not complete in the wait window, "
            "`dispatch_error` host workflow could not be started.",
        ]
    )

    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--inputs-dir",
        type=Path,
        default=Path("matrix-inputs"),
        help=(
            "Directory containing applicability.json, <host>-report.json, "
            "<host>-status.txt, and <host>-run-url.txt files."
        ),
    )
    parser.add_argument(
        "--out",
        type=Path,
        help="Write the matrix to this file (default: stdout).",
    )
    args = parser.parse_args()

    matrix = render(args.inputs_dir)

    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(matrix)
    else:
        sys.stdout.write(matrix)

    return 0


if __name__ == "__main__":
    sys.exit(main())
