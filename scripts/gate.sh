#!/usr/bin/env bash
# The commit gate: every check must pass, and a failure stops the chain.
# Use as `scripts/gate.sh && git commit …` (a `;` anywhere in a hand-written
# chain once let an unformatted commit through).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -q -- -D warnings
# The reader rule (P0's rule 3, theseus-wjy): every crate, method, notification,
# edge kind, and label has its reader, or a reserved marker naming the row that
# brings it. The suite runs it again; first, alone, so a miss stops the gate in
# seconds and says what to add, every miss at once.
cargo nextest run --workspace --no-fail-fast \
  -E 'package(theseus-core) & kind(lib) & test(/^tests_registry::/)'
cargo nextest run --workspace --no-fail-fast
# The web apps' protocol types, which a theseus-protocol test writes from the
# Rust ones (theseus-0g4): a type changed without its TypeScript fails here.
if ! git diff --quiet -- web/src/protocol.gen ||
  [ -n "$(git ls-files --others --exclude-standard -- web/src/protocol.gen)" ]; then
  echo "web/src/protocol.gen changed: the protocol's Rust types changed without their TypeScript; git add it"
  exit 1
fi
# The lifecycle budgets of §9 (FAST, theseus-qa0): cold start, clean shutdown
# with a job running, SIGKILL then restart, a binary swap with the job's
# wrapper adopted, 10 runs each on an empty store, p95 against the budget plus
# the measured noise margin; and restore, measured, whose restored store must
# serve. Debug binaries: they are never faster than release, so a pass here
# holds for release.
cargo build -q -p theseusd -p theseus-sim
# Other processes' dirty pages are flushed first, so a start's fsync never
# pays for their writeback (1.3 GB of it once put a clean shutdown's p95 at
# 148 ms). And one stalled fsync does not fail the gate: a miss runs the
# bench once more, and only a second miss fails it, as a real regression
# does (theseus-hee). Each run's p50s, p95s, and limits are appended to a
# history outside the tree, which every worktree shares, labelled with the
# branch and the commit judged; a miss is recorded even when its rerun
# passes. A passing run warns about a phase within 10% of its limit.
# `theseus-sim bench history` reads it (theseus-1hk).
history="${THESEUS_BENCH_HISTORY:-$HOME/.cache/theseus/bench-history.csv}"
label="$(git rev-parse --abbrev-ref HEAD) $(git describe --always --dirty)"
lifecycle() {
  target/debug/theseus-sim bench lifecycle --runs 10 --check --record "$history" --label "$label"
}
# A neighbour's sustained IO (a parallel build, a package install) stalls a
# start's fsyncs for seconds, and then the bench measures the neighbour, not
# Theseus. On 2026-10-01 an openclaw rotation held IO pressure near 50 %, and
# a restart's p95 was 2.3 s, where the same tree had passed at 41 ms. So each
# run first waits, for up to 5 minutes, until the kernel's IO and CPU
# pressure (PSI, `some avg10`) are under 10 % and 20 %, and it says what it
# saw. The budgets don't change. Without PSI it doesn't wait.
#
# Pressure alone misses one neighbour: many busy cores and nothing queued.
# Then each thread runs slower (all-core turbo, shared SMT siblings) with
# little CPU pressure. At 15:25 the same day every phase ran about 2x slow,
# untouched code included, at load 14 on 16 cores and CPU pressure 2 %;
# about 16:39 a run missed with the load near 20. So the wait also holds
# while the 1-minute load average is at or over the core count, the cores
# oversubscribed. That bar, not half the cores, keeps a long neighbouring
# build from stalling every gate; the wait is bounded at 5 minutes
# (theseus-611s).
settle() {
  [ -r /proc/pressure/io ] && [ -r /proc/pressure/cpu ] || return 0
  local io cpu load waited=0
  local cores
  cores=$(nproc)
  while :; do
    io=$(awk '/^some/ {split($2, a, "="); print int(a[2])}' /proc/pressure/io)
    cpu=$(awk '/^some/ {split($2, a, "="); print int(a[2])}' /proc/pressure/cpu)
    load=$(awk '{print $1}' /proc/loadavg)
    if [ "$io" -lt 10 ] && [ "$cpu" -lt 20 ] && awk -v l="$load" -v c="$cores" 'BEGIN {exit !(l < c)}'; then break; fi
    if [ "$waited" -ge 300 ]; then
      echo "lifecycle: still busy after 5 minutes (IO pressure $io %, CPU $cpu %, load $load on $cores cores); measuring anyway"
      return 0
    fi
    sleep 5
    waited=$((waited + 5))
  done
  if [ "$waited" -gt 0 ]; then
    echo "lifecycle: waited $waited s for the machine to settle (IO pressure $io %, CPU $cpu %, load $load on $cores cores)"
  fi
  return 0
}
sync
settle
lifecycle || {
  echo "lifecycle: a budget was missed; running the bench once more"
  sync
  settle
  lifecycle
}
cargo deny --log-level error check
if [ -d web/node_modules ]; then (cd web && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
# The cockpit (theseus-45n5): lint, type-check, and build. Its build is not committed (several MB, new with each
# edit); the install builds it before the release build, and a binary without it says so at /cockpit/.
if [ -d cockpit/node_modules ]; then (cd cockpit && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
git diff --quiet -- crates/theseusd/web/dist || { echo "web dist changed by the build: commit it"; exit 1; }
echo "gate: ok"
