#!/usr/bin/env bash
# Install the repo-tracked git hooks for the Clean Language compiler.
#
# Usage:   ./scripts/install-hooks.sh
# Uninstall: git config --unset core.hooksPath
#
# What this does:
#   Sets core.hooksPath = .githooks   (per-clone; not global)
#   From that point on:
#     .githooks/pre-commit runs on every `git commit`.
#     .githooks/pre-push   runs on every `git push`.
#
# Environment escape hatches:
#   CLN_SKIP_PRE_COMMIT=1  git commit …    → skip advisory checks for one call
#   CLN_SKIP_PRE_PUSH=1    git push …      → skip pre-push checks
#   git commit --no-verify                  → skip ALL hooks for one call
#   git push --no-verify                    → skip ALL hooks for one push

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [ ! -d .githooks ]; then
    echo "error: .githooks/ directory not found in $REPO_ROOT" >&2
    exit 1
fi

CURRENT=$(git config --get core.hooksPath || true)
if [ "$CURRENT" = ".githooks" ]; then
    echo "core.hooksPath is already set to .githooks — nothing to do."
    exit 0
fi

if [ -n "$CURRENT" ]; then
    echo "warning: core.hooksPath is currently '$CURRENT' — overwriting to '.githooks'."
fi

git config core.hooksPath .githooks

# Make every hook executable (in case a clone lost the +x bit).
chmod +x .githooks/* 2>/dev/null || true

echo "installed: core.hooksPath = .githooks"
echo "hooks now active:"
ls -1 .githooks/ | grep -v '^\.' | sed 's/^/  - /'
echo
echo "escape hatches:"
echo "  CLN_SKIP_PRE_COMMIT=1 git commit …    # skip pre-commit checks"
echo "  CLN_SKIP_PRE_PUSH=1   git push   …    # skip pre-push checks"
echo "  git commit --no-verify                # skip ALL hooks for one commit"
echo "  git push --no-verify                  # skip ALL hooks for one push"
