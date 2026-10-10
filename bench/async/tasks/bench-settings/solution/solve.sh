#!/bin/bash
# The oracle (theseus-2wxa): the four runs at once, then the best.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for b in 16 32 64 128; do bench-batch "$b" > "$out/$b" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sed -n 's/^batch size \([0-9]*\): \([0-9]*\) ops\/s$/\2 \1/p' | sort -n | tail -1 \
  | awk '{print "batch_size = " $2}' > "$APP/bench.toml"
