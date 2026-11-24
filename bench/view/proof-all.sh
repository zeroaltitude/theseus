#!/bin/sh
# The view's whole proof in one command (issue 44), on a machine that builds: the daemon and the CLI built,
# the daemon test of /new and of a shared place with a second person (the stand-in Discord), then every case of
# bench/view/proof.py (one context, the view) with the 30-turn and the long cost runs, under the cut's acceptance
# (--expect-cut). It ends with the case table, the cost summaries and the acceptance, and exits nonzero when
# the build, the daemon test or the acceptance fails.
#
#   sh bench/view/proof-all.sh [an empty output directory]
#
# A build that fails stops it: the suite never runs on binaries an earlier build left.
set -u
cd "$(git rev-parse --show-toplevel)" || exit 1
out=${1:-$(mktemp -d)/proof}
log=$(mktemp -d)
echo "== proof-all: head $(git rev-parse --short HEAD), output $out"

cargo build -q -p theseusd -p theseus >"$log/build" 2>&1
rc=$?
tail -20 "$log/build"
echo "== build exit $rc"
[ "$rc" -eq 0 ] || exit 1

cargo nextest run -p theseusd --test view_proof --no-fail-fast >"$log/daemon" 2>&1
daemon=$?
grep -E "PASS|FAIL|Summary" "$log/daemon"
[ "$daemon" -eq 0 ] || grep -A2 "panicked" "$log/daemon" | cut -c1-600
echo "== daemon test (view_proof) exit $daemon"

PYTHONDONTWRITEBYTECODE=1 python3 bench/view/proof.py --bin-dir target/debug --out "$out" --cost-turns 30 \
  --cost-long --expect-cut --debug 2>/dev/null | tee "$log/proof"
# A pipe reports `tee`'s status: the suite's verdict is read from what it wrote (no file: a failure).
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); sys.exit(2 if r.get("expect_cut") else 0)' \
  "$out/proof.json" 2>/dev/null
proof=$?

echo
echo "== summary"
sed -n '/^| case | result/,/^| retired_keys/p' "$log/proof"
echo
grep -E "^compilations|^sent |^cold start|^### " "$log/proof"
sed -n '/^## The cut.s acceptance/,$p' "$log/proof"
echo "== daemon test exit $daemon, suite and acceptance exit $proof"
[ "$daemon" -eq 0 ] && [ "$proof" -eq 0 ]
