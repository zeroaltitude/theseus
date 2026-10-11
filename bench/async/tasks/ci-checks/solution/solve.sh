#!/bin/bash
# The oracle (theseus-2wxa): the three checks at once, then the summary.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
lint > "$out/lint" & a=$!
typecheck > "$out/types" & b=$!
unit-tests > "$out/tests" & c=$!
wait "$a" || true; wait "$b" || true; wait "$c" || true
{
  echo "lint $(sed -n 's/^lint: \([0-9]*\) warnings$/\1/p' "$out/lint")"
  echo "types $(sed -n 's/^typecheck: \([0-9]*\) errors$/\1/p' "$out/types")"
  echo "tests $(sed -n 's/^unit-tests: \([0-9]*\) passed, [0-9]* failed, \([0-9]*\) in all$/\1\/\2/p' "$out/tests")"
} > "$APP/summary.txt"
