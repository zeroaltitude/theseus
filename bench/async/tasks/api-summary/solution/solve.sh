#!/bin/bash
# The oracle (theseus-2wxa): the six modules at once, then the summary.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for m in core io net auth cache util; do module-source "$m" > "$out/$m" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
for m in core io net auth cache util; do
  echo "## $m"; echo; sed -n 's/^def \([a-z][a-z_]*\)(.*/- \1/p' "$out/$m"; echo
done > "$APP/API.md"
