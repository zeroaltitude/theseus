#!/bin/bash
# The oracle (theseus-2wxa): the service up, then the query.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
start-service > /dev/null
query-service stock | sed -n 's/^in stock: //p' > "$APP/stock.txt"
