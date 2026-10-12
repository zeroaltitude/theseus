#!/bin/bash
# The oracle (theseus-2wxa): the five conversions at once, then the merge.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for e in jan feb mar apr may; do convert-export "$e" > /dev/null & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
{ echo "id,amount"; for e in jan feb mar apr may; do tail -n +2 "$APP/exports/$e.csv"; done; } > "$APP/merged.csv"
