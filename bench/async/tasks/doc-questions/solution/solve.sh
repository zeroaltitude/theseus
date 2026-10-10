#!/bin/bash
# The oracle (theseus-2wxa): the eight notes at once, then the answers.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for i in 01 02 03 04 05 06 07 08; do archive-get "notes-$i" > "$out/$i" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
{
  echo "1 $(cat "$out"/* | sed -n 's/^The release codename is \([A-Z]*\)\.$/\1/p')"
  echo "2 $(cat "$out"/* | sed -n 's/^The on-call rotation lasts \([0-9]*\) days\.$/\1/p')"
  echo "3 $(cat "$out"/* | sed -n 's/^The primary database listens on port \([0-9]*\)\.$/\1/p')"
} > "$APP/answers.txt"
