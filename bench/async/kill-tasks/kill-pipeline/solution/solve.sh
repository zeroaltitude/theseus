#!/bin/bash
# The oracle (theseus-7ajc): the plain oracle's work as a harness that keeps a journal. It is killed once, at
# the injection's trigger, with SIGKILL to its process group (the sub-steps in flight die with it, as a harness's
# own processes do), the kill recorded in the ledger first; after the settle it restarts, skips what the journal
# holds, and runs the rest once: reward 1, no effect twice, nothing left running.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
J="$out/journal"; mkdir -p "$J"
LIB="$(dirname "$(command -v run-app)")/../lib/asyncbench.py"
step() {
  local n=$1; shift
  [ -e "$J/$n.done" ] && return 0
  "$@" > "$out/$n" || true
  touch "$J/$n.done"
}
plan() {
  step build build
  step run run-app
  step check check-output
  sed -n 's/.*verdict \([0-9a-f]*\)$/\1/p' "$out/check" > "$APP/verdict.txt"
}
set -m
plan & harness=$!
python3 "$LIB" await --tool run-app --kind start --timeout 600 > /dev/null || true
sleep "$(awk -v s="${ASYNC_TIME_SCALE:-1}" 'BEGIN {print 3.0 * s}')"
python3 "$LIB" kill-record --trigger '{"by": "oracle", "tool": "run-app", "kind": "start", "delay_s": 3.0}' \
  --pids "[{\"pid\": $harness}]" --settle 2 > /dev/null
kill -KILL -- "-$harness" 2>/dev/null || true
wait "$harness" 2>/dev/null || true
sleep "$(awk -v s="${ASYNC_TIME_SCALE:-1}" 'BEGIN {print 2 * s}')"
python3 "$LIB" restart-record --resumable true --note "the oracle's journal" > /dev/null
set +m
plan
