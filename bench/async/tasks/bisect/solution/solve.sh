#!/bin/bash
# The oracle (theseus-2wxa): each commit's worktree, then its test, the commits at once.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for c in $(awk '{print $1}' "$APP/commits.txt"); do
  (make-worktree "$c" > /dev/null && test-commit "$c" > "$out/$c") & pids+=($!)
done
for pid in "${pids[@]}"; do wait "$pid" || true; done
for c in $(awk '{print $1}' "$APP/commits.txt"); do
  if grep -q FAIL "$out/$c"; then echo "$c" > "$APP/first-bad.txt"; break; fi
done
