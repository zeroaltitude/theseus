#!/bin/bash
# The oracle (theseus-2wxa): the four configurations at once, then the settings that differ.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for e in dev staging prod perf; do env-config "$e" > "$out/$e" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sort -u | cut -d= -f1 | uniq -d > "$APP/differs.txt"
