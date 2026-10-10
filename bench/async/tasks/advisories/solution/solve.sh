#!/bin/bash
# The oracle (theseus-2wxa): the five lookups at once, then the table.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for a in 1001 1002 1003 1004 1005; do advisory "ADV-$a" > "$out/$a" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
for a in 1001 1002 1003 1004 1005; do
  echo "ADV-$a $(sed -n 's/^  severity: //p' "$out/$a") $(sed -n 's/^  fixed in: //p' "$out/$a")"
done > "$APP/advisories.txt"
