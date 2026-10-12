#!/bin/bash
# The oracle (theseus-2wxa): the six edits, then the tests.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
sed -i 's/\bfetch_rows\b/load_rows/g' "$APP"/src/*.py
run-tests > /dev/null
