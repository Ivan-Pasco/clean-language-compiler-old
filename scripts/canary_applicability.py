#!/usr/bin/env python3
"""Generate an applicability manifest for the cross-component canaries.

Each canary in ``tests/cln/canaries/*.cln`` declares its namespace and host
list in a front-matter header of the form::

    // Namespace: <name> (Layer X, <host-list>)

Where ``<host-list>`` is a comma-separated set of tokens the umbrella prompt
already uses:

    * ``all hosts``                → runs everywhere
    * ``all`` (bare token)         → same as above (some canaries use this)
    * ``server + node-server``     → runs on the two server-class hosts
    * ``server``                   → server-only
    * ``node-server``              → node-server-only
    * ``browser only``             → runs on the browser (frame.ui) host only
    * ``browser + server``         → runs on browser + both server-class hosts

This script writes an ``applicability.json`` mapping the canary namespace to
which of ``server``, ``node-server``, ``browser`` should attempt to run it.
The three per-host L2 runners consume this manifest instead of hardcoding
their own applicability tables.

Usage::

    scripts/canary_applicability.py [--out applicability.json]
                                    [--canaries-dir tests/cln/canaries]

Exit codes: 0 success, 2 usage error, 3 no canaries found.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CANARIES = REPO_ROOT / "tests" / "cln" / "canaries"
DEFAULT_OUT = REPO_ROOT / "tests" / "output" / "canaries" / "applicability.json"

# Scan for the first `// Namespace: … (…)` block in each canary. The
# namespace phrase can span multiple `//`-prefixed lines (e.g. canvas.cln
# wraps its namespace list across 3 lines), so this is applied to the
# whole file with re.DOTALL and the "namespace" group is intentionally
# non-greedy up to the first `(`. Everything between the parens is then
# scanned by parse_host_list — that scan doesn't care about position, so
# `(Layer 2 browser, frame.client)` and `(Layer 3, server + node-server)`
# both work. The header is authoritative; if a canary omits it, we fall
# back to filename stem + all hosts (fail-open, so a missing header does
# not silently drop coverage).
NAMESPACE_BLOCK_RE = re.compile(
    r"//\s*Namespace:\s*(.+?)\(([^)]*)\)",
    re.DOTALL,
)


def parse_host_list(raw: str) -> list[str]:
    """Expand a canary's host-list phrase into a sorted list of host tokens.

    Recognized tokens: ``server``, ``node-server``, ``browser``. Anything
    else is ignored so future host classes can be added without breaking
    older canaries.

    "all" and "all hosts" expand to the full set. Standalone "browser" (as
    in "Layer 2 browser") is recognized. "browser-only" and "browser only"
    both restrict to browser.
    """
    text = raw.lower()
    hosts: set[str] = set()

    if "all hosts" in text or re.search(r"\ball\b", text):
        hosts.update(["server", "node-server", "browser"])
        return sorted(hosts)

    # Order matters: check the longer "node-server" before the bare "server"
    # so that "node-server" doesn't get double-counted or misread as a plain
    # "server" hit.
    if "node-server" in text:
        hosts.add("node-server")
    # "server + node-server" and standalone "server" both fire this branch;
    # "node-server" alone does NOT — the earlier check consumed it.
    if re.search(r"(?<!-)\bserver\b", text):
        hosts.add("server")
    if "browser" in text:
        hosts.add("browser")

    return sorted(hosts)


def scan_canary(path: Path) -> tuple[str, list[str]]:
    """Return (namespace, [applicable_hosts]) for one canary file.

    Falls back to the filename stem + ``all`` if the header is missing —
    that fail-open is deliberate. A canary without a header still gets
    surfaced in the matrix; a canary that shouldn't run on a host it
    doesn't apply to would rather show up as ``skip`` than get silently
    dropped.
    """
    text = path.read_text(encoding="utf-8", errors="replace")
    match = NAMESPACE_BLOCK_RE.search(text)
    if not match:
        return path.stem, ["server", "node-server", "browser"]

    # Use the filename stem for the manifest key so downstream lookups
    # are stable even if the human-readable namespace phrase changes.
    return path.stem, parse_host_list(match.group(2))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--canaries-dir", type=Path, default=DEFAULT_CANARIES)
    args = parser.parse_args()

    if not args.canaries_dir.is_dir():
        print(f"error: canaries dir not found: {args.canaries_dir}", file=sys.stderr)
        return 2

    canaries = sorted(args.canaries_dir.glob("*.cln"))
    if not canaries:
        print(f"error: no canaries in {args.canaries_dir}", file=sys.stderr)
        return 3

    manifest = {
        "generated_from": str(args.canaries_dir.relative_to(REPO_ROOT)),
        "canaries": {},
    }
    for cln in canaries:
        namespace, hosts = scan_canary(cln)
        manifest["canaries"][namespace] = {
            "hosts": hosts,
            "wasm": f"{namespace}.wasm",
        }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(f"wrote {args.out} ({len(manifest['canaries'])} canaries)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
