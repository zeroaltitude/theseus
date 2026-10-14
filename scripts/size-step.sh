#!/usr/bin/env bash
# The gate's size phase (theseus-2m7jg): `theseus-sim bench size --check` on a release binary, when there is one of the
# tree under test.
#
#   scripts/size-step.sh <history file> <label>
#
# The gate builds debug, whose size says nothing; a release binary comes from scripts/build.sh, which writes
# target/<profile>/.theseus-build-stamp: "<commit> <digest> <theseusd bytes>", the commit and the digest of the tree's
# edits as scripts/tree-stamp.sh prints them, and the binary's size. A binary carries no hash of its own, so the stamp
# is its proof: the check runs only when the stamp equals the tree under test's and the binary's size. Otherwise it
# says "size: skipped: no release build of <commit>" and returns 0 at once.
#
# The check fails on growth of more than 1 MB over the last recorded size of the same profile (the label's
# `size:<profile>`), and prints §9's 60 MB as a miss with its gap, which fails nothing.
#
# Environment (for the tests): THESEUS_SIZE_TARGET (the cargo target directory, default target), THESEUS_SIM (the
# bench binary, default <target>/debug/theseus-sim) and THESEUS_SIZE_STAMP (the tree under test's stamp, default
# scripts/tree-stamp.sh's: a test of the check gives the stamp it wrote, since a suite that runs beside it may touch
# the working tree between its stamp and this one).
set -euo pipefail
cd "$(dirname "$0")/.."
history="${1:?size-step: the history file}"
label="${2:?size-step: the label}"
target="${THESEUS_SIZE_TARGET:-target}"
sim="${THESEUS_SIM:-$target/debug/theseus-sim}"
now="${THESEUS_SIZE_STAMP:-$(scripts/tree-stamp.sh)}"
for profile in release release-thin; do
  dir="$target/$profile"
  stamp="$(cat "$dir/.theseus-build-stamp" 2>/dev/null || true)"
  { [ -n "$stamp" ] && [ -f "$dir/theseusd" ]; } || continue
  if [ "$stamp" = "$now $(stat -c %s "$dir/theseusd")" ]; then
    exec "$sim" bench size --check --dir "$dir" --record "$history" \
      --label "$label size:$profile" --baseline-tag "size:$profile"
  fi
done
edits=""
[ "${now##* }" = clean ] || edits="+edits"
echo "size: skipped: no release build of $(git rev-parse --short HEAD)$edits (scripts/build.sh builds and stamps one)"
