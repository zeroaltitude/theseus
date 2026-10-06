#!/bin/bash
# The oracle: the six parts at once, each tried again on a transient error
# (exit 75) and never after a success, then the summary.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
one() {
  local rc
  while :; do
    rc=0
    ingest "$1" > "$out/$1" || rc=$?
    [ "$rc" -eq 75 ] || return "$rc"
  done
}
pids=()
for p in 1 2 3 4 5 6; do one "$p" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
total=0
for p in 1 2 3 4 5 6; do
  rows=$(sed -n 's/^part [0-9]*: \([0-9]*\) rows$/\1/p' "$out/$p")
  echo "$p $rows"
  total=$((total + rows))
done > "$APP/ingest-summary.txt"
echo "total $total" >> "$APP/ingest-summary.txt"
