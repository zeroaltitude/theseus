#!/bin/bash
# The oracle (theseus-2wxa): the codemod, the tests, the fix, the tests again.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
codemod > /dev/null
test-suite > "$out/first" || true
sed -i 's/legacy_mode/compat_mode/g' "$APP/tests/test_flags.py"
test-suite > /dev/null
