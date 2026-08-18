#!/usr/bin/env bash
# Reports the dependency count of each build configuration, and fails when the
# minimal one grows past its ceiling.
#
# Issue #18 §D5. The point of the contract crate and of `--no-default-features`
# is that a host which wants memory ports and nothing else does not compile a
# storage engine, a native library, or an HTTP stack. That property is invisible
# in a diff: a dependency arrives transitively, through a feature enabled two
# crates away, and the PR that causes it looks innocent. Printing the numbers on
# every run makes the regression visible on the PR that caused it, which is the
# only moment it is cheap to fix.
#
# The ceiling applies only to the minimal configuration. The richer ones are
# reported, not gated: their sizes are a consequence of what an engine needs,
# and a number nobody chose is not a budget worth failing on.
set -euo pipefail

# Deliberately generous: the minimal build links 40 crates today. This is a
# ratchet against accidental growth, not a target to optimise towards — a limit
# set at today's exact count would fail on the first legitimate addition and get
# raised without thought, which teaches everyone to ignore it.
MINIMAL_CEILING="${MINIMAL_CEILING:-50}"

count() {
  # `-e normal` excludes dev- and build-dependencies: a test-only crate is not
  # something a consumer links.
  cargo tree "$@" -e normal --prefix none 2>/dev/null \
    | sed 's/ (\*)$//' | awk 'NF' | sort -u | wc -l | tr -d ' '
}

printf '%-52s %s\n' "configuration" "crates"
printf '%-52s %s\n' "----------------------------------------------------" "------"

minimal=$(count -p tinymemory --no-default-features)
printf '%-52s %s\n' "tinymemory --no-default-features" "$minimal"
printf '%-52s %s\n' "tinymemory --all-features" "$(count -p tinymemory --all-features)"
printf '%-52s %s\n' "tinymemory-api" "$(count -p tinymemory-api)"
printf '%-52s %s\n' "tinymemory-tinycortex (default)" "$(count -p tinymemory-tinycortex --no-default-features)"
printf '%-52s %s\n' "tinymemory-tinycortex --features memory-git" "$(count -p tinymemory-tinycortex --features memory-git)"
printf '%-52s %s\n' "tinymemory-remote" "$(count -p tinymemory-remote)"

echo
if [ "$minimal" -gt "$MINIMAL_CEILING" ]; then
  echo "the minimal build links $minimal crates, over its ceiling of $MINIMAL_CEILING" >&2
  echo >&2
  echo "A host that asks for no features should get the contract, the registry" >&2
  echo "and the mandatory composition — nothing that links a storage engine or" >&2
  echo "an HTTP stack. Check what the new dependency arrived through:" >&2
  echo >&2
  echo "  cargo tree -p tinymemory --no-default-features -e normal" >&2
  exit 1
fi
echo "minimal build links $minimal crates, within its ceiling of $MINIMAL_CEILING"
