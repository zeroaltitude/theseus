#!/bin/bash
# The oracle (theseus-2wxa): the five fetches at once, then those behind.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for r in api web worker infra docs; do repo-status "$r" > "$out/$r" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
for f in $(grep -l 'is behind' "$out"/*); do basename "$f"; done > "$APP/behind.txt"
