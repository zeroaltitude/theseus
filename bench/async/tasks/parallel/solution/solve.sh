#!/bin/bash
# The oracle: the six digests at once, then the aggregate.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
pids=()
for p in alpha bravo charlie delta echo foxtrot; do
  digest "$p" > "$out/$p" &
  pids+=($!)
done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sed 's/: / /' > "$APP/digests.txt"
