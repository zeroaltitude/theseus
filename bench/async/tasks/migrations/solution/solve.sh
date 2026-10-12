#!/bin/bash
# The oracle (theseus-2wxa): the migrations in their order.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
for m in 001 002 003; do migrate-db "$m" > "$out/$m"; done
sed -n 's/.*schema checksum //p' "$out/003" > "$APP/schema.txt"
