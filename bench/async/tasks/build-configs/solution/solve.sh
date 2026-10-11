#!/bin/bash
# The oracle (theseus-2wxa): the three builds at once, then the sizes.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for c in debug release minsize; do build-config "$c" > "$out/$c" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sed -n 's/^built \([a-z]*\): .*, \([0-9]*\) bytes$/\1 \2/p' > "$APP/sizes.txt"
