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
# The tree it judges: its own checkout's, or THESEUS_GATE_ROOT's when the door runs main's copy from outside it.
cd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"
export PATH="$HOME/.cargo/bin:$PATH"
# No incremental cache (theseus-4vsrz): a lane is built once, gated once, and then re-sent or removed, so the cache is
# paid for and never reused (47 GB of one lane's 68 GB target, which took /home to 99% mid-gate). Set here, before the
# first cargo and for the whole run, because flipping it half way changes cargo's unit hashes and builds a second
# complete set of artifacts. The locked part and any gate this one starts inherit it. A caller's own value stands: an
# edit loop that wants the cache says CARGO_INCREMENTAL=1.
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"

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
# A phase that passed may name how, in the table: `scrub ok` and `scrub skipped` read differently at a glance.
phase_label=""
phase() {
  phase_now="$1"
  phase_began=$SECONDS
  phase_label=""
  shift
  "$@"
  phase_names+=("${phase_label:-$phase_now}")
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
# THE TREE THE GATE JUDGES (theseus-4vsrz). The gate compiles one tree and then runs the tests of whatever tree is there,
# so a tree that moved in between (a branch checked out in it, a reset, an edit) gave a failure that looked exactly like a
# real one: on 2026-11-07 four red tests on a join's worktree that passed 15 of 15 on the same commit. The gate reads
# the tree's identity before its first compile and checks it again before the lock, when it takes the lock (the wait can
# be hours), before the suite, and at the end. A tree that moved ABORTS the gate with exit status 75 and a message of
# its own, never a test failure; and a gate that failed is checked too, since a failure on a moved tree is no verdict.
# The identity is the commit, a digest of the edits beyond it (tracked and untracked: the digest scripts/tree-stamp.sh
# prints, minus one path), and the length and digest of HEAD's reflog, which a checkout away and back writes twice: a tree
# detached and restored in the window is the same commit and the same files, and not the tree the compile and the tests
# between them ran on. It is computed here and not by that script, because the gate must not depend on a file of the tree
# it judges: a branch cut before the script, and the throwaway repositories the guard's own tests run the gate in, lack it.
# Blind spots, deliberately: an edit made and undone in the window moves no ref and leaves no mark, and
# cockpit/src/protocol.gen is left out because the protocol phase rewrites it itself and judges it itself.
tree_moved_status=75
tree_unseen=(':(exclude)cockpit/src/protocol.gen')
tree_identity() {
  local head digest log
  head="$(git rev-parse HEAD 2>/dev/null || echo none)"
  digest="$({
    GIT_OPTIONAL_LOCKS=0 git diff HEAD --binary -- . "${tree_unseen[@]}" 2>/dev/null || true
    git ls-files -z --others --exclude-standard -- . "${tree_unseen[@]}" 2>/dev/null | xargs -0 -r sha256sum || true
  } | sha256sum | cut -c1-16)"
  # sha256 of nothing, cut: the tree is what HEAD says.
  [ "$digest" != "$(printf '' | sha256sum | cut -c1-16)" ] || digest=clean
  log="$(git rev-parse --git-path logs/HEAD 2>/dev/null || true)"
  if [ -f "$log" ]; then
    echo "$head $digest reflog:$(wc -l <"$log" | tr -d ' '):$(git hash-object --no-filters "$log" | cut -c1-12)"
  else
    echo "$head $digest reflog:none"
  fi
}
# The paths with a state, to name the ones that changed when a tree moved.
tree_paths() {
  GIT_OPTIONAL_LOCKS=0 git status --porcelain -uall -- . "${tree_unseen[@]}" 2>/dev/null || true
}
tree_begin() {
  tree_identity >"$gate_tmp/tree.base"
  tree_paths >"$gate_tmp/tree.paths"
}
# What moved, said so it cannot be taken for a test failure: the tree built, the tree found, and what differs.
tree_moved_report() {
  local where=$1 now=$2 base b_head b_digest b_log n_head n_digest n_log b_len
  local logfile
  base="$(cat "$gate_tmp/tree.base")"
  read -r b_head b_digest b_log <<<"$base"
  read -r n_head n_digest n_log <<<"$now"
  echo "gate: ABORTED, THE TREE MOVED ($where): this is NOT a test failure and no verdict on the code"
  echo "gate:   built: $label = $base"
  echo "gate:   found: $(git rev-parse --abbrev-ref HEAD 2>/dev/null) $(git describe --always --dirty 2>/dev/null) = $now"
  [ "$b_head" = "$n_head" ] || echo "gate:   HEAD moved from ${b_head:0:10} to ${n_head:0:10}"
  if [ "$b_digest" != "$n_digest" ]; then
    echo "gate:   the edits beyond HEAD changed ($b_digest to $n_digest); paths whose state changed (< at the start, > now):"
    { diff "$gate_tmp/tree.paths" <(tree_paths) | grep '^[<>]' | head -20 | sed 's/^/gate:     /'; } || true
  fi
  if [ "$b_log" != "$n_log" ]; then
    echo "gate:   HEAD's reflog changed ($b_log to $n_log): something checked out, reset or committed in this tree; since the start:"
    logfile="$(git rev-parse --git-path logs/HEAD 2>/dev/null || true)"
    b_len="$(cut -d: -f2 <<<"$b_log")"
    { [ -f "$logfile" ] && tail -n "+$((${b_len//[!0-9]/0} + 1))" "$logfile" | cut -f2- | head -10 | sed 's/^/gate:     /'; } || true
  fi
  echo "gate: what was compiled and what was tested were not all of one tree, so nothing above is a pass or a failure of it."
  echo "gate: do not check out, reset or edit a tree while a gate runs in it; rerun the gate (exit status $tree_moved_status)"
}
# Is the tree not the one the gate began with? Prints the report and says yes (status 0) when it is not. A tree whose
# identity cannot be read now counts as moved: what cannot be seen has not been shown to hold.
tree_moved() {
  local now
  [ -s "$gate_tmp/tree.base" ] || return 1
  now="$(tree_identity 2>/dev/null || echo unreadable)"
  [ "$now" != "$(cat "$gate_tmp/tree.base")" ] || return 1
  tree_moved_report "$1" "$now"
}
tree_check() {
  if tree_moved "$1"; then exit "$tree_moved_status"; fi
}

# A gate that a signal ends reads `$?` as 0 in this trap, so `gate_done` says it did not finish.
gate_done=""
on_exit() {
  local status=$?
  if [ "$part" = locked ]; then
    [ "$status" -eq 0 ] || not_reached
    save_locked_record
    return
  fi
  # A gate that failed is checked once more: on a tree that moved, the red is not a verdict.
  if [ "$status" -ne 0 ] && [ "$status" -ne "$tree_moved_status" ] && tree_moved "the gate failed, and its tree had moved"; then
    status=$tree_moved_status
  fi
  rm -rf "$gate_tmp"
  if [ "$status" -eq "$tree_moved_status" ]; then
    summary aborted
    echo "gate: ABORTED${phase_now:+ in $phase_now}: the tree moved, so this run is no verdict, pass or fail"
    exit "$status"
  fi
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
#
# What the suite does not run is said first (theseus-4vsrz). The suite passes no `-E`, so a `default-filter` in
# `.config/nextest.toml` is the only filter there is, and it skips its tests in every run: nextest counts them as
# "skipped" with the #[ignore]d ones, so a stale line (uncommitted, kept in a lane from an earlier run: four tests
# skipped from every gate on 2026-11-07, gate green) left nothing in the result to read. A gate never refuses one, since
# the ARM runner sets its known skips this way; it names each line, with its profile and place, and whether HEAD has it
# or the working tree alone does, says so again beside the suite's summary, and calls the phase `suite filtered`.
suite_filters() {
  local cfg=.config/nextest.toml found profile lineno text where
  found="$(awk '
    /^[[:space:]]*\[profile\.[A-Za-z0-9_-]+\]/ { p = $0; sub(/^[[:space:]]*\[profile\./, "", p); sub(/\].*$/, "", p); next }
    /^[[:space:]]*\[/ { p = ""; next }
    p != "" && /^[[:space:]]*default-filter[[:space:]]*=/ { v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); printf "%s\t%d\t%s\n", p, FNR, v }
  ' "$cfg" 2>/dev/null || true)"
  if [ -z "$found" ]; then
    echo "gate: suite filters: none (no default-filter in $cfg, and the gate passes no -E): every test the workspace builds runs, but the #[ignore]d ones"
    return 0
  fi
  echo "gate: suite filters: ACTIVE, a default-filter skips tests in every run below (nextest counts them as \"skipped\", with the #[ignore]d ones):"
  while IFS=$'\t' read -r profile lineno text; do
    where="only in the working tree, not in HEAD"
    if git show "HEAD:$cfg" 2>/dev/null | grep -qxF -- "$(sed -n "${lineno}p" "$cfg")"; then where="committed in HEAD"; fi
    echo "gate:   profile $profile, $cfg:$lineno, $where: $text"
  done <<<"$found"
  echo "gate:   this run's profile: ${NEXTEST_PROFILE:-default}; a stale line goes with: git checkout -- $cfg"
  phase_label="suite filtered"
}
suite() {
  local log="$gate_tmp/suite.log"
  suite_filters
  cargo nextest run --workspace --no-fail-fast 2>&1 | tee "$log"
  if [ "$phase_label" = "suite filtered" ]; then
    echo "gate: the suite above ran under a default-filter (suite filters, above): its \"skipped\" count includes the tests the filter excluded"
  fi
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
# The check writes them itself first, by running that one test, so it judges
# the Rust as it stands and not the files as the last run left them: before
# theseus-5epuo it only diffed, ran ahead of the suite that writes them, and a
# stale file passed this gate and dirtied the next one. The test is built
# (test_build); it takes a second. A run of no tests fails: nextest exits 0
# for a filter that matches nothing, so the count is read from its summary.
protocol_types() {
  local log="$gate_tmp/protocol-types.log"
  cargo nextest run --workspace --no-fail-fast \
    -E 'package(theseus-protocol) & test(=ts::the_cockpits_types_are_generated_from_the_rust_ones)' 2>&1 | tee "$log"
  if ! grep -qE 'Summary .* 1 test run: 1 passed' "$log"; then
    echo "gate: the generator test did not run once and pass (theseus-5epuo): the files below were not regenerated from the Rust types"
    exit 1
  fi
  if ! git diff --quiet -- cockpit/src/protocol.gen ||
    [ -n "$(git ls-files --others --exclude-standard -- cockpit/src/protocol.gen)" ]; then
    echo "cockpit/src/protocol.gen changed: the protocol's Rust types changed without their TypeScript; git add it"
    git status --short -- cockpit/src/protocol.gen
    exit 1
  fi
}

# The lifecycle budgets of §9 (FAST, theseus-qa0): cold start, clean shutdown
# with a job running, SIGKILL then restart, a binary swap with the job's
# wrapper adopted, 10 runs each on an empty store, the median against the
# budget plus the measured noise margin (the p95 of ten samples is the slowest
# of them, so it was the machine's worst moment and not the code that decided
# a phase: theseus-ao9eq); and restore, measured, whose restored store must
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
# the wait 2 minutes (the owner, 14:20: "Your pick is good"; theseus-lf1n). With
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

# The size check (theseus-2m7jg), scripts/size-step.sh: it judges a release binary only when scripts/build.sh built it
# from this tree (the stamp it wrote), else says "skipped: no release build of <commit>" and costs nothing. It fails on
# growth of more than 1 MB over the last recorded size of that profile, and prints §9's 60 MB miss without failing on
# it. A count, not a time: it runs in a lane's gate too.
size_step() {
  scripts/size-step.sh "$history" "$label"
}

# The supply-chain check, offline (theseus-goa8; review 2's SC2). Licences, bans, and
# sources need no network, and advisories are read from the database as its last fetch
# left it, so an outage of GitHub or crates.io can no longer fail a commit: 27e1237
# met a crate yanked between two gates and had to bump Cargo.lock mid-step.
# The gate's database is a pin (theseus-4vsrz): only the owner's `cargo deny fetch db`
# moves it, so the result depends on the tree and not on the day. `scripts/deny-daily.sh`
# fetches into a database of its own, files an issue for a finding, and never touches
# this one; the gate says when its database is old. A
# machine that has never fetched has no database: the gate then checks the rest and
# says that advisories were not, since a gate that fetched here would fail on an outage.
deny_check() {
  local db age
  db="$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/advisory-dbs/advisory-db-*/ 2>/dev/null | head -1 || true)"
  if [ -z "$db" ]; then
    echo "deny: no advisory database in ${CARGO_HOME:-~/.cargo}/advisory-dbs: advisories NOT checked (\`cargo deny fetch db\` makes the gate's pin)"
    cargo deny --offline --log-level error check bans licenses sources
    return
  fi
  if [ -f "${db}.git/FETCH_HEAD" ]; then
    age=$((($(date +%s) - $(stat -c %Y "${db}.git/FETCH_HEAD")) / 86400))
    [ "$age" -le 7 ] || echo "deny: the advisory database is $age days old; it is the gate's pin, and \`cargo deny fetch db\` moves it (scripts/deny-daily.sh checks a fresh one apart from it)"
  fi
  # A red here can come from the database moving while nobody touched the tree (it is the gate's pin: only a fetch
  # into it moves it, and deny-daily.sh keeps a database of its own). Say so, and say whose call it is: the gate
  # itself never fetches, never ignores an advisory.
  cargo deny --offline --log-level error check || {
    local rc=$?
    if cargo deny --offline --log-level error check bans licenses sources >/dev/null 2>&1; then
      echo "deny: ONLY advisories failed; bans, licences and sources are clean. The advisory database is $(deny_db_age "$db") days old; it is the gate's pin and moves only when something fetches into it (the owner's \`cargo deny fetch db\`), not with a branch: read the advisory ids above before blaming this change. Owner action: ignore each id in deny.toml with its reason, or bump the crate."
    fi
    return "$rc"
  }
}
# Whole days since the advisory database at $1 was last fetched ("unknown" without a FETCH_HEAD).
deny_db_age() {
  if [ -f "${1}.git/FETCH_HEAD" ]; then
    echo $((($(date +%s) - $(stat -c %Y "${1}.git/FETCH_HEAD")) / 86400))
  else
    echo unknown
  fi
}

# The keel guard (theseus-pw1q.1; the owner's decision on self-improvement, 2026-10-09): a range that deletes or
# loosens a test, a budget or a ceiling fails here, before any compile, unless a commit in it signed by a key in
# scripts/keel-signers (at the base) acks each finding with a `Keel:` trailer; scripts/keel-guard.py says the rules.
# The range is the branch's (merge-base with main..the working tree), or a join's merge whole (HEAD^1..HEAD);
# THESEUS_KEEL_BASE=<rev> names its base. The guard that judges is the base's own copy when the base has one, so a
# branch cannot loosen the guard that judges it (a change to it is itself a finding), and the range's base is found
# here, as the guard finds it, not by the branch's copy. There is no switch to skip it.
keel_base=""
# Main for the keel: origin/main, or the local main when there is no origin/main (theseus-pw1q.18). A join's local main
# is ahead of origin/main by what this gate judges (its merge, and any fix committed after it), so taking it judged
# HEAD..HEAD, nothing, whenever a fix followed the merge (the people join's gate, 2026-10-09).
keel_main() {
  git rev-parse --verify -q 'origin/main^{commit}' || git rev-parse --verify -q 'main^{commit}' || {
    echo "keel: no main and no origin/main to judge against: set THESEUS_KEEL_BASE=<rev>" >&2
    return 1
  }
}
keel_range_base() {
  local main remote first
  if [ -n "${THESEUS_KEEL_BASE:-}" ]; then
    git rev-parse --verify -q "$THESEUS_KEEL_BASE^{commit}" || {
      echo "keel: no commit $THESEUS_KEEL_BASE (THESEUS_KEEL_BASE)" >&2
      return 1
    }
    return 0
  fi
  main="$(keel_main)" || return 1
  remote="$(git rev-parse --verify -q 'origin/main^{commit}' || true)"
  # A join's merge, not yet on origin/main, is judged whole; one already there was judged at its join, so a branch
  # cut at it is judged from it.
  if git rev-parse --verify -q 'HEAD^2' >/dev/null; then
    first="$(git rev-parse 'HEAD^1')"
    if git merge-base --is-ancestor "$first" "$main" &&
      ! { [ -n "$remote" ] && git merge-base --is-ancestor HEAD "$remote"; }; then
      echo "$first"
      return 0
    fi
  fi
  git merge-base HEAD "$main"
}
keel_guard() {
  command -v python3 >/dev/null || { echo "keel: no python3 to run scripts/keel-guard.py"; return 1; }
  local guard=scripts/keel-guard.py
  keel_base="$(keel_range_base)"
  if git cat-file -e "$keel_base:scripts/keel-guard.py" 2>/dev/null; then
    git show "$keel_base:scripts/keel-guard.py" >"$gate_tmp/keel-guard.py"
    guard="$gate_tmp/keel-guard.py"
  fi
  python3 "$guard" --base "$keel_base"
}
# The door (theseus-pw1q.18; the keel-guard review's finding 3). A join runs the merge's gate.sh, which is the
# branch's, so a branch that dropped its keel phase, or any check, was judged by its own weakened copy. Now the gate
# that judges is main's: when this copy is not main's (origin/main's, as the keel takes main) and main's has the door,
# main's copy runs first, from outside the tree, and judges it; this copy then runs on only when the branch itself
# changes gate.sh, so its own new checks are proved before they land. Main's copy started from outside the tree (by
# a caller that trusts nothing in it: scripts/AGENTS.md, "The door") runs the tree's own copy after it the same way.
# THESEUS_GATE_DOOR is the door's word to the copy it starts (judge: main's, which ends there; own: the tree's,
# after main's passed), not an option.
door_after=""
door_done=""
keel_door() {
  local main theirs mine copy
  [ "${THESEUS_GATE_DOOR:-}" != judge ] || return 0
  main="$(keel_main 2>/dev/null)" || return 0
  theirs="$(git rev-parse -q --verify "$main:scripts/gate.sh" 2>/dev/null)" || return 0
  # Not `git cat-file … | grep -q`: under pipefail, grep's early exit can SIGPIPE cat-file and fail the pipe.
  if [[ "$(git cat-file blob "$theirs")" != *THESEUS_GATE_DOOR* ]]; then
    echo "door: main's gate.sh has no door yet, so this copy judges"
    return 0
  fi
  mine="$(git hash-object "$self")"
  if [ "$mine" = "$theirs" ]; then
    if [ "$(git hash-object scripts/gate.sh)" != "$theirs" ] && keel_gate_changed; then
      door_after=1
      echo "door: main's copy judges; this tree changes scripts/gate.sh, so its own copy runs after this gate passes"
    fi
    return 0
  fi
  [ "${THESEUS_GATE_DOOR:-}" != own ] || return 0
  # Under a scripts/ of its own, so what knows a gate by `scripts/gate.sh --locked-part` in its argv (review-step.sh,
  # lock_users' readers) knows this one too.
  copy="${TMPDIR:-/tmp}/theseus-gate-door-$theirs/scripts/gate.sh"
  if [ ! -f "$copy" ] || [ "$(git hash-object "$copy")" != "$theirs" ]; then
    mkdir -p "${copy%/gate.sh}" &&
      git cat-file blob "$theirs" >"$copy.$$" && chmod 755 "$copy.$$" && mv -f "$copy.$$" "$copy"
  fi
  echo "door: this gate.sh is not main's (${main:0:10}); main's judges this tree first ($copy)"
  if ! THESEUS_GATE_DOOR=judge THESEUS_GATE_ROOT="$PWD" "$copy"; then
    echo "door: main's gate failed on this tree"
    return 1
  fi
  if keel_gate_changed; then
    echo "door: main's gate passed; this branch changes scripts/gate.sh, so its own copy runs now"
    return 0
  fi
  echo "door: main's gate passed, and this branch does not change scripts/gate.sh"
  door_done=1
}
# Does the range change gate.sh (the working tree's against the keel's base)?
keel_gate_changed() {
  local base
  base="$(keel_range_base)" || return 1
  ! git diff --quiet "$base" -- scripts/gate.sh
}
# Main's copy started from outside the tree, done: the tree's own changed copy runs now, after main's passed.
door_own() {
  echo "door: this tree changes scripts/gate.sh; main's gate passed, so its own copy runs now"
  THESEUS_GATE_DOOR=own "$PWD/scripts/gate.sh"
}

# The guard's planted-erosion suite (about 40 throwaway repositories, signed acks among them: seconds), run when the
# guard, its suite or this gate (whose keel range and door it tests) differs from the range's base, which a change to
# any is; otherwise it would prove again what the base's gate proved.
keel_tests() {
  local own=(scripts/keel-guard.py scripts/test_keel_guard.py scripts/gate.sh)
  if git diff --quiet "$keel_base" -- "${own[@]}" && [ -z "$(git ls-files --others --exclude-standard -- "${own[@]}")" ]; then
    echo "keel tests: the guard is the base's; its suite ran there"
    return 0
  fi
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q scripts/test_keel_guard.py
}

# The name scrub over what the branch adds (theseus-ev3ir): a `.cases.toml` is a case file, not a pack, so the pack-text
# test never read the one two sensitive names got through in. The scrubber is outside the repo (its patterns are the
# names; see scripts/scrub-added.sh), and only the WSL host has it. Where it is PRESENT the phase runs and must pass;
# where it is ABSENT (the daemon's L1 self-build view, the ARM and cloud hosts) the phase SKIPS LOUDLY and the gate
# goes on, and the table says `scrub skipped`: a join gate showing that is not evidence for a push. The guard is the
# scrub the conductor runs at the JOIN on the WSL host (scripts/scrub-branch.sh), over the staged merge; this phase is
# the convenience that catches a hit earlier. THESEUS_GATE_NO_SCRUB=1 skips it by choice (the ARM runner sets it).
# The judge is MAIN's scripts/scrub-added.sh when main has one (as the guard phase takes main's copy): a branch that edits
# the driver is not judged by its own copy. The base is the merge base of origin/main and HEAD; a staged join merge is
# judged whole, as a working-tree diff. A tree that adds nothing (main's own tip) has nothing to scan: exit 3, accepted.
# The exit code is read with `|| code=$?`, because under errexit a bare call stops the gate before `$?` is read.
scrub_added() {
  local code=0 driver=scripts/scrub-added.sh main
  if [ -n "${THESEUS_GATE_NO_SCRUB:-}" ]; then
    echo "gate: scrub SKIPPED (THESEUS_GATE_NO_SCRUB) — the added lines were NOT scrubbed for names; a join must be gated on the WSL host"
    phase_label="scrub skipped"
    return 0
  fi
  main="$(git rev-parse --verify -q 'origin/main^{commit}' || git rev-parse --verify -q 'main^{commit}' || true)"
  if [ -n "$main" ] && git cat-file -e "$main:scripts/scrub-added.sh" 2>/dev/null; then
    git show "$main:scripts/scrub-added.sh" >"$gate_tmp/scrub-added.sh"
    driver="$gate_tmp/scrub-added.sh"
  fi
  bash "$driver" || code=$?
  case "$code" in
    0) phase_label="scrub ok" ;;
    3)
      echo "gate: scrub: nothing added since the merge base of origin/main: nothing to scan"
      phase_label="scrub empty"
      code=0
      ;;
    4)
      echo "gate: scrub SKIPPED — no scrubber on this host: the added lines were NOT scrubbed for names; a join must be gated on the WSL host, where it is"
      phase_label="scrub skipped"
      code=0
      ;;
  esac
  return "$code"
}
# The phase: the driver, then the driver's own tests (a scrubber that stopped finding a planted fake name fails here,
# not just at the next branch's review), which need the scrubber too, so a skipped phase runs neither.
scrub_phase() {
  scrub_added
  [ "$phase_label" != "scrub skipped" ] || return 0
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q scripts/test_scrub_added.py
}

# This script's own tests (theseus-4vsrz): scripts/test_gate_tree.py runs the real gate in a throwaway repository, its
# cargo, node and npm stand-ins, and moves the tree under it, to prove that a gate whose tree moved aborts (exit 75)
# and that its cargo runs without an incremental cache. About 2 s. They run when this tree differs from main in the
# script or the tests, which is when they can have changed; otherwise main's gate ran them and the phase costs a
# diff. main is origin/main, or the local main without one.
gate_tests() {
  local own=(scripts/gate.sh scripts/test_gate_tree.py) main
  main="$(git rev-parse --verify -q 'origin/main^{commit}' || git rev-parse --verify -q 'main^{commit}' || true)"
  if [ -n "$main" ] && git diff --quiet "$main" -- "${own[@]}" && [ -z "$(git ls-files --others --exclude-standard -- "${own[@]}")" ]; then
    echo "gate tests: gate.sh and its tests are main's; its gate ran them"
    return 0
  fi
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q scripts/test_gate_tree.py
}

# One npm script of the cockpit, its output kept in `$gate_tmp/<app>-<script>.log` (theseus-o8nk). A failure prints
# the log's last 40 lines, where the phase table alone said only "cockpit <- failed here". A passing `test` tees
# node's own summary (`ℹ tests N`, `ℹ pass N`, ...) into the gate's own output, which `arm.sh tail` keeps even
# after the lane that ran it is gone (review L, L-3: a bare rc cannot tell 918 from any other silent shortening).
npm_step() {
  local app=$1 script=$2 log="$gate_tmp/$1-$2.log"
  if (cd "$app" && npm run -s "$script") >"$log" 2>&1; then
    [ "$script" = test ] && grep '^ℹ ' "$log" | sed "s/^/gate: $app $script: /"
    return 0
  fi
  echo "gate: \`npm run $script\` failed in $app/; the last 40 lines of its output:"
  tail -n 40 "$log" | sed 's/^/  /'
  return 1
}
# The cockpit's node, pinned (theseus-selom, `NODE_PIN` in self_build.rs). The L1 self-build jail
# binds exactly that one version read-only, but `/usr` is always bound too, so on a host whose
# system node is new enough for `command -v npm` to pass, the phase used to run on whatever node
# happened to be first on PATH and say nothing about it: a green gate on the wrong toolchain
# (review L, L-2). The pin is only a pin if every gate checks it, not only the jail's.
cockpit_node_pin() {
  local pin
  pin=$(sed -n 's/^pub const NODE_PIN: &str = "\(.*\)";/\1/p' crates/theseus-core/src/sandbox/self_build.rs)
  if [ -z "$pin" ]; then
    echo "gate: could not read NODE_PIN out of crates/theseus-core/src/sandbox/self_build.rs"
    return 1
  fi
  if ! command -v node >/dev/null 2>&1; then
    echo "gate: no \`node\` on PATH; the cockpit is pinned to v$pin: nvm install $pin"
    return 1
  fi
  local running
  running=$(node -v)
  if [ "$running" != "v$pin" ]; then
    echo "gate: node is $running on PATH, but the cockpit is pinned to v$pin: nvm install $pin, then rerun the gate"
    return 1
  fi
}
# The cockpit (theseus-45n5), the web UI at / since the Observatory retired (theseus-vm3n.6): lint, its pure
# modules' tests (node's own runner, theseus-9o5n), type-check, and build. Its build is not committed (several MB,
# new with each edit); the install builds it before the release build, and a binary without it says so at /. It runs
# before the compiles, so the suite's tests of / read this build: a debug theseusd reads it as it serves.
cockpit() {
  cockpit_node_pin && cockpit_modules && npm_step cockpit lint && npm_step cockpit test && npm_step cockpit build && cockpit_built
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
  n="$(cat "$gate_tmp/registry.log" "$gate_tmp/protocol-types.log" "$gate_tmp/suite.log" 2>/dev/null | grep -c '^ *Compiling ' || true)"
  if [ "$n" -gt 0 ]; then
    echo "gate: NOTE: $n crate(s) were compiled under the lock: the tree changed after the compile phases (rerun the gate before you commit), or they built something other than the tests run"
  fi
}

# What a failed locked part never ran: a phase that fails ends the gate, so the ones after it have no verdict,
# which the table alone does not say. `machine_order` is machine_checks's order, and the two are kept together.
machine_order=("reader rule" "protocol types" lifecycle jobs turn size suite)
not_reached() {
  local name seen="" left=()
  for name in "${machine_order[@]}"; do
    if [ -n "$seen" ]; then left+=("$name"); fi
    if [ "$name" = "$phase_now" ]; then seen=1; fi
  done
  # machine_order must stay in step with machine_checks: a phase in neither
  # list silently prints no NOT RUN line at all (theseus-zsddv).
  case "$phase_now" in
    "" | "lock wait") ;;
    *) [ -n "$seen" ] || echo "gate: BUG, the failed phase \"$phase_now\" is not in machine_order, so the NOT RUN list is missing" ;;
  esac
  if [ "${#left[@]}" -gt 0 ]; then
    echo "gate: NOT RUN, no verdict, after the failure in $phase_now: ${left[*]}"
  fi
  compiled_under_lock
}

# The checks that need the machine to themselves, in order: the locked part, which holds the
# lock across them, and whose binaries the compile phases built beforehand. The benches run
# the test build's `theseusd`, `theseus-sim`, and `theseus-index` (the workspace's features,
# which the features phase holds an install's to; see test_build).
#
# The suite is last: a phase that fails ends the gate, so the one that fails most (a timing test
# under load) must not hide the checks after it, three of which a lane's gate makes (protocol
# types, turn, size; theseus-zsddv). `compiled_under_lock` follows it, since it reads the suite's log.
machine_checks() {
  phase "reader rule" registry
  phase "protocol types" protocol_types
  phase lifecycle lifecycle_bench
  phase jobs jobs_bench
  phase turn turn_step
  phase size size_step
  tree_check "before the suite"
  phase suite suite
  compiled_under_lock
}

# Take the lock around machine_checks, by running the gate again as the locked part. The wait,
# the hold, and the part's phases go into this run's log and table.
locked_checks() {
  local asked status=0 began="" kind secs name rec="$gate_tmp/locked.rec" users held queued
  tree_check "before the lock: the compiles are done"
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
  THESEUS_GATE_ROOT="$PWD" flock -o "$lock" "$self" --locked-part "$gate_tmp" "$asked" "$label" || status=$?
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
  tree_check "when it took the lock, after waiting $((began - asked)) s"
  machine_checks
  exit 0
fi

# The tree's identity, before anything runs: every check below is a check of this tree (THE TREE THE GATE JUDGES).
tree_begin
# The door, then the keel: neither needs a build, so a branch that removed what would fail it stops in seconds.
phase door keel_door
if [ -n "$door_done" ]; then
  gate_done=1
  summary ok
  echo "gate: ok (main's gate judged this tree)"
  exit 0
fi
phase keel keel_guard
phase "keel tests" keel_tests
phase fmt cargo fmt --all -- --check
# Names a branch adds, before the heavy compiles: it fails in seconds (theseus-ev3ir). After fmt, which is the
# phase the guard's own gate tests expect to fail next in a throwaway repository.
phase scrub scrub_phase
# This script's own tests, after fmt like the scrub (gate_tests says when they run).
phase "gate tests" gate_tests
# The shape budget (theseus-goa8; review 2's C1): the file ceiling is scripts/shape.sh, here; function length
# and complexity are clippy's lints, held by the next phase.
phase shape scripts/shape.sh
# The five shipped binaries get every feature the tests' build gives them (theseus-dr2x).
phase features features
phase clippy cargo clippy --workspace --all-targets -q -- -D warnings
# jev-probe and its module are behind the judge crate's `probe` feature, which the workspace build leaves off.
phase clippy-probe cargo clippy -p theseus-judge --features probe --all-targets -q -- -D warnings
# The cockpit before the suite, which reads its build at / (theseus-vm3n.6).
phase cockpit cockpit
# Every compile first, without the lock: the test binaries (what the suite runs), and with them the binaries
# the benches run (theseus-7ykr).
phase "test build" test_build
# bench/recall's Python tests (a cache-read price drift went unseen without them); Python >= 3.11, or it says it
# skipped. AFTER the test build, never before it (theseus-a3woz): test_drive.py drives target/debug's theseus,
# theseusd, theseus-index and theseus-sim and skips itself when one is missing, so from ahead of the build a warm
# lane tested the previous head's binaries (on 2026-11-03 a renamed config key left the stale daemon failing its
# TOML parse). test_gate_order.py holds this order.
phase "recall tests" scripts/recall-tests.sh
locked_checks
phase deny deny_check
if [ -n "$door_after" ]; then phase "own gate.sh" door_own; fi
tree_check "at the end"
gate_done=1
summary ok
echo "gate: ok"
