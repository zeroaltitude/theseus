# Cloud report: turn-one-frame (theseus-2uby)

Branch `cloud/20261010-turn-one-frame`, cut from main at 2fd1f654 (store format 26). Two commits on top of the task
commit, then this report. Started 20:15 UTC, report at 22:30 UTC.

## What shipped: the middle way, and why

**Shipped: frames 1 and 2 merged** (the admission, `turn.started` and the input's node in one frame). A plain turn on
a resumed session now writes **4 frames** (was 5) and pays **2 syncs before its model's first byte** (was 3): the
input's frame, then the provider call's plan and dispatch.

**Not shipped: the full one-frame start** (the admission and the input riding the first dispatch's frame, 1 sync).
I built it and ran theseus-core's suite against it (patch kept out of the branch; the approach is described under
"Left" below). It failed 18 tests. The failures were not about frame counts. They show that the compile and several
readers beside it assume the input is already in the store when the compile runs:

- compaction (8 tests in `tests_compaction`): an overflow rang instead of compacting, because the input sat in the tail
  with nothing left in the prefix to summarize;
- `continue.v1`'s judgment, dispatched at the compile, read the store and saw the previous message as the last one
  (`tests_continue::a_signal_and_no_trigger_asks_continue_in_shadow_once`);
- route's inbound state for the next message read the previous one as missing
  (`tests_inbound::the_next_messages_state_reads_the_previous_message_and_the_reply`);
- tiering's decode counts and its "every request the same with the cache off" (3 tests), and one situation test.

Each needs a fix in a subsystem other rows own (compaction, the judge's readers, tiering). That is not a change to
make at the end of a session, so I reverted it, and step 1 stands as the middle way.

The brief's first item ("compile first, then one frame") also assumes the compile can run before any frame. Most of
what runs before the compile is safe without the admission written: `run_inner`'s setup, catch-up with nothing to
take, the routing and recall reads. The input's node is the problem.

## Step 1: the admission rides the input's frame (f4309f1)

**Found.** `admit_input` wrote the input's wake and `running` in a frame of its own. `turn_body` then wrote the input's
node with the waiting rows (`turn.started`) in the next frame. The kernel's in-memory hold (the `held` set: TurnHeld,
the ceiling) and the durable write were one call (`admit`).

**Changed.**
- Kernel (`kernel.rs`: `admit` split into `hold` and `write_running`; new `held.rs`):
  - `Kernel::hold_turn` holds the turn in memory and writes nothing. It applies `admit`'s checks: queued, waiting or
    blocked; no turn already held; room under the ceiling.
  - `Kernel::admit_held` writes the admission in the caller's frame. It writes the same records as `admit_input` (the
    wake, then `running`), re-reads the execution under the frame's lock, and refuses a cancelled one (`NotRunnable`).
    It is idempotent (`Kernel::admitted`).
- The cancel and `/stop` now count a held turn as running (`turn_running = Running || holds_turn`):
  - **Cancel:** its sweep of unanswered calls waits for the turn's end, as it does for a running turn.
  - **Stop:** it marks the held turn and leaves the execution where it was. The admission keeps the mark, so the first
    plan is refused (`KernelError::Stopped`).
  - **Defect this fixes:** without it, a stop of the waiting execution parked it on input, and the admission's wake
    then undid the stop. That is theseus-id9's lost update in a new place. kernel-sim found it, and the planted revert
    below shows it.
- Core (`turn/admit_step.rs`; `turn.rs` has only the calls):
  - `run_body` holds an input's turn. The wait path (ceiling, turn held) still uses `admit_input` as before.
  - The input's frame is one kernel transaction:
    `admit_held` → `stopped_since`'s mark (theseus-hmwv) → the turn's waiting rows (`Store::take_waiting`, put back if
    the frame fails) → the node, plus the session record when the target moved. The WAL order matches the old two
    frames: `execution.queued`, execution, `execution.running`, `turn.started`, node.
  - A turn whose catch-up has anything to take writes its admission alone first, as before: results, outstanding
    calls, a question, a resume, or a due wake. A resumed call may plan, and `plan` requires a running execution.
  - A turn whose only news is its tasks' reports is admitted in the frame that takes them, so that turn keeps its old
    frame count.
  - An MCP prompt's input is admitted alone.
  - A turn that ends before it was admitted (a fault) is admitted in its end's frame (`end_step.rs`).
  - Continuations are unchanged.
- Admission is still a function the frame calls (`admit_held`), so admission-fifo's replacement of the wait can drop
  in. A turn that waited is admitted already, and `admit_held` writes nothing for it.
- `syncs_before_call` on the turn's trace counts the frames up to and including the first call's dispatch.
  - The turn bench reads it per run and checks it: verdict `syncs_plain`, history column `syncs_plain`,
    `perf::PLAIN_SYNCS_BEFORE_CALL = 2`.
  - `PLAIN_TURN_FRAMES` 5 → 4 and `TOOL_TURN_FRAMES` 9 → 8. These are lowered, so the keel has no finding.
- kernel-sim holds half of its input turns first (`kernel_sim/held.rs`), with a crash, a cancel or a stop between the
  hold and the admission. Its stop rule (`stop_row`) now allows a held turn's queued and running rows after the stop
  that marked it.
- Tests adjusted to the new counts:
  - `tests_m3::a_plain_turn_stays_within_its_frame_budget`, the frames-counted test (first turn 8 → 7, plain 5 → 4)
    and `tests_inbound`'s judged budget, each 5 → 4;
  - the `<= 5` bounds in `tests_judge`, `tests_judge_surfaces`, `tests_continue`, `tests_rerank`, `tests_push` and
    `tests_recall`, tightened to 4 (all pass);
  - the core output golden, regenerated (`THESEUS_GOLDEN=write`). Its diff is only "each input's admission and node
    are one frame".
- `rpc/tests_ordered.rs`'s frame hold now waits in `theseus_store::blocking`, as the store's writer does.
  - Before, it blocked its worker outright. `reads_and_other_sessions_answer_while_an_input_waits` then timed out on
    `health`.
  - gdb showed the cause: the turn's worker blocked in the hook. With no disk wait (`block_in_place`) before the
    input's frame any more, that worker still held the connection's next request in its run queue.
  - A real slow frame always waits in `blocking`, so this was the test's artifact. The test's assertions are
    unchanged.
- AGENTS.md (root, theseus-core, theseus-sim, scripts) updated to the new counts and the admission's path.

**Proved.**
- New tests:
  - kernel `tests_held.rs` (4): the hold writes nothing and refuses a second turn; the admission rides the caller's
    frame and writes once; a cancel after the hold refuses the admission and writes nothing; a stop after the hold
    marks the turn and the admission keeps it (the plan is refused `Stopped`).
  - core `tests_m3/admit_frames.rs` (4):
    - a resumed session's plain turn: 2 syncs, 4 frames, the first frame exactly
      `[execution.queued, execution, execution.running, turn.started, node]`;
    - a session opened by `session.open`, first turn: 3 syncs (its first compilation's frame is the third);
    - a cancel while the turn is held: fails `execution_cancelled`, no dispatch, input not stored, the slot freed;
    - a stop by the kernel alone while held: `turn_running` true, stopped with no dispatch, parked on input, mark
      cleared;
    - (step 2) a failed input frame.
- Planted reverts, each restored with `cp` and `touch`, with `git status` clean afterwards:
  1. **The old two-frame start** (the admission in its own frame, then the rows and the node). Failed:
     `an_inputs_admission_rides_its_inputs_frame` (3 syncs, not 2), `a_plain_turn_stays_within_its_frame_budget`,
     `frames_counted` (8 vs 7), `tests_inbound::a_judged_turn_keeps_its_frame_budget` (5 vs 4). Before tightening,
     `tests_judge`'s and `tests_judge_surfaces`' `<= 5` let it through, which is why I tightened them.
  2. **The cancel re-read removed** (`admit_held` writes `running` from whatever it reads, with no wake and no state
     check). Failed: kernel `a_cancel_after_the_hold_refuses_the_admission` and
     `a_held_turns_admission_wakes_a_waiting_execution_in_its_frame`; core
     `a_cancel_while_the_turn_is_held_ends_it_with_no_dispatch` (the turn succeeded) and
     `an_inputs_admission_rides_its_inputs_frame`; kernel-sim seed 1, "a held turn on exe_legacy_waiting was admitted
     after its cancel".
  3. **The stop's `holds_turn` removed.** Failed: kernel `a_stop_after_the_hold_marks_the_turn_its_admission_keeps`,
     core `a_stop_while_the_turn_is_held_ends_it_with_no_dispatch`, and both kernel-sim gate tests ("a stop … while
     a turn held it did not count the turn").
- theseus-id9's lost-update test (`tests.rs`, "The lost update theseus-id9 was filed for") and the rest of the kernel
  package pass: all 199 tests, the 4 new ones included.
- kernel-sim, 100 seeds each, all invariants held:
  - `kernel-sim --seed 1 --seeds 100 --p-race 0.3`: 3 min 38 s; 349 held input turns (36 cancelled before their
    admission, 32 stopped, 276 admitted); 1,596 raced turns; 1,421 crashes; 40,200 checks.
  - `--p-race 0`: 584 held (50 cancelled, 52 stopped, 468 admitted).
- Under load (nice 19, with four `while :` loops at nice 0, killed by pid): the new tests,
  `a_plain_turn_stays_within_its_frame_budget`, `frames_counted` and all of `tests_ordered` passed 25 of 25 tests,
  3 runs of 3.
- Crash: kernel-sim's new crash point "after hold" (the turn held, nothing written, then a crash) and the existing
  "after admit" ran in the 200 seeds above. A crash before the input's frame leaves the execution as it was; after
  it, the turn is admitted with its input stored, and the start recovers it as `interrupted`, as any crash mid-turn.
  I wrote no real-SIGKILL daemon test for this step; see the live check.

## Step 2: a failed input frame (e2978a3)

A test (`a_failed_input_frame_writes_neither_half_and_the_turn_ends_waiting`) fails the input's frame with the
fault seam, as a full disk would. Afterwards:
- nothing of the frame is written, no call goes out, and the input is not stored;
- the fault's end admits and ends the held turn in one frame: the execution is waiting, never left running, turns +1;
- the next input runs a turn.

This guards the failure path. It is not an atomicity proof: planted revert 1 does not make it fail, since the end
reached is the same.

## Frames 4 and 5: the accounting decision for the owner (not built)

Merging the provider's completion into the turn's end frame would cut one more frame, but it changes what a crash
leaves:
- **Today:** a crash between the answer and the end frame finds the call settled (completion and node written) and
  the turn interrupted. The next turn reads the answer.
- **Merged:** the same crash finds a dispatched call with no completion. The start's reconcile marks it
  `outcome_unknown` (`earlier.rs`, `in_process_before_restart`) and books its reservation as spent (theseus-f3wr),
  not its real cost. The model's answer is lost, the next turn has nothing to read, and the user is billed the
  worst case.

Whether that trade is acceptable for one sync after the answer is the owner's call.

## The live check for the maintainer

On the install build (`scripts/build.sh --profile release-thin`):

1. `target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --runs 10 --check`: a scratch
   daemon on the stand-in model. It should show `frames_plain: 4 … budget 4: ok`, `frames_tool: 8 … ok`,
   `syncs_plain: 2 … budget 2: ok`, and a plain turn's first frame
   `[ledger:execution.queued, execution, ledger:execution.running, ledger:turn.started, node]`.
2. `target/release-thin/theseus-sim bench lifecycle --theseusd target/release-thin/theseusd --runs 10 --check`:
   `LIFECYCLE OK`.
3. On a scratch daemon with a real model (own `--config`, `--socket`, `--state-dir`):
   - `theseus --socket S ask "hi"` twice in one session, then `theseus --socket S --json ask --session <id> "again"
     | jq .trace.attrs.syncs_before_call` should print `2`. The narrative and `turn.trace` should look as before.
   - `theseus --socket S ask "something slow"` then, from a second shell, `theseus --socket S executions cancel <id>`
     the moment the turn starts: the turn ends `execution_cancelled` or as a normal cancelled turn, never with a
     dispatched call after the cancel.
4. SIGKILL: `kill -9` the scratch daemon during a stream of turns, restart it, and check `theseus executions` and
   `theseus history`. Every execution should be waiting, or interrupted then retried. No session should have an
   execution running with no turn, and no stored input should lack an admission before it.

## Left, uncertain, and for the owner

- **The full one-frame start (1 sync)**, for tool-loop-frames or a row of its own.
  - **The approach I tried:**
    - defer the input's node into the turn's waiting rows;
    - make the turn's transcript read show a waiting node at a pending position (`u64::MAX - 1`, before a pending
      recall's);
    - make `TurnState::commit` feed waiting nodes to `wrote`, so they join the kept transcript;
    - leave pending ids out of a new compilation's `includes`;
    - admit and stage the waiting rows in the first dispatch's frame (`dispatch_admitted`), writing them alone if
      the plan is refused;
    - call `ordered::applied()` once the input lands.
  - **What breaks:** the readers listed at the top. They need the compile's notion of the input (the pending node)
    rather than the store: compaction's prefix and summary, `continue.v1`'s and route's state readers, and tiering.
- **A recompile's frame** (the session's first compile, a ring, a context change) is still a sync before the first
  byte: 3 syncs on a new session's first turn. It could ride the dispatch frame (`persist_compilation` under the
  session lock around `plan_and_dispatch`).
- **A session with no execution** (`execution_for`: an old record, or a test's `put_session`) opens it in two frames
  of its own (the execution, then the session record). `session.open` does it in one.
- **Cosmetic:** a report-reading turn's frame puts `turn.started` before `execution.queued`. Its waiting rows are
  prepended by the store handle, not staged after the admission as the input's frame stages them. The golden shows
  it. The stop's mark on a held turn records the previous turn's number (`turn: e.turns`). Nothing reads it.
- **Docs to update at review:**
  - the spec's Part III item, `docs/status.md`, and `docs/design/roadmap-v2.md`'s turn-one-frame row: frames 5 → 4,
    syncs 3 → 2, the middle way and why;
  - `scripts/AGENTS.md`'s release-profile paragraph still says "A plain turn is … 5 frames" (a measurement of record
    from 2026-10-02; I left it).
- **Siblings:**
  - admission-fifo replaces `admit()`'s wait loop. The held path is the first try only, so the two meet in
    `run_body`'s `admitted` match.
  - job-handles, stop-nudge and money don't touch the input's frame.
- **Store format:** no bump. No stored record gained a field; `syncs_before_call` is a trace attribute and the
  history's column is outside the store.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f654 scripts/gate.sh`.

**Why `THESEUS_KEEL_BASE`:** this clone's `main` and `origin/main` are at a9ad950, older than the branch's base
2fd1f65. The guard's default merge-base then judged main's own history and reported 5 findings that aren't this
branch's. With the base named, the keel reports 0 findings.

On the final commit (e2978a3):
- keel, fmt, shape, features, clippy, cockpit and test build pass.
- The suite ran 3,690 tests: 3,657 passed and 33 failed. All 33 are the known L1 failures: 19 theseus-sandbox
  contract tests, its bench's `spawn_100`, and 13 in theseusd's `sandbox` (root and no job cgroup, theseus-pv6i).
- I ran the phases after the suite myself:
  - protocol types: ok;
  - `theseus-sim bench turn --check --runs 5 --burst 0`: `frames_plain` 4/4, `frames_tool` 8/8, `syncs_plain` 2/2;
  - `cargo deny --offline check`: advisories, bans, licenses and sources ok, after a `cargo deny fetch`. The setup
    chain's fetch had been skipped by an early compile error.
- The first gate run (before step 2) also failed `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`:
  `{"dispatched":2,…}`, expected 1.
  - **Why it isn't this branch's:** the test waits until one action is dispatched, then reads the counts again. In
    between, the continuation that the late result woke can dispatch its model call; the test's own comment names
    that call. A continuation is admitted by the old path.
  - It passed alone, and 5 of 5 times at nice 19 beside four busy loops.

Benches on the final commit (debug build, this 4-core VM; reported, not tuned):
- `bench turn --check` (10 runs and the burst):
  - plain: wall p50 46.2 ms, p95 51.4 ms, daemon p50 39.0 ms, 4 frames;
  - tool-call: wall p50 98.1 ms, p95 114.5 ms, 8 frames;
  - burst: 30 turns in 1,529 ms (51.0 ms a turn), 150 frames;
  - fdatasync p50 0.2 ms; RSS 72.3 MB after the start, 95.3 MB after the burst.
- `bench lifecycle --runs 10 --check`: LIFECYCLE OK.

  | Phase | p50 | p95 | Budget |
  |---|---|---|---|
  | Cold start | 19.9 ms | 28.8 ms | 50 + 7 ms |
  | From the copy | 19.8 ms | 34.2 ms | 50 + 7 ms |
  | Shutdown | 10.9 ms | 32.6 ms | 100 + 4 ms |
  | SIGKILL restart | 20.6 ms | 25.3 ms | 150 + 25 ms |
  | Swap | 23.6 ms | 32.5 ms | 200 + 2 ms |
  | Cancel | 9.7 ms | 13.8 ms | 100 ms |

  The swap's job was adopted, and a cancel took 2 frames.

**Keel findings expected:** none. `python3 scripts/keel-guard.py` with the base named reports "keel: ok … 0
findings". Every count change lowers a budget.

No new dependencies. Cargo.lock and the package-lock.json files are unchanged.
