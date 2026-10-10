#!/bin/bash
# The oracle (theseus-2wxa): the four checks at once, then the failing one restarted and checked.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for s in auth billing search mail; do health "$s" > "$out/$s" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid" || true; done
bad=$(basename "$(grep -l '503' "$out"/* | head -1)")
restart "$bad" > /dev/null
health "$bad" > /dev/null
echo "$bad" > "$APP/restarted.txt"
