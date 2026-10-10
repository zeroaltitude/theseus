#!/bin/bash
# The oracle (theseus-2wxa): the two builds at once, then the link.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
build-lib core > /dev/null & a=$!
build-lib ui > /dev/null & b=$!
wait "$a"; wait "$b"
link-app | sed -n 's/^linked out\///p' > "$APP/artifact.txt"
