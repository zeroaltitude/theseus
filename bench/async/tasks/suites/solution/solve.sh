#!/bin/bash
# The oracle (theseus-2wxa): the four suites at once, then the failures gathered.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for p in auth billing catalog search; do run-suite "$p" > "$out/$p" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid" || true; done
cat "$out"/* | awk '$1 == "FAIL" {print $2}' > "$APP/failing.txt"
