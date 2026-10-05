# Cloud report: queue-frames (theseus-2xep, theseus-6qwr)

Branch `cloud/20261005-queue-frames`, from main at 60b43fb6 (store format 20) plus the task commit 1b930976.
Started 20:22 UTC, finished 22:19 UTC. Two steps, each its own green commit:

| Step | Issue | Commit |
|---|---|---|
| 1. A completion that queues its execution writes `execution.queued`, why `result` | theseus-2xep | cad513d6 |
| 2. A late result's wake rides in the turn's end frame | theseus-6qwr | a508daff |

No store format change, no new dependency, no protocol type changed (`cockpit/src/protocol.gen/` untouched).

## Step 1: theseus-2xep

### What I found

- The row belongs in `accept_completion`, not `completion_with`. `completion_with` is only a `Kernel::frame` around
  `accept_completion`, and `accept_completion` is also called directly: the AWS hands' poller (`aws/hands/poller.rs`
  three times, `group.rs`, `overdue.rs`), the turn (`turn.rs`, `turn/compaction.rs`), the kernel's reconcile
  (`kernel.rs`), and kernel-sim. In `completion_with` the direct callers would still queue with no row. Its three
  added lines in `accept_completion` (a flag set where the state moves, and the row pushed right after the action's
  row) cover every path, `take_completion_with` and `mark_unknown` included. kernel.rs went from 3,008 to 3,016 lines
  (ceiling 3,030).
- The move is only `Waiting` with a matching `Wake::Actions` → `Queued`. A completion on an execution that is
  already queued or running writes no queue row: it is no move.
- **Queues with no why.** `end_turn`'s arm for results already waiting (`queued_results` not empty at a `Wait` end)
  is exactly "a result queued it": it now says `why: "result"`. `TurnEnd::Requeue` keeps no why: nothing in the core
  ends a turn with it (only kernel tests and kernel-sim do), it means "yield to the scheduler", and `result` is not
  that word. If the owner wants one, `yield` is the honest word; I left it alone.
- **The store's version rule does not apply.** `why` is a key of a ledger row's `data` (a `serde_json::Value`), not
  a field of a stored record type, and `"result"` is a new value of it. No record gains a field, no encoding changes,
  so `MANIFEST_FORMAT` stays 20 and `tests_layouts.rs` gains nothing.
- **Readers.**
  - theseus-core's push board (`push.rs`) took a `result` why from an `action.*` row's `execution_state` when no
    `execution.queued` row named one. It now reads `execution.queued` only. One side effect, which I think is right:
    a completion that settles while the execution is *already* queued (by input, say) used to show `why: result` on
    that frame's view; now its view carries no why, as any other frame that touches a queued execution without
    queueing it.
  - theseusd's `tests/push.rs` `row_state` no longer reads action rows; the push prove passes with execution rows
    alone (`push prove: 42 state rows, 38 execution.changed, 5 executions, 2 confirm events`).
  - The cockpit's time machine (`cockpit/src/lib/timemachine.ts`) already read states and whys from `execution.*`
    rows only, so until now it never saw a completion's queue: the session stayed "waiting" in a replay. It now sees
    `queued · result` with no change to the cockpit.
  - kernel-sim's `stops.rs` and `tasks.rs` read `execution.*` rows; nothing there needed a change.
- **kernel-sim invariant (added).** `kernel_sim/queues.rs`: an observer on each kernel the sim builds (and rebuilds
  at each restart) sees every committed frame whole; a frame that moves an execution to `queued` from any other
  known state must hold that execution's `execution.queued` row. The one other word is a start's requeue of an
  interrupted turn (`Running` → `Queued`), whose row is `execution.interrupted`; I accepted that rather than add a
  queue row to the start's frame. kernel_sim.rs: 2,451 → 2,465 lines.

### What I changed (cad513d6)

- `crates/theseus-kernel/src/kernel.rs`: the row in `accept_completion`; `why: "result"` on `end_turn`'s
  `queued_results` arm.
- `crates/theseus-core/src/push.rs`: the why from `execution.queued` only.
- `crates/theseusd/tests/push.rs`: `row_state` reads execution rows only.
- `crates/theseus-sim/src/kernel_sim/queues.rs` (new) and three call lines in `kernel_sim.rs`.
- Tests: `tests_push::a_result_that_queues_its_execution_writes_its_row_why_result` (new: a `proc.run` that
  outlives its 1 s wait parks the turn on the job; the heartbeat's drain queues it; the board's and the watcher's
  view say `queued`/`result`; the ledger holds `action.succeeded` with `execution_state: queued` immediately
  followed by `execution.queued {why: result}`, both in one WAL frame, read from the segments).
  `theseus-kernel tests::full_lifecycle_one_action_one_turn` now expects the completion's frame to be 5 records,
  not 4, and its last `execution.queued` to say `result`.
- Goldens, only the lines this moves:
  - `kernel_frames.txt`: one line. The `end_turn_with (wait on the job)` frame, which ends with a result already
    queued, gains `"why":"result"`. No completion in that script moves a waiting execution (its two completions
    with `execution_state: queued` land on an execution already queued), so no completion frame moved there.
  - `core_output.txt`: the heartbeat's completion frame gains its second ledger record, `execution.queued` with
    `why: result`.
- AGENTS.md: theseus-sim's names the new invariant (in step 2's commit).

### How I proved it

- `cargo nextest run --workspace -E 'package(theseus-kernel) | (package(theseusd) & binary(push)) | package(theseus-sim)'`:
  211 passed. theseus-core's `tests_push`, `tests_continuations`, the frame-budget tests (a plain turn's 5) and the
  output golden: all pass.
- kernel-sim, 200 seeds, before (main's build) and after; every invariant held in all four runs. At `--p-race 0`
  the two TOTAL lines are identical but for the wall time (the run is deterministic, and the new row changes no
  decision):
  - before, `--p-race 0`: `TOTAL 200 seeds: 5166 crashes (3513 startup faults) · 23958 turns · 34591 actions … 25494 completions … 80400 invariant checks · 966051 ms · all invariants held`
  - after, `--p-race 0`: `TOTAL 200 seeds: 5166 crashes (3513 startup faults) · 23958 turns · 34591 actions … 25494 completions … 80400 invariant checks · 836845 ms · all invariants held`
  - before, `--p-race 0.3`: `TOTAL 200 seeds: 2955 crashes (2600 startup faults) · 12231 turns · 17509 actions … 3288 raced turns (6753 ops on a second thread, 829 of them transactions, 184 crashes inside one) … 80400 invariant checks · 615183 ms · all invariants held`
  - after, `--p-race 0.3`: `TOTAL 200 seeds: 2893 crashes (2626 startup faults) · 11936 turns · 16888 actions … 3161 raced turns (6520 ops on a second thread, 785 of them transactions, 170 crashes inside one) … 80400 invariant checks · 464637 ms · all invariants held`
    (raced runs reproduce only up to their first race, so their counts differ run to run).
- **Planted revert: the completion's row removed** (`if queued && false`). Failed, each as the bug would:
  - `theseus-core tests_push::a_result_that_queues_its_execution_writes_its_row_why_result`: the view said
    `why: None`, not `Some("result")`;
  - `theseusd::push every_execution_row_has_its_event_and_every_event_its_row` (with the new `row_state`): "the
    event at 91 … says queued; the rows say Some((87, …, "waiting"))";
  - `theseus-kernel tests::full_lifecycle_one_action_one_turn` (4 records, not 5);
  - `theseus-core tests_output::the_cores_output_matches_its_golden`;
  - kernel-sim's new invariant, at once: `seed 1: after step 28: a frame queued an execution without its
    execution.queued row: exe_… waiting -> queued at 322, its rows ["action.succeeded"]`.
  Restored, `touch`ed, `git status` clean of it.

## Step 2: theseus-6qwr

### What I found

- **The composition.** `turn/end_step.rs::end_and_wake` is one `Kernel::frame` on the turn's kernel view: it reads
  the stop, calls `end_turn_with` (which joins the outer frame), and when the turn says `rewake` and no stop was read,
  a nested `k.frame(&[id], |k| k.wake(id, "late_result"))`. The nested frame's error is logged (`warn`) and not
  returned, so it takes back only what it staged; the end stands. `turn.rs` keeps only the call (+5 lines, 3,457 of
  3,523) and records `WokenAgain` only when the wake was staged.
- **The stop's window: yes, it changed the decision, and reading it inside the frame closes it.** `run` read
  `stopped` from the execution before `end_turn_with`, outside the lock. A `/stop` landing between that read and the
  end's lock was taken by `end_turn` itself (parked on input, why `stopped`), and then the separate
  `wake(late_result)` queued the stopped execution anyway: a stopped turn that took a late result went on. Now the
  stop is read inside the frame, under the lock `end_turn` reads it under, so the end and the wake decide from the
  same read. (A stop landing after the frame finds the execution queued, as it would find any queued execution.)
  The outer `stopped` still feeds the rest of `run` (the failure run, `StoppedAtStep`, the failed notice's class);
  that window remains for them and is outside this task: worth an issue if the owner wants those to agree too.
- **Which failing wake a test can reach.** A kernel no longer accepting can't be reached after startup (no
  transition lowers the phase). A terminal execution can: a cancel that lands during a turn which also took a late
  result. `end_turn` then writes nothing and returns the cancelled execution, and the nested wake fails
  `NotRunnable`. If that error were returned, the end would be `Err`, `run` would log "end_turn failed" and, worse,
  skip `answer_after_cancel` for the calls the cancel cut (`cancelled` is read from the end's `Ok`). The unit test
  `a_wake_that_cant_happen_takes_back_nothing` holds it. One consequence: such a cancel now logs one `warn` ("a late
  result's wake was not written with the turn's end"), where the old `let _ =` was silent. If that is noise, skip
  the wake when the end left the execution terminal; I kept the path a test can reach.
- **What now comes after the wake** (it used to come before it): the store's flush, `outbox.posted` for the turn's
  posts and report, `announce_closed`, the session hold's drop, `judge.after_turn`, `pass.after_turn`, the `Parked`
  line, and `admission.notify_waiters()`. On the `rewake` path (`Ok` only: `rewake` is false on an error) none of
  them must come before the same execution's next turn:
  - the posts are indexed in memory (`Outbox::posted`); the next turn's own posts are staged in its own frames and
    indexed after its own end, which needs its admission, compile, model call and end first;
  - `judge.after_turn` and `pass.after_turn` only dispatch (a spawn, a channel send);
  - the session record already rode in the end's frame; the hold only delays another writer of it, and the next
    turn's write waits for its drop on `theseus_store::blocking`, with nothing here waiting on that turn;
  - the failure bookkeeping (`extend_failing`, the fault's wake), the terminal-session close and the cancel's sweep
    run only on paths where no late-result wake happens (an error, a terminal end);
  - the `Parked` and `WokenAgain` lines may now interleave with the next turn's first lines in the narrative, which
    is live progress, best effort.
  The driver admits at its next tick (500 ms) or an admission notice, as before.
- **FAST.** A turn that took a late result writes one frame fewer. The plain turn's 5 and the tool-call turn's 9
  hold (`theseus-sim bench turn --check`).

### What I changed (a508daff)

- `crates/theseus-core/src/turn/end_step.rs` (new): `end_and_wake` and three unit tests:
  `the_end_and_the_late_results_wake_are_one_frame` (one frame; `execution.waiting` then `execution.queued`
  `late_result`), `a_stopped_turn_queues_nothing` (a stop landed during the turn: the end parks on input, why
  `stopped`, and nothing queues), `a_wake_that_cant_happen_takes_back_nothing` (a cancel landed: the end is `Ok`
  with the cancelled execution, no frame written).
- `crates/theseus-core/src/turn.rs`: the `mod` line, the call, and `WokenAgain` on `woke`.
- `crates/theseus-core/src/tests_push.rs`: `a_settled_wait_parked_before_a_late_results_end_returns_after_its_turn`,
  after the answer's test: a job's result lands (the heartbeat's drain) while a second turn's model call is in
  flight; a `session.wait` until settled, parked after that turn's running view, is still parked once the board has
  the end's frame (`queued`, why `late_result`), and returns `settled` only after the late result's turn
  (`continue_execution`) ends, at a position past the end; every view the watcher got in between says `queued` or
  `running`; the end's row and the queue's row are in one WAL frame. `rig_jobs` gained a model delay.
- `crates/theseus-core/AGENTS.md`, `crates/theseus-sim/AGENTS.md`: one line each.

### How I proved it

- theseus-core: `tests_push` (all 10), `tests_continuations` (all 4), the `end_step` tests, every frame-budget test
  (`tests_m3::a_plain_turn_stays_within_its_frame_budget` and the judged ones), the output golden (unchanged by this
  step: no scenario there takes a late result during a turn): 28 of 28.
- **Under load, 10 runs** (`nice -n 19`, four `while true; do true; done` loops at nice 0, killed by their pids):
  `tests_push` and `tests_continuations`, 14 tests each run, **10 of 10 runs green** (each ~25 s). One test is left
  out of those runs: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` passes nextest's
  120 s kill under this load. **It does so on main too**: I checked out 1b930976's crates, rebuilt, and ran it alone
  under the same load: `TIMEOUT [120.509s]`. It is a lagging-client test that neither step touches; it takes 8.8 s
  unloaded. Not on the known-flake list: worth an issue (theseus-core, timing under load).
- **Planted revert: the wake back after the end, in its own frame** (end committed, then `kernel.wake` outside it):
  `tests_push::a_settled_wait_parked_before_a_late_results_end_returns_after_its_turn` failed ("the end and the late
  result's wake are one frame": end row at 91, queue row at 93); with that frame assertion switched off, the same
  test failed at the wait's own check ("the end woke the settled wait parked before it": 0 waits left, 1 expected:
  the wait returned at the end). `end_step::the_end_and_the_late_results_wake_are_one_frame` failed too.
- **Planted revert: the nested wake's error returned instead of logged**: `end_step::a_wake_that_cant_happen_takes_back_nothing`
  failed (a terminal execution: the cancel's case).
- **Planted revert: the stop read inside the frame ignored**: `end_step::a_stopped_turn_queues_nothing` failed.
- Each restored and `touch`ed; `git status` showed only the step's files.

## The live check (the maintainer's)

A scratch daemon on a fresh state dir, Discord and the web off, on the stand-in model. **I ran checks 1 and 2 here,
exactly as written, on the branch's debug build**; what each showed is quoted under it. (`proc.run` needs a
workspace root, or every call waits for approval; a bare `sleep` keeps the floor out of it.)

```bash
D=$(mktemp -d /tmp/qf.XXXX); mkdir -p $D/work
cat > $D/rules.json <<'J'
[
  {"when": "run the slow one", "calls": [{"name": "proc_run", "input": {"argv": ["sleep", "1.8"]}}]},
  {"when": "anything new", "calls": [{"name": "proc_run", "input": {"argv": ["sleep", "0.95"]}}]},
  {"when": "", "text": "Plain."}
]
J
cat > $D/theseus.toml <<T
[server]
state_dir = "$D/state"
socket = "$D/theseus.sock"
[model]
api_base = "http://127.0.0.1:9448"
[secrets]
anthropic_api_key = "env:QF_FAKE_KEY"
[tools]
proc_sync_secs = 1
projects_dir = "$D/work"
[policy.tools]
"proc.run" = "open"
[discord]
enabled = false
[web]
enabled = false
T
target/debug/theseus-sim fake-model --rules $D/rules.json & FAKE=$!
QF_FAKE_KEY=x target/debug/theseusd --config $D/theseus.toml & DAEMON=$!
T="target/debug/theseus --socket $D/theseus.sock"
```

The stand-in answers a tool's result, and a late result, with `Done.`

1. **A completion's queue says why.** The first turn's `sleep 1.8` goes to the background after 1 s and parks the
   turn on the job.
   ```bash
   $T --json ask "run the slow one" > $D/a1.json; S=$(jq -r .session_id $D/a1.json)
   sleep 3; $T ledger -n 60 -s $S | grep -E "action.succeeded|execution\.(queued|waiting|running)"
   ```
   Shows the job's `action.succeeded` (`execution_state: queued`, `duration_ms` about 1800) and, at the next position,
   `execution.queued {"why":"result"}`, then the driver's late-result turn. Here: positions 68 and 69.
2. **A late result during a later turn.** The second turn runs its own `sleep 0.95` in sync, so it is running when
   the first job ends at 1.8 s. P is the ledger's last position (a `wait` until settled would block: a session
   parked on its job is not settled).
   ```bash
   $T --json watch --all > $D/watch.ndjson & WATCH=$!
   $T --json ask "run the slow one" > $D/a2.json; S=$(jq -r .session_id $D/a2.json)
   P=$($T --json ledger -n 1 | jq '.rows[0].position')
   $T --json ask -s $S "anything new" > $D/a3.json & $T --json wait $S --until settled --after $P --timeout 60s
   $T ledger -n 80 -s $S | grep -E "action.succeeded|execution\.(queued|waiting|running)"
   jq -c --arg s "$S" 'select(.session_id == $s) | {p: .position, st: .state, why}' $D/watch.ndjson
   ```
   Shows: the first job's `action.succeeded` while the second turn runs (`execution_state: running`); that turn's end
   row `execution.waiting` (wake input) and then `execution.queued {"why":"late_result"}` in one frame; the late
   result's turn (`resumed: true`) and its end. The watch has no `waiting` view between the second turn's `running`
   and that end, and none at the end row's own position. The `wait` returns `settled`, `already: false`, at the late
   result's turn's end. Here: running 287 → (job's row 312) → end row 341, queue row 343 → views `287 running`,
   `343 queued late_result`, `345 running`, `365 waiting`; the wait returned at 365. If the job ends before the second
   turn starts, its row says `why: result` instead: run the two `ask` lines again.
3. `target/debug/theseus-sim bench lifecycle --check` and `theseus-sim bench turn --check` as main's. I ran only
   `bench turn --check --runs 5 --burst 0` (the gate's): plain 5 frames, tool-call 9, ok.

Stop: `$T shutdown; kill $FAKE $WATCH`.

## Left and uncertain

- The outer `stopped` read in `TurnRunner::run` still has the window for everything but the wake (see step 2).
- A cancel during a turn that also took a late result now logs one `warn`.
- `TurnEnd::Requeue` writes no why (see step 1).
- `a_client_that_stops_reading_hears_what_it_lost_and_catches_up` times out under the load recipe on main as here.
- Docs for the maintainer: Part III's item for V3; `docs/status.md`'s recently landed; roadmap-v1.1's V3 row done;
  `docs/technical-overview.md` if it lists `execution.queued`'s whys (`result` and `late_result` now both appear in
  it, `result` from a completion and from an end with results waiting).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before each commit, in the tree committed:

- **Step 1 (cad513d6):** fmt, shape, features, clippy, cockpit, test build, reader rule: ok. Suite: 2,790 run,
  2,756 passed, 34 failed: the 33 known L1 tests (theseus-sandbox's 19 `contract` tests and its bench's
  `spawn_100`; theseusd's 13 `sandbox` tests: root daemon, no job cgroup, theseus-pv6i) and
  `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia, known; it passed alone
  at once after). Then the phases after the suite, by hand: protocol types ok (nothing in `protocol.gen` changed),
  `theseus-sim bench turn --check --runs 5 --burst 0` ok (5 and 9 frames), `cargo deny --offline check` ok
  (advisories, bans, licences, sources; the database fetched at setup).
- **Step 2 (a508daff):** the first run failed in clippy (`too_many_lines` on the new push test); I split two helpers
  out of it and ran the gate again. Fmt through reader rule: ok. Suite: 2,794 run, 2,761 passed, 33 failed, all
  the known L1 tests above, nothing else. After the suite: protocol types ok, `bench turn --check` ok (5 and 9),
  `cargo deny --offline check` ok.
- The lifecycle and jobs benches were skipped (`THESEUS_GATE_NO_BENCH=1`), as the brief says; the owner's machine
  runs them.
- Under the load recipe, not in the gate: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up`
  passes nextest's 120 s kill, on main as on this branch (see step 2).

