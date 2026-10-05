# Cloud report: cloud/20261005-sim2 (theseus-81ig, theseus-celu.35)

Branch `cloud/20261005-sim2`, from `2bff1446` (the task commit on main's `80ef1dea`). Started 08:35 UTC.

Commits:
- `3dad9a68` sim: kernel-sim's coverage counts come from a run with no second thread (theseus-81ig)
- `55cca822` sim: kernel-sim drives /stop, tasks' reports, wakes due in a busy turn, and the outbox under crashes (theseus-celu.35)
- this report (not for main)

Nothing outside crates/theseus-sim and .config/nextest.toml changed. The kernel was not touched: every planted
bug below was put in, run, and taken back out (each file restored with `git checkout` and `touch`ed, `git status`
clean after each). No new dependency.

## Step 1: theseus-81ig, a deterministic coverage check

**Found.** As the brief says: the raced run (`--p-race 0.3`) reproduces only up to its first race, so the
"series put back" count varied (a plain run of the old command here gave `1 series put back`). The same was true
of the other check that hung on interleaving: "of them transactions" (racing-thread frames). I checked with a new
`--verbose` line naming each race's ops: neither seed 1's nor seed 2's first race held a `Frame`, so that count
also hung on later races.

**Changed** (`3dad9a68`):
- `the_kernel_holds_its_invariants_under_seeded_faults` now runs `--seed 1 --seeds 2 --steps 300 --p-race 0`
  and reads every coverage count from it (asked, unsent a cancel ended, repeating, series put back), and checks
  that no turn was raced.
- A new test, `the_kernel_holds_its_invariants_with_raced_turns`, runs the old raced command and checks only
  "all invariants held", "raced turns > 0", and "of them transactions > 0".
- To make that last count fixed by the seed: **a run's first race always appends a `RaceOp::Frame`**
  (kernel_sim.rs, `race_a_turn`). Everything up to and including the first race's op list comes from the seed,
  and the frame counter counts ops picked, so the count is ≥ 1 on every run. This is a small change to what the
  sim does, made on purpose; the owner may prefer another scenario.
- `--verbose` prints each race's ops.
- The theseus-81ig override is gone from .config/nextest.toml.

**Proved.**
- `--p-race 0` twice: identical TOTAL lines (with the `ms` field removed).
- The raced command three times: 23, 30 and 23 raced turns in its TOTAL, with 6, 9 and 6 transactions on the
  racing thread.
- 50 runs of both kernel tests (`target/debug/deps/sim-* the_kernel_holds`) at `nice -n 19`: **50 passed, 0
  failed** (median 10.7 s, slowest 169 s). **Deviation from the recipe:** three busy loops, not four. With four
  loops on this 4-core VM, each niced run took 180 to 220 s (the loops take every core), so 50 runs would have
  taken about three hours. I ran 3 runs with four loops (all passed), then the 50 with three.
- Gate: suite failed only on the 33 known L1 tests (see below); fmt, shape, features, clippy, cockpit, test
  build, reader rule passed; protocol types clean; `cargo deny --offline check` ok.

## Steps 2 to 5: theseus-celu.35

All in one commit, `55cca822`. I did not split it by step: the operations share one turn-end helper
(`end_turn_posting`, which stages a post in the frame that ends a turn) and one call site in `take_a_turn`, so a
stop-only commit would have carried half of the outbox's plumbing. Each operation is a module of its own:
`kernel_sim/stops.rs`, `tasks.rs`, `outbox.rs`, the additions in `wakes.rs`, and `counts.rs` (the report's counts,
`Sim2Counts`, one field of `SimReport`, and the world's sim2 state). kernel_sim.rs has the `mod` lines, the roll,
the race's arm, the calls to the checks and the ledger rules, and is 2,447 lines (under 2,500; not listed in
scripts/long-files.txt). main.rs: only kernel-sim's two `println!`s gain one `{}` and the totals one `add`.

### What differed from the brief (the code wins)

- 37a's wakes were in the sim already (set, take, cancel, the series checks), and the heartbeat's reconcile
  already ran `fire_due`. I added only what the brief named as missing, plus a check of the due scan (below).
- The step's picker: as the brief says, no task had a parent, so no report wake ever ran. Executions opened as
  `SessionKind::Task` with no parent exist (the old sim's "tasks"); `open_task` and `stop_execution` refuse them
  as tasks, which the sim now checks.
- Held posts and the credential request: driven by neither.
- A turn's end stages its posts in its frame: the sim uses `end_turn_with` and `outbox_stage`, as the core does.
- **Doc mismatch in the kernel** (not fixed: kernel-fixes owns the kernel): `wakes.rs`'s `drop_wakes` comment says
  an execution "that ends drops its wakes (a cancel, `/stop`, a task's end)", and kernel.rs's `cancel` comment
  says "`/stop` and a cancel clear them". `stop_execution` keeps the wakes, as stops.rs says and the sim now
  checks. The two comments should drop `/stop`.

### Step 2: `/stop` (stops.rs)

**Operations.** A third of the picker's old "input" branch is now the operator's stop (a fifth of that path
after the wake cancel): `stop_execution` of an open conversation between its turns; 30% of the time `stop_call`
of one running call; 10% of the time a stop of a task, which must be refused (`StopTask`) and write nothing. In a
turn, 5% of conversations' turns are stopped while they run (`stop_in_turn`). On the racing thread, one op in ten
is `RaceOp::Stop` for a conversation (a task gets a cancel). Crashes: after a stop, after a call's stop, after a
stop in a turn.

**Invariants.**
- After `stop_execution`: open, not cancelled; a turn ran iff it was Running; Running with `stopped` set, or
  Waiting on input; same session, limit, spend, no question; same tasks (ids and states) and same wakes (ids and
  due times); declined = every planned or authorized action, each now Cancelled; told to stop = every dispatched,
  not-yet-told, non-provider call, each now `cancel = requested`.
- After the backends are killed (`kill_jobs`): every call told to stop has settled. This check also runs after
  every raced turn, for its cancels' and stops' calls.
- Its next input (half the time): `admit_input` runs a turn, which reads every stopped call as a result.
- A stopped turn: `plan_action` and `set_wake` both refused with `KernelError::Stopped`, nothing written; its
  end, whatever the sim asked (complete, a due time, a failure, requeue), leaves it Waiting on input. After a crash
  in it, the start leaves it Waiting on input.
- `stop_call`: its execution's record is unchanged; a second `stop_call` returns None and writes nothing.
- From the ledger, in WAL order (`stop_row`), on raced turns too: after an `execution.stopped` with
  `turn_running`, no `action.planned` (but a post's), `wake.set` or `budget.carved` of it, and no
  queued/running/complete/failed/blocked row, until `execution.waiting why: stopped`, `execution.interrupted`
  with `stopped_by`, or `execution.cancelled`.
- The "never runs again" check still reads only `execution.cancelled`.

### Step 3: tasks and their reports (tasks.rs), and scheduled wakes (wakes.rs)

**Operations.** 15% of turns open a task (`open_task`, `want` 500 to 4,000 µ$, half with `wake_parent`); 20% of
those run the same call again, which must find the task and write nothing; a task's own turn must be refused
(`TaskDepth`, nothing written); a parent with nothing left is refused (`NothingToCarve`). Every turn takes its
reports first (`take_reports`). A task's turn that waits on nothing it sent ends (complete, or a fourth of the
time failed) two times in five, so reports and their wakes happen in a run. Tasks are cancelled by the existing
cancel op, and their wakes cancelled by the existing wake-cancel op (37b's unpark).

**Design choice the owner should hear:** a task's budget question is always declined, never approved. An approved
reset of a task's spend is the one way past its carve (tasks.rs says so), and the parent's spend then counts it, so
the parent's spend can pass its own limit, which the sim's existing "nothing reserved past the limit" check would
call a bug. The sim drives resets on conversations only.

**Invariants** (`check_tasks`, every check):
- Depth one; a task is `SessionKind::Task`.
- No open task waits on input with no wake (`task_unparked`).
- For an open parent: its carve for each task equals what the task can still spend, exactly: `limit - spent`
  while it runs, `reserved + held_unknown` once it has ended (absent at 0). No session outspends its limit through
  its tasks: the existing budget check over every execution.
- An ended task's report is on its open parent's `reports` until a turn has read it, and gone after; an open
  task's never is. Each report is read once (`take_reports` fails a second read), and only by its own parent, of
  a task that has ended. `woke` is a subset of what was read, of tasks with `wake_parent` that were not cancelled.
- `report_wakes` holds exactly the ended, unread, not-cancelled `wake_parent` tasks of an open parent.
- A free parent (waiting on input, a time, a job, an execution) holds no report's wake unless a stop freed it
  since the last due scan.
- From the ledger: no `task.report_wake` for a cancelled task; between two `execution.queued why: report` of a
  parent, a turn of it ran or a stop parked it (reports that land together start one turn).

**Scheduled wakes, what was missing:** a wake that falls due while its turn runs. Now 15% of plain turns advance the
clock past the soonest pending wake before ending; `end_a_turn` checks that an end that would park a free execution
(input, a time, its jobs) with a wake due leaves it Queued, the wake kept. And `check_due_scan`: right after the
heartbeat's reconcile, no open execution is `due_now` (a due time come, or free with a due wake or a report's
wake); it also clears "freed by a stop".

### Step 4: the outbox (outbox.rs)

**Operations.** Two in five turn ends stage a reply in the end's frame (`end_turn_with` + `outbox_stage`), in the
plain path, the batch path, a stopped turn, and the turn after a stop. Before each step's roll, 30% of the time
the fake binding runs: 30% of those plan a notice outside a turn (`outbox_plan`) about a random execution (open or
ended) or the daemon; then it delivers one open post: dispatch (or a retry that must write nothing and keep the
first time), send under the post's key (its id, as Discord's nonce) to a fake channel that keeps one message per
key, settle with the channel's message id, and 30% of the time a second settle. The channel refuses 10% of first
sends (settled Failed). Crashes after the stage, after the plan, after the dispatch before the send, after the send
before the settle, after the settle, each at twice the usual crash chance. At quiesce the binding runs every open
post.

**Invariants** (`check_outbox`, every check; `post_record`, in the WAL scan):
- Every OUTBOX record the WAL gains is, in order, one the sim wrote: so nothing else (a cancel, a stop, an
  execution's end, the reconciler) writes a post.
- Every 20th check and at the end, each post is read back as the binding last left it, and there is no stray post.
- No post among the kernel's `actions()`; none in any execution's `outstanding`, `queued_results`, or
  `Wake::Actions`.
- The channel holds at most one copy of each post, and a succeeded post's `external_op_id` is that copy.
- A second settle returns `Already` with the record unchanged, and writes nothing.
- At the end, every post is Succeeded with exactly one copy, or Failed (refused) with none.

The existing "a cancelled execution never writes `action.planned`" rule now skips post rows (`tool: outbox`): a
cancelled execution's card says how it closed.

### Step 5: the gate's run

tests/sim.rs asserts, on its `--p-race 0` run, that each of these ran: stops, stops while a turn ran, next inputs
that ran a turn, calls stopped alone, tasks opened, depth-one refusals, reports read, report wakes, wakes due in a
turn, posts staged in a turn's end, posts planned outside one, posts sent again, second settles, crashes around a
post. The counts on that run (seeds 1 and 2, 300 steps): 17 stops (5 in a turn), 2 calls stopped alone, 10 tasks
(5 waking), 5 reports read (1 woke its parent), 7 wakes due in a turn, 50 posts staged and 54 planned, 4 sent again,
36 second settles, 16 crashes around a post.

**Time.** The gate's sim tests, debug build, this VM, no load: main's one test (raced run) about 2 s; after step 1,
3.6 s for the `--p-race 0` run and 2.1 s for the raced run (in parallel); after sim2, **7.9 s and 7.2 s**.
Profiling showed the per-step checks dominate; I cut the outbox check's cost (it compared every post by reading
them all back at every check, 1.9 s of the run's 11 s; it now checks each post record as the WAL scan reads it,
reads all of them back every 20th check) and lowered the crash boost around posts from 5x to 2x. What remains is
more work per run: about 50% more turns, tasks as more executions every check reads, and the posts. If the owner
wants it nearer main's, the cheapest lever is the binding's 0.3 per step.

### Proof

- `target/release/theseus-sim kernel-sim --seeds 40 --steps 1000 --p-race 0`: all invariants held (156 s). TOTAL
  (sim2 clause, older wording): 293 stops, 117 while a turn ran, 146 calls told to stop, 71 unsent declined, 80 next
  inputs ran a turn; 68 calls stopped alone, 24 task stops refused · 278 tasks opened, 121 waking their parent, 41
  found again, 340 refused at depth one; 136 reports read, 22 woke their parent, 7 read together · 222 wakes due in
  a turn queued by its end · 1288 posts staged in a turn's end, 3612 planned outside one; 4415 sent, 187 sent
  again, 501 refused, 4900 settled, 1391 second settles, 822 crashes around a post.
- The same at `--p-race 0.3`: all invariants held (89 s). 660 raced turns, 159 transactions, 32 crashes inside
  one · 214 stops, 77 while a turn ran, 139 calls told to stop · 114 tasks opened, 47 reports read, 7 woke · 58
  wakes due in a turn · 460 staged, 3671 planned, 149 sent again, 1228 second settles, 659 crashes around a post.
- The `--p-race 0` command twice: identical output, `ms` fields removed (41 lines).
- 8 more raced seeds × 300 steps (debug): all held.
- These runs were the build before the last commit's two cosmetic changes (the counts clause's wording, and
  clippy's `is_multiple_of` and a type alias), which change no behaviour.
- Planted bugs (debug build, `--p-race 0 --steps 1000`, first seed of 1..10 that fails):
  - kernel `stop_execution` sets a declined action to Authorized instead of Cancelled: seed 2, "a stop of exe_…
    declined act_…, and it is Authorized" (stop_and_check).
  - kernel `task_ended` without the push onto the parent's `reports`: seed 1, after step 26, "exe_task…'s report
    (Cancelled) never reached exe_…" (check_tasks).
  - kernel `outbox_settle` without its `is_settled` early return: seed 1, "a second settle of post out_… changed
    it: Now(…)" (deliver_post).
  - fake binding sends under `<key>-<random>`: seed 1, after step 554, "post out_… is in the channel 2 times:
    [msg_74, msg_75]" (check_outbox).
- 50 runs of the whole tests/sim.rs binary (its 5 tests) at `nice -n 19` beside three busy loops (the same
  deviation as step 1): **50 passed, 0 failed**; each run 11.9 to 17.3 s (median 14.8 s).

### Findings

No kernel bug found: no new invariant failed on any seed I ran (40 + 40 at 1000 steps, plus about 20 at 300).

### Counts kernel-fixes may move

- theseus-jnnj (a cancelled job's late completion taken twice writes one row): `late-after-cancel` and `dup`
  counts can drop; no sim2 invariant reads those rows.
- theseus-m9iy (an earlier process's in-process calls settle unknown after a restart): `unknown → resolved` moves;
  a raced or batch turn's in-process call a crash cut is already allowed to end unknown or cancelled.
- theseus-oqxw (a nested lock panics): the sim calls `end_turn_with` (a frame whose `end_turn` takes the family
  lock inside), as the core does; it nests no lock of its own. If that composition starts panicking, the sim
  shows it at once on every seed.
- theseus-g11i (the deadline's stop of a whole tree): not reached by the sim.

## Live checks for the maintainer (release build, 16 cores)

```
cargo build --release -p theseus-sim
target/release/theseus-sim kernel-sim --seeds 200 --steps 1000 --p-race 0
target/release/theseus-sim kernel-sim --seeds 200 --steps 1000 --p-race 0.3
```
Each should end `all invariants held`, with a TOTAL whose sim2 clause has every count above 0 (on 40 seeds here:
about 290 stops, 280 tasks, 136 reports, 22 report wakes, 220 busy wakes, 4,900 posts settled, 820 crashes around a
post at `--p-race 0`; fewer turns, so fewer of each, at 0.3). Then the gate three times:
```
scripts/gate.sh
```
`theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` and `…_with_raced_turns` pass with no
retry (nextest prints no `FLAKY` for them; their override is gone).

## Left, or uncertain

- The forced first-race `Frame` (step 1) and the always-declined task budget question (step 3) are choices.
- "Reports that land together start one turn" is checked as "two report queues of a parent have a turn or a stop
  between them" and counted (`read together`, 7 in 40 seeds); a stronger check would need the sim to know which
  tasks ended while the parent was busy.
- The sim's time grew about 2x (above).
- Docs for the maintainer: the two kernel comments that say `/stop` drops wakes (above). The spec's Part III item and
  docs/status.md are yours. I updated crates/theseus-sim/AGENTS.md's kernel-sim entry (the new modules, and the
  coverage-counts rule).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before each commit: fmt, shape, features, clippy,
cockpit, test build, and reader rule passed; the suite ran 2,563 tests, 2,530 passed, and **33 failed, all the known
L1 tests** (theseus-pv6i: 19 in theseus-sandbox's contract, its bench's `spawn_100`, and 13 in theseusd's sandbox
tests). No other test failed, and none was retried as flaky. The phases after the suite, run by hand: protocol
types clean, nothing compiled under the lock, `cargo deny --offline check` ok (advisories, bans, licenses,
sources). The deny database was fetched in setup. The benches were skipped (`THESEUS_GATE_NO_BENCH`).
