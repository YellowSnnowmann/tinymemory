#!/usr/bin/env bash
# Issue #18 §C1 acceptance: nothing outside core's engine module names the
# tinycortex crate in code.
#
# The issue's literal check -- `grep -rl tinycortex core/src` -- counts prose:
# doc comments, string literals, and log tags like "[tinycortex:sync]" match it
# and always will. What the criterion *means* is that no file outside
# `core/src/engine/` reaches the engine through a code path. This script tests
# that: a `use tinycortex...` item or a `tinycortex::` path segment, in a
# non-comment position, outside the engine module.
set -euo pipefail

cd "$(dirname "$0")/../.."

# Strip comment lines (`//`, `///`, `//!`) before matching so prose cannot
# trip it; then require the crate name in path position.
offenders="$(
  grep -rln --include='*.rs' 'tinycortex' core/src \
    | grep -v '^core/src/engine/' \
    | while read -r f; do
        if sed -E 's://.*$::' "$f" \
           | grep -Eq '(^|[^A-Za-z0-9_])(use[[:space:]]+tinycortex\b|tinycortex::)'; then
          echo "$f"
        fi
      done
)"

if [ -n "$offenders" ]; then
  echo "core/src files outside the engine module reach tinycortex in code:" >&2
  echo "$offenders" | sed 's/^/  /' >&2
  echo >&2
  echo "Route through core/src/engine/ (the seam) or the memory contract." >&2
  exit 1
fi
echo "engine containment holds: no code path names tinycortex outside core/src/engine/"
