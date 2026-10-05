# Cloud report: kernel-fixes (theseus-g11i, theseus-jnnj, theseus-m9iy, theseus-oqxw)

Branch `cloud/20261005-kernel-fixes`, cut from `5aa77157` (main as cloned, store format 16, unchanged here). Started
08:36 UTC, report at 10:20 UTC. Four steps, one commit each, each gated and pushed:

| Step | Issue | Commit |
|---|---|---|
| 1. The deadline's stop of a whole tree | theseus-g11i | `2d4c3afc` |
| 2. A late completion's second taker | theseus-jnnj | `389ed841` |
| 3. An earlier process's in-process calls | theseus-m9iy | `1f1dacdf` |
| 4. A nested lock | theseus-oqxw | `ba6455a5` |

No store format change, no new dependency, no protocol type, no config key. `tests_frames.rs`'s golden is
byte-identical after every step (no line moved). kernel.rs is at 3,008 of its 3,030 ceiling (+16: a trait method, a
field and its three initializers, two calls, and step 2's closure).

## Differences between the brief and the code

- **Spawn without fork, and the cgroup:** as the brief says. Step 1's emptiness test depends on them: since
  theseus-ypqg a wrapper's only children are its command and the orphans it inherits as a subreaper, so `waitpid`
  answering ECHILD means its job has no process left. The cgroup stop keeps `populated 0` as its emptiness test
  (reliable) and ignores the reap's answer; the tree stop that follows it uses the answer.
- **`harness::drive` sits at clippy's 100-line limit.** Step 3's call is one line there, and its body is
  `Core::mark_earlier_calls` in `rpc/driver.rs`.
- **What waits on a provider call after a restart (m9iy).** The brief says the requeued turn waits on it. I found
  no code that waits on it in the turn: `park` leaves provider calls out of `Wake::Actions`, and a requeued turn
  plans a new call. What the stale call held was the execution's `outstanding` entry (the session reads as having
  work out) and its reservation. Marking it settles both: it leaves `outstanding`, its result is queued (and
  `take_results` takes it and skips it, as it skips any provider call), and an execution parked on it (only a
  test can make one) is queued.
- **`reset_budget` frees nothing of such a call.** Before the mark, its reservation is in `reserved_micros`. After
  it, `held_unknown_micros`. A reset zeroes `spent_micros` only, and `available_after_reset` subtracts both. So the
  reservation an earlier process's call left is held for good, as any unknown is: the call can never be resolved,
  since no completion will come. The owner may want a rule for an unknown whose process is gone (release it, or
  book it as spent, at the mark). I left it as the brief's §3.16 says (held, never released).
- **An overdue in-process call at startup** (the daemon down past its deadline) is still marked by startup's own
  reconcile, before serving (`overdue_no_evidence`), as before. That write predates this change. I left it,
  since it is outside the brief.

## Step 1: theseus-g11i, the deadline's stop of a whole tree

**Found.** Each case now prints, on a failure, the stop's verdict (`detail.stop`, or the cancel's `Verdict`), the
wrapper's pid, and for each process its scan found: state, start time, parent, `SigPnd`, `ShdPnd`, `SigBlk`, and the
tick count now. Reproductions of the case on the old stop (binary `tree-*`, `the_deadline_stops_the_whole_tree_too
--exact`):

| Load | Runs | Failed |
|---|---|---|
| nice 19 beside four busy loops (the recipe), sequential | 100 + 300 | 0 |
| four runners, each in its own pid namespace (`unshare --pid --fork --mount-proc`), at nice 19, beside eight loops | 200 | 0 |
| two runners at SCHED_IDLE (`chrt -i 0`), own pid namespaces, beside four loops | 20 | 0 |
| **four runners sharing /proc**, at nice 19, beside eight loops | 100 | **94** |

Every failure in the shared runs was the test's own scan, which is reading (c). Every case scans the whole
machine's `/proc` for a fixed marker (`300.1802`), so:
- 27 runs failed with a verdict of `{"killed":2,"ms":10..22,"scope":"descendants","survivors":0,"verified_by":"tree"}`
  while the scan found sleepers whose parents were another run's wrapper. The failure the issue describes: both
  sleepers "running" just after a verified completion.
- 67 runs failed because another run's scan had SIGKILLed their sleepers: `detail.stop` null, or `timed out`
  missing with `exit signal: 9`.

No run, on any load, showed a verdict with a survivor (reading a). No run showed a verified verdict with this run's
own process left (reading b). The gate holds the shared lock for its suite, so two gates' suites never overlap. A
lane running the kernel's tests outside a gate (`cargo nextest run -E 'package(theseus-kernel)'`, as the crate's
guide says to), beside a gate, would give exactly this failure. I can't confirm that is what happened on the owner's
machine.

**Changed** (`2d4c3afc`):
- tests/tree.rs: each case's marker is `300.180<n><this run's pid>` (`marker_of`), so a scan finds this run's
  processes alone. Each failure prints what the stop said and what each process found was (`none_left_after`,
  `described`). No wait was widened and no check removed. The deadline case still scans before it reads the
  verdict's fields, and now prints the verdict with the scan.
- tree.rs: I fixed (a) and (b) as well. Neither showed here, but neither can be ruled out on a slower or busier
  machine, and the cancel's stop and the cgroup's tail share this code:
  - (b): `tree::stop`'s `reap` now returns `tree::Left`. The wrapper's `reap_all` answers `None` on ECHILD,
    `Some` when `waitpid` returns 0, and `Unknown` otherwise. Phase 1's early return, the freeze's end, and the
    kill's end each need an empty scan *and* a reap that does not say `Some`. A child the reap still counts and no
    scan ever finds is reported (`Stopped::unseen`, counted in `survivors`, `why`: "a child no scan found"), never
    verified.
  - (a): the kill's wait keeps each killed process's pidfd (opened and checked against its start time, as the
    signal's was) and ends only when every one polls as exited, asleep on them between looks (`wait_exit`).
    `KILL_WAIT` is 2 s, up from 500 ms. The daemon's `ANSWER_WAIT` is 3 s, up from 2.5 s, so freeze (0.5 s) plus
    kill (2 s) still fit before the daemon kills the wrapper's group.
- job.rs: `reap_all` returns `Left`, `stop_tree` counts `unseen`, and `ANSWER_WAIT` changed. cgroup.rs: `stop`
  takes the new reap. kernel AGENTS.md: tree.rs and tests/tree.rs lines.

**Proved:**
- The new binary: 0 of 300 at the recipe's load, sequential. 0 of 100 in the shared-/proc four-runner setup that
  failed 94 of 100 on the old one.
- tests/tree.rs whole, 20 times at nice 19 beside four busy loops: 20 of 20, 6 of 6 cases each. (One earlier set of
  20 ran on a build with the planted revert still compiled in, and passed too. I reran it on the right build.)
- A load-free test, `tree::tests::a_scan_that_misses_a_live_child_does_not_end_the_stop`: an injected scan misses
  this process's `sleep` child on its first three reads, and the reap (`waitpid` of that child) says it is left.
  New code: killed 1, left 0, the child dead. **Planted revert (the stop's old ending, phase 1 returning on an empty
  scan):** it fails with `the stop ended while its child ran: Stopped { killed: 0, survivors: [], unseen: false,
  ms: 0 }`. Restored, touched, `git status` clean.
- `a_child_no_scan_finds_is_a_survivor_not_a_verified_stop`: a reap that always says `Some`, a scan that finds
  nothing: unverified, "1 process outlived the kill: a child no scan found".
- I have no load-free test for (a): I found no way to keep a SIGKILLed process off the CPU without load (SIGKILL
  wakes a stopped one, and a vfork parent's wait is killable).

**The deaf case's** time grew by 0.5 s with `ANSWER_WAIT` (3.3 s).

## Step 2: theseus-jnnj, a late completion's second taker

**Found:** as the issue says. `completion_with`'s take path treated only `Succeeded` and `Failed` as taken.

**Changed** (`389ed841`): a `Cancelled` action with `completions_seen >= 1` reads as `Accepted::Taken`. Kernel guide:
the job_wait line.

**Proved:** `tests::a_cancelled_jobs_late_completion_taken_twice_writes_one_row` cancels a dispatched job, verifies
the cancel, takes its late completion twice: `LateAfterCancel` then `Taken`, one frame, one
`completion.late_after_cancel` row, `completions_seen` 1. **Planted revert (the `Cancelled` arm removed):** the
second take is not `Taken` (it writes `LateAfterCancel` again), and the test fails. Restored, touched.

## Step 3: theseus-m9iy, an earlier process's in-process calls

**Changed** (`1f1dacdf`):
- `Evidence::in_process`, a default trait method: `action.tool == PROVIDER_TOOL` (`provider.messages`). That is
  how an in-process call is told from a job with no format change. It is a default so the core could widen it later.
  I scoped it to provider calls on purpose: a dispatched *tool* call is answered by its turn's resume
  (`check_dispatched`: a job by its spool and wrapper, a harness tool run again, the rest
  `interrupted_by_restart`), and a mark here would pre-empt that resume's answer. Jobs are untouched.
- Startup's step 4 scan (`reconcile_with(.., false)`): each dispatched, not-overdue in-process call is noted in
  `Kernel::earlier`, in memory, shared with views, and nothing is written.
- `Kernel::mark_earlier_calls_unknown` (new `earlier.rs`) takes the list and marks them all `outcome_unknown` in
  **one frame** (`Kernel::frame` over their executions, `mark_unknown` inside with reason
  `in_process_before_restart`; the completion's producer is `reconciler:in_process_before_restart`). A call settled
  since is skipped. The driver calls it once, before its loop (`Core::mark_earlier_calls`, which logs and wakes
  admission). The heartbeat's `reconcile` (due=true) calls it too, as a backstop. A crash before the tick loses
  nothing: the next start scans again.
- Kernel guide: `earlier.rs`.

**Proved:**
- `tests_earlier::an_earlier_processs_provider_call_is_unknown_at_the_first_tick_and_a_job_is_not`: three
  sessions: a provider call in a turn that dies, a job parked on, and a session parked on a provider call of its
  own. Kernel dropped and reopened. Startup writes 2 frames (the interrupted turn, then the step rows: the same
  as without the calls) and marks nothing. The first tick marks both calls in **one** frame, with the new
  producer. The job stays `dispatched`, its session waiting. The requeued execution has the call in
  `queued_results`, none `outstanding`, and its 100 µ$ moved from reserved to held-unknown. The parked session is
  queued. A second tick writes nothing.
- `..the_heartbeat_marks..` (the backstop) and `..a_call_this_process_dispatched_is_left_as_it_is`.
- **Planted revert (the startup collection removed):** both restart tests fail (`left: []`, the calls expected).
  Restored, touched.
- **Live, on a scratch daemon here** (fresh state dir, Discord and web off, `[model] api_base` a python listener that
  accepts and never answers, the key `env:`): `theseus ask` dispatched the provider call, then `kill -9` and a
  restart. The ledger shows `server.started` at …490, `driver.started` at …492, and
  `action.outcome_unknown` for the call at …494, then the requeued turn's `turn.started` at …498.
  `theseus executions` showed `held $1.3041` (the old call's reservation, held), and the new turn's own call in
  flight. I did not run the brief's job half (a job of another session on `fake-model` across the kill).
- kernel-sim, 40 seeds, `--p-race 0` and `0.3`: all invariants held (16,080 checks each), after step 3 and again
  after step 4.

## Step 4: theseus-oqxw, a nested lock

**Found: nothing nests on purpose.**
- Every transition that takes several locks takes them in one `lock_all` call: `lock_family`, used by
  `locked_action` for a task and its parent; the transaction's lock of what it names and their parents; and
  startup step 2's `lock(&ids)`.
- `reopen` and `follow_limit`, called under step 2's lock, only build rows.
- A transition inside a frame takes no lock (`ExecLocks::none`).
- The outbox's actions are their own record kind, and `outbox.rs` takes no execution lock. `outbox_stage` in a
  turn's end frame stages records.
- The observer hands frames on.
- The reconcile's and startup's scans run each transition on its own, holding nothing across it.

The whole suite (2,570 tests) and kernel-sim at both race settings met no panic. The L1 tests fail early on this VM,
so the L1 paths were not exercised.

**Changed** (`ba6455a5`): `ExecLocks::lock_all` checks a thread-local count of the `ExecLock`s the thread holds, of
any `ExecLocks`. When it is above 0, it panics before taking anything. If one of the ids is held by this thread, it
gives the old "locked twice on one thread" message (kept). Otherwise it panics with "a lock taken while this thread
holds another: name it in the one Kernel::frame (locking [..])". O(1): one thread-local read per transition, one
increment and decrement per lock. Guides: locks.rs's header and the kernel guide's lock invariant.

**Proved:**
- `locks::tests::a_lock_taken_while_this_thread_holds_another_panics_and_takes_nothing` (also across two
  `ExecLocks`; another thread's lock still waits).
- `tests_tx::a_lock_taken_while_the_thread_holds_another_panics_and_writes_nothing`. On one thread: the panic,
  nothing written, no lock held. Two threads, each holding the execution the other's closure locks: both panic
  at once.
- **Planted revert (the check removed):** both tests fail. The one-thread half fails at once (the nested call
  returned `Ok(Execution ..)`). With that half also removed, the two-thread half fails in 5.17 s with `no panic:
  the two transactions wait on each other: Timeout`: the deadlock is real on the old code, and the test fails fast.
  Restored, touched.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Every phase before the suite passed
each time (fmt, shape, features, clippy, cockpit, test build). Gate 3's first run failed in clippy (`drive` at 104
lines); I moved the call into `Core::mark_earlier_calls` and reran.

Each suite failed only on:
- **The 33 known L1 tests** (theseus-pv6i): theseus-sandbox's contract tests and `spawn_100`, and theseusd's sandbox
  tests, the VM running as root.
- **Gate 1 only:** `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`. It is not on the known
  list, and **it fails on main's code too**: with the four kernel files reverted to main, it failed 3 of 6 runs
  alone on an idle machine. With this branch, 1 of 3. The typed-ahead `echo back` lands on the prompt line
  (`ok> back`), so the wait for `\nback\n` times out at 15 s. It is a terminal race (theseus-core's term, which
  uses only `tree::descendants`, unchanged here). It needs an issue of its own.

Then I ran the phases after the suite by hand (machine_checks' rest): the protocol types (unchanged), the turn bench
(5 frames plain, 9 tool: ok), and `cargo deny --offline check` (advisories, bans, licences, sources ok; the fetch at
setup succeeded). Final suite (gate 4, the branch's head): 2,570 tests run, 2,537 passed, 33 failed (L1), 19
skipped.

**The lifecycle bench** (`theseus-sim bench lifecycle --check`, not part of a lane's gate) on this 4-core VM, at the
head: 3 of 4 runs passed every budget. The first run printed `LIFECYCLE BUDGET MISSED`, and its phase line had
scrolled out of what I kept. The startup steps there read store 0.10, load 0.17, spool 0.05, reconcile 0.21, and
accepting 1.47 ms (p50). The start path's only addition is a `Vec` push per dispatched provider call in the scan.

## The maintainer's live checks (a 16-core machine)

**1. The deadline case, 300 times under load, main's build and this branch's.** The branch's test file prints the
verdict. To make main's failures print theirs, put the branch's tests/tree.rs on main with the old shared marker
(it builds against main's library: it uses only `tree::stat`, which main has):

```bash
# main's build, with the printout and the old fixed marker:
git checkout main && git show cloud/20261005-kernel-fixes:crates/theseus-kernel/tests/tree.rs > crates/theseus-kernel/tests/tree.rs
sed -i 's/format!("300.180{n}{}", std::process::id())/format!("300.180{n}")/' crates/theseus-kernel/tests/tree.rs
# then, on each build (this one, and the branch's head after `git checkout -- . && git checkout cloud/20261005-kernel-fixes`):
cargo nextest run --workspace -E 'package(theseus-kernel) & binary(tree)' --no-run
BIN=$(ls -t target/debug/deps/tree-* | grep -v '\.d$' | head -1)
pids=""; for i in $(seq "$(nproc)"); do sh -c 'while :; do :; done' & pids="$pids $!"; done
f=0; for i in $(seq 300); do nice -n 19 "$BIN" the_deadline_stops_the_whole_tree_too --exact > /tmp/tree-one.txt 2>&1 || { f=$((f+1)); cat /tmp/tree-one.txt >> /tmp/tree-fails.txt; }; done
kill $pids; echo "failed $f of 300"; grep -o 'the stop said: .*' /tmp/tree-fails.txt | sort | uniq -c
```

Expect 0 on both when nothing else runs the case. To see reading (c) on main, run the loop twice at once (two
shells). main's build then fails, each failure printing a verified verdict (`survivors 0`) beside sleepers whose
parent is the other run's wrapper. The branch's build fails none. A failure on the branch that prints a verdict
with `survivors` above 0, or `a child no scan found`, is reading (a) or (b) caught honestly, not a false verify.

**2. A provider call across a `kill -9`.** Scratch daemon, fresh state dir. Ports 9461 and 9462 are free choices.

```bash
mkdir -p /tmp/m9iy && cd /tmp/m9iy
cat > hang.py <<'EOF'
import socket, threading, sys
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(64)
def hold(c):
    try:
        while c.recv(65536): pass
    except OSError: pass
while True:
    c, _ = s.accept(); threading.Thread(target=hold, args=(c,), daemon=True).start()
EOF
cat > rules.json <<'EOF'
[{"when": "run the job", "calls": [{"name": "proc.run", "input": {"argv": ["sh", "-c", "sleep 20; echo job-done"]}}], "text": "started"}]
EOF
cat > theseus.toml <<'EOF'
[model]
live = "hang"
[providers.hang]
api_base = "http://127.0.0.1:9461"
api_key_secret = "anthropic_api_key"
[providers.fake]
api_base = "http://127.0.0.1:9462"
api_key_secret = "anthropic_api_key"
[profiles.hang]
provider = "hang"
model = "claude-sonnet-5-5"
[profiles.fake]
provider = "fake"
model = "claude-sonnet-5-5"
[secrets]
anthropic_api_key = "env:FAKE_ANTHROPIC_KEY"
[discord]
enabled = false
[web]
enabled = false
EOF
python3 hang.py 9461 & HANG=$!
theseus-sim fake-model --addr 127.0.0.1:9462 --rules rules.json & FAKE=$!
FAKE_ANTHROPIC_KEY=x theseusd --config theseus.toml --socket ./sock --state-dir ./state > d1.log 2>&1 & D=$!
T="theseus --socket ./sock"
$T profile use fake && $T ask "run the job"     # session A: its proc.run job starts (answer `$T confirm` if it asks)
$T profile use hang && ($T ask "say hello" &)    # session B: its provider call waits for ever
sleep 2; $T executions                           # B: outstanding 1; A: its job running
kill -9 $D; rm -f sock
FAKE_ANTHROPIC_KEY=x theseusd --config theseus.toml --socket ./sock --state-dir ./state > d2.log 2>&1 & D=$!
sleep 2; $T ledger | grep -E 'server.started|driver.started|action.outcome_unknown'
$T ledger --json | grep -o '"producer":"reconciler:[a-z_]*"' | sort | uniq -c
$T profile use fake                              # so B's requeued turn, and anything after, can finish
sleep 25; $T executions; $T history <A's session>   # A's job completed (job-done) after the restart
$T shutdown; kill $HANG $FAKE
```

What each step should show:
- **The ledger after the restart:** `action.outcome_unknown` for B's call within milliseconds of `driver.started`.
  Here it was 2 ms after, and 4 ms after `server.started`.
- **The producer count:** one `reconciler:in_process_before_restart`, and no `overdue_no_evidence`.
- **`executions`:** B with `held` equal to the old call's reservation. A's job still dispatched until it ends, then
  completed with `job-done` in A's history.
- **B's session:** ready once its requeued turn answers, after the switch to `fake`.

I ran the B half here (the result is above), not the A half or the profile switch.

**3. The lifecycle bench, as main's:** `theseus-sim bench lifecycle --check` on the branch's debug build, with the
gate's settle (or a full `scripts/gate.sh` at the join). The start path gained no frame and no read: one `Vec` push
per dispatched provider call, in a scan that already ran. The driver's start writes one frame only when an earlier
process left a provider call in flight.

## Left, uncertain, and for the owner

- **Which reading the owner's failure was.** The data here supports (c), a scan of the whole machine's /proc for a
  marker another run shared. (a) and (b) are fixed defensively, and only (b) has a load-free test.
- **A held-unknown that can never resolve** (step 3): an earlier process's provider call's reservation stays in
  `held_unknown_micros`, which no reset frees. A design question: release it, or book it as spent, at the mark?
- **`term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` fails on main** on this VM (3 of 6, alone): a new
  timing finding for its own issue, maybe the flaky list.
- **Docs for the maintainer** (not edited, per the brief):
  - the spec's Part III items for the four steps;
  - `docs/status.md`;
  - the spec text on a stop's verdict (M4 18a): the kill's wait is now 2 s and ends on each pidfd, and a
    wrapper answers within 3 s past the grace;
  - the reconciler's text (§3.16): an earlier process's provider call is unknown at the first tick.
- **kernel-sim (theseus-sim) is unchanged.** No invariant moved. It doesn't call `mark_earlier_calls_unknown`
  directly; it reaches it through `reconcile`, its "heartbeat".
