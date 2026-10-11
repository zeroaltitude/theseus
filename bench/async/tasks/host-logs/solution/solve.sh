#!/bin/bash
# The oracle (theseus-2wxa): the six logs at once, then the host with the error.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for h in web-1 web-2 web-3 web-4 web-5 web-6; do fetch-log "$h" > "$out/$h" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
basename "$(grep -l ' ERROR ' "$out"/* | head -1)" > "$APP/culprit.txt"
