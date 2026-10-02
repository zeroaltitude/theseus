#!/usr/bin/env bash
# The commit gate: every check must pass, and a failure stops the chain.
# Use as `scripts/gate.sh && git commit …` (a `;` anywhere in a hand-written
# chain once let an unformatted commit through).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# Phase timings (theseus-goa8; review 2's C7). Each check runs under `phase`,
# which times it, and the gate prints the table before it ends, a failed run's
# too, naming the phase that failed and how long it ran: where the minutes go,
# and which phase to speed up. Whole seconds: a phase is seconds to minutes.
# Each run's table is appended to a log outside the tree, so the gate's own
# cost has a history (`$THESEUS_GATE_TIMES`, by default
# `~/.cache/theseus/gate-times.csv`: time, label, phase, seconds, status).
history="${THESEUS_BENCH_HISTORY:-$HOME/.cache/theseus/bench-history.csv}"
times="${THESEUS_GATE_TIMES:-$HOME/.cache/theseus/gate-times.csv}"
flaky_log="${THESEUS_FLAKY_LOG:-$HOME/.cache/theseus/flaky.csv}"
label="$(git rev-parse --abbrev-ref HEAD) $(git describe --always --dirty)"
gate_tmp="$(mktemp -d "${TMPDIR:-/tmp}/theseus-gate.XXXXXX")"
gate_began=$SECONDS
phase_names=()
phase_secs=()
phase_now=""
phase_began=0
summary_printed=""
phase() {
  phase_now="$1"
  phase_began=$SECONDS
  shift
  "$@"
  phase_names+=("$phase_now")
  phase_secs+=("$((SECONDS - phase_began))")
  phase_now=""
}
# The table, once, and its log. `status` is how the gate ends.
summary() {
  [ -z "$summary_printed" ] || return 0
  summary_printed=1
  local status=$1 total=$((SECONDS - gate_began)) i when
  when="$(date -Iseconds)"
  echo "gate: phase times, in seconds"
  for i in "${!phase_names[@]}"; do
    printf '  %-16s %5d\n' "${phase_names[$i]}" "${phase_secs[$i]}"
  done
  if [ -n "$phase_now" ]; then
    printf '  %-16s %5d  <- failed here\n' "$phase_now" "$((SECONDS - phase_began))"
  fi
  printf '  %-16s %5d\n' total "$total"
  mkdir -p "$(dirname "$times")" 2>/dev/null || true
  {
    [ -s "$times" ] || echo "time,label,phase,seconds,status"
    for i in "${!phase_names[@]}"; do
      printf '%s,"%s",%s,%s,ok\n' "$when" "$label" "${phase_names[$i]// /_}" "${phase_secs[$i]}"
    done
    if [ -n "$phase_now" ]; then
      printf '%s,"%s",%s,%s,failed\n' "$when" "$label" "${phase_now// /_}" "$((SECONDS - phase_began))"
    fi
    printf '%s,"%s",total,%s,%s\n' "$when" "$label" "$total" "$status"
  } >>"$times" 2>/dev/null || echo "gate: the phase times were NOT logged to $times"
}
on_exit() {
  local status=$?
  rm -rf "$gate_tmp"
  if [ "$status" -ne 0 ]; then
    summary failed
    echo "gate: FAILED${phase_now:+ in $phase_now}"
  fi
}
trap on_exit EXIT

# The suite, and then the tests that passed only on a retry. `.config/nextest.toml`
# retries named flaky tests (and only those), so a flake costs a rerun of the test
# and not of the gate; this says which ones flaked, so a retry is recorded and not
# forgotten, in the output and in `$THESEUS_FLAKY_LOG` (time, label, test, attempt).
# How a test gets onto the list, and off it, is in scripts/AGENTS.md.
suite() {
  local log="$gate_tmp/suite.log"
  cargo nextest run --workspace --no-fail-fast 2>&1 | tee "$log"
  local flaky
  flaky="$(grep -E '^ *FLAKY ' "$log" || true)"
  if [ -n "$flaky" ]; then
    echo "gate: these tests passed only on a retry (scripts/AGENTS.md, \"The flaky list\"):"
    echo "$flaky" | sed 's/^ */  /'
    mkdir -p "$(dirname "$flaky_log")" 2>/dev/null || true
    {
      [ -s "$flaky_log" ] || echo "time,label,test,attempt"
      echo "$flaky" | awk -v t="$(date -Iseconds)" -v l="$label" '{printf "%s,\"%s\",%s %s,%s\n", t, l, $(NF-1), $NF, $2}'
    } >>"$flaky_log" 2>/dev/null || true
  fi
}

# The reader rule (P0's rule 3, theseus-wjy): every crate, method, notification,
# edge kind, and label has its reader, or a reserved marker naming the row that
# brings it. The suite runs it again; first, alone, so a miss stops the gate in
# seconds and says what to add, every miss at once.
registry() {
  cargo nextest run --workspace --no-fail-fast \
    -E 'package(theseus-core) & kind(lib) & test(/^tests_registry::/)'
}

# The web apps' protocol types, which a theseus-protocol test writes from the
# Rust ones (theseus-0g4): a type changed without its TypeScript fails here.
protocol_types() {
  if ! git diff --quiet -- web/src/protocol.gen ||
    [ -n "$(git ls-files --others --exclude-standard -- web/src/protocol.gen)" ]; then
    echo "web/src/protocol.gen changed: the protocol's Rust types changed without their TypeScript; git add it"
    exit 1
  fi
}

# The lifecycle budgets of §9 (FAST, theseus-qa0): cold start, clean shutdown
# with a job running, SIGKILL then restart, a binary swap with the job's
# wrapper adopted, 10 runs each on an empty store, p95 against the budget plus
# the measured noise margin; and restore, measured, whose restored store must
# serve. Debug binaries: they are never faster than release, so a pass here
# holds for release.
#
# Other processes' dirty pages are flushed first, so a start's fsync never
# pays for their writeback (1.3 GB of it once put a clean shutdown's p95 at
# 148 ms). And one stalled fsync does not fail the gate: a miss runs the
# bench once more, and only a second miss fails it, as a real regression
# does (theseus-hee). Each run's p50s, p95s, and limits are appended to a
# history outside the tree, which every worktree shares, labelled with the
# branch and the commit judged; a miss is recorded even when its rerun
# passes. A passing run warns about a phase within 10% of its limit.
# `theseus-sim bench history` reads it (theseus-1hk).
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
# A lane's gate skips the timing bench (THESEUS_GATE_NO_BENCH=1, set by the
# lane recipe): the gate that joins the lane to main runs it, on main's tree,
# and its settle step held the shared gate lock for minutes while every other
# agent's gate queued behind it. A lane whose work touches the start path
# runs `theseus-sim bench lifecycle` alone once before its join.
lifecycle_bench() {
  if [ -n "${THESEUS_GATE_NO_BENCH:-}" ]; then
    echo "lifecycle: skipped (THESEUS_GATE_NO_BENCH: a lane's gate; the join's gate runs it)"
    return 0
  fi
  sync
  settle
  lifecycle || {
    echo "lifecycle: a budget was missed; running the bench once more"
    sync
    settle
    lifecycle
  }
}

# The turn bench (theseus-goa8; review 2's S4 and consideration 8): a plain
# turn's frames, counted from the daemon's WAL, against §9's per-turn overhead
# restated as frames (5 today; the floor is 2). A frame is one fdatasync, so
# the count does not depend on the disk or the load: it needs no quiet machine,
# and runs in a lane's gate as well. Its timings (wall time, memory) are
# recorded only where the lifecycle bench is recorded, so the history holds no
# number measured beside a busy neighbour. A miss reruns once, as the lifecycle
# bench's does: a frame another writer put inside the window is one run's, and a
# real regression writes it every time. In a lane's gate: five runs of each
# kind, and no burst (about 5 s); at the join, ten runs and a burst of 30 turns,
# which the history records (about 11 s).
turn_bench() {
  target/debug/theseus-sim bench turn --check "$@"
}
turn_step() {
  local args=(--runs 5 --burst 0)
  [ -n "${THESEUS_GATE_NO_BENCH:-}" ] || args=(--record "$history" --label "$label")
  turn_bench "${args[@]}" || {
    echo "turn: the frames budget was missed; running the bench once more"
    turn_bench "${args[@]}"
  }
}

web_apps() {
  if [ -d web/node_modules ]; then (cd web && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
}
# The cockpit (theseus-45n5): lint, type-check, and build. Its build is not committed (several MB, new with each
# edit); the install builds it before the release build, and a binary without it says so at /cockpit/.
cockpit() {
  if [ -d cockpit/node_modules ]; then (cd cockpit && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
}
web_dist() {
  git diff --quiet -- crates/theseusd/web/dist || { echo "web dist changed by the build: commit it"; exit 1; }
}

phase fmt cargo fmt --all -- --check
phase clippy cargo clippy --workspace --all-targets -q -- -D warnings
phase "reader rule" registry
phase suite suite
phase "protocol types" protocol_types
phase build cargo build -q -p theseusd -p theseus-sim
phase lifecycle lifecycle_bench
phase turn turn_step
phase deny cargo deny --log-level error check
phase web web_apps
phase cockpit cockpit
phase "web dist" web_dist
summary ok
echo "gate: ok"
