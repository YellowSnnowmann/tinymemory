#!/usr/bin/env bash
# Issue #18 §C1 acceptance: nothing outside core's engine module names the
# tinycortex crate in code.
#
# The issue's literal check -- `grep -rl tinycortex crates/tinymemory-core/src` -- counts prose:
# doc comments, string literals, and log tags like "[tinycortex:sync]" match it
# and always will. What the criterion *means* is that no file outside
# `crates/tinymemory-core/src/engine/` reaches the engine through a code path. This script tests
# that: a `use tinycortex...` item or a `tinycortex::` path segment, in a
# non-comment position, outside the engine module.
set -euo pipefail

cd "$(dirname "$0")/../.."

# Strip comment lines (`//`, `///`, `//!`) before matching so prose cannot
# trip it; then require the crate name in path position. The audit probed the
# first version of this regex and found three bypasses, each closed below:
# `extern crate tinycortex;` (no `::`), whitespace between the crate name and
# the path separator (`tinycortex ::memory`), and a `//` inside a string
# literal on the same line eating a real use (`let u="//x"; use tinycortex::A;`
# — comment-stripping must not fire inside quotes). Block comments can still
# yield false POSITIVES (prose inside `/* */` is not stripped), which fails
# safe: a human looks, nothing slips through.
offenders="$(
  grep -rln --include='*.rs' 'tinycortex' crates/tinymemory-core/src \
    | grep -v '^crates/tinymemory-core/src/engine/' \
    | while read -r f; do
        # Strip string literals first (so a `//` inside one cannot hide the
        # rest of the line), then line comments; then match path positions.
        if sed -E 's:"([^"\\]|\\.)*"::g' "$f" \
           | sed -E 's://.*$::' \
           | grep -Eq '(^|[^A-Za-z0-9_])(use[[:space:]]+tinycortex\b|extern[[:space:]]+crate[[:space:]]+tinycortex\b|tinycortex[[:space:]]*::)'; then
          echo "$f"
        fi
      done
)"

if [ -n "$offenders" ]; then
  echo "tinymemory-core files outside the engine module reach tinycortex in code:" >&2
  echo "$offenders" | sed 's/^/  /' >&2
  echo >&2
  echo "Route through crates/tinymemory-core/src/engine/ (the seam) or the memory contract." >&2
  exit 1
fi
echo "engine containment holds: no code path names tinycortex outside crates/tinymemory-core/src/engine/"
