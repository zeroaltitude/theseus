#!/usr/bin/env bash
# The commit gate: every check must pass, and a failure stops the chain.
# Use as `scripts/gate.sh && git commit …` (a `;` anywhere in a hand-written
# chain once let an unformatted commit through).
#
# THE SHARED LOCK (theseus-rx91). The suite's timing-sensitive tests and the benches
# need the machine to themselves, so one gate's tests never land in another's bench:
# `~/.cache/theseus-gate.lock` (`$THESEUS_GATE_LOCK_FILE`). Who takes it, and for
# how long, is the caller's choice, `$THESEUS_GATE_LOCK`:
#
#   outer (the default): the CALLER holds the lock for the whole run, and the gate
#     takes none: `flock -o ~/.cache/theseus-gate.lock scripts/gate.sh`, or the
#     chain's theseus-quiet.sh. The join's gate on main runs this way: it wants the
#     whole machine, and runs rarely.
#   inner (`THESEUS_GATE_LOCK=inner scripts/gate.sh`): the GATE takes the lock,
#     only around the reader rule, the suite, and the benches, after every compile
#     has run without it. A lane's gate runs this way, so it stops holding every
#     other gate up through its own fmt, clippy, and builds (minutes), and queues
#     for the tests alone (about two minutes).
#
#   *** NEVER WRAP AN INNER-MODE GATE IN `flock`, OR IN theseus-quiet.sh. ***
#   The wrapper would hold the lock that the gate then waits for, and the gate would
#   wait for itself forever. The gate checks, and refuses to start (exit 2) when a
#   process above it holds the lock.
#
# How inner mode works: after its compile phases the gate runs itself again, as
# `flock -o LOCK gate.sh --locked-part …`, the "locked part", which runs the checks
# that need the machine. `-o` closes the lock's fd before the part starts, so nothing
# the tests start (a daemon that outlives its test) can keep the lock after the part
# ends (theseus-e6xj); and no function of the gate ever holds a lock's fd. The part
# hands its phase times back in a file, so one table covers both halves. Nothing in
# the part should compile, since the compile phases built what it runs; a note says so
# when something does.
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# `--locked-part DIR ASKED LABEL` is the gate calling itself (inner mode), not an option.
part=whole
if [ "${1:-}" = --locked-part ]; then
  [ "$#" -eq 4 ] || { echo "gate: --locked-part is the gate's own call, not an option" >&2; exit 2; }
  part=locked
  locked_dir=$2
  asked=$3
  label=$4
elif [ "$#" -ne 0 ]; then
  echo "usage: scripts/gate.sh   (no arguments; THESEUS_GATE_LOCK=inner|outer, THESEUS_GATE_NO_BENCH=1)" >&2
  exit 2
fi
mode="${THESEUS_GATE_LOCK:-outer}"
case "$mode" in
  outer | inner) ;;
  *)
    echo "gate: THESEUS_GATE_LOCK is '$mode'; it is 'outer' (the default) or 'inner'" >&2
    exit 2
    ;;
esac
lock="${THESEUS_GATE_LOCK_FILE:-$HOME/.cache/theseus-gate.lock}"

# Is the lock file open in process $1? /proc answers for this user's processes only.
holds_lock() {
  local fd
  for fd in /proc/"$1"/fd/*; do
    [ "$fd" -ef "$lock" ] && return 0
  done
  return 1
}
# An inner-mode gate under a caller that holds the lock would wait for it forever: say so, and stop.
refuse_nested_lock() {
  local pid=$PPID stat
  while [ "$pid" -gt 1 ]; do
    if holds_lock "$pid"; then
      echo "gate: THESEUS_GATE_LOCK=inner takes the lock itself, but process $pid ($(cat "/proc/$pid/comm" 2>/dev/null)), above this gate, already holds $lock" >&2
      echo "gate: it would wait for itself forever. Run it without the outer flock (or theseus-quiet.sh), or leave THESEUS_GATE_LOCK unset for the outer mode" >&2
      exit 2
    fi
    stat="$(cat "/proc/$pid/stat" 2>/dev/null)" || return 0
    stat=${stat##*) }
    pid="$(echo "$stat" | cut -d' ' -f2)"
    [ -n "$pid" ] || return 0
  done
}
# Who holds the lock, and who is queued for it, for the log: lines `held PID DIR` and `queued PID DIR`, the
# queue in order (DIR says which lane). A process waiting in `flock` has the lock file open too, so the open
# files alone do not say who holds it: /proc/locks lists the waiters, and whoever else has the file open holds
# it (also when its `flock` child took the lock and exited, as theseus-quiet.sh's does, or when an orphan
# inherited it). A holder's own children that inherited the fd (that script's `sleep`) are not listed.
lock_users() {
  local ino dev key pid d ppid queued
  local -a open=()
  ino="$(stat -c %i "$lock" 2>/dev/null)" || return 0
  dev=$((16#$(stat -c %D "$lock")))
  key="$(printf '%02x:%02x:%d' $(((dev >> 8) & 0xfff)) $(((dev & 0xff) | ((dev >> 12) & ~0xff))) "$ino")"
  queued="$(awk -v key="$key" '$0 ~ key && /->/ { for (i = 1; i <= NF; i++) if ($i == key) print $(i - 1) }' /proc/locks 2>/dev/null || true)"
  for d in /proc/[0-9]*; do
    pid=${d#/proc/}
    if holds_lock "$pid" && ! grep -qx "$pid" <<<"$queued"; then open+=("$pid"); fi
  done
  for pid in "${open[@]}"; do
    ppid="$(awk '{ sub(/^.*\) /, ""); print $2 }' "/proc/$pid/stat" 2>/dev/null || true)"
    case " ${open[*]} " in *" $ppid "*) continue ;; esac
    echo "held $pid $(readlink "/proc/$pid/cwd" 2>/dev/null || echo '?')"
  done
  for pid in $queued; do echo "queued $pid $(readlink "/proc/$pid/cwd" 2>/dev/null || echo '?')"; done
  return 0
}
if [ "$mode" = inner ] && [ "$part" = whole ]; then refuse_nested_lock; fi

# Phase timings (theseus-goa8; review 2's C7). Each check runs under `phase`,
# which times it, and the gate prints the table before it ends, a failed run's
# too, naming the phase that failed and how long it ran: where the minutes go,
# and which phase to speed up. Whole seconds: a phase is seconds to minutes.
# Each run's table is appended to a log outside the tree, so the gate's own
# cost has a history (`$THESEUS_GATE_TIMES`, by default
# `~/.cache/theseus/gate-times.csv`: time, label, phase, seconds, status).
# In inner mode the table covers both halves, with the wait for the lock as its
# own row (`lock wait`).
history="${THESEUS_BENCH_HISTORY:-$HOME/.cache/theseus/bench-history.csv}"
times="${THESEUS_GATE_TIMES:-$HOME/.cache/theseus/gate-times.csv}"
flaky_log="${THESEUS_FLAKY_LOG:-$HOME/.cache/theseus/flaky.csv}"
if [ "$part" = whole ]; then
  label="$(git rev-parse --abbrev-ref HEAD) $(git describe --always --dirty)"
  gate_tmp="$(mktemp -d "${TMPDIR:-/tmp}/theseus-gate.XXXXXX")"
else
  gate_tmp=$locked_dir
fi
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
# The locked part's phases, for the gate that called it: one line each, `phase SECONDS NAME`,
# and `failed SECONDS NAME` for the one that stopped it. (Its first line, `began EPOCH`, is
# written when it takes the lock.) Written however the part ends, a failure's too.
save_locked_record() {
  local i
  {
    for i in "${!phase_names[@]}"; do
      printf 'phase %s %s\n' "${phase_secs[$i]}" "${phase_names[$i]}"
    done
    if [ -n "$phase_now" ]; then
      printf 'failed %s %s\n' "$((SECONDS - phase_began))" "$phase_now"
    fi
  } >>"$gate_tmp/locked.rec" 2>/dev/null || true
}
# A gate that a signal ends reads `$?` as 0 in this trap, so `gate_done` says it did not finish.
gate_done=""
on_exit() {
  local status=$?
  if [ "$part" = locked ]; then
    save_locked_record
    return
  fi
  rm -rf "$gate_tmp"
  if [ "$status" -ne 0 ] || [ -z "$gate_done" ]; then
    [ "$status" -ne 0 ] || echo "gate: stopped by a signal"
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
    -E 'package(theseus-core) & kind(lib) & test(/^tests_registry::/)' 2>&1 | tee "$gate_tmp/registry.log"
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

# The supply-chain check, offline (theseus-goa8; review 2's SC2). Licences, bans, and
# sources need no network, and advisories are read from the database as its last fetch
# left it, so an outage of GitHub or crates.io can no longer fail a commit: 27e1237
# met a crate yanked between two gates and had to bump Cargo.lock mid-step.
# `scripts/deny-daily.sh` keeps the database fresh and files an issue for a finding
# (and is the only thing that fetches); the gate says when the database is old. A
# machine that has never fetched has no database: the gate then checks the rest and
# says that advisories were not, since a gate that fetched here would fail on an outage.
deny_check() {
  local db age
  db="$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/advisory-dbs/advisory-db-*/ 2>/dev/null | head -1 || true)"
  if [ -z "$db" ]; then
    echo "deny: no advisory database in ${CARGO_HOME:-~/.cargo}/advisory-dbs: advisories NOT checked (scripts/deny-daily.sh fetches one)"
    cargo deny --offline --log-level error check bans licenses sources
    return
  fi
  if [ -f "${db}.git/FETCH_HEAD" ]; then
    age=$((($(date +%s) - $(stat -c %Y "${db}.git/FETCH_HEAD")) / 86400))
    [ "$age" -le 7 ] || echo "deny: the advisory database is $age days old; scripts/deny-daily.sh refreshes it"
  fi
  cargo deny --offline --log-level error check
}

# One npm script of a web app, its output kept in `$gate_tmp/<app>-<script>.log` (theseus-o8nk). A failure prints
# the log's last 40 lines, where the phase table alone said only "cockpit <- failed here".
npm_step() {
  local app=$1 script=$2 log="$gate_tmp/$1-$2.log"
  if (cd "$app" && npm run -s "$script") >"$log" 2>&1; then return 0; fi
  echo "gate: \`npm run $script\` failed in $app/; the last 40 lines of its output:"
  tail -n 40 "$log" | sed 's/^/  /'
  return 1
}
web_apps() {
  if [ -d web/node_modules ]; then npm_step web lint && npm_step web build; fi
}
# The cockpit (theseus-45n5): lint, its pure modules' tests (node's own runner, theseus-9o5n), type-check, and build.
# Its build is not committed (several MB, new with each edit); the install builds it before the release build, and a
# binary without it says so at /cockpit/.
cockpit() {
  if [ -d cockpit/node_modules ]; then npm_step cockpit lint && npm_step cockpit test && npm_step cockpit build; fi
}
web_dist() {
  git diff --quiet -- crates/theseusd/web/dist || { echo "web dist changed by the build: commit it"; exit 1; }
}

# The binaries the benches run, one list for both modes: built after the suite in outer mode (`build`),
# and before the lock in inner mode (`bench build`). A new tool the benches start is added here.
bench_build() {
  cargo build -q -p theseusd -p theseus-sim -p theseus-index
}

# Nothing compiles under the lock: the compile phases built what the locked part runs. When
# something does, the tree changed after them (fmt, shape, and clippy then judged an older
# tree), or they built something other than the tests run. Say so: it also explains a long hold.
compiled_under_lock() {
  local n
  n="$(cat "$gate_tmp/registry.log" "$gate_tmp/suite.log" 2>/dev/null | grep -c '^ *Compiling ' || true)"
  if [ "$n" -gt 0 ]; then
    echo "gate: NOTE: $n crate(s) were compiled under the lock: the tree changed after the compile phases (rerun the gate before you commit), or they built something other than the tests run"
  fi
}

# The checks that need the machine to themselves, in order: the lock is held across them. In
# outer mode the caller holds it and this runs inside the whole gate, with the bench binaries
# built between the suite and the benches; in inner mode it is the locked part, whose binaries
# the compile phases built beforehand. Cargo links the binaries of the build it last ran, so in
# inner mode the benches run the test build's `theseusd` and `theseus-sim` (the workspace's
# features, which the install has too; the suite's cargo links them over the `-p` build's, and the
# compile phases run the test build last, so they leave what the benches will find), and in
# outer mode the `-p` build's.
machine_checks() {
  phase "reader rule" registry
  phase suite suite
  phase "protocol types" protocol_types
  if [ "$mode" = inner ]; then compiled_under_lock; fi
  if [ "$mode" = outer ]; then phase build bench_build; fi
  phase lifecycle lifecycle_bench
  phase turn turn_step
}

# Inner mode: take the lock around machine_checks, by running the gate again as the locked part.
# The wait, the hold, and the part's phases go into this run's log and table.
locked_checks() {
  local asked status=0 began="" kind secs name rec="$gate_tmp/locked.rec" users held queued
  asked=$(date +%s)
  echo "gate: the compiles are done; taking the shared gate lock ($lock) for the tests and benches"
  users="$(lock_users || true)"
  held="$(echo "$users" | awk '$1 == "held" { printf "  pid %s, in %s\n", $2, $3 }')"
  queued="$(echo "$users" | awk '$1 == "queued" { printf "  pid %s, in %s\n", $2, $3 }')"
  if [ -n "$held" ]; then
    echo "gate: waiting, the lock is held by:"
    echo "$held"
  fi
  if [ -n "$queued" ]; then
    echo "gate: queued for it ahead of this gate, in order:"
    echo "$queued"
  fi
  phase_now="lock wait"
  phase_began=$SECONDS
  flock -o "$lock" "$self" --locked-part "$gate_tmp" "$asked" "$label" || status=$?
  phase_now=""
  if [ -f "$rec" ]; then
    while read -r kind secs name; do
      case "$kind" in
        began)
          began=$secs
          phase_names+=("lock wait")
          phase_secs+=("$((began - asked))")
          ;;
        phase)
          phase_names+=("$name")
          phase_secs+=("$secs")
          ;;
        failed)
          phase_now=$name
          phase_began=$((SECONDS - secs))
          ;;
      esac
    done <"$rec"
  fi
  if [ -z "$began" ]; then
    echo "gate: the shared lock was not taken, so the tests and benches did not run"
    phase_now="lock wait"
    phase_began=$((SECONDS - ($(date +%s) - asked)))
    exit "$((status == 0 ? 1 : status))"
  fi
  echo "gate: lock released after holding it $(($(date +%s) - began)) s"
  [ "$status" -eq 0 ] || exit "$status"
}

if [ "$part" = locked ]; then
  began=$(date +%s)
  printf 'began %s\n' "$began" >"$gate_tmp/locked.rec"
  echo "gate: lock taken after waiting $((began - asked)) s"
  machine_checks
  exit 0
fi

phase fmt cargo fmt --all -- --check
# The shape budget (theseus-goa8; review 2's C1): the file ceiling is scripts/shape.sh, here; function length
# and complexity are clippy's lints, held by the next phase.
phase shape scripts/shape.sh
phase clippy cargo clippy --workspace --all-targets -q -- -D warnings
if [ "$mode" = inner ]; then
  # Every compile first, without the lock: the bench binaries, then the test binaries (what the suite runs).
  # Cargo links the binaries of the build it ran last, and the suite's cargo (in the locked part) links the
  # test build's, so this order leaves target/debug as the benches will find it.
  # (`env -u`: nextest warns that --no-run ignores the lane's NEXTEST_TEST_THREADS.)
  phase "bench build" bench_build
  phase "test build" env -u NEXTEST_TEST_THREADS cargo nextest run --workspace --no-run
  locked_checks
else
  machine_checks
fi
phase deny deny_check
phase web web_apps
phase cockpit cockpit
phase "web dist" web_dist
gate_done=1
summary ok
echo "gate: ok"
