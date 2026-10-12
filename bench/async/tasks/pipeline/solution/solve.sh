#!/bin/bash
# The oracle (theseus-2wxa): each step after the one it needs.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
build > /dev/null
run-app > /dev/null
check-output | sed -n 's/.*verdict \([0-9a-f]*\)$/\1/p' > "$APP/verdict.txt"
