#!/usr/bin/env bash
# The identity of the tree a release binary is built from (theseus-2m7jg): the commit, and a digest of what the
# working tree changes beyond it (tracked changes and untracked, unignored files). build.sh writes it beside the
# binaries it builds, and the gate's size phase builds the same string for the tree under test: equal strings mean
# the binaries are of this commit and these edits. A binary carries no hash of its own, so this stamp is its proof.
#
#   scripts/tree-stamp.sh          prints "<commit> <digest>" (digest "clean" when the tree matches the commit)
set -euo pipefail
cd "$(dirname "$0")/.."
head="$(git rev-parse HEAD 2>/dev/null || echo none)"
digest="$({
  git diff HEAD --binary 2>/dev/null || true
  git ls-files -z --others --exclude-standard 2>/dev/null | xargs -0 -r sha256sum || true
} | sha256sum | cut -c1-16)"
# sha256 of nothing, cut: the tree is clean.
empty="$(printf '' | sha256sum | cut -c1-16)"
[ "$digest" != "$empty" ] || digest=clean
echo "$head $digest"
