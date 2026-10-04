#!/usr/bin/env bash
# The commit gate: every check must pass, and a failure stops the chain.
# Use as `scripts/gate.sh && git commit …` (a `;` anywhere in a hand-written
# chain once let an unformatted commit through).
#
# THE SHARED LOCK (theseus-rx91, theseus-lew7). The suite's timing-sensitive tests
# and the benches need the machine to themselves, so one gate's tests never land in
# another's bench: `~/.cache/theseus-gate.lock` (`$THESEUS_GATE_LOCK_FILE`). The
# gate takes it itself, only around the reader rule, the suite, and the benches,
# after every compile has run without it, so it never holds another gate up through
# its own fmt, clippy, and builds (minutes), and queues for the tests alone (about
# two minutes). One mode, everywhere: the outer mode, where the caller held the lock
# for the whole run, is gone. `THESEUS_GATE_LOCK=inner`, from when there were two,
# is accepted and changes nothing; `outer` is refused.
#
#   *** NEVER WRAP THE GATE IN `flock`, OR IN theseus-quiet.sh. ***
#   The wrapper would hold the lock that the gate then waits for, and the gate would
#   wait for itself forever. The gate checks, and refuses to start (exit 2) when a
#   process above it holds the lock.
#
# How it takes the lock: after its compile phases the gate runs itself again, as
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

# `--locked-part DIR ASKED LABEL` is the gate calling itself under the lock, not an option.
part=whole
if [ "${1:-}" = --locked-part ]; then
  [ "$#" -eq 4 ] || { echo "gate: --locked-part is the gate's own call, not an option" >&2; exit 2; }
  part=locked
  locked_dir=$2
  asked=$3
  label=$4
elif [ "$#" -ne 0 ]; then
  echo "usage: scripts/gate.sh   (no arguments; THESEUS_GATE_NO_BENCH=1, THESEUS_GATE_BENCH_ALLOWANCE=PERCENT)" >&2
  exit 2
fi
case "${THESEUS_GATE_LOCK:-inner}" in
  inner) ;;
  outer)
    echo "gate: THESEUS_GATE_LOCK=outer is gone (theseus-lew7): the gate takes the shared lock itself, around its tests and benches. Unset it, and run the gate with no flock or theseus-quiet.sh around it" >&2
    exit 2
    ;;
  *)
    echo "gate: THESEUS_GATE_LOCK is '$THESEUS_GATE_LOCK'; leave it unset: the gate takes the shared lock itself" >&2
    exit 2
    ;;
esac
# The busy allowance (theseus-lew7): the overage a timing budget may take, as a percentage of its limit, when the
# machine never settled before the bench (see settle()). Calibrated from the bench history: it covers 95% of the
# runs measured on a busy machine at normal priority, the code otherwise healthy. 0 judges every run strictly.
allowance="${THESEUS_GATE_BENCH_ALLOWANCE:-65}"
case "$allowance" in
  '' | *[!0-9]*)
    echo "gate: THESEUS_GATE_BENCH_ALLOWANCE is '$allowance'; it is a whole percentage (0: strict)" >&2
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
# A gate under a caller that holds the lock would wait for it forever: say so, and stop.
refuse_nested_lock() {
  local pid=$PPID stat
  while [ "$pid" -gt 1 ]; do
    if holds_lock "$pid"; then
      echo "gate: the gate takes the shared lock itself, but process $pid ($(cat "/proc/$pid/comm" 2>/dev/null)), above this gate, already holds $lock" >&2
      echo "gate: it would wait for itself forever. Run it with no flock (or theseus-quiet.sh) around it" >&2
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
if [ "$part" = whole ]; then refuse_nested_lock; fi

# Phase timings (theseus-goa8; review 2's C7). Each check runs under `phase`,
# which times it, and the gate prints the table before it ends, a failed run's
# too, naming the phase that failed and how long it ran: where the minutes go,
# and which phase to speed up. Whole seconds: a phase is seconds to minutes.
# Each run's table is appended to a log outside the tree, so the gate's own
# cost has a history (`$THESEUS_GATE_TIMES`, by default
# `~/.cache/theseus/gate-times.csv`: time, label, phase, seconds, status).
# The table covers both halves, with the wait for the lock as its own row
# (`lock wait`).
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

# The cockpit's protocol types, which a theseus-protocol test writes from the
# Rust ones (theseus-0g4): a type changed without its TypeScript fails here.
protocol_types() {
  if ! git diff --quiet -- cockpit/src/protocol.gen ||
    [ -n "$(git ls-files --others --exclude-standard -- cockpit/src/protocol.gen)" ]; then
    echo "cockpit/src/protocol.gen changed: the protocol's Rust types changed without their TypeScript; git add it"
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
  target/debug/theseus-sim bench lifecycle --runs 10 --check --record "$history" --label "$label" "${busy_args[@]}"
}
# A neighbour's sustained IO (a parallel build, a package install) stalls a
# start's fsyncs for seconds, and then the bench measures the neighbour, not
# Theseus. On 2026-10-01 an openclaw rotation held IO pressure near 50 %, and
# a restart's p95 was 2.3 s, where the same tree had passed at 41 ms. So each
# run first waits, for up to 2 minutes, until the kernel's IO and CPU
# pressure (PSI, `some avg10`) are under 10 % and 20 %, and it says what it
# saw. The budgets don't change. Without PSI it doesn't wait.
#
# Pressure alone misses one neighbour: many busy cores and nothing queued.
# Then each thread runs slower (all-core turbo, shared SMT siblings) with
# little CPU pressure. At 15:25 the same day every phase ran about 2x slow,
# untouched code included, at load 14 on 16 cores and CPU pressure 2 %;
# about 16:39 a run missed with the load near 20. So the wait also holds
# while the 1-minute load average is high. Its first bar was the core count,
# the cores oversubscribed (not half the cores, so that a long neighbouring
# build would not stall every gate), with the wait bounded at 5 minutes
# (theseus-611s).
#
# Since 2026-10-03 the bar is three quarters of the cores (12 of 16), and
# the wait 2 minutes (Eddie, 14:20: "Your pick is good"; theseus-lf1n). With
# the lanes' compilers no longer paused for the bench (theseus-lew7), the
# strict misses cluster at loads of 12 to 16: at normal priority a phase
# missed in 41 % of the runs at a load of 12 or more, against 10 % from 8 to
# 12 and 6 % under 8. The old bar called that band quiet and judged it
# strictly, so a second miss there failed a join. Now a gate there waits,
# then measures with the busy allowance, which was calibrated on that band.
# And since a busy machine now means the allowance, not a likely failure,
# the shorter wait keeps it from holding the shared lock for 5 minutes.
#
# When the 2 minutes pass with no quiet window, the bench measures a busy
# machine, and its timing budgets get the busy allowance (theseus-lew7):
# `--allowance` with `$THESEUS_GATE_BENCH_ALLOWANCE`, a percentage of each
# limit. A phase over its limit by no more than that passes, and the bench
# says so ("busy: allowance +N% applied …"); its history row records the
# strict verdict, a miss, with the allowance it passed on. A quiet window, a
# machine without PSI, and an allowance of 0 keep every budget strict, and a
# count (the turn bench's frames) never gets one.
busy_args=()
settle() {
  busy_args=()
  [ -r /proc/pressure/io ] && [ -r /proc/pressure/cpu ] || return 0
  local io cpu load waited=0
  local cores bar
  cores=$(nproc)
  bar=$(awk -v c="$cores" 'BEGIN {print c * 3 / 4}')
  while :; do
    io=$(awk '/^some/ {split($2, a, "="); print int(a[2])}' /proc/pressure/io)
    cpu=$(awk '/^some/ {split($2, a, "="); print int(a[2])}' /proc/pressure/cpu)
    load=$(awk '{print $1}' /proc/loadavg)
    if [ "$io" -lt 10 ] && [ "$cpu" -lt 20 ] && awk -v l="$load" -v b="$bar" 'BEGIN {exit !(l < b)}'; then break; fi
    if [ "$waited" -ge 120 ]; then
      if [ "$allowance" -eq 0 ]; then
        echo "lifecycle: still busy after 2 minutes (IO pressure $io %, CPU $cpu %, load $load on $cores cores, quiet under $bar); measuring anyway, strictly (THESEUS_GATE_BENCH_ALLOWANCE=0)"
      else
        busy_args=(--allowance "$allowance")
        echo "lifecycle: still busy after 2 minutes (IO pressure $io %, CPU $cpu %, load $load on $cores cores, quiet under $bar); measuring anyway, with the busy allowance: +$allowance% over a timing budget's limit"
      fi
      return 0
    fi
    sleep 5
    waited=$((waited + 5))
  done
  if [ "$waited" -gt 0 ]; then
    echo "lifecycle: waited $waited s for the machine to settle (IO pressure $io %, CPU $cpu %, load $load on $cores cores, quiet under $bar)"
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

# The jobs bench's L1 row (theseus-mll1; design §2.10): an L1 job's start, from
# the wrapper's spawn to the command's exec, 20 runs, p95 against §2.2's 25 ms.
# A real start (a clone, namespaces) can't run on a paused clock, and the suite
# runs under any load (its own tests beside it put a p95 at 1.1 s), so the suite
# measures the row and this bounds it, on the machine the lifecycle bench just
# settled, with the busy allowance when that settle found no quiet window.
# Skipped with it in a lane's gate; a miss reruns once, as its does.
jobs_bench() {
  if [ -n "${THESEUS_GATE_NO_BENCH:-}" ]; then
    echo "jobs: skipped (THESEUS_GATE_NO_BENCH: a lane's gate; the join's gate runs it)"
    return 0
  fi
  target/debug/theseus-sim bench jobs --class l1 --runs 20 --check "${busy_args[@]}" || {
    echo "jobs: an L1 start's p95 missed its target; running the bench once more"
    sync
    settle
    target/debug/theseus-sim bench jobs --class l1 --runs 20 --check "${busy_args[@]}"
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

# One npm script of the cockpit, its output kept in `$gate_tmp/<app>-<script>.log` (theseus-o8nk). A failure prints
# the log's last 40 lines, where the phase table alone said only "cockpit <- failed here".
npm_step() {
  local app=$1 script=$2 log="$gate_tmp/$1-$2.log"
  if (cd "$app" && npm run -s "$script") >"$log" 2>&1; then return 0; fi
  echo "gate: \`npm run $script\` failed in $app/; the last 40 lines of its output:"
  tail -n 40 "$log" | sed 's/^/  /'
  return 1
}
# The cockpit (theseus-45n5), the web UI at / since the Observatory retired (theseus-vm3n.6): lint, its pure
# modules' tests (node's own runner, theseus-9o5n), type-check, and build. Its build is not committed (several MB,
# new with each edit); the install builds it before the release build, and a binary without it says so at /. It runs
# before the compiles, so the suite's tests of / read this build: a debug theseusd reads it as it serves.
cockpit() {
  cockpit_modules && npm_step cockpit lint && npm_step cockpit test && npm_step cockpit build && cockpit_built
}
# The cockpit's modules (theseus-i5xo). Without them the phase used to skip, and the daemon's tests of / then took
# their not-built branch and passed, so a gate could pass never having served the real shell. Now a missing
# cockpit/node_modules is installed from package-lock.json: `npm ci --offline` from npm's cache, then `npm ci` over
# the network when the cache lacks a package. When neither can, or npm is missing, the gate fails saying so.
cockpit_modules() {
  local log="$gate_tmp/cockpit-ci.log"
  [ -d cockpit/node_modules ] && return 0
  if ! command -v npm >/dev/null 2>&1; then
    echo "gate: cockpit/node_modules is missing, and there is no npm to install it: install Node.js and npm, then"
    echo "gate: rerun the gate (the cockpit is built before the suite, whose tests of / read its build)"
    return 1
  fi
  echo "gate: cockpit/node_modules is missing; installing it from npm's cache (npm ci --offline)"
  (cd cockpit && npm ci --offline --no-audit --no-fund) >"$log" 2>&1 && return 0
  echo "gate: npm's cache lacks a package; installing over the network (npm ci)"
  (cd cockpit && npm ci --no-audit --no-fund) >>"$log" 2>&1 && return 0
  # A half-installed tree would be taken for an installed one next time.
  rm -rf cockpit/node_modules
  echo "gate: \`npm ci\` failed in cockpit/, so the cockpit is neither checked nor built; the last 40 lines of its output:"
  tail -n 40 "$log" | sed 's/^/  /'
  return 1
}
# The build the suite's tests of / read, where theseusd embeds it (rust-embed, `cockpit/dist/`).
cockpit_built() {
  [ -f crates/theseusd/cockpit/dist/index.html ] && return 0
  echo "gate: \`npm run build\` passed in cockpit/, but crates/theseusd/cockpit/dist/index.html, the shell the tests of /"
  echo "gate: read, is not there: vite.config.ts's outDir and theseusd's web.rs must name the same directory"
  return 1
}

# The shipped features (theseus-dr2x). scripts/build.sh builds only the five binaries an install ships, and the
# gate tests the whole workspace; cargo unifies a dependency's features over the packages it builds, so a crate
# outside the five that turns on a feature of a dependency the five link would give the tests a feature the install
# lacks, and the install would be a build the gate never compiled. This compares cargo's tree (it compiles nothing:
# about a second) of the five alone with the whole workspace's, and fails naming each package the five link that the
# workspace builds with other features, and the features it adds. scripts/AGENTS.md ("Features") says what to do.
features_of() {
  cargo tree -q -e normal,build --prefix none -f '{p} {f}' "$@" | sed -E 's/ \([^)]*\)//g' | sort -u
}
features() {
  local five=() p alone whole widened
  while read -r p; do five+=(-p "$p"); done < <(scripts/build.sh --shipped)
  alone="$(features_of "${five[@]}")"
  whole="$(features_of --workspace)"
  # A line is `name version features`. A package can be built twice, with two feature sets (a build dependency's
  # and a normal one's), so a line of the workspace's that the five lack is a widening, and the features named are
  # those no build of the package among the five has.
  widened="$(awk '
    NR == FNR { seen[$0] = 1; alone[$1 " " $2] = alone[$1 " " $2] "," $3; next }
    !($0 in seen) && (($1 " " $2) in alone) {
      split(alone[$1 " " $2], a, ","); split("", has); for (i in a) has[a[i]] = 1
      n = split($3, w, ","); add = ""
      for (i = 1; i <= n; i++) if (!(w[i] in has)) add = add (add == "" ? "" : ", ") w[i]
      print "  " $1 " " $2 ": the workspace builds it with " (add == "" ? "another set of its features (" $3 ")" : add)
    }' <(echo "$alone") <(echo "$whole"))"
  [ -z "$widened" ] && return 0
  echo "gate: the whole workspace widens the features of a package the five shipped binaries link, so an install"
  echo "gate: (scripts/build.sh, the five alone) builds it without them, and the gate tests a build no install has:"
  echo "$widened"
  echo "gate: name the feature in the shipped crate that links the package, as theseus-discord's manifest names"
  echo "gate: twilight-gateway's, or drop it from the crate outside the five (scripts/AGENTS.md, \"Features\")"
  return 1
}

# The test build, the last compile before the lock: every test binary the suite runs, and the debug binaries the
# benches run, `target/debug/theseusd`, `theseus-sim`, and the `theseus-index` the daemon starts beside itself,
# which cargo builds for the integration tests that run them. Until theseus-7ykr a `bench build` (`cargo build -p`
# of the five shipped binaries) ran before it, a second feature set of the shared crates (6 minutes cold here,
# seconds to a minute warm), and cargo links `target/debug/<bin>` from the build it ran last, so this one replaced
# them before any bench ran: the benches only ever ran this build's binaries. They have the workspace's features,
# and the features phase holds the five's to the same. Cargo's messages name every binary the build produced,
# fresh or relinked, and this fails naming a bench binary it did not, where the benches would run a stale one.
bench_bins=(theseusd theseus-sim theseus-index)
test_build() {
  local json="$gate_tmp/test-build.json" b exe
  # (`env -u`: nextest warns that --no-run ignores the lane's NEXTEST_TEST_THREADS.)
  env -u NEXTEST_TEST_THREADS cargo nextest run --workspace --no-run \
    --cargo-message-format json-render-diagnostics >"$json"
  for b in "${bench_bins[@]}"; do
    exe="$(grep -o "\"executable\":\"[^\"]*/debug/$b\"" "$json" | head -1 | cut -d'"' -f4 || true)"
    if [ -z "$exe" ] || ! [ "target/debug/$b" -ef "$exe" ]; then
      echo "gate: the test build did not build target/debug/$b${exe:+ (it built $exe)}, which the benches run,"
      echo "gate: so they would run a stale one. Cargo builds a package's binaries for its integration tests, and"
      echo "gate: none needs $b now: build it in test_build, after the tests' build"
      return 1
    fi
  done
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

# The checks that need the machine to themselves, in order: the locked part, which holds the
# lock across them, and whose binaries the compile phases built beforehand. The benches run
# the test build's `theseusd`, `theseus-sim`, and `theseus-index` (the workspace's features,
# which the features phase holds an install's to; see test_build).
machine_checks() {
  phase "reader rule" registry
  phase suite suite
  phase "protocol types" protocol_types
  compiled_under_lock
  phase lifecycle lifecycle_bench
  phase jobs jobs_bench
  phase turn turn_step
}

# Take the lock around machine_checks, by running the gate again as the locked part. The wait,
# the hold, and the part's phases go into this run's log and table.
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
# The five shipped binaries get every feature the tests' build gives them (theseus-dr2x).
phase features features
phase clippy cargo clippy --workspace --all-targets -q -- -D warnings
# The cockpit before the suite, which reads its build at / (theseus-vm3n.6).
phase cockpit cockpit
# Every compile first, without the lock: the test binaries (what the suite runs), and with them the binaries
# the benches run (theseus-7ykr).
phase "test build" test_build
locked_checks
phase deny deny_check
gate_done=1
summary ok
echo "gate: ok"
