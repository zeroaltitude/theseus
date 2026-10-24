#!/usr/bin/env bash
# bench/recall's Python tests (the gate's "recall tests" phase). They import
# tomllib, so they need Python 3.11 or newer: the first of python3.13, 3.12,
# 3.11 and python3 on PATH that is new enough runs them. With none, say so and
# pass. Quiet unless a test fails.
set -euo pipefail
cd "$(dirname "$0")/.."
py=
for c in python3.13 python3.12 python3.11 python3; do
  if command -v "$c" > /dev/null 2>&1 \
    && "$c" -c 'import sys; sys.exit(sys.version_info < (3, 11))' > /dev/null 2>&1; then
    py=$c
    break
  fi
done
if [ -z "$py" ]; then
  echo "recall tests: skipped (no Python >= 3.11 on PATH; bench/recall's tests need tomllib)"
  exit 0
fi
log=$(mktemp)
trap 'rm -f "$log"' EXIT
if ! PYTHONDONTWRITEBYTECODE=1 "$py" -m unittest discover -s bench/recall -p 'test_*.py' > "$log" 2>&1; then
  cat "$log"
  exit 1
fi
