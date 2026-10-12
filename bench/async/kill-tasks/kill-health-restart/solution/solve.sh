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
LIB="$(dirname "$(command -v restart)")/../lib/asyncbench.py"
step() {
  local n=$1; shift
  [ -e "$J/$n.done" ] && return 0
  "$@" > "$out/$n" || true
  touch "$J/$n.done"
}
plan() {
  pids=()
  for s in auth billing search mail; do step "h-$s" health "$s" & pids+=($!); done
  for pid in "${pids[@]}"; do wait "$pid" || true; done
  bad=$(basename "$(grep -l '503' "$out"/h-* | head -1)"); bad=${bad#h-}
  step restart restart "$bad"
  step recheck health "$bad"
  echo "$bad" > "$APP/restarted.txt"
}
set -m
plan & harness=$!
python3 "$LIB" await --tool restart --kind start --timeout 600 > /dev/null || true
sleep "$(awk -v s="${ASYNC_TIME_SCALE:-1}" 'BEGIN {print 5.0 * s}')"
python3 "$LIB" kill-record --trigger '{"by": "oracle", "tool": "restart", "kind": "start", "delay_s": 5.0}' \
  --pids "[{\"pid\": $harness}]" --settle 2 > /dev/null
kill -KILL -- "-$harness" 2>/dev/null || true
wait "$harness" 2>/dev/null || true
sleep "$(awk -v s="${ASYNC_TIME_SCALE:-1}" 'BEGIN {print 2 * s}')"
python3 "$LIB" restart-record --resumable true --note "the oracle's journal" > /dev/null
set +m
plan
