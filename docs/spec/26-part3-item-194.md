# The Ship of Theseus, chapter 26: Part III, A4's Items 194 to 203 ([index](README.md))
### Item 194. Queue frames: a completion that queues its execution writes `execution.queued {why: result}` in its frame, and a late result's wake rides in its turn's end frame, so a settled wait parked before returns at the late result's turn's end (theseus-2xep and theseus-6qwr; v1.1's step V3; the eighth cloud batch's queue-frames session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, finished 15:19; cad513d6 and a508daff; reviewed 21:20 to 21:40 by local reviewer R20, stack R, and accepted with the stack at 22:57; joined 2026-10-06 00:21 at c44dce47, a signed merge onto 8ea6669e, by the stack-R joiner; installed 10:12 at 21bf5454, install #6)

**Why.** roadmap-v1.1's V3, "every queue writes its row, in its frame":
- **theseus-2xep.** A job's completion that queued its parked execution wrote no `execution.queued` row, only the
  action row's `execution_state`. Every reader of execution rows (the push's prove, the cockpit's time machine, which
  reads states and whys from `execution.*` rows only) missed the queue: in a replay the session stayed "waiting".
- **theseus-6qwr (P3; theseus-jj9f's second shape,** left when the answer was made one frame at step 0owd). A turn whose
  background result landed during it ended parked on input, and then a second frame queued it for the late result
  (`kernel.wake(&exec_id, "late_result")` after the session hold dropped). For one frame every surface read the
  session ready between two turns, and a `session.wait --until settled` parked before returned there, before the late
  result was read. And `run` read `stopped` before the end's lock: a `/stop` landing between that read and the end was
  taken by `end_turn` (parked on input, why `stopped`), and the separate wake then queued the stopped execution anyway.

**What landed** (theseus-kernel's `kernel.rs`, theseus-core's new `turn/end_step.rs`, `turn.rs` and `push.rs`,
theseus-sim's new `kernel_sim/queues.rs`, theseusd's push prove; the merge 13 files, +599 −28; no package, store format
change, protocol type or config key: `result` is a new value of an existing ledger row's `why`, which is
`serde_json::Value` data, not a stored record's field).
- **The completion's queue row** (cad513d6; 2xep). The row belongs in `accept_completion`, not `completion_with`, which
  is only a frame around it: the AWS hands' poller, the turn, the compaction, the kernel's reconcile and kernel-sim
  call `accept_completion` directly. Three lines set `queued` where a `Waiting` execution with a matching
  `Wake::Actions` moves to `Queued`, and push `execution.queued {execution_id, why: "result"}` right after the
  action's row, in the same frame (a completion on an execution already queued or running is no move and writes no
  row). `end_turn`'s arm for results already waiting now says `why: "result"`; `TurnEnd::Requeue` keeps no why
  (nothing in the core ends a turn with it). The push board reads a queue's why from `execution.queued` alone (its old
  fallback, an `action.*` row's `execution_state`, is gone), so a completion that settles while the execution is
  already queued no longer shows `why: result`; theseusd's push prove reads execution rows only (42 state rows, 38
  `execution.changed`, 5 executions, 2 confirm events). The cockpit's time machine now shows `queued · result` with no
  change to the cockpit. **A kernel-sim invariant**: an observer on each kernel the sim builds (and rebuilds at each
  restart) sees every committed frame whole, and fails the run on a frame that moves an execution to `queued` from
  another known state without its `execution.queued` row (a start's requeue of an interrupted turn may say
  `execution.interrupted`). kernel.rs went from 3,008 to 3,016 lines (ceiling 3,030).
- **The end and the wake in one frame** (a508daff; 6qwr). `turn/end_step.rs::end_and_wake` is one `Kernel::frame` on
  the turn's kernel view: it reads the stop **under the frame's lock**, calls `end_turn_with` (which joins the outer
  frame), and when the turn says `rewake` and no stop was read, wakes the execution (`late_result`) in a nested frame
  whose error is logged (`warn`) and takes back only what it staged; the end stands. turn.rs keeps the call and records
  `WokenAgain` only when the wake was staged. A failing wake a test can reach: a cancel that lands during a turn that
  also took a late result leaves the execution terminal, and the nested wake fails `NotRunnable`; returning that error
  would have made the end `Err` and skipped `answer_after_cancel` for the calls the cancel cut. The session listed what
  now comes after the wake (the flush, `outbox.posted`, `announce_closed`, the hold's drop, `judge.after_turn`,
  `pass.after_turn`, the `Parked` line, the admission notice) and why none must come before the same execution's next
  turn. A turn that took a late result writes one frame fewer.
- AGENTS.md lines in theseus-core and theseus-sim.

**How it is proven.**
- **The session's tests.** New `tests_push::a_result_that_queues_its_execution_writes_its_row_why_result` (a
  `proc.run` outlives its 1 s wait and parks the turn; the heartbeat's drain queues it; `action.succeeded` with
  `execution_state: queued` and `execution.queued {why: result}` in one WAL frame, read from the segments), the
  kernel's lifecycle test (the completion's frame 5 records, not 4), end_step's three
  (`the_end_and_the_late_results_wake_are_one_frame`, `a_stopped_turn_queues_nothing`,
  `a_wake_that_cant_happen_takes_back_nothing`), and `tests_push::a_settled_wait_parked_before_a_late_results_end_returns_after_its_turn`
  (a wait until settled, parked after the second turn's running view, is still parked once the board has the end's
  frame and returns only after the late result's turn ends). Goldens: core_output.txt's heartbeat completion frame
  gains the queue row; kernel_frames.txt's `end_turn_with (wait on the job)` gains `"why":"result"`. kernel-sim, 200
  seeds, before and after: at `--p-race 0` the TOTAL lines are identical but for the wall time (5,166 crashes, 23,958
  turns, 34,591 actions, 80,400 invariant checks, all held); raced at 0.3, all invariants held. Under the load recipe,
  10 of 10 runs of `tests_push` and `tests_continuations` green. Plants: the completion's row removed (the new test,
  the push prove, the kernel's lifecycle test, the golden and kernel-sim's invariant at once: "a frame queued an
  execution without its execution.queued row"); the wake back in its own frame; the nested wake's error returned; the
  stop read ignored: each failed. The session found one test timing out under load on main as on the branch,
  `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` (120 s; 8.8 s unloaded).
- **The review** (R20, stacked on turn-stack's review commit 2844936f over main acf26214; review commit 8537ec82): the
  build, **both goldens as merged** (the core's 3 lines beside situations' 146 with no overlap, the kernel's 1), clippy
  `-D warnings`, protocol 31 of 31 and shape clean; theseus-core, theseus-kernel, theseus-sim and theseusd's push prove,
  **1,506 of 1,506**. **3 of 3 planted reverts caught**: the completion's row not written (six tests fail, kernel-sim's
  two seeded runs among them, by the new invariant); the wake back in a frame of its own after the end (end row 91,
  queue row 93); the stop read ignored. kernel-sim before and after at 40 seeds: at `--p-race 0` both TOTAL lines read
  1,077 crashes, 5,010 turns, 7,086 actions, **equal but for the wall time**; raced, all invariants held in all four
  runs.
- **Live, against main as the control** (R20; the report's checks exactly as written: `proc_sync_secs = 1`, `proc.run`
  open, a `sleep 1.8` for the first turn and a sync `sleep 0.95` for the second; scratch daemons of the whole stack's
  build and of main's, fresh state dirs, transient user units, the stand-in model). **A completion's queue says why:**
  on the stack `67 action.succeeded (execution_state queued)`, then **`68 execution.queued {why: result}`**, then the
  driver's turn at 70; on main, `67`, then `69 execution.running`, no queue row. **A late result during a later turn**
  (a wait until settled parked at the second turn's start): on the stack the end's `207 execution.waiting` and `209
  execution.queued late_result` in one frame, the watch's views `151 running → 209 queued late_result → 211 running →
  231 waiting`, and **the wait returned `settled` at 231**, the late result's turn's end; on main the views read `150
  running → 207 waiting → 209 queued late_result → …`, and **the wait returned at 207**, before the late result was
  read.
- **FAST** (R20, the whole stack R against acf26214, one hold, palindrome order): frames 5 and 9 on both arms (the
  bench's turn takes no late result, so it shows neither change), plain +0.8 ms, tool call −2.1 ms; **no cost**.

**The join** (stack R's second; the stack-R joiner). Lock `cloud-queue-frames-join` 23:38:44, right after
turn-stack's done line, queued behind stack T's history-pages lock (23:03:37); clear at 00:00:21 after 21 min 37 s.
R20's dry-run script failed on the moved main (it named the branches by their origin refs, which each join deletes,
and still printed "the same tree … yes"); the joiner's copy named the reviewed heads by hash: on 8ea6669e queue-frames
and smalls clean, and the whole dry-run stack differing from R20's review tree by exactly main's own patch since R20's
base (cli-tests and history-pages, 32 files). The merge onto 8ea6669e at 00:00:21: **clean** (turn.rs, core_output.txt
and theseus-core's AGENTS.md auto-merged); `queue-frames/resolve.py`'s one guarded step for crash-hold (both edit
`accept_completion`'s rows) found crash-hold not on this base and did nothing; 13 files, +599 −28; **the staged tree
1e00d277 equal to the dry run's**; turn.rs 3,496 of 3,523, kernel.rs 3,016 of 3,030. No join fix. The warm (the test
build 3 min 37 s, clippy clean), then theseus-core, theseus-kernel, theseus-sim and the push prove, **1,515 of 1,515**
(R20's 1,506 plus history-pages' nine new rpc tests; the lagging-client test passed in 13.5 s at load 25). The signed
merge **c44dce47** (8ea6669e and 6e01ce18), 00:12:04. Its gate (00:12:11 to 00:21:25, ok; 150 s waiting for the shared
lock behind three reviewers' steps): **2,862 of 2,862** (1 slow, 22 skipped; history-pages' 2,857 plus 5), mid-hour;
lifecycle in every budget (cold start p50 21.5 / p95 25.4 ms; from the config copy 21.6 / 29.3; clean shutdown 30.5 /
47.7; a post in flight 71.4 / 78.5; SIGKILL then restart 25.1 / 32.2; binary swap 47.7 / 54.1; restore 133.4 / 147.7);
L1 start 5.75 / 6.35 ms; turn frames 5 and 9 (plain p50 73.1 ms, tool call 150.1), every p50 at or below the gate
before. Pushed 00:21:35, the branch deleted, done line 00:21:50; theseus-2xep and 6qwr closed with the hash. When
crash-hold joined after it (Item 197), its joiner met the `accept_completion` conflict from the other side.
The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). Every queue says why: a job's result that
queues its parked turn writes `execution.queued {why: result}` in the completion's own frame, so `theseus ledger`, the
push and every reader of execution rows see it; a job that ends while a later turn runs is woken in that turn's end
frame, so the watch reads `running → queued late_result → running → waiting` with no `waiting` between, and a `wait
--until settled` parked before returns at the late result's turn's end. A completion written by an older build replays
with no why on the push board (display only). No config key, store format, protocol type or package. Health after the
restart: `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms (store 4.6, kernel
36.9 ms; load 11 to 19, not a quiet reading), Discord ready, the judge's live packs `security.v3`, `route.v1` and
`rerank.v1` as before, memory live on the `baseline` arm, voice ready, `cgroup: delegated`, the unit active with
NRestarts 0, and no error or warning in the journal; the store from format 22 to 23 at its first write, after the
install's backup.

**Divergences.** The row is written in `accept_completion`, not in `completion_with` as the brief said, so the
direct callers write it too. `TurnEnd::Requeue` keeps no why. A cancel during a turn that also took a late result now
logs one `warn`, where the old `let _ =` was silent.

**Known gaps.** theseus-0u6g (P2, filed by R20): the lagging-client test times out under the load recipe, on main as
on the branch (it passed in all three of stack R's gates, 5.6 to 5.8 s). R20's "For Eddie", each recommended and none
filed: let `run` use the stop `end_and_wake` reads (the outer `stopped` read still has its window for the failure run,
`StoppedAtStep` and the failed notice's class: words, not state; P3); skip the nested wake when the end left the
execution terminal (one `is_terminal()` check), so the `warn` stays for the unexpected; leave `TurnEnd::Requeue` without
a why until a caller appears (`yield` then); accept that an older build's completions replay with no why.

### Item 195. Smalls: publish records a round's progress with its states, the TUI keeps another surface's message in its written place, the budget question's loop writes its `loop.ended` row, and the static musl build compiles again (theseus-5ihy, theseus-u8ig, theseus-v6yc and theseus-nhg4; the eighth cloud batch's smalls session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5; 65300118, a76f3860, 2df912b6 and e3428929; reviewed 21:35 to 22:00 by local reviewer R20, stack R, with the stack's suite, FAST, live and static-build steps to 22:50, and accepted with the stack at 22:57; joined 2026-10-06 00:41 at c07bbe6a, a signed merge onto c44dce47, by the stack-R joiner; installed 10:12 at 21bf5454, install #6)

**Why.** Four P3s, one commit each:
- **theseus-5ihy.** Seen in the sparse-config lane's live check (2026-10-03): `theseusd check` once printed "8
  secret(s) resolved in 0 ms (nothing to fetch)", eight secrets ready with no settle time and no method.
- **theseus-u8ig.** The static musl build (`bench/build.sh`, `scripts/build.sh --target x86_64-unknown-linux-musl`,
  for the benchmark containers) warned on a deprecated `libc::time_t` in theseus-core's `wake.rs`.
- **theseus-v6yc.** In theseus-tui's detail pane an operator's message written by another surface (Discord, the CLI)
  was read when its `node.written` arrived (`session.history { n: 5 }`, then the node by id) and appended when the
  answer came back: after any reply that had streamed meanwhile.
- **theseus-nhg4.** One of two record nits C2 kept byte-identical (theseus-j6qn): a loop that ends on the budget
  question wrote no `loop.ended` row. (The other, a vault-wait line's stray spaces, had already left with the vault's
  act gate, theseus-zmgb.)

**What landed** (theseus-core's `secrets.rs`, `wake.rs`, `fact/turn.rs`, `turn.rs`, theseus-store's `pressure.rs`,
theseus-tui's `detail.rs` and `app.rs`, theseusd's `main.rs`; new `tests_budget_loop.rs` and theseus-tui's
`tests_order.rs`; the merge 14 files, +547 −41; no package, store format change, protocol type or config key).
- **Publish's order** (65300118; 5ihy). The race, proven: `SecretBoard::publish` sent the states (`send_modify`, which
  wakes every `settle_all` waiter) and only then locked `progress` to set the rounds, method and settle time, so a
  waiter reading `status()` between the halves saw `ready` with `method: None`, `rounds: 0`, `settled_ms: None`.
  `check` printed exactly that as "resolved in 0 ms (nothing to fetch)", and the same read feeds the
  `secrets.resolved` row and the startup log's secrets phase. Now publish writes the round's progress inside the
  `send_modify` closure, after inserting the states and before the send notifies anyone; the closure runs under the
  watch's write lock, so the states and their progress land together. The lock order is watch-write → progress
  (`status` drops its watch borrow before it locks `progress`; `begin_round`, `retry_at` and `started_at` lock
  `progress` alone). `check`'s line moves into `resolved_words(&SecretsStatus)`, byte-identical.
- **The static musl build** (a76f3860; u8ig). wake.rs's seconds go through `(unix_ms / 1000).try_into()`, the type
  inferred from `localtime_r`'s argument, so no `time_t` alias is named, and a count that does not fit takes the
  existing UTC path instead of wrapping. **A second musl break the issue had not named:** theseus-store's
  `pressure::idle_this_thread` (b334e485, theseus-tood) built `libc::sched_param { sched_priority: 0 }`, and musl's
  `sched_param` has four more fields, so on main the static build failed to compile (`E0063`), not just warned; it is
  zeroed now (`std::mem::zeroed()`, `sched_priority` 0 on both libcs).
- **The TUI's order** (2df912b6; v6yc; theseus-tui only). `Detail` keeps `places: Vec<(node_id, Option<usize>)>` in
  the order the `node.written`s came: another surface's message marks its place after every line so far, and the mark
  closes the reply's open line, so what streams next starts below it and no line is ever split; the fetched node is
  spliced in at the mark, later marks at the same place move down (two messages keep their written order), marks move
  up when lines past `KEPT` drop (one whose line went puts its message at the end), a read that finds nothing or fails
  drops its mark, and a reload clears the marks. The TUI's own message is unchanged: it shows at once.
- **The budget loop's row** (e3428929; nhg4). `LoopEndedOnBudget` had no `KIND`, so it sent the notification and
  wrote no row. It gets `KIND = LoopEnded` and a row in `LoopEnded`'s shape: `{"loop": i, "outcome": {"loop_index": i,
  "provider_stop_reason": null, "tool_calls": 0, "output_chars": 0}, "advancer": "budget", "decision": {"decision":
  "budget"}, "usage": <all zero>}`. The record moved from `ask_budget` to the model call's over-budget exit, because
  `ask_budget`'s other caller, a hands group over the budget, is checked before any loop opens, where the fact named a
  loop that had already ended or never started: **that path now sends no `loop.ended`** (a client counting ends per
  start was miscounting). A row of an existing kind rides in an existing frame: no frame added, no format bump.

**How it is proven.**
- **The session's tests.** `secrets::tests::a_waiter_woken_by_a_publish_reads_its_method_and_time` forces the
  interleaving (a test-only thread-local hook in `publish` right after the send blocks until a waiter on a second
  thread, parked in `settle_all` on its own runtime, has read `status()`), so it cannot pass by timing; theseusd's
  `checks_secrets_line_names_the_rounds_method_and_time`. `cargo check --target x86_64-unknown-linux-musl -p
  theseus-core` clean, with a plant of the old wake.rs line giving the deprecation warning back (`bench/build.sh` not
  run to the end: the VM had 2.4 GB of disk free). `tests_order.rs` (5: another surface's message above the turn and
  its reply; two answered in reverse order keep their written order; a not-found or failed read leaves the pane as it
  was; `KEPT` moves a place up or sends a dropped one to the end; the TUI's own message at once); the TUI's suite 32.
  `tests_budget_loop::the_loop_that_asks_the_budget_question_writes_its_loop_ended_row` (a $1.40 limit; the turn ends
  `budget` at 2 loops with 2 `loop.started` and 2 `loop.ended` rows); the core golden moves exactly two lines in the
  budget scenario. Plants (publish's old order; the node always appended; `KIND` removed) each failed. Under the load
  recipe, 20 runs, 20 of 20.
- **The review** (R20, stacked on queue-frames' review commit 8537ec82; review commit d65be6ce): the build, **the core
  golden as merged** (smalls' 2 lines beside queue-frames' 3 and situations' 146), clippy `-D warnings`, protocol and
  shape clean; theseus-core, theseus-tui, theseusd and theseus-store **1,612 of 1,612**; **the whole workspace suite on
  the stack R, 2,854 of 2,854** (`--retries 0`, no `RUST_MIN_STACK`, started 22:06 inside the hour, two other
  reviewers' trees building beside it; 307 s). **3 of 3 planted reverts caught**: publish's old order (`method: None`
  for `Some("inject")`); the fetched node appended at the end (4 of 5 order tests fail; the own-message test passes, as
  it should); `KIND` removed (the row test, 1 `loop.ended` for 2 loops, and the golden). **u8ig, for real:**
  `bench/build.sh` on the merged stack, **exit 0 in 6 min 2 s**, release-thin, no warning and no error (`time_t`
  nowhere in the output), **theseus 5,867,496 bytes and theseusd 59,352,288 bytes, each static-pie**, each answering
  `--version`; main's `sched_param` literal planted back fails `cargo check --target x86_64-unknown-linux-musl -p
  theseus-store` with E0063. The binaries were deleted after.
- **Live, against main** (R20; scratch daemons of the stack's frozen build and main's, fresh state dirs, transient user
  units, the stand-in model, configs written by a script, the port checked first). **nhg4:** with `[kernel]
  spend_limit_usd = 0.01`, `ask "hello"` ends `budget` (needed $1.31, available $0.01): the stack writes one
  `loop.ended` row (advancer and decision `budget`, no stop reason, zero usage), main none (1 `loop.started`, 0
  `loop.ended`). **5ihy:** `theseusd check` on a config with two `env:` secrets, 350 runs an arm, alternating: **main
  said "2 secret(s) resolved in 0 ms (nothing to fetch)" 3 times, the stack never** (every line "(local)", 2 to 17 ms);
  each daemon's `secrets.resolved` row read method `local`, 7 ms, 1 round. **v6yc:** the stack's `theseus-tui` in a
  detached tmux pane with the session focused, then ten asks from another client: **all ten operator lines sit right
  under their turn's line and above its reply** (main's TUI was not frozen for a control; the plant shows the old order
  fails).
- **FAST** (R20, the whole stack R against acf26214, one hold, palindrome order): frames 5 and 9 on both arms, plain
  +0.8 ms, tool call −2.1 ms; **no cost**. smalls adds nothing on the start path or a turn's: a row in a frame that
  exists, a lock taken inside a send that happened anyway, `check`'s words, and the TUI's pane.

**The join** (stack R's third; the stack-R joiner). Lock `cloud-smalls-join` 00:12:42, while queue-frames' gate ran,
queued behind it; clear at 00:22:11 (9 min 29 s). The dry run on c44dce47 (00:12:53): smalls clean, format 22, the
core's 80 test `mod` lines sorted. The merge onto c44dce47 at 00:22:11: **clean** (lib.rs, turn.rs, core_output.txt
and theseusd's main.rs auto-merged; no resolve script); 14 files, +547 −41; **the staged tree 9f94cf1c equal to the dry
run's**; turn.rs 3,496 of 3,523. No join fix. The warm (the test build 3 min 2 s, clippy clean), then theseus-core,
theseus-tui, theseusd, theseus-store and the kernel's frames golden, **1,623 of 1,623** (R20's 1,612 plus
history-pages' ten new tests and the golden). The signed merge **c07bbe6a** (c44dce47 and 0084847a), 00:31:56. Its gate
(00:31:57 to 00:39:58, ok; 107 s waiting for the shared lock behind three reviewers' steps): **2,870 of 2,870** (1 slow,
22 skipped; queue-frames' 2,862 plus smalls' 8), mid-hour; lifecycle in every budget (cold start p50 23.6 / p95 29.7
ms; from the config copy 22.6 / 32.0; clean shutdown 35.5 / 53.5; a post in flight 75.3 / 80.8; SIGKILL then restart
27.4 / 31.9; binary swap 49.7 / 66.2; restore 144.5 / 152.7); L1 start 6.51 / 7.40 ms; turn frames 5 and 9, plain p50
80.6 ms and tool call **173.5** (the gate before: 73.1 and 150.1). **Read as neighbour load, not smalls:** the bench
history's load column read 7.16 and 7.45 at this gate's benches, against 3.2 to 4.1 at the four gates before; two
reruns of the turn bench on the same build right after, at load 14, read plain 110.4 and 151.0 ms and tool call 247.4
and 236.6 (frames 5 and 9), so the same binary reads far slower as load rises; and R20's whole-stack A/B, smalls
included, found no cost. The joiner did not A/B smalls alone (no frozen c44dce47 build existed). Pushed 00:41:30, the
branch deleted, done line 00:41:47 (stack R complete); theseus-5ihy, u8ig, v6yc and nhg4 closed with the hash. With
u8ig on main a static build of main is possible again, which the async, efficiency and b5 held-out bench runs had
waited for (the chain log, 00:51). The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). `theseusd check` names the method ("(local)",
"(inject)") and never "resolved in 0 ms (nothing to fetch)"; another surface's message lands in the TUI's pane where it
was written, above the reply that streamed meanwhile; a loop that ends on the budget question writes its `loop.ended`
row (the cockpit prints it as "loop 1 ended: , 0 tool calls → budget" until a one-line follow-up in summary.ts).
Installs stay glibc (`release-thin`). No config key, store format, protocol type or package. Health after the restart:
`theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms (store 4.6, kernel 36.9 ms;
load 11 to 19, not a quiet reading), Discord ready, the judge's live packs `security.v3`, `route.v1` and `rerank.v1` as
before, memory live on the `baseline` arm, voice ready, `cgroup: delegated`, the unit active with NRestarts 0, and no
error or warning in the journal; the store from format 22 to 23 at its first write, after the install's backup.

**Divergences.** The musl fix reached `pressure.rs`, outside the task's named files (four lines, no sibling touching
it). The budget row's `decision` is `{"decision": "budget"}`, not `Decision::EndTurn("budget")`, matching the
notification. The hands path no longer sends a `loop.ended`. The mark closes a reply's open line, so a reply mid-line
when another surface's message arrives goes on below it.

**Known gaps.** R20's "For Eddie", each recommended: a one-line follow-up in `cockpit/src/lib/summary.ts` for the new
row's line (say "before its call" when there is no stop reason); accept the mark closing the open line, the budget
decision's shape and the hands path's silence. A musl `cargo check --target x86_64-unknown-linux-musl -p theseus-core`
catches libc differences (a struct literal of a libc type breaks there); better still a CI step, since main's static
build failed to compile for days without anyone seeing it (CI's workflow builds no musl target). No issue filed for
these.

### Item 196. WAL mark skip: a start skips the WAL directory's sync when the index's checkpoint or a mark vouches for the found segment, and a store an older format wrote still syncs it once (theseus-3q29; the eighth cloud batch's wal-mark-skip session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, its report at 14:25; e4818dc6 and d87ffc32; reviewed 21:42 to 23:26 by local reviewer R21, stack S, with one P3 finding, and accepted with the stack at 23:52; joined 2026-10-06 01:13 at 9a7faa87, a signed merge onto c07bbe6a, by the stack-S joiner; installed 10:12 at 21bf5454, install #6)

**Why.** theseus-3q29 (P2, FAST), from R9's review of wal-sync (Item 176). Since c67g (Item 176) an open that finds
its last segment syncs the log's directory with its first frame, once, so a segment a dead process created is as
durable as its frames: one more sync wait before serving, on every start. On a quiet disk that directory fsync costs
about nothing (0.18 ms in c67g's report), but R9 measured +12.5 ms on every start of a busy pair, since its price is
the disk's journal: nothing when idle, a full flush when another writer has metadata pending. The open already knew,
in most starts, that the found segment's name had been synced: xprd's rule (Item 52) syncs a new segment's name before
any frame in it is reported durable, so a synced position in the found segment proves its name was synced.

**What landed** (theseus-store's `wal.rs`, `store.rs`, new `wal/tests/vouch.rs`, its AGENTS.md; the merge 5 files,
+343 −47; no package, no store format change, nothing new stored, no protocol type or config key).
- **The skip** (e4818dc6). `open_from` already computed `synced = max(synced_to, walk.mark)`, capped at the last
  position found, and a frame's mark is below its own position, so `synced >= first position of the found segment` is
  the whole rule. `Walk` now keeps `first`, the first position of the segment walked last when it was walked from offset
  0 (the found segment's, after either walk). `fn vouch` decides `Recovery::vouched: Option<Vouch>` (`Checkpoint |
  Mark`): the tail-only open that began in the found segment (the checkpoint's record is there) is `Checkpoint`; else,
  for a found segment with a frame (`first <= last`), the checkpoint (capped at `last`) at or past `first` is
  `Checkpoint`, the open's `synced` at or past `first` is `Mark`, and anything else is `None`; an empty found segment
  is never vouched for. `append_segment` leaves the log's directory out of `unsynced_dirs` when vouched, and the open
  logs `wal: the found segment's name is vouched for` at debug level. The walk of every segment moved, unchanged, into
  `walk_every` (clippy's 100-line limit).
- **The close** (d87ffc32). `open_once` reads `behind` (the manifest's format below this build's) before the WAL opens;
  when set, the log's directory joins `name_dirs` for the first frame's sync (deduped by `sync_with_first_frame`), so a
  store an older build wrote, whose marks may be a pre-c67g build's that synced no found segment's name, pays the sync
  once. Six lines; `MANIFEST_FORMAT` untouched.
- theseus-store's AGENTS.md: the invariant "a segment's name is as durable as its frames" now describes the skip.

**Why the rule holds** (R21, read on the merged tree). `Wal::sync` runs the fdatasync, then each pending directory's
`sync_all` (under `unsynced_dirs`' lock), and only then `durable` and `synced.fetch_max(last)`: a sync that fails at the
fdatasync cuts and returns before the directories (they stay pending), and one whose directory fsync fails returns
before the mark moves. So since xprd no frame can carry a mark at or past a new segment's first position before that
segment's name was synced. The checkpoint is taken under `appending` after the writer's sync and index, so it claims
only synced positions. A checkpoint past the WAL's last position vouches at the WAL, but `open_once` then refuses the
open ("index checkpoint N is past the WAL's last position M"). Copies of a WAL (`theseusd restore`, `--repair`, the S3
restore) sync every copied segment and the restored directory before the open (theseus-ez3), so no production path
opens an unsynced copy; every production open goes through `open_once`.

**How it is proven.**
- **The session's tests.** `vouch.rs`'s five, each counting the directory syncs at the first and second frame: two
  synced batches in the found segment, `Mark`, (0, 0); one batch with the checkpoint at its position, `Checkpoint`,
  (0, 0); the tail-only open with the checkpoint in the found segment (`synced_to = 0`), `Checkpoint`, and with a mark
  there, `Mark`; a mark and a checkpoint vouching only for the segment before, `None`, (1, 0), in both walks; an empty
  found segment 3 with every position checkpointed, `None`, (1, 0). The store's
  `a_store_an_older_format_wrote_syncs_its_logs_name_with_the_first_frame` (reopened: `Mark`, 0 syncs; the manifest set
  to `MANIFEST_FORMAT - 1`: 1 at the first frame and the manifest moved; reopened again: 0). c67g's and xprd's tests
  keep their 1. Plants (any mark vouching; the found segment never synced; vouching ignored; `behind` ignored) each
  failed. strace on scratch daemons, counting `fsync(...store/wal>)` on the second start: main 1 after a clean stop and
  1 after a SIGKILL, the branch 0 and 0 (a kill leaves no checkpoint, but the later batches' marks are at least the
  segment's first position); a manifest set back to format 19: 1, and the manifest moved. `theseus-sim crash-test
  --restarts 8 --writers 4` and `--iterations 50 … --tear true --seed 7`: 0 committed records lost. Under the load
  recipe, theseus-store's tests 5 times, 80 of 80 each.
- **The review** (R21, on main acf26214, store format 22; review commit aa6d67b2): the build, clippy `-D warnings`,
  protocol 31 (protocol.gen unchanged) and shape clean; theseus-store, -follow and -index whole, **162 of 162**; **the
  whole workspace suite on the stack S, 2,860 of 2,860** (`--retries 0`, 22:36 to 22:41, the L1 tests included).
  **3 of 3 planted reverts caught**, independent of the report's: a mark one short of the found segment vouching (3
  fail); the close removed (the older-format test); any tail-only open vouched for by the checkpoint wherever its
  record lies (3 fail). **R21's probes:** a SIGKILL right after a segment's creation, before its first sync, reopened in
  both walks: `vouched None`, the sync paid (**caught**); a checkpoint past the found segment's end: vouched at the WAL,
  refused by the store (**caught at the store**); a store behind pays once (the test and strace); and **the close's
  gap, missed** (below).
- **Live** (R21; scratch daemons of main's frozen build and the branch's, fresh state dirs, the stand-in model, a first
  start and a turn, then a clean stop or a SIGKILL, then a second start under `strace -f -e trace=fsync,fdatasync`).
  Main's second start went `fdatasync(index.redb)`, `fdatasync(000000001.seg)` (the startup frame), `fsync(store/wal)`;
  **the branch's had no directory fsync anywhere**, after a clean stop or a SIGKILL; a store set one format behind
  synced it once, its manifest moving from 21 to 22.
- **FAST: a gain, no cost** (R21; A main acf26214 and B the whole stack S, frozen debug builds, one hold 22:32 to 22:35,
  palindrome order, IO PSI 0.2 to 7.6, load 7 to 12). The turn: frames 5 and 9 on every run, plain A 91.1 / B 93.0 ms,
  tool call A 186.4 / B 187.0, inside the spread. The lifecycle, B − A at p50: cold −3.1 ms, vault −10.6, shutdown
  −5.3, in flight −5.6, kill −15.8, swap −15.4. The daemon's own clock (the kernel's `accepting` step, the startup
  frame's write and sync): in the busier pair A 10.2 to 19.4 ms against B 8.1 to 10.4, in the quieter about equal.
  **Under slow syncs** (the IO-stall rig: device-mapper's delay target, 150 ms flushes, under a dedicated ext4; `bench
  lifecycle --phases cold,kill`, A B B A): **main paid a second flush at 4 of 20 restarts** (`accepting` 253, 395, 340
  and 310 ms against about 172 for one), **the branch at none**; a clean restart's time to serving p50 379 against
  346 ms.

**What the review found.** **theseus-xva3 (P3):** the close keys on the manifest, and `upgrade_manifest` moves it
durably *before* the writer's first frame, whose sync carries the directory's. If that sync fails, or the process dies
in the few milliseconds between, the next start reads the manifest as current, `behind` is false, and old marks vouch:
R21's probe (a store set a format behind, opened, its first frame's sync failed, reopened) read "manifest moved anyway
true, dir syncs in that process 0; the next open vouched Some(Mark), dir syncs at its first frame 0". It could lose a
name only with all of: a found segment whose creator died before its first sync, a pre-c67g build's false marks
appended to it, that crash, and a power loss before the next roll on a filesystem whose file fdatasync does not commit
the new entry (ext4's ordered journal does). The fix is small and free: sync the log's directory inside
`upgrade_manifest`, before the manifest moves. **The format-20 window** the report named (a store last written by a
build in d8ac9b54 alone, format 20 before c67g) is closed: d8ac9b54 stood alone on main for 13 minutes and was never
installed; installs #4 and #5 carried c67g, and the stack joined at format 22, so any format-20 or -21 store is behind
and pays once.

**The join** (stack S's first; the stack-S joiner, 2026-10-05 23:52 to 2026-10-06 01:47). The joiner took its first
lock only once stack R's smalls had one (`queue-ready.sh`): `cloud-wal-mark-skip-join` at 00:21:41, clear at 00:41:47
after 20 min 11 s. Its own dry-run script (R21's, taking the slugs still to join and comparing each step's tree, and the
result against R21's reviewed stack merged onto the same base) ran clean on every base from 8ea6669e to fc96e2da. The
merge onto c07bbe6a at 00:41:52: no conflict (store.rs auto-merged beside main's format bumps), 5 files, +343 −47,
**the staged tree 22b4c696 equal to the dry run's**; wal.rs 2,430 lines. No join fix. The joiner's own wait loop cut
the take script off right after `git rerere` (a merging command inside a timed loop); the merge was complete and staged,
and `take-rest.sh` ran the remaining checks, which all held. The warm (the test build 6 min 14 s: theseus-store sits
under most of the workspace), then theseus-store, -follow, -index and the core's golden, **165 of 165**. The signed
merge **9a7faa87** (c07bbe6a and 9b0d911a), 00:52:35. With two reviewers holding the gate lock at 00:52:47, a :52 start
risked a suite across 01:00 (theseus-5a50), so the gate waited for minute :01; it then waited 392 s for the lock, so a
:52 start would indeed have crossed the hour. Gate 01:01:00 to 01:13:21, ok: **2,876 of 2,876** (1 slow, 22 skipped;
smalls' 2,870 plus 6), 01:08:13 to 01:12:46; lifecycle in every budget (cold start p50 22.2 / p95 26.1 ms; from the
config copy 22.8 / 25.1; clean shutdown 59.0 / 73.6; a post in flight 72.4 / 86.9; SIGKILL then restart 30.1 / 43.0;
binary swap 48.3 / 53.6; restore 135.8 / 147.9); L1 start 6.12 / 8.23 ms; turn frames 5 and 9 (plain p50 76.0 ms, tool
call 161.8). The SIGKILL restart's 30.1 / 43.0 came right after the 392 s wait beside reviewers' runs; the next two
gates, the same code plus the rest of the stack, read 26.1 / 30.1 and 26.4 / 27.5, and the daemon's own serving p50 was
19.45 ms against smalls' 19.80: no FAST cost. Pushed 01:13:36, the branch deleted, done line 01:14:00; theseus-3q29
closed with the hash, theseus-xva3 left open. The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). A start no longer syncs `store/wal` when the
index's checkpoint or a mark vouches for the found segment. Install #6's build is format 23 (soul-import,
Item 201) and Eddie's store was 22, so by the close's rule its first start under that build syncs the
directory once and later starts skip it (not measured on his daemon). No config key or protocol type.
Health after the restart: `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms
(store 4.6, kernel 36.9 ms; load 11 to 19, not a quiet reading), Discord ready, the judge's live packs `security.v3`,
`route.v1` and `rerank.v1` as before, memory live on the `baseline` arm, voice ready, `cgroup: delegated`, the unit
active with NRestarts 0, and no error or warning in the journal; the store from format 22 to 23 at its first write,
after the install's backup.

**Divergences.** Small, from the brief: `open_from` passed clippy's limit, so its every-segment walk moved into
`walk_every`, unchanged. The tail-only open's checkpoint vouches by itself, whatever `synced_to` says, as the brief
asked; the store always passes the checkpoint as `synced_to`, so only a caller passing `at` alone meets that branch, and
a test holds it. On a behind store `Recovery::vouched` still reports `Mark` (it says what the WAL found), while the
store syncs anyway.

**Known gaps.** theseus-xva3 (P3), R21's finding: fix after the join, by syncing the log's directory inside
`upgrade_manifest` (the same cost, one directory fsync on the first start after an upgrade). Not a blocker; Eddie's
store is not exposed (every start since install #4 ran c67g). A quiet-machine re-run of the A/B, and a slow-sync run
beside a writer on the same mount (which would show main paying the flush every time), were left optional.

### Item 197. Crash hold: an earlier process's in-flight provider call is booked as spent at its reservation when the restart marks it unknown, so a reset frees it, where main held it for good (theseus-f3wr; Eddie's decision of 2026-10-05 13:12, tactic A; the eighth cloud batch's crash-hold session, the DM thread's own sixteenth row, fired 2026-10-05 13:21 from 60b43fb6, Opus 5.5, its report at about 14:35; ca9c1be5, b22e0c91 and f7c76bcb; reviewed 22:15 to 23:26 by local reviewer R21, stack S, and accepted with the stack at 23:52; joined 2026-10-06 01:22 at fc96e2da, a signed merge onto 9a7faa87 with crash-hold's resolve script for queue-frames' block, by the stack-S joiner; installed 10:12 at 21bf5454, install #6)

**Why.** theseus-m9iy (Item 173) settles a provider call an earlier process had in flight as `outcome_unknown` at the
driver's first tick after a restart (reason `in_process_before_restart`). Its reservation was then held unknown
(`hold_reservation_in` moves it into `held_unknown_micros`), and nothing could free it: no completion will come, and a
reset leaves held money alone (§3.10, Item 54). So each crash with a call in flight shrank the session's room for good,
and a session that needed that room was stuck until its limit was raised. theseus-f3wr (filed 2026-10-05 08:02,
waiting for Eddie) asked whether to book it as spent instead. **Eddie, 13:12:** "follow your recommendation": tactic A,
an earlier process's in-flight call is booked as spent at its unknown mark, the reservation as an estimate, for the
earlier-process reason only. The request reached the provider before the crash and may have been charged, and no
completion can bring its cost.

**What landed** (theseus-kernel's `kernel.rs`, `earlier.rs`, `types.rs` and tests; theseus-sim's stand-in; the merge 8
files, +389 −46; no package, store format change, protocol type or config key: the marker lives in the action's
existing `detail: Option<Value>` and in a row's `data`).
- **The kernel books it** (ca9c1be5). `accept_completion(c)` is `accept_completion_as(c, false)` and `mark_unknown(id,
  reason)` is `mark_unknown_as(id, reason, false)`. With `book`, and only on a first `Unknown` (`book && !was_unknown &&
  c.outcome == Unknown`), the reservation arm calls `book_reservation_in`, which is `settle_reservation_in` at the
  reservation's own amount: spent rises by it, reserved falls by it, held is unchanged. The action gets `detail =
  {"cost_basis": "reservation"}`, and its `action.outcome_unknown` row `cost_basis` and `cost_usd` = the reservation
  (it was `null`). `earlier.rs`'s `mark_earlier_calls_unknown` is the one caller that passes `true`; its helpers
  (`booked`, `mark_booked`, `booked_row`, `book_reservation_in`, `resolve_in`) and a module paragraph say why. **Every
  other unknown mark holds as before**: `overdue_no_evidence` in the reconciler, `wrapper_lost` in the driver,
  `interrupted_by_restart` in `toolrun/resume.rs`, and the AWS hands' overdue and poller marks all reach
  `accept_completion_as(.., false)`; the kernel's frames golden is byte-identical. **Counted once:** the resolve arm
  (a real outcome arriving later) and the late-after-cancel arm share `earlier::resolve_in`: a held call does exactly
  what it did before; a booked call keeps the estimate, takes nothing held, and books only a cost above the reservation
  (a real cost is never hidden, and only a reset lowers spend). No producer can resolve an earlier process's provider
  call, but the heartbeat's reconcile re-probes every `OutcomeUnknown` action, so both arms go through the helper. A
  task's parent gets the booked amount through the usual `carry_to_parent`, from the child's spend delta, and its carve
  shrinks by the same amount: booked once, nowhere else.
- **Who can book** (R21, read on the merged tree): `Kernel::earlier` is filled only by `note_earlier`, called only from
  startup's `reconcile_with(.., due = false)` (once per process, `Core::new`), and only for a dispatched, not-overdue
  provider call `Evidence::in_process` names; the mark re-reads each action under its lock and skips one no longer
  `Dispatched`. So nothing books a call this process still runs.
- **Four tests** (b22e0c91): the reset frees it; an overdue mark still holds; a late resolution counts it once; a
  task's parent counts it once; the existing marking test now asserts `(spent, reserved, held) == (100, 0, 0)` and the
  marker.
- **The stand-in's hold** (f7c76bcb; outside the brief's files, for the live check): a rules `Rule` takes `hold_ms`
  (default 0), and a rules stand-in answers each connection on its own thread, so one held call holds nothing else. A
  60 s hold meets the daemon's 60 s first-byte timeout, so the check uses 45 s.

**How it is proven.**
- **The session's tests:** theseus-kernel 162 (158 on main plus 4); theseus-core's budget, task, reset and held tests,
  93. `a_booked_calls_late_resolution_counts_it_once`: two booked calls (500 and 300) and one held overdue call (700)
  start at `(800, 0, 700)`; a resolution at 200 keeps it, one at 450 gives `(950, 0, 700)`, the held call's at 100
  `(1050, 0, 0)`. `a_tasks_earlier_call_is_booked_once_in_its_parent_too`: a task that had spent 4,000 with a 10,000
  call in flight reads `(14000, 0, 0)` after the mark, the parent's spend 14,000, its carve 26,000 → 16,000, its
  `available()` unchanged. Plants (the mark settling with no cost: 4 fail; every unknown mark booking: 4 fail, the
  frames golden among them; the parent booked twice) each failed. Under load, the 12 kernel tests matching earlier,
  booked, overdue or reset, three runs, 12 of 12. The session's own live run on debug builds: after a SIGKILL with a
  call held in flight, spent $1.3071 and held $0; the row `"cost_basis":"reservation","cost_usd":1.307098`; then a
  clean stop with the retry in flight, the limit lowered to $2, spent $2.6142 (two bookings), and an approved reset
  freeing everything (spent $0.0002 after the next call, resets 1).
- **The review** (R21, onto wal-mark-skip's review commit aa6d67b2 over main acf26214; review commit a173e46d): the
  build, clippy `-D warnings`, protocol 31 and shape clean; **199 of 199** (theseus-kernel whole, 162; the stand-in's
  4; theseus-core's budget, held, reset and earlier tests, 33, `tests_m3::a_reset_that_cannot_free_what_is_held_unknown_asks_once_and_does_not_loop`
  among them); the stand-in's 1.5 s timing test 10 of 10 at nice 19 beside 16 busy loops (1.58 to 2.93 s); **the whole
  workspace suite on the stack S, 2,860 of 2,860**. **3 of 3 planted reverts caught**, different from the report's: the
  booking dropped in the kernel's arm (4 fail: `(0, 0, 100)` against `(100, 0, 0)`, `(0, 40000)` against `(40000, 0)`,
  …); a late resolution booked twice (`(1000, 0, 700)` against `(800, 0, 700)`); the reconcile's `overdue_no_evidence`
  mark booking a call of this process (3 fail).
- **Live, both builds** (R21; scratch daemons of main's frozen acf26214 and of the stack through crash-hold, fresh state
  dirs, transient user units, the stack's stand-in in both arms holding the answer 45 s; `theseus budgets` as spent,
  reserved, held, lifetime, resets):

  | step | main | the stack |
  |---|---|---|
  | a turn's call in flight | (0, 1.3072, 0, 0, 0) | (0, 1.3072, 0, 0, 0) |
  | SIGKILL, restart, the driver's first tick | **(0, 0, 1.3072, 0, 0)**: held | **(1.3072, 1.3072, 0, 0, 0)**: booked (the retry in flight again) |
  | the `action.outcome_unknown` row | `reconciler:in_process_before_restart`, no `cost_basis`, `cost_usd` null | the same producer, `cost_basis: reservation`, `cost_usd` 1.307224 |
  | a clean stop with the retry in flight, limit $2, restart | (0, 0, **2.6144**, 0, 0) | (**2.6144**, 0, 0, 0, 0), 2 booked rows |
  | the budget question | "a reset leaves it held, so resetting its spend to $0 cannot make the call fit. Raise `[kernel] spend_limit_usd` above $3.92 …" | "This session has spent $2.61 of its $2 limit. Reset its spend to $0 and continue?" |
  | approved (the reset) | (0, 0, **2.6144**, 0, 1): still stuck | **(0.0002, 0, 0, 0.0002, 1)**: the call answered at once |

- **FAST:** nothing on the start path. Startup's reconcile only notes the calls, as before (`due = false` writes
  nothing); the booking is a few additions inside the frame the mark already wrote, at the driver's first tick after
  serving (or the heartbeat); on the turn path `accept_completion` passes one `false`. The whole stack's A/B (R21, in
  Item 196): turns unchanged (frames 5 and 9), every lifecycle phase at or below main's at p50. **No
  cost.**

**The join** (stack S's second; the stack-S joiner). Lock `cloud-crash-hold-join` 00:53:01, during wal-mark-skip's wait
for :01; clear at 01:14:00. The merge onto 9a7faa87 at 01:14:06: **one conflict, theseus-kernel's `kernel.rs`**,
`accept_completion`'s ledger push, where queue-frames (Item 194) had joined first: R21's guarded
`crash-hold/resolve.py` ("crash-hold's row data, then queue-frames' ExecutionQueued row; kernel.rs 3,025 lines of its
3,030 ceiling; markers left: 0"), the block reading `let mut data = …; if book { crate::earlier::booked_row(&mut data,
&a); } frame.push(…)`, then `if queued { … LedgerKind::ExecutionQueued … }`, both before `self.commit(&frame)`; rerere
recorded it; theseus-sim's AGENTS.md auto-merged; 8 files, +389 −46; **the staged tree 9d004e31 equal to the dry
run's**. No join fix. The warm (the test build 47.8 s on a quiet machine), then theseus-kernel whole, the stand-in's
tests, the core's budget, held, reset and earlier tests and the core's golden, **215 of 215**. The signed merge
**fc96e2da** (9a7faa87 and 642e9b31), 01:16:31, its message naming the resolve. Its gate (01:16:34 to 01:22:10, ok, no
lock wait): **2,881 of 2,881** (1 slow, 22 skipped), 01:17:01 to 01:21:35; lifecycle in every budget (cold start p50
22.1 / p95 35.7 ms; from the config copy 29.4 / 38.3; clean shutdown 32.4 / 50.4; a post in flight 71.1 / 76.3; SIGKILL
then restart 26.1 / 30.1; binary swap 46.4 / 49.8; restore 134.4 / 166.9); L1 start 5.76 / 6.67 ms; turn frames 5 and
9 (plain p50 73.1 ms, tool call 153.1). Pushed 01:22:20, the branch deleted, done line 01:22:31; theseus-f3wr closed
with the hash. The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). An earlier process's in-flight provider call is
booked as spent at its reservation when the restart marks it unknown (the row says `cost_basis: reservation`), so a
reset frees it; a clean stop's call in flight is booked the same way at the next start. This build books only the
marks it makes and converts no hold made by installs #4 and #5, so the install read `theseus budgets` after the
restart: **nothing held on any of the six sessions** ($0.0000 held), so none was stuck on an older hold and no limit
needed raising. No config key, store format, protocol type or package. Health after the restart: `theseusd check` exit
0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms (store 4.6, kernel 36.9 ms; load 11 to 19, not a
quiet reading), Discord ready, the judge's live packs `security.v3`, `route.v1` and `rerank.v1` as before, memory live
on the `baseline` arm, voice ready, `cgroup: delegated`, the unit active with NRestarts 0, and no error or warning in
the journal; the store from format 22 to 23 at its first write, after the install's backup.

**Divergences.** The stand-in's `hold_ms` is outside the brief's files (small, for the live check only). A session
script once cut two existing tests from `tests_earlier.rs` while rewriting another; they were put back unchanged from
main before either commit, and the second gate ran on the final tree.

**Known gaps.** R21's "For Eddie", each recommended: (1) `interrupted_by_restart` holds no money today (a tool's plan
reserves 0), so leave it, and book it as the earlier-process mark does if a tool ever reserves (a test that a tool
call's plan reserves 0 would make that visible); (2) **an earlier process's call already overdue at the start is still
held for good** (startup's reconcile marks it `overdue_no_evidence` before serving; a provider call's deadline is 600
s, so a daemon down longer than that after a crash keeps the hold): do the report's fix, noting an overdue in-process
call at startup and booking it at the first tick, which also takes a write off the start path; (3) "spent" and
"lifetime" now differ by the estimates (lifetime is what the provider reported): keep lifetime, and say the booked
amount in `theseus budgets` and the cockpit, whose time machine folds spend from `provider.call` rows only and whose
ledger summary prints the row's `cost_usd` as a cost, not an estimate; (4) keep booking a clean stop's call in flight
(each restart that cuts a call books its worst case, $1.31 on Sonnet 5.5 at 128,000 output tokens, which the reset
clears); a later refinement could book what the stream reported so far.

### Item 198. Durability on: each durability session lists only its own prefix (`ListItsPrefix` under `StringLike`), health's text and the cockpit's AWS card show the tender's state, and at install #6 the S3 backup went on for Eddie's daemon (theseus-bfk9 and theseus-9ai1, with theseus-2fnx; Eddie's "Yes" of 2026-10-05 17:35; a cloud row the DM thread launched at 17:37 after reviewing the batch-7 AWS live checks, fired 17:39 from c4f79e9f, Opus 5.5, its report at about 18:35; b8297bc8, b2758865 and 33ace03e; reviewed 22:23 to 23:26 by local reviewer R21, stack S, and accepted with the stack at 23:52; joined 2026-10-06 01:40 at fdb592b1, a signed merge onto fc96e2da, by the stack-S joiner; probed live with the joined policies at 08:10 and run on a scratch daemon 08:12 to 08:37; installed 10:12 at 21bf5454, install #6, with `durability = true`)

**Why.** Two findings of the batch-7 AWS live checks on the operator's own account (theseus-2xdt), both owed before
durability could go on:
- **theseus-bfk9 (P2; check 1, 16:52).** Every session minted as the daemon mints the tender's (the owner role, an
  inline policy, no managed ones, the tender's tags) showed what real S3 does: for a HEAD or GET of a missing key, S3
  checks the implied `s3:ListBucket` with `s3:prefix` set to **the object's own key**. So plain `StringLike` on
  `durability/<deployment>/*` already answers a missing key 404 / `NoSuchKey`, while the `StringLikeIfExists` the
  tender's and the restore's `ListItsPrefix` carried (Items 147 and 171) also admitted a list that names no prefix:
  either session could list the whole bucket's key names. The code's comments gave the wrong reason ("judged with the
  `GetObject`'s own context, which has no `s3:prefix`"), and the test fake shared it, which is why a planted revert to
  `StringLike` used to "fail" four tests.
- **theseus-9ai1 (P2; check 2, 16:58).** The tender's status (`AwsDurabilityStatus`: state, lag,
  `oldest_unshipped_unix_ms`, counts, the error) reached only `theseus --json health`; `theseus health`, the TUI and the
  cockpit rendered nothing, so a failing tender was invisible.

**Eddie, 2026-10-05 17:35:** "Yes" to turning durability (the S3 backup) on at install #6, after bfk9's fix; at 17:41
the DM thread added 9ai1 as the second precondition, and filed theseus-2fnx (P1) for the config change, which waited
on this row's join and a live probe of the joined policies.

**What landed** (theseus-core's `aws/durable.rs`, `aws/durable/read.rs` and tests, the CLI's `render/aws.rs`, the
cockpit's `lib/durability.ts` and `SystemsCards.tsx`; the merge 11 files, +616 −48; no package (Cargo.lock, every
Cargo.toml and package-lock.json unchanged), store format change, protocol type or config key: `durability = true` is
an existing key).
- **`StringLike` in both sessions** (b8297bc8; bfk9). `durable::policy` and `read::policy` read `{"StringLike":
  {"s3:prefix": ["durability/<dep>/*"]}}`; their comments and module docs now say what real S3 does (the implied list
  carries the key as `s3:prefix`, so a missing key under the prefix is 404 / `NoSuchKey`, and a list that names no
  prefix carries none, so `StringLike` refuses it), and "listing the key names under its own prefix", not "the bucket's
  key names". The fake's `lists_without_prefix` became `may_list(policy, prefix: Option<&str>)`, a small IAM judge for
  `s3:ListBucket` (a condition on a key the request lacks fails unless its operator ends `IfExists`; `StringLike` globs,
  `StringEquals` matches exactly), and `may_know_missing` asks it with `Some(key)`. New `aws/tests_list_prefix.rs`:
  each session lists only its own prefix (the prefix itself, `…/wal/`, a key) and not a list with no prefix, `""`,
  `durability/`, another deployment's prefix or key, or the prefix without its slash; the fake's own `IfExists` model;
  the real tender's `Bucket::checksum` and the real restore's `Reader::get` on a missing key (404 / `Missing` under
  its prefix, 403 / `AccessDenied` under another deployment's); and an ignored `print_the_durability_policies`, which
  prints both policies for an account, region and deployment from the environment.
- **Health's two lines** (b2758865; 9ai1). `durability_lines`, called from `aws_tended_lines` when the account has a
  status: `aws: <acct> durability caught_up · shipped to position 1,234, 3 min ago · nothing unshipped`, then `aws:
  <acct> durability since the start: 2 segments, 1 tail, 0 blobs, 1,500 rows, 2,097,152 bytes · to
  s3://theseus-<acct>-<region>/durability/<deployment>/ and the table theseus-durability` (two lines, because one ran
  past about 200 characters; the state stays short). Waiting says its reason (`waiting for the WAL's sync: position
  1240 is written, not yet synced`), with `lag 2 min: the oldest record not yet shipped was written 2 min ago`;
  `nothing shipped yet`, or `shipped to position N (before this start)`. Loud: `aws: <acct> WARNING: durability
  failing: <error> (it retries)` and `… WARNING: durability stopped: <error> (it will not retry; the store is not
  shipped off the machine)`.
- **The cockpit's AWS card** (33ace03e; 9ai1). `durabilityView(d, now)`, pure and importing only types, returns the
  label, tone, error, shipped, lag, counts and where in the CLI's words (`caught_up` ok, `waiting` and `shipping` idle,
  `failing` wait, `stopped` fault, an unknown state said as it is); `DurabilityFields` shows a `durability` title with a
  pill and the error or reason in full, wrapped, then the fields. Four tests in `cockpit/test/durability.test.ts`.

**How it is proven.**
- **The session's tests:** theseus-core's `aws::` and the CLI's `render::aws`, 151 of 151; the cockpit's lint, 90 of 90
  tests and the build. Plants: `StringLikeIfExists` back in `durable::policy` (2 of 15 fail) and in `read::policy`; the
  fake judging a HEAD or GET with no `s3:prefix` (6 of 25 fail); the `durability_lines` call removed (4 fail); `failing`
  without `WARNING:`; `stopped` mapped to idle in the cockpit (89 of 90): each failed. One gate over the three commits
  (they touch disjoint files): every failure the 33 known L1 tests the cloud VM cannot run as root.
- **The review** (R21, onto crash-hold's review commit a173e46d; review commit c112dcc9; no AWS call): the build,
  clippy `-D warnings`, protocol 31 (no wire type moved), the cockpit (`npm ci --offline`, lint 0 errors, 90 of 90, the
  build) and shape clean; **151 of 151**; **the whole workspace suite on the stack S, 2,860 of 2,860**. **4 of 4
  planted reverts caught**: `StringLikeIfExists` back in the tender's policy (2 fail: "a list with no prefix passed",
  and the literal); the fake's HEAD and GET judged with no `s3:prefix` (6 of 25 fail: the old wrong model is now
  caught); health's `stopped` without `WARNING:` (the report planted `failing`); the cockpit's `stopped` mapped to the
  wait tone (89 of 90). The scrub's 14 hits on 12-digit numbers were all the invented test account.
- **Offline live check** (R21; a loopback-only namespace, `unshare -rn`, so no AWS call was possible; the invented
  account bound with an `endpoint` on a loopback port nothing listens on, a deployment of its own, `durability = true`;
  the stand-in model; the stack's frozen debug binaries). Health at 0.6 s: `durability waiting for the start to settle
  · nothing shipped yet · nothing unshipped` with the second line's zero counts and destination; at 2.7 s: `WARNING:
  durability failing: s3 PutObject: AWS account … is not bound: sts:GetCallerIdentity failed: … Connection refused …
  (it retries)`, the lag following; `--json health` the same state and error, `lag_ms` 2288; the daemon's log showing
  the retries at 5 s and 10 s. The cockpit's `/systems` page in headless Chrome in the same namespace: under the AWS
  card, DURABILITY with a yellow `failing: it retries` pill, the error in full, and the fields. `caught_up`, `shipping`
  and `stopped` need a real account or code to reach live; the render and cockpit tests hold their words and tones.
- **The policies for the live probe:** printed by the branch's ignored test for the batch-7 probe deployment (the
  account id left a placeholder); both `ListItsPrefix` statements `StringLike` on that deployment's prefix, statement
  for statement the probe script's `no-ifexists` tender policy and `StringLike` restore policy.
- **After the join, on the operator's own account** (the DM thread, 2026-10-06). The probe with the joined policies
  (`durable.rs` and `durable/read.rs` byte-identical at c112dcc9 and fdb592b1), each inline in a session minted as the
  daemon mints it: **12 of 12 passed on real S3** at 08:10 (a missing key's HEAD 404 and GET `NoSuchKey`; lists with no
  prefix, with `durability/` and with Eddie's deployment's prefix refused; its own prefix allowed). Then a scratch
  daemon on the joined build a1bcbee2, 08:12 to 08:37, **passed all five steps**: `caught_up` with every WAL byte
  shipped; a restore giving the same session and history; health's text agreeing with `--json` on all 11 fields;
  CloudTrail showing the minted tender and restore sessions with `ListItsPrefix` under `StringLike`; a teardown to 0
  versions and 0 rows; AWS cost under $0.001. Two observations, not filed: the settle wording hides a seeded cursor for
  the first seconds, and a restore logs a cosmetic `truncated_bytes=0` warning.
- **FAST:** a policy is a value built when a session is minted (the tender's after `START_AFTER`, 2 s after serving;
  the restore's in an offline command); the lines render in the CLI and the page from what health already carried.
  Nothing on the daemon's start, stop or turn path.

**The join** (stack S's third; the stack-S joiner). Lock `cloud-durability-on-join` 01:16:41, during crash-hold's
gate; clear at 01:22:31. The merge onto fc96e2da at 01:22:32: no conflict (theseus-core's AGENTS.md auto-merged), 11
files, +616 −48, **the staged tree e71936b4 equal to the dry run's, which also equals R21's reviewed stack merged onto
fc96e2da**; package-lock.json's hash unchanged. No join fix. The warm (the test build 44.5 s), then the core's `aws::`,
the CLI's `render::aws` and the core's golden, **153 of 153**; in cockpit/, `npm ci --offline` with the gate's node
(293 packages from npm's cache) and `npm test`, 90 of 90. The signed merge **fdb592b1** (fc96e2da and bb5ed269),
01:25:23. Its gate started at minute :25 and **waited 538 s for the lock** behind two reviewers' workspace test builds,
then ran to 01:40:40, ok: **2,890 of 2,890** (1 slow, 23 skipped, the new ignored policy printer among them), 01:35:28
to 01:40:04; the cockpit phase ok on the fresh node_modules; lifecycle in every budget (cold start p50 23.2 / p95 32.8
ms; from the config copy 26.4 / 32.6; clean shutdown 32.1 / 51.9; a post in flight 74.5 / 80.1; SIGKILL then restart
26.4 / 27.5; binary swap 47.3 / 49.8; restore 139.5 / 181.9); L1 start 6.33 / 6.85 ms; turn frames 5 and 9 (plain p50
75.1 ms, tool call 153.5). Pushed 01:40:48, the branch deleted, done line 01:41:02; theseus-bfk9 and 9ai1 closed with
the hash; theseus-2fnx noted and left open for the probe and the install. The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6; theseus-2fnx). The installed `theseusd` embeds
21bf5454, which contains fdb592b1. The install's config step (a precheck script: every `[secrets]` value asserted a
vault reference before and after, the one account table's deployment required to be Eddie's, the result checked with
tomllib, written by temp and rename) added **`durability = true`** to Eddie's account table, with consolidation's limit
(Item 188); a second run changed nothing. **Durability came on and caught up:** at 13 and 31 s after
the restart health read `shipping · nothing shipped yet` (its first pass shipped the whole store), and at 10:13:18
`caught_up · shipped to position 5,732 · nothing unshipped`, since the start 1 tail, 207 blobs, 1,493 rows, 5,298,704
bytes: the open WAL segment's 5,016,582 bytes plus the store's 207 blob files' 282,122 (the brief had expected the WAL
alone; the scratch runs had no blobs). S3 held the same: 208 versions, no delete marker, one tail covering bytes 0 to
5,016,582 with no gap or overlap; CloudTrail showed the tender's session with source identity Eddie's deployment and
`ListItsPrefix` under `StringLike`. **The restore drill matched**: Eddie's prefix restored with the installed binaries
into a scratch directory under a deployment of its own (never his, so nothing could ship into his prefix), 1 segment,
2,087 frames, 5,732 records, in 20.2 s; a daemon on it with no AWS account bound read 6 sessions with the same ids and
332 history nodes with the same ids and positions as his, every compared field equal; the scratch directory was then
removed. theseus-2fnx closed: "durability ON at install #6 (21bf5454)". Health after the restart otherwise:
`theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms (store 4.6, kernel 36.9
ms; load 11 to 19, not a quiet reading), Discord ready, the judge's live packs `security.v3`, `route.v1` and
`rerank.v1` as before, memory live on the `baseline` arm, voice ready, `cgroup: delegated`, the unit active with
NRestarts 0, and no error or warning in the journal; the store from format 22 to 23 at its first write, after the
install's backup. Rollback: remove the line and restart. The budget read after a day was left as a follow-up.

**Divergences.** None from the brief. Health prints the tender in two lines, not one. The TUI has no durability line
(the task left the TUI alone).

**Known gaps.** R21's "For Eddie", each recommended: keep health's two lines; the TUI's durability line, a small
follow-up in the CLI's words; nits for a later polish: the lag mixes units at small ages ("lag 7 s: … written 0 min
ago"), and the card's `to` field cuts the destination at its width. The fake's IAM judge covers what the policies use
and does not model `Deny`, `NotAction` or `GetObject`'s own resource scope; the live probe is the real judge.

### Item 199. Bench hygiene: the async Theseus record counts cut calls, tasks' tool calls and a full read, the async oracles leave nothing in /tmp, and the sampler keeps a reaped child's last interval (theseus-eq1a, theseus-3rjr and theseus-99by; R16's and the stack-B joiner's findings on the bench stack, Items 184 to 186; the ninth cloud batch's bench-hygiene session, fired 2026-10-05 20:07 from 4a449460, Opus 5.5, its report at 22:52; 494662f4, 7f384f44, 2f6ce238 and 3ba60833; reviewed 2026-10-06 00:19 to 01:10 by local reviewer R25, the bench stack, with one live trial; merged 01:41 at a545098f, a signed merge onto fdb592b1, the first of the bench stack's two merges under one lock and one gate, by the bench-stack joiner, whom the account's weekly limit stopped at 01:52; gated, pushed and joined 08:18 by its relaunch; nothing to install, bench/ only)

**Why.** Three P3s from the batch-8 bench stack's review and join (R16 and the stack-B joiner, 2026-10-05):
- **theseus-eq1a.** The async harness's Theseus record (bench-async-measured) left out three things: a call a stop cut
  (`efficiency.ledger_spend` read only `provider.call` rows, while the kernel books a `provider.cut` row's estimate as
  spent), task sessions' tool calls (the conversation's alone were counted), and rows past the ledger read's 1000 (the
  finish read the newest 1000 with nothing saying when that was full).
- **theseus-3rjr.** The parallel, fanout and interrupt oracles' `solve.sh` made `out=$(mktemp -d)` and never removed
  it: each run of bench/async's suite left four `/tmp/tmp.*` dirs (the batch-8 join had to remove eight).
- **theseus-99by.** test_sampler's namespace test skipped for a non-root user; ThisHost's 0.05 s work tolerance failed
  under load; SamplerCost's ratio flaked; and a sampler a failed test started could outlive it (R16 found one running
  32 minutes).

**What landed** (bench/ only: Python, shell and Markdown; the merge 13 files, +549 −171; no Rust, no new dependency,
`bench/theseus-bench.toml` untouched).
- **The async record** (494662f4; eq1a). The finish reads the tasks first, then each task session's history
  (`theseus-history-<task id>.json`, ids kept only if they match `^[A-Za-z0-9_-]+$`, since they name files), then
  `ledger -n 1000 -k provider.call` and `-k provider.cut` (`theseus-cuts.json`). `ledger_spend` sums each cut row's
  tokens and estimate beside the calls, counted apart: `cut_calls` (not in `model_calls`), `cut_cost_usd`
  (estimated), `billed_usd` (the `provider.call` rows'), and `cost_usd` = billed + estimated, what the kernel booked as
  spent; a `provider.error` row stays out, and a cut with `sent: false` counts as a cut call with zero estimates.
  `tool_calls_from` is `conversation and tasks` when the tasks file and every task's history read (a trial with no task
  included), else `conversation`. `truncated: true` when either read returned the cap (1000 rows): the trial's oldest
  calls may be missing, its numbers a floor. Harbor's cost (`driver.spend`) adds the cuts too, with `cut_calls` in the
  trial's metadata. bench/async/README.md says each.
- **The oracles' temporary dirs** (7f384f44; 3rjr). `trap 'rm -rf "$out"' EXIT` after each `mktemp -d`; test_tasks gives
  each run a `TMPDIR` of its own under its scratch root and checks it is empty after the oracle ran.
- **The sampler and its tests** (2f6ce238; 99by).
  - **(b) Where ThisHost's CPU went: a sample is not one instant.** The session logged every class increment and
    every vanished process per sample, and widened one window (a pause after the sampler reads the *reaper's* `stat`):
    the reaper's `stat` was read just before it reaped the harness, and both were gone by their own reads, so the next
    sample's `cutime` growth matched no vanished process and went to the reaper's class, "outside"; the work lost was
    the child's last interval. With a 200 ms pause main's sampler read low in 6 runs of 6 (0.132 to 0.251 s), the fixed
    one within 0.012 s in 6 of 6. **The fix** (`Tracker.observe`): vanished processes whose parent's `cutime` grew by
    less than they were last seen to use wait for that parent's next sample, once, with what it grew by banked; a
    second miss drops them as before. sampler.py's head states it, and its "What it misses" names the case left (a
    child read alive whose parent is read after reaping it, possible only after a pid wrap). ThisHost's 0.05 s stays:
    the cause was the sampler, not the kernel's rounding.
  - **(a) InANamespace** makes its namespace as any user (`unshare --user --map-root-user --pid --fork --mount-proc`
    first, then a plain `--pid` as root), skipping only when both are refused, and ends with the test
    (`--kill-child`). Its bound scales with the host's procfs: the sampler's share of a core must stay under
    `NAMESPACE_RATIO` (6.5) times the share that reading the namespace's 61 `stat`s alone would take at 4 samples a
    second, that read timed in the test.
  - **(c) SamplerCost** times the bare read and the sampler in turn each round (15 rounds of 30 samples), each keeping
    its least; the sampler must stay under `SAMPLER_OVER_BARE` (2.1) times the bare read, and the teeth test holds F4
    and F5 past it, timed the same way.
  - **(d) Every sampler a test starts is stopped:** `stop_samplers`, registered with `addCleanup` to run first, stops,
    waits for, SIGTERMs and at 30 s SIGKILLs every sampler naming the test's directory, and fails the test ("a sampler
    outlived its test").
- **A timing test found under load** (3ba60833). `test_a_trial_that_never_settles_ends_at_its_deadline` failed once
  (10.5 s against 8): the stand-in `theseus` slept its whole 5 s job regardless of `wait --after`'s `--timeout`, which
  the real daemon honours. The stand-in now ends the wait at its timeout; no driver code changed.

**How it is proven.**
- **The session's tests and plants.** New `LedgerRecord` tests (a cut call and a failed call: `cut_calls` 1,
  `billed_usd` 0.0423, `cut_cost_usd` 0.0071, `cost_usd` 0.0494; the cuts from their own file; two tasks' histories; a
  1000-row read `truncated`), the stand-in daemon answering `ledger -k`, `tasks` and a task's `history`, and EndToEnd
  on the workspace's daemon (`cut_calls` 0, `truncated` false, `conversation and tasks`, 2 tool calls). Two new
  `Classes` fixture tests for the reap race fail on main's sampler. Plants (cuts ignored; tasks' tool calls dropped;
  `truncated` never set; fanout's trap removed; the cleanup removed with the start forced to fail) each failed. Under
  the load recipe, on the cloud's 4-core VM: test_sampler 0 of 10 failed before and after; bench/async before left 4
  `tmp.*` dirs every run (40), after none, and failed once, the timing test step 4 fixed, then 10 of 10. The Rust gate
  measured nothing of these commits (every failure the 33 known L1 tests).
- **The review** (R25, on 8ea6669e; review commit 61af6951; no Rust built, no cargo, no gate). bench/harbor 69,
  bench/report 9, bench/async 40, under the host's python3 3.14 and Harbor's venv 3.13, async's EndToEnd on install
  #5's release-thin build: all green but **one failure of InANamespace's new bound under the venv** (ratio 8.8 against
  6.5, beside other trees' Rust builds); no `/tmp/tmp.*` dir (32 before, 32 after) and no sampler left. **12 of 12
  planted reverts caught**: the report's five, and R25's seven (`>` for `>=` at the cap; the carry's once-only guard
  removed; the carry never taken; Harbor's cost without the cuts; task ids from a missing key; the finish's id filter
  passing nothing). **The reap race, replicated** on this 16-core host: with the reaper's read held 200 ms before its
  reap, main read work low in **6 of 6** runs (to −0.202 s), the branch within 0.009 s in **6 of 6**; at the host's
  natural loads neither side missed (ThisHost within 0.011 s in 32 runs). bench/async under the recipe, 3 of 3.
- **Live** (R25; the key by the run script's own resolution, never printed). **Free, against a real daemon** of
  install #5's build on a stand-in Messages API, read by the branch's own finish and record: the session called
  `task_create` and the task ran `proc_run` (`tool_calls` 2, `conversation and tasks`); a stop 3 s into a slow stream
  wrote one real `provider.cut` row (estimated, sent, input 707, output 140, $0.002814), and the record read
  `model_calls` 4, `cut_calls` 1, `cut_cost_usd` 0.002814, `billed_usd` 0.005, `cost_usd` 0.007814, Harbor's cost the
  same. **Paid, one fan-out trial** (b5's static musl build, since no static build of main existed yet): reward 1,
  $0.0359, 7 `provider.call` rows and `model_calls` 7, `billed_usd` = `cost_usd` = Harbor's cost to the cent,
  `cut_calls` 0, 11 tool calls (the model ran the six parts as jobs, making no task), `truncated` false.
- **What the review found.** **Two of 99by's bounds don't fit this host:** they were calibrated on the cloud's 4-core
  VM. SamplerCost's sampler/bare ran 1.38 to 2.59 against 2.1 beside builds (5 of 12 probes over; the test failed in 1
  of its 8 runs), and F5 once fell to 2.07, under the teeth test's 2.1; InANamespace's ratio ran up to 8.8 against 6.5
  (2 of 28 runs, both beside heavy builds). This host's worst sampler (2.59) is over the VM's best F5 (2.29), so no
  single constant serves both: **theseus-ufe5** (P3), with two doc slips (the low-pid case counts a child's whole life
  twice, not its last interval; the report's 2.2 µs is 6.2 µs). Both are test flakes a rerun alone clears, not a fault
  in the bench code, and main's old bound flaked here too.

**The join** (the bench stack's, both branches under one lock and one bench/ gate; R25: it may go ahead of batch 8's
rest). R25's dry run on c07bbe6a: both clean in order, the result's bench/ tree identical to the review's, nothing
outside bench/ differing from main. **The first session** (01:23 to 01:52): the lock
`cloud-b9-bench-stack-join` at 01:26:01 in one guarded call, queued behind durability-on's (five more joins queued
behind it: soul-import, learning-fixes and the tools stack); clear at 01:41. The merges onto fdb592b1: **a545098f**
(bench-hygiene, 01:41:29; "Automatic merge went well", 13 files, +549 −171, its bench/ tree the review commit's) and
**a1bcbee2** (bench-recall-plan, Item 200, 01:41:38), both signed, no conflict, no join fix, `git diff
--cached --check` clean; the scrub over the whole join (22 files, 1,106 added lines) 0 hits in every family, and the
extra name, path and key check 0; then a release-thin build of a1bcbee2 (01:41:54 to 01:53:12). **The account's weekly
usage limit stopped the joiner at 01:52**, the merges unpushed; a 02:30 recovery died the same way within a minute and
touched nothing. **The relaunch** (08:07 to 08:26, after the limit reset at 08:00) re-checked everything against git,
not the logs: both merges' parents and signatures, `HEAD:bench` equal to the review commit's bench/ tree, nothing
outside bench/ differing from origin/main (no join since R25's base touched bench/), the build's theseusd embedding
a1bcbee2. **The gate, bench/'s suites** on read-only copies of that build, under both Pythons (08:09:18 to 08:18:15),
every run rc 0: harbor 69 (7 Harbor-only skips on the host, none in the venv), report 9, async 40 (EndToEnd under both),
recall 69 with no skip (its stand-in smoke asserting the daemon's overhead at most 13,750); SamplerCost and InANamespace
passed both times at load 1.4 to 2.6, and recall passed beside a neighbour's Rust build at load 15 to 28. No sampler
left and **`/tmp/tmp.*` 32 before and 32 after, the same names**: 3rjr's fix held, where batch 8's join had removed 8.
The overhead measured free on the joined build: **13,599**, the same as install #5's. Pushed 08:18:53 (origin/main
a1bcbee2), both branches deleted 08:19:00, done line 08:19:06; theseus-eq1a, 3rjr and 99by closed at a545098f,
theseus-5dey and dp3y at a1bcbee2; theseus-7gir.16 and .17 noted that the full runs come next. The store stays at
format 22.

**The install.** Nothing installs from this join: bench/ is a Python harness, with no daemon code, config key, store
format or package. It sits in install #6's tree (21bf5454), whose daemon it does not touch.

**Divergences.** Step 4 was found under load, not in the brief. The bounds of 99by (a) and (c) were calibrated on the
cloud's 4-core VM; R25's host shows them too tight beside builds (theseus-ufe5).

**Known gaps.** theseus-ufe5 (P3): a floor timed as the share is (an average, not a least), and the sampler held
against F5 in one interleaved loop rather than a constant; until then rerun SamplerCost or InANamespace alone when it
fails beside a build. R25's "For Eddie", each recommended: keep `cost_usd` with the cut estimates, and for the cancel
family publish `billed_usd` and `cut_cost_usd` beside it, saying that Claude Code's record carries no estimate for a
request its interrupt cut; leave paging the ledger (`truncated` marks a trial past 1000 calls, and `theseus rpc
ledger.tail` with `after` pages today should one get there); write the async docs when the async results are
published. The async families' full runs (about $1) need a static build of main (`bench/build.sh`, a Rust slot), possible
since smalls (Item 195); efficiency at full size (about $47) and the b5 held-out rerun (theseus-7gir.22, about
$10) wait on the same build.

### Item 200. Bench recall plan: `--stale retracted` binds a retraction to the value it governs, and a recall plan records its overhead, which the driver holds its daemon to (theseus-5dey and theseus-dp3y; R16's findings on bench-recall-fixes, Item 186; the ninth cloud batch's bench-recall-plan session, fired 2026-10-05 20:07 from 4a449460, Opus 5.5, done by 22:35; f277682e, 812aa080 and 27c5417f; reviewed 2026-10-06 00:37 to 01:10 by local reviewer R25, the bench stack, with one live smoke; merged 01:41 at a1bcbee2, a signed merge onto a545098f, the second of the bench stack's two merges under one lock and one gate; gated, pushed and joined 08:18 by the relaunched bench-stack joiner; nothing to install, bench/ only)

**Why.** Two P3s from R16's review of bench-recall-fixes (2026-10-05):
- **theseus-5dey.** `--stale retracted` (the recall bench's lenient stale rule, beside the strict default) scored the
  old value given as current *right* whenever a retraction word shared its sentence: `retracts_only` checked only that
  each sentence naming the old value held a `RETRACT` word, so "It's 27340, no longer 38013" passed. All four of the
  issue's replies scored right.
- **theseus-dp3y.** The smoke's plan had no room for the daemon's overhead to grow (a growth of 71 tokens had rung the
  smoke's marks), nothing recorded what overhead a progression was planned at, and `plan_bulks` could ship a plan its
  written logs missed (rounding between tokens and bytes).

**What landed** (bench/recall only: Python and Markdown; the merge 9 files, +557 −125; standard library only, no Rust,
nothing in bench/async, bench/harbor, bench/report or the bench profile).
- **The retraction governs the old value** (f277682e; 5dey). `RETRACT` becomes `RETRACT_PREFIX`, `RETRACT_SUFFIX` and
  `WAS_BEFORE`, with `REACH` (4), `NOT_REACH` (1) and `CANCEL`; `governed()` decides one occurrence, and
  `retracts_only(text, kind, old, new)` needs **every occurrence of the old value governed and the new value stated at
  least once ungoverned**. A phrase reaches one value: the nearest value of the asked kind on its side, **at most four
  words off, inside one clause** (a sentence, cut again at a semicolon or a dash). Four words reach past an article, an
  adjective and a noun ("instead of the old port 27340"); a clause, because a semicolon or dash usually starts a new
  assertion; "nearest on its side" makes "moved from 27340 to 38013" govern only 27340. Three shapes: a prefix (moved,
  changed or switched from, ignore, disregard, forget, previously, formerly, used to be, instead of, rather than,
  replaced or superseded not followed by "by", no longer, wrong about, the old, the former, and `not` at most one word
  off); a suffix with the value its subject (X is/was/has been … no longer, not current, replaced, retired,
  deprecated, outdated, stale, wrong, gone; X used to be); and around ("was X … before/originally/at first/until"). A
  prefix right after said/mentioned/noted/told you or don't/never/didn't is cancelled ("as I said previously, it's
  38013"; "don't forget 27340"). `strict` is unchanged. Some shapes are scored conservatively, stale ("earlier it was
  27340", "27340 → 38013", "Don't use 27340"); each is a one-line widening with a test.
- **The planned overhead, recorded and held** (812aa080; dp3y part 1). `Progression.overhead_tokens`, written only when
  set, so an older file loads with none and keeps its digest (it is held to today's `OVERHEAD_TOKENS`, and run.json
  says `planned_recorded: false`). `generate.py --overhead` threads it through the planning; `OVERHEAD_CUSHION` = 50
  beside `OVERHEAD_TOKENS` (13,700). `drive.py --arm theseus` reads, after the first turn and before any probe, the
  earliest `context.compiled` row's estimate less that turn's user message (`overhead_of`), records `overhead`
  (planned, recorded, measured, cushion, `past_cushion`, allowed) in run.json, and **past the plan by more than the
  cushion stops the daemon cleanly and exits 3**, naming both numbers ("the daemon's system prompt and tools are 13,599
  tokens, past the 13,099 the progression was planned at by 500 …: its marks may ring or fail. Generate it again with
  --overhead 13599, or run it as it is with --allow-overhead"). Only the Theseus arm measures (Claude Code's prompt is
  not Theseus's). The smoke's digest moves only by the key (25df5ff56f522723 without it, a6a203b842f48d21 with it).
- **The written bounds checked** (27c5417f; dp3y part 2). The choice: verify the written plan rather than plan with
  slack. `plan_bulks` yields candidates (the smallest window first, then the fewest reads), and `build` writes each and
  keeps the first whose written bytes pass `plan_misses` at the planned overhead **and at the cushion more** (fit, the
  crossing at the mark or the turn after, each read turn alone under its limit, no log past one result, a session
  without a mark fitting), so the driver's refusal threshold is exactly what the generator guarantees, and a plan that
  already held is kept byte for byte. The cushion check moved two smokes' windows (seed 8 41000 → 42000, seed 11 50000
  → 51000); every full plan is unchanged. Offline, smoke and full at every overhead from 13,528 to 14,000, four seeds:
  3,784 plans, 0 misses.

**How it is proven.**
- **The session's tests:** bench/recall 69 tests green at each commit. New scoring tests (the issue's four replies and
  three more wrong under both rules; p001's live reply, "moved from 27340 to 38013" and nine more right shapes right
  under `retracted`; the reach, the clause bounds, versions and dates); the refusal (planned at 13,099: exit 3 after one
  turn, both numbers named, nothing left running); the stand-in smoke end to end at the measured overhead; the
  progression's key round-tripping with an older file's digest kept; `test_a_thin_plan_is_planned_again_until_its_written_bounds_hold`
  and the sweep test. The scratch daemon of its build measured **13,599**. Plants (the old rule; the refusal skipped;
  the written-bounds check skipped) each failed. Under load, `test_drive` 11 tests OK in 403 s; the whole suite at nice
  19 starved (the CPU-bound sweep got 1 % of a core) and was stopped after 15 minutes.
- **The review** (R25, onto bench-hygiene's review commit 61af6951; review commit 9feab64b). **bench/recall 69 of 69
  under both Pythons**, no skips, with install #5's release-thin build (the stand-in smoke, the refusal, the timeout
  test with `--allow-overhead`, theseus-1xgs's exam-names test). **10 of 13 planted reverts caught**: the report's
  three, and R25's `CANCEL` off, the reach unbounded, no cut at a semicolon or dash, the user message kept in the
  measure, `>=` for `>` at the cushion, the key written as null, the key not read back. Not caught: **R1b**,
  `retracts_only` not requiring the new value stated free (every test green; that half is what makes "It's no longer
  27340, and not 38013 either" wrong); **R6b**, `plan_misses` checking the planned overhead only, not the cushion
  (nothing pins seed 8's or 11's window); and R9, benign (a plan sized from the wrong number is still checked at the
  overhead it records).
- **The rule against 39 hard replies** (R25's own, each with a careful reader's verdict; strict scores every one wrong,
  as before). Under `retracted`: a retraction then a re-assertion, right 9 of 10; two old values in one phrase, 2 of 7
  (a phrase governs one value, so five right replies score stale: conservative); the new value only in a quote, 3 of
  5; other old-kept shapes, 6 of 8 (`the old` read as a retraction in "the old port 27340 is back" and "still works",
  and the contrived "27340 is no longer wrong"); ordinary right shapes, 4 of 9. **5dey's class (a phrase governing the
  new value) is closed**; the four false rights left and the conservative false stales went to **theseus-qryz** (P3).
- **The plan, worked from its bytes.** The measure is free and exact: the seed-7 smoke planned at 1,000 tokens, driven
  on a scratch daemon of install #5's build against the counting stand-in, was refused after its first turn naming
  13,599. The smoke and the full keep their bytes but for the key (the full: 6a022a24498bda8f without it,
  fbb1acb2e7718237 with it). **The under side is unchecked:** the generator guarantees [plan, plan + 50] and the driver
  refuses only above it; today's daemon is 101 under the default plan, and every plan still crosses, smoke seed 12 by
  only 31 tokens; 150 under, it would not (**theseus-tqa3**, P3, with R6b).
- **Live, the smoke on the Theseus arm** (R25; install #5's build, the review commit's profile, a throwaway
  python:3.12-slim container as R25's user, the key by the run script's resolution and never printed; the default plan
  at 13,700). run.json's `overhead`: planned 13,700 (recorded), **measured 13,593**, `past_cushion` false (the
  container 6 tokens under the host's stand-in measure: its workspace path and date differ). **30 turns, every one
  exit 0; 9 of 9 facts delivered; nothing left running; $0.7569.** Compactions at 11 (`compaction`, the mark's
  crossing as planned), 19 (`ring`) and 26 (`compaction`); session 2, planned to fit whole, filled again as in R16's
  run (its replies averaged 579 bytes against the plan's 400: R16's open recommendation). Strict and `retracted` scored
  the same: recall 67 %, abstention 50 %. **p001 was a right answer both rules scored stale**: it named the old port as
  a search term and inside a loopback address, calling its earlier mention wrong, with no phrase in reach: a live false
  stale of theseus-qryz's class.

**The join.** Under one lock and one bench/ gate with bench-hygiene, told in Item 199: merged at
**a1bcbee2** (01:41:38, onto a545098f), clean, 9 files, +557 −125; `HEAD:bench` after it equal to R25's review commit
9feab64b's bench/ tree; recall 69 under both Pythons on a release-thin build of a1bcbee2, its stand-in smoke holding
the daemon's overhead at most 13,750, and the joined build measuring 13,599; pushed 08:18:53; theseus-5dey and dp3y
closed at a1bcbee2.

**The install.** Nothing installs: bench/ only. It sits in install #6's tree (21bf5454).

**Divergences.** The written plan is verified rather than planned with slack (the session's choice, R25 accepted).
`OVERHEAD_TOKENS` stays 13,700 for the tests' default.

**Known gaps.** theseus-qryz (P3: the four false rights, the two-old-value false stales, R1b's untested half) and
theseus-tqa3 (P3: the lower bound, and R6b). R25's "For Eddie", each recommended: keep the cushion of 50 and add the
lower bound (`plan_misses` at planned − 50 too, the driver refusing more than 50 off either way); **generate a
published run at the measured overhead** (measure the build that will run, free, and generate at it; at 13,599 the full
seed-7 plan is digest cb4f3c43b746d1bd, window 94000, marks 120/320/520, 228 facts and 204 probes, $11.75 estimated);
publish strict as the headline, with `retracted` beside it only after theseus-qryz; and **run recall's full after this
join** (Eddie's 2026-10-05 16:43 go): strict's scores don't change with it, but the overhead guard stops a $12-to-$16
run at its first turn if the daemon's prompt grew past its plan, instead of ringing its marks. The full runs were left for
a quiet machine (the chain log, 2026-10-06 08:34).

### Item 201. The importer: `theseus import openclaw` brings the operator's prior assistant history in as imported sessions, closed, private, idempotent and erasable by tag, a place the pipeline leaves unnamed read by its kind; store format 23 (theseus-0lrr.6; cloud row B1, launched by the DM thread 2026-10-05 17:10, fired 17:13 from c4f79e9f, Opus 5.5, its report at 19:02; 4be682e4 and f306c939; reviewed 22:58 to 2026-10-06 00:49 by local reviewer R24, and accepted with a required join fix at 01:22; queued from 01:26, stopped by the account's weekly limit at 01:52 and relaunched at 08:08; joined 09:11 at 79be3213, a signed merge onto a1bcbee2 with R24's join fix, by the soul-import joiner; store format 22 to 23; installed 10:12 at 21bf5454, install #6, nothing imported at the install)

**Why.** theseus-0lrr.6, the importer for the operator's prior assistant history (his OpenClaw history). A pipeline
outside this repository writes that history as episode files: one JSON line per episode, with its messages, an optional
summary and its labels, hashed. Theseus needs to take them in as its own memory: each episode an imported session that
never takes a turn, private under the place rule (§2.15 of the memory design), each message a node at its own time
with its provenance, outside text treated as outside text, idempotent so a batch can be retried, and erasable by tag.

**What landed** (theseus-core's new `import/` and `rpc/import.rs`, theseus-protocol's `import.rs`, the CLI's
`import.rs`, theseus-index's extractor and tender; the merge 57 files, +3,398 −70; no package or config key; **store
format 22 → 23**; protocol additions only: a module, three methods on one table line, two ledger kinds, eight
regenerated TypeScript files). Steps 1 to 4 share one commit (4be682e4): step 1 alone has no writer, and each test reads
more than one step.
- **The imported session.** `SessionRecord.imported: Option<Box<ImportedFrom>>` (the tag, the episode's id and hash,
  its source, agent, place, labels, triage, as-of, message count, the file and line it came from, when it came in, and
  the erase's receipt): a flag, not a new `SessionKind`, so no client's match changes; kind `conversation`, label
  `imported`. The id is `ses_ep` + the episode id's 64 hex digits, and `p` is no hex digit, so "is this imported?" is a
  string test with no read wherever the turn path asks it. Node ids are the session's tail and the index, so a retried
  batch writes the same ids. `Origin::Import` and three NODE bodies: `Imported { text, integrity, source, unit, sha256,
  idx }`, `ImportedSummary { text, cites, model }` and `Erased { was, at_ms, why }`. A node's `created_at_ms` is the
  message's own time, so recall's header gives the model the message's date. **Closed:** no execution, so the kernel
  never drives it, and `turn.submit` refuses it before anything is touched (−32005, "… is an imported session: closed
  and read-only, it takes no turn"). **Private:** `TurnRunner::place_of` says `Private` for an imported id whatever
  place the episode names. `session.list` leaves imported sessions out, so an import does not push every real session
  off the sidebar; `import.list` lists them by tag. Integrity `outside` makes the index mark the node external, so
  recall drops it `untrusted` unless `[memory] include_external`; a recalled item's frozen header names the import, its
  author and integrity, its source, the place and tag, and the message's date. No owner's name is in the code.
- **The import** (`import.episodes`; `theseus import openclaw <file>...`). Each line is checked: `format` 1, every field
  against the format's lists, RFC 3339 times, the summary's cites, and **the hash, sha256 of the canonical JSON as
  Python writes it** (`sort_keys`, no spaces), in both `ensure_ascii` forms, floats in Python's repr. A batch is one
  frame (the episodes' nodes, their session records scoped `import:<tag>`, the tag's counts and an `import.batch` row),
  cut past 4,000 records or 8 MiB with a pressure wait between; one import or erase at a time, on the blocking pool.
  **Idempotent:** the same id and hash is skipped; another hash is rejected and named, not overwritten; an erased
  episode is not imported again; a malformed line is rejected by its number and the batch goes on. Import and erase
  are judged as the owner's act from a private place, and are in `OPERATORS`. The CLI streams each file a line at a
  time, 256 lines or 4 MiB a batch, one in flight.
- **Erase by tag** (`import.erase`; `theseus import erase --tag T --why …`). §5.6's payload erasure is not built (no
  `Redaction` record; nothing rewrites a WAL frame), so the erase writes each node of the tag again under its id,
  origin, author and time as `Body::Erased`, each session's record with its receipt (who, when, why), the tag's counts
  and an `import.erased` row, then asks a running tender to forget the nodes; the index's follower drops each
  tombstoned node as it reads it, so a rebuild leaves them out too.
- **Found and fixed on the way:** a rebuild met a node and its tombstone in one batch and kept the node, since the
  skip path asked `engine.holds`, which reads only what is committed: `ingest` now keeps the nodes it indexed in the
  batch (`fresh`). And `import::shown` (f306c939) shows an imported node by its newest record, once, in `session.history`
  and `node.list`, whose scans hold the originals beside their tombstones.

**How it is proven.**
- **The session's tests** (invented fixtures, among them two lines a Python generator wrote): an episode file imports
  once and a second run skips every episode; an imported session takes no turn; `session.list` leaves them out; a
  changed hash, a truncated line, `format 2` and a forged text are each refused by line number; an import from no
  private place is refused; a byte changed in a Python-written line is refused; an erased tag leaves nothing to recall
  (every node's newest record a tombstone, receipts, history and listing only tombstones, a re-import refused); the
  index's real tender on a real WAL drops a tombstoned node, and a fresh index over the same WAL holds only the live
  one; a shared place recalls no import while a CLI turn recalls it with the message's date; outside text is never
  placed as instruction. Each of its plants failed. **Rate** (debug, a 4-core VM): a synthetic file of 10,000 episodes
  (15.8 MB) imported in **8.5 s wall** (40,000 nodes in 40 frames, `health` beside it p50 11 ms); the same file again,
  all skipped in 4.0 s; erasing them all, **4.2 s in 13 frames**.
- **The review** (R24, on main acf26214; review commit 8828d0f1 with the join fix): the build, clippy `-D warnings`,
  protocol, the cockpit and shape clean; **the whole workspace suite 2,858 of 2,858**; **all nine planted reverts that
  ran fail as they should**, R24's two among them (the explicit `Private` line removed, caught only by the join fix's
  new test; a null place name rejected); three were not run, for time. R24 read each privacy rule's code, not only its
  test.
- **The real files, which the cloud never saw** (counts only; no episode text printed or logged). The branch's own
  parser over every real line: **every hash matched Python's canonical form**; but the pipeline writes `place.name:
  null` for many places, as the format allows, and the branch read the name as a string, so it would have rejected many
  real episodes (the join fix); one line's date is no real date, which the importer rightly refuses (the pipeline's to
  fix). Then **all of the real files, about 21,000 episodes, through a scratch daemon in 55.7 s** (debug, BM25 only),
  that one line rejected and none for the hash, `health` answering beside it at p50 26.3 ms.
- **Live, a dozen real episodes** (scratch daemons, the stand-in model, nothing of Eddie's): imported, then 12 skipped
  on a second run; none in `theseus sessions`; a turn on one refused. A private recall admitted only imported items,
  each header at the message's own time to the minute, a null-named place read by its kind. **A shared place** (a CLI
  session tied to a shared channel by a probe) admitted nothing, every imported candidate dropped for `place`. **The
  erase:** one frame, 19 ms, "its follower had dropped them at their tombstones"; the index fell from 53 nodes to 6;
  after a restart no imported item was recalled and the earlier recalls' manifests served none of the erased text; a
  re-import was refused, 12 of 12.
- **FAST.** Nothing on the start path; on the turn path a string test, and one session read for an admitted imported
  item's header. The A/Bs (the session's, and R24's two holds under load) found nothing. **At full scale:** a turn with
  recall 206.6 → 236.4 ms p50; **`session.list` slowed, the whole list p50 23.3 → 501.0 ms and a page of 20 19.7 →
  210.0 ms**, since both read past every imported session (**theseus-7087**, P2); a stop during the whole tag's erase
  waited 13.78 s for it (**theseus-autz**, P3).

**The join fix** (R24's `joinfix.py`, required): a place's name may be null, `place_name` naming such a place by its
kind (two tests); a test that holds the explicit private-place rule
(`an_imported_session_tied_to_a_shared_channel_is_still_private`); and memory-checks' `tests_stub_kinds.rs`
(Item 187), which matches `Body` exhaustively, gets the three bodies (VARIANTS 8 → 11), without which the
test build fails (E0004).

**The join.** The lock at 01:26:16, behind durability-on's and the bench stack's; the weekly limit stopped the joiner
at 01:52 and a 02:30 recovery; relaunched at 08:08, it redid nothing, and its joincheck cleared at 08:19:17. The merge
onto a1bcbee2 (08:19:29): three conflicts with history-pages (Item 193), `rpc/methods.rs`, protocol
`lib.rs` and `scripts/long-files.txt`, where rerere replayed R24's recorded resolution (`session_history` keeps the
page with its cursors and passes its nodes through `import::shown`); joinfix.py's 8 edits; `renumber.py` computing the
format from main's (22 + 1 = 23, no change needed); **the staged tree b8101e7c equal to R24's dry run, byte for byte**;
protocol `lib.rs` 2,715 lines at a ceiling of 2,715. A paged walk can still show an erased imported node twice (`shown`
dedupes within one read). The warm took 26 min beside a deliberate load and two builds; then **160 of 160**. The signed
merge **79be3213**, 08:47:25. The gate waited for minute :01, then 262 s for the lock, and passed at 09:11:24: **2,908 of
2,908** (1 slow, 23 skipped); lifecycle in every budget (cold start p50 25.0 / p95 32.2 ms; SIGKILL then restart 29.0 /
36.1; binary swap 51.4 / 60.9); turn frames 5 and 9 (plain p50 76.8 ms, tool call 157.9). Cold start, swap and resident
memory (70.0 MB) sat slightly above the five gates before it while the daemon's own start clock did not move (serving
20.32 ms): likely the bigger binary; one gate cannot tell it from noise. Pushed 09:11:32, done line 09:11:50;
theseus-0lrr.6 closed. **The store moved from format 22 to 23.**

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). `theseus import openclaw|list|erase` is on
Eddie's daemon; **nothing was imported at the install**. His store moved from format 22 to 23 at the build's first
write, after the install's backup, and an older build now refuses it, so the backup is the way back. The real import is
the operator's later step; R24's order: the pipeline's bad line fixed, a backup, a run into a scratch or copied store
first (an erased episode cannot be imported again), then the import, about a minute, with theseus-7087 landed first.
Health after the restart: `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms
(load 11 to 19, not a quiet reading), Discord and voice ready, the judge's live packs as before, memory live on the
`baseline` arm, `cgroup: delegated`, the unit active with NRestarts 0, no error or warning in the journal;
`MANIFEST.json` read 23 after the first write, the backup's 22.

**Divergences.** Steps 1 to 4 are one commit. The brief's `Import { source, unit, sha256 }` origin is the `import`
origin with those fields in the body (`Origin` is `Copy`, stored in every node). An erased episode is not imported
again. The index's `EXTRACTOR_VERSION` is not bumped (no store held such a node; a bump rebuilds every index).

**Known gaps.** theseus-7087 (P2), before the real import or right after (the cockpit polls the whole list every 3 s);
theseus-autz (P3), stop-aware frames as theseus-1o8i made for other paced work. R24's "For Eddie", each recommended:
**an erase hides, it does not delete**: the payloads stay in the WAL, in backups and in what the durability tender
shipped, until §5.6's in-place redaction is built, which must come before any erase must be final; **a `Recall` node
written before an erase** can still render the erased source in that one session (fix with §5.6); an erased episode
cannot be imported again under any tag (ids are content-derived), so import into a scratch store first; keep the strict
hash; show imported sessions apart in health's count; add `derived_from` edges for a summary's cites with B2, and before
B2 mark a summary external when its episode had outside messages; the cockpit's views of tags and the three node kinds;
**every private place recalls the import**, an MCP client's session included, so keep `/mcp` local until Eddie decides
which surfaces may; and expect the vector index's embedding backlog after a full import (only BM25 was measured).

### Item 202. Voice turns: in a voice call a barge-in holds the reply until the words over it decide (a wordless sound, an echo, a backchannel or "go on" resumes the cut sentence from its held audio; words cut it), and a reply waits for the floor (theseus-9ln5 and theseus-kpa7; the voice lane's designs 1 and 5; Eddie's "yes" of 2026-10-05 22:29; a cloud row the DM thread launched at 22:30, fired 22:33 from acf26214, Opus 5.5, its report at 23:45; 74750d9d, 751789d5, 141d19d1 and c2fe6f7a; reviewed 2026-10-06 01:24 to 01:52 and, after the account's weekly limit stopped it, 08:09 to 08:43 by local reviewer R27, stack B9-voice, and accepted at 09:06, its join placed early in the queue; joined 09:37 at f33f0eca, a signed merge onto 79be3213, by the B9-voice joiner; installed 10:12 at 21bf5454, install #6)

**Why.** Eddie's voice call of 2026-10-05 (21:02 to 21:06), as the voice lane (theseus-vk84) counted it from the
ledger: **166 sentences composed, 40 heard whole (24 %), 7 cut, 119 never said; 5 of the 7 barge-ins were set off by
wordless sounds** (sounds whose transcripts came back empty), and each cleared the whole queue, later replies
included; and the session recorded every reply as spoken, so later turns built on text he never heard. The engine's
`barge_in` cleared the queue at the 300 ms stop and bumped the synthesis generation, though the sound's transcript
later came back empty and never became a turn. `play_next` started a clip as soon as its audio was
ready, so Theseus talked over a speaker still mid-sentence, and a thought split by a short pause was answered by its
first half. The lane mined a private voice codebase for behaviours only, never its code (Eddie, 21:45 and 21:53), and
wrote seven designs, each Theseus's own in the engine's terms; Design 1 (hold, then decide; theseus-9ln5) and Design 5
(the floor; theseus-kpa7) are this row. Eddie said "yes" to launching it at 22:29.

**What landed** (`crates/theseus-voice`: `engine.rs` (1,247 lines), the new `heard.rs` and `tests/turns.rs`, `lib.rs`,
the crate's AGENTS.md; in theseus-discord only `pump`'s match and the tests' `heard()`; the merge 7 files, +1,833 −107;
no protocol, store, config or package change, `Cargo.lock` untouched).
- **The rules** (`heard.rs`, new, pure, unit-tested): `words` (lower-cased runs of letters, digits and apostrophes);
  `is_echo` (at least 2 words, at least 60 % of the distinct words among the given sentences' words); `is_backchannel`
  (1 to 3 words, segmented into the task's list; "uh-huh" is the words `uh huh`); `is_resume` (the whole utterance one
  of the task's phrases, such as "go on"); and `classify(text, overlap, sentences)`: an empty transcript is `Wordless`
  everywhere; over speech (`Overlap::Speech`) echo, then backchannel, then resume, else words; in the tail
  (`Overlap::Tail`, begun within 1.2 s after the last queued sentence ended) echo only; with no overlap, words.
- **Hold, then decide** (74750d9d; 9ln5). The 300 ms stop (`hold`) stops the clip on the same tick as before (the same
  `Out::Stop`) but **keeps the queue**: `Hold { since, seq, what, into, why }`; the held item's clip is cleared so its
  stopped clip's `Ended` does not pop it, and it keeps its audio; synthesis and playback wait while held, and a
  synthesis in flight finishes and keeps its audio (no generation bump). At each VAD's first speech frame,
  `what_is_over` records an `Opening`: what it is over (`Saying { what, sentence, text }` for the queue's front, or
  `Preparing { turn }` when a turn is in flight), the overlap, and the sentences it may echo. At the transcript,
  `Utterance { over, heard_as }` is emitted for every transcript (the pump books the transcription's spend from it),
  and **only words become a turn**. Over speech, words **commit** the cut: a `Cut` per reply or report queued, then
  `BargeIn { speaker, what, dropped }` as before; anything else records the hold's `why` (the plainest heard: resume,
  backchannel, echo, wordless). **Resume**, once no listed VAD is open and no over-speech transcript is due: the hold
  ends, a cut acknowledgment is dropped (not replayed), `Resumed { what, why, held }` is emitted, and `play_next` plays
  the cut sentence again **from its held audio**. **The late cut:** short words over speech (under the 300 ms stop)
  commit at their transcript. **An echo makes its speaker echo-prone** for the call: their 300 ms stop is off, and their
  words cut late at the transcript, so loudspeakers do not stutter. A report cut by words goes back to the front of the
  reports from its cut sentence (reports are now split once, at `Command::Report`). The call's end (`Leave`, the
  handle's drop, the seam's end) with anything queued emits `Cut { why: CallEnded }`. theseus-discord's `pump` ignores
  `Resumed` and `Cut` (voice-heard, theseus-qb8o, writes their rows).
- **The floor** (751789d5; kpa7). A reply's, a report's or the acknowledgment's first clip (`opens`) does not start
  while any listed VAD is open, or while a contending utterance's transcript is still due, so the reply waits for the
  transcript too and plays if it is not words. `contends(p, item)`: the utterance closed after the item was queued, or
  it is one of the turn's own speakers and began within 1.5 s of their last speech in the turn (`SPLIT_THOUGHT`). Words
  that contend **supersede** a waiting reply or report (`Cut { why: Superseded, heard: 0 }`, no `BargeIn`) and are the
  next turn; a reply that arrives with a contending utterance already transcribed as words is superseded at once (a
  thought split by a pause gets one answer); words over speech when nothing queued has begun supersede rather than
  count as a barge-in, so `voice.barge_in` rows stay real cuts. Turns still start while a reply plays.
- **The module doc and AGENTS.md** (141d19d1) say what turns, send and barge-in now do; **two more tests** (c2fe6f7a):
  a cut acknowledgment is not said again, and leaving with a reply unsaid cuts it.

**How it is proven.**
- **The session's tests:** `tests/turns.rs`, 14 tests through the seam in virtual time (WAV fixtures, the stand-in
  speech, two listed speakers), each asserting exact times. A 400 ms laugh with an empty transcript stops the reply at
  1.8 s and the cut sentence plays again from its start at 2.6 s, all three sentences with three syntheses and one
  `Resumed`, no `Cut` or `BargeIn`; its own sentence heard back resumes as an echo, while "Stop." over it cuts and is
  the next turn; "Yeah." resumes as a backchannel and a short "Mm-hm." stops nothing; "Go on." while held resumes with
  no turn; words over the second of four sentences cut it at their transcript; a short "No." cuts late; a failed
  transcription over speech commits; a cut report comes back from its cut sentence; a reply ready while the speaker
  talks waits for a cough and is superseded by words; "Can you make yourself a" / 800 ms / "tool to order" gets one
  answer. `heard.rs`'s 7 unit tests hold each rule and both sides of the 60 % line. The pipeline's barge-in test still
  stops at 2.3 s; its `BargeIn` moved to 3.3 s, the commit at the speaker's transcript, the moment of the turn it
  asserts. theseus-voice 62 (41 before), five runs under the load recipe 62 of 62; theseus-discord 122. Its nine plants
  (the stop clearing the queue fails 9 tests, every transcript words 8, and so on) each failed.
- **The review** (R27; first onto fc96e2da, then, after the weekly limit stopped it at 01:52, onto fdb592b1 as review
  commit 73d489f9, the crates' files of the two merges byte for byte the same): every build step clean (rustfmt, the
  test build, clippy `-D warnings`, protocol 31 with protocol.gen unchanged, shape); theseus-voice and theseus-discord
  **184 of 184** on both merges; **the whole workspace suite 2,910 of 2,911** (`--retries 0`), the one failure main's
  own wall-clock catalog test under load (`one_service_decodes_in_under_5_ms`, a debug best-of-5 budget: filed
  theseus-rnl3); theseus-voice 3 times under the load recipe, 62 of 62; a three-way dry run of main, discord-live
  (Item 207, also in theseus-discord) and this branch builds, passes clippy and passes 191 of 191; the
  merge matrix clean onto main and onto main plus each of the 14 unjoined cloud branches. **13 planted reverts**, the
  report's five and R27's eight (two of them FAST claims): **the branch's tests catch 11**; another listed speaker's
  words superseding a waiting reply, and `Leave` while held, are caught only by R27's probes (theseus-e6mj). The
  public-repository scrub: the branch's added lines carry no hit.
- **The state machine, case by case** (R27's 13 virtual-time probes): words over a resumed sentence hold it again and
  cut it; a reply that comes while held goes unsaid with the held one when words commit (main had played it after the
  cleared queue); two speakers over a hold wait for both, and words decide; `Leave` while held cuts it and resumes
  nothing. And four findings: **a transcript that never comes holds the reply for the call** (with the provider's 20 s
  request bound, 20 s of silence, then a cut with no turn), and a listed speaker's steady sound holds the floor while it
  lasts, where main lost such replies outright (theseus-aq4t); **an answer that repeats its question's words** ("The
  daily view." to "the daily or the monthly view?") is heard as an echo: no turn, and its speaker loses the 300 ms stop
  for the call (theseus-3ug0; the rule is the task's, word for word); **a "yes" that answers a closing question is no
  turn** when it overlaps the question's end or when another reply is queued but has not begun (theseus-1cz8); and a cut
  report still waiting at the call's end gets no `Cut` (theseus-qrwx).
- **FAST** (R27, the claims against the code, two of them planted): the stop comes on the same tick (the same speech
  frames against the same `barge_frames`, the same `Out::Stop`; the stop one 20 ms frame late fails 9 tests); a
  committed cut makes the next turn in the same loop iteration as before (`Done::Transcribed`, `heard`, `commit`, then
  `advance`'s `start_turn`; the pipeline test's turn still at 3.3 s); a resume replays the held audio with no new
  synthesis (dropping the held audio fails the sound test's count of 3 syntheses for 3 sentences). The rules are string
  splits and set lookups per utterance in the engine's own task; nothing runs before serving or on the core's turn
  path, so no bench. Holding also stops synthesis ahead, so what a wordless stop used to discard is not made. The new
  waits are by design and only while someone talks.
- **Spend and rows:** each transcript is booked once, echo and backchannel included; `Cut` and `Resumed` are neither
  booked nor rows yet; `voice.barge_in` rows come at the commit, and only for real cuts, so health's barge-in count
  drops to real cuts; nothing reads a row's time as the stop's.

**The join** (stack B9-voice; the B9-voice joiner). R27 found it shares no file with any unjoined branch ("may join
early"), so **the DM thread placed its lock at 09:05 with the queue time 01:27:00**, right after soul-import's and
ahead of learning-fixes', so that voice-heard could launch about two hours sooner. The joincheck cleared after
soul-import's done line (09:11:50). The take (09:12:01 to 09:12:15): the dry run on 79be3213 (`merge-tree` clean;
`MANIFEST_FORMAT` main 23, branch 22, merged 23; no file changed on both sides since the base; theseus-voice and
theseus-discord 0 files different from R27's review commit), then the merge, "Automatic merge went well", no conflict,
no resolve script, no join fix; 7 files, +1,833 −107; **the staged tree 1f140125 the dry run's tree less the two cloud
files**. The warm (the test build 3 min 42 s, clippy clean), then theseus-voice, theseus-discord, theseus-protocol and
the core's golden, **217 of 217** (protocol.gen unchanged). The signed merge **f33f0eca** (79be3213 and 27e18ed0),
09:21:14. may-build said "not yet: 3 other trees building" at 09:21:20 (three reviewers compiling, memory under 8 GB)
and yes at 09:23:51; the gate started 09:23:54 (minute 23), waited 302 s for the lock behind those reviewers' steps,
and passed at 09:37:05: **2,929 of 2,929** (1 slow, 23 skipped; soul-import's 2,908 plus 21), 09:31:39 to 09:36:27,
the catalog test passing; lifecycle in every budget (cold start p50 27.5 / p95 37.0 ms; from the config copy 26.4 /
30.2; clean shutdown 34.6 / 49.2; a post in flight 77.0 / 89.6; SIGKILL then restart 28.8 / 35.4; binary swap 52.7 /
63.1; restore 148.8 / 165.4); the daemon's own clock serving at p50 21.58 ms; L1 start 7.08 / 7.71 ms; turn frames 5
and 9 (plain p50 83.0 ms, tool call 164.6); resident memory 68.8 MB after the start. **An observation, not a finding:**
the plain turn and cold start were the highest of the last eight gates (cold start 23.2, 25.0, then 27.5 ms at
durability-on's, soul-import's and this gate), every budget with a wide margin, while the daemon's own start phases
matched soul-import's gate within 0.25 ms; nothing of this branch runs at the start or on the core's turn path, and the
turn bench runs with Discord off; an A/B of 79be3213 against f33f0eca in one hold would settle it. Pushed 09:37:12 to
09:37:14, the branch deleted, done line 09:37:26; theseus-9ln5 and kpa7 closed with the hash. The store stays at format
23. voice-heard (theseus-qb8o, rkvl, 9zft) launched at 09:49.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). In a voice call a barge-in now holds until the
words decide: a wordless sound, an echo, a backchannel or "go on" resumes the cut sentence from its held audio; words
cut it; replies wait for the floor; a thought split by a pause gets one answer; `voice.barge_in` rows come at the
commit, so health's barge-in count is real cuts. No config key, store format, protocol type or package. The install
removed the voice debug logging drop-in from the night before, so the journal is at its default level. Eddie's live
check (the report's seven steps and R27's four) waits until after theseus-3ug0 and 1cz8, which became the voice-echo
task (Item 215). Health after the restart: `theseusd check` exit 0, 9 secrets ready 1.05 s after the start,
startup serving at 45.3 ms (store 4.6, kernel 36.9 ms; load 11 to 19, not a quiet reading), Discord ready, **voice
ready**, the judge's live packs `security.v3`, `route.v1` and `rerank.v1` as before, memory live on the `baseline` arm,
`cgroup: delegated`, the unit active with NRestarts 0, and no error or warning in the journal; the store from format 22
to 23 at its first write, after the install's backup.

**Divergences.** **The floor applies to a reply's or report's first clip only**, not between the sentences of one
already playing: applied between sentences, a speaker on loudspeakers (whose VAD hears Theseus) would hold every
sentence boundary for 700 ms, the stutter the hold removes; between sentences, an utterance over speech is decided by
the hold's rules. Any listed speaker's words contend for a waiting reply, not only its turn's speakers. A failed
transcription over a reply that has not begun supersedes it, with no turn. The backchannel list is exactly the task's
("Mm-hmm" passes; "mhmm" does not).

**Known gaps.** R27's "For Eddie", each recommended: join, then fix **theseus-3ug0** and **theseus-1cz8** (P2) together
in one small follow-up before the live check, since both drop a real answer in silence where main made it a turn (an
echo a near-whole, in-order copy: at least 3 words, 80 % of them in one contiguous run; echo-prone only after two echo
verdicts; a reply that has not begun counts as the tail; a wider backchannel list, "mhmm", "oh, okay"); **theseus-aq4t**
(P2, low urgency: main was worse in both cases): a hold bound (transcripts still due about 3 s after the last
overlapping utterance closed decide as failed) and a floor bound (a reply waiting about 8 s on a floor held only by
sound plays); **theseus-e6mj** (P2): copy R27's two probes into `turns.rs`; **theseus-qrwx** (P3): a cut report waiting
at the call's end gets its `Cut`; **theseus-rnl3** (P2, main's catalog test under load). Keep the report's choices (the
floor for a first clip only, any listed speaker contending, `Resumed.why` the plainest, a failed transcription
superseding). Later, a spoken line for a failed transcription ("Sorry, I didn't catch that"), with theseus-9zft's.

### Item 203. Learning fixes: replay and the loop share one rule for whether a lean was right, the audit's requests run off its low thread, and the prove leaves a learned loop version's tasks out as `learned_version` and names its placement (theseus-m5az, theseus-bgg5, theseus-ag0t, theseus-vh67 and theseus-u4t3; the eighth cloud batch's learning-fixes session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5; 09378382, 1e57fae4, 8d623cb5, 0b38c8ee and df4f73fa; reviewed 2026-10-05 23:51 to 2026-10-06 01:09 by local reviewer R22, stack L; joined 2026-10-06 09:58 at 21bf5454, a signed merge onto f33f0eca, by the stack-L joiner, relaunched after the account's weekly limit stopped its first run at 01:52; installed 2026-10-06 10:12 at 21bf5454, install #6)

**Why.** Five gaps the learning steps' reviews left, each with its fix or its test named:
- **theseus-m5az.** 25d's replay (Item 144) read a yes-or-no (Noul) answer's label truth as whether the answer was
  right. `replay.rs`'s `right` took `graded(..)`'s second value, and for a Noul answer on a yes-or-no label `graded`
  returns the calibration pair: the probability and the label's truth, not whether the lean met it. So a judgment
  that leaned no on a label of yes counted as right. The learning loop's cloud session (25f, Item 164) had found it and
  written the correct rule (`Top::Noul(lean) => lean == x`) privately in `propose.rs`; local reviewer R7 checked it on
  main.
- **theseus-bgg5.** A thread the learning tender's low thread starts inherits its nice 19 (and the nightly run's
  `SCHED_IDLE`) for life, and tokio's blocking pool starts its threads on the caller's own thread when none is idle:
  a host-name lookup (reqwest's resolver runs `getaddrinfo` through `spawn_blocking`) reached through `block_on` from
  the low thread could leave a pool thread at nice 19, which then serves the daemon's other blocking work, the turn
  path's among it (R7, reviewing learn-loop, with a probe). learn-loop's join (Item 164, R7's join fix 4) had already
  moved the nightly loop's writer request and the replay's `Caller::ask` onto the runtime; the owner's
  `theseus judge audit` still polled its provider with `rt.block_on` on its `learning` thread.
- **theseus-ag0t** (R13, reviewing prove-wire-in, Item 179). Since learn-loop, the loop point judges with the newest
  learned version the ladder placed, while the prove read only `loop.v1` judgments: once a learned loop version is
  placed, its tasks would read as `never_judged`, and loop.v1's canary would in fact have been displaced in those
  sessions.
- **theseus-vh67** (R7's plant 4 on learn-loop). `JudgeService::placed` gives a learned canary only to sessions in its
  canary arm; planting that check out passed every test, though a live parent's candidate below the minimum goes to
  a 0.2 canary on no more evidence than its train errors fixed.
- **theseus-u4t3** (R13, Item 179). No test held that a declined move to canary leaves the prove's default window
  where it was.

**What landed** (`theseus-core`'s `learning/` and `rpc/judge_prove.rs`, one doc comment in theseus-protocol; the merge
12 files, +468 −66, without the cloud files; no new package, no store format change (23 stays 23), and nothing on the
start or turn path). Five commits, one per issue, in the order 1, 2, 4, 5, 3 (the two tests touch no code; the third
is the largest and shares `tests_prove.rs` with the fifth).
- **One `right`** (09378382, m5az). `report::right(a, t)`, beside `graded`, is the one rule; `propose.rs`'s private
  copy moved into it word for word, and `replay.rs`'s `right` calls it. `graded`'s doc now says what it gives: the
  calibration pair, which for a Noul is the label's yes or no. Its other users (the report's calibration pairs,
  `items.rs`, the labeled counts) want that pair and are untouched (R22 read each).
- **The audit's requests off the low thread** (1e57fae4, bgg5). A helper, `send_on_runtime(rt, &provider, request)`,
  clones the provider's `Arc`, moves it and the request into a task on the runtime (`rt.block_on(rt.spawn(..))`) and
  waits for it on the low thread; a `JoinError` becomes the request's error, counted `failed` and booked at its
  reservation as any failed request is. Reading the answer, the labels and the frame stay on the low thread. Inline it
  took `audit_run` past clippy's 100 lines, so it is a helper. The backfill already sent through replay's
  `Caller::ask` and needed no change.
- **A learned loop version in the prove** (df4f73fa, ag0t). `Input.learned` holds the names of loop.v1's learned
  versions (`lineage().names_of_root(LOOP_PACK)`, filled by `prove_input`); `build` collects the sessions a learned
  version judged, and `arm_of_task` leaves those tasks out as `learned_version`, checked after `cancelled` and before
  `never_judged`. A replay candidate such as `loop.v2`, which is not in the lineage, still says nothing. **The mixed
  case** (loop.v1's and a learned version's judgments in one task, which arises when a placement changes mid-task) is
  left out as `learned_version` too: the record's `false_completion` and `success` read the last judgment, which would
  be loop.v1's even where the learned version judged the last stop, so the record would credit loop.v1's arm with an
  outcome another version shaped (the same reason `both_arms` is left out). `prove_window` now reads `pack:loop`'s
  `pack.mode` rows once whatever the branch (a `--since` had returned before reading them), picks the default window
  exactly as before (the latest loop.v1 `canary` row not declined), then appends, when a learned version of the
  lineage was placed (`shadow`, `canary` or `live`, not declined) inside `[since, until]`, "; a learned version stood
  in loop.v1's place: loop.v101 moved to shadow on <day>", with ", the latest of N placements" when there were more.
  With none, the line is byte for byte as before. theseus-protocol's `left_out` doc lists `learned_version`, and the
  cockpit's generated `JudgeProveResult.ts` carries the doc comment; no cockpit code reads the reasons.
- **Two tests** (8d623cb5, vh67; 0b38c8ee, u4t3). `a_live_parents_candidate_below_the_minimum_goes_to_the_canary` now
  finds a session in each of loop.v101's arms and checks `placed` and `JudgeService::plan_loop_end` (what a turn's end
  calls to choose the version and mode) in each; the session chose `plan_loop_end` over whole turns, since a real
  turn makes its own session id. `the_default_window_is_the_canarys` writes a declined loop.v1 canary row after the
  accepted move and checks the window, its line and the task count unchanged.

**How it is proven.**
- **The session's tests:** tests_replay's `a_noul_that_leaned_against_its_label_is_an_error_the_candidate_fixes`
  (gull's `announced_unfinished` labeled yes with the incumbent at 0.2, an error the candidate fixes; tern's labeled
  no, right; totals fixed 2 and broken 1; `errors: true` selects exactly heron and gull); tests_audit's
  `an_audits_requests_are_polled_off_its_low_thread` (a stand-in provider records `getpriority` of the thread that
  polls each request: a 3-request audit must record the test thread's own nice value three times); tests_prove's
  `a_task_a_learned_version_judged_is_left_out_as_learned` (a task moves from `never_judged` to `learned_version`;
  then the mixed case: `learned_version` 2, through `judge.prove` too) and
  `the_window_names_a_learned_version_placed_inside_it` (no placement, the line as before; a declined row changes
  nothing; a shadow placement named with its day; a later canary named with "the latest of 2 placements"; once
  loop.v1's own canary move comes after both, the line names neither); and the two tests above. Each planted revert
  failed its test (replay's old rule: `left: ([], [])`; the old `block_on`: `left: [19, 19, 19]`, `right: [0, 0, 0]`;
  the learned judgments dropped; the window's append removed; the arm check out; `!m.declined` removed). 63 of 63 in
  the targeted suites; under load, four suites (30 tests) at nice 19 beside four busy loops, 30 of 30 three times. An
  earlier run's 8 failures were the session's VM disk filling (a second 26 GB target), not the code. The cloud gate's
  33 failures were a root VM's L1 tests (theseus-pv6i), none on the reviewer's machine.
- **The review** (R22, review commit 03b3e765 on main 591259fe, turn-stack's join, format 22): the build clean
  throughout (fmt, the test build, clippy, theseus-protocol's 31 with `JudgeProveResult.ts` matching, the cockpit's
  lint, 86 tests and build, shape); **428 of 428** in the suites the branch touches, the core's golden and the CLI's
  50 goldens among them (stack T's three `judge prove` byte goldens render a canned answer, so the daemon-side reason
  cannot move them); **the whole workspace on the merged tree, 2,848 of 2,848** (`--retries 0`, 01:01 to 01:08).
  **All 8 planted reverts caught**: the brief's six and two of R22's (the mixed task counted in its arm; the window
  line naming a placement from before the window). Plant 1 also ran all ten `tests_learn_loop` scenarios under
  replay's old rule, and all ten passed: the loop's proposals, holds, placements and rollbacks do not depend on
  replay's rule.
- **Live** (R22, scratch daemons on fresh state dirs; the stand-in model, a stand-in Jev, no key; arm B the merged
  tree, arm N the same tree with main's `audit.rs`). bgg5: six asks, 15 s quiet, then `judge audit loop.v1 --sample
  4` behind a proxy that held each request 11 s, past the blocking pool's 10 s keep-alive: both arms read "4 asked, 0
  failed; 8 audit labels" in 44.1 s, and on both no thread but `learning` was ever at nice 19 or `SCHED_IDLE`. The
  hazard did not trigger even on main's audit: an idle daemon keeps 19 threads (17 runtime workers on a 16-CPU VM),
  something uses the pool more often than its keep-alive, and each lookup found a warm pool thread. So main's hazard
  needs a busy pool; the branch removes it by construction, and plant 2 holds it at nice 0. ag0t: a staged
  `pack.version` row for loop.v101 and its shadow placement; prove 0 named the placement in the window line; after
  `packs promote loop.v1 --canary 0.5` and four tasks, "records by arm: canary 0, control 0", "left out:
  learned_version 4", and every one of the 12 `judge.call` rows was `loop.v101` with `pack_arm` `all`; after
  `packs promote loop.v101 --canary 0.5` and four more, the line named loop.v101's canary move and "left out:
  learned_version 8".
- **FAST.** Nothing on the start or turn path: m5az is the owner's replay and the nightly loop, both on a `learning`
  thread; bgg5 runs the audit's request I/O as one task at a time on a runtime worker, only when the owner asks; ag0t
  is a read on demand on the blocking pool, which now also reads `pack:loop`'s few rows and the cached lineage names.
  `bench turn --check` on the merged build, in a quiet hold (CPU PSI avg10 0.57): frames 5 and 9, plain p50 76.3 ms,
  tool call 161.4 ms, level with stack H's A/B on 3dba509d.

**What the review found.** Two findings outside the branch's code, neither blocking the join:
- **theseus-nwa5 (P2).** `placed` returns a learned version in shadow for every session (it checks the arm only for a
  learned canary), and the point then asks `mode_for` it: shadow. So while a learned loop version stands in shadow,
  **loop.v1's own canary acts in no session**, with nothing said; seen live above (prove 1). The nightly loop places
  in shadow only over a recording parent, so the case is the owner promoting loop.v1 to a canary while a learned
  version still stands in shadow.
- **theseus-clbx (P3).** The window line does not name a learned version placed before the window and still standing
  in it, though its tasks fill `learned_version` (seen in the same prove).
- And **theseus-fner (P3):** the audit's nice test passes on either build when the test process itself runs at nice
  19 (both read 19), as the report said; keyed on the polling thread's name it would hold at any nice.

**The join** (stack L, the branch alone). The first joiner took the lock `cloud-learning-fixes-join` at 01:28:17,
queued behind the bench stack, soul-import and durability-on, and was stopped at 01:52 by the account's weekly limit
with nothing merged; the DM thread re-armed the locks at 08:05:42, and the relaunched joiner resumed at 08:08. A lock
the DM thread placed ahead of it at 09:05 for voice-turns (Eddie's voice priority) held it until 09:37:26: 1 h 29 min
in the queue. Its `take-2.sh` read the store format from origin/main, where the first holder's had fixed it at 22,
since soul-import had moved main to 23. R22's dry run on each new main (a1bcbee2, 79be3213, then f33f0eca at 09:37:44)
was clean, with no file changed on both sides since the base. The merge at 09:37:59 onto f33f0eca: no conflict, no
resolve.py, no join fix; the staged tree equalled the dry run's tree less the two cloud files; 12 files, +468 −66,
the largest touched file `tests_prove.rs` at 986 lines; `MANIFEST_FORMAT` 23, main's. The warm (09:38:22 to 09:45:11;
the test build 4 min 29 s, since theseus-protocol's doc comment rebuilds most of the workspace; clippy clean); then
R22's suites plus `tests_output`, **437 of 437** in 30.2 s (the core 105, theseus-judge 136, theseus-protocol 31, the
CLI 165 with its 52 goldens), the branch's six new or changed tests among them. The signed merge **21bf5454**
(f33f0eca and 464ab903), 09:47:22. Its gate (09:48:00 to 09:58:28, ok; 221 s of it waiting for the lock behind two
reviewers' whole-workspace suites and a clippy, started at minute 48 because both suites were already in their test
binaries): **2,933 of 2,933** (1 slow, 23 skipped; voice-turns' 2,929 plus the branch's four new tests), the suite
from about 09:53:08 to 09:57:52, so no hour was crossed and both runaway-mode tests passed; lifecycle in every budget
(cold start p50 25.2 / p95 37.0 ms; from the config copy 24.0 / 26.7; clean shutdown 34.0 / 49.8; a post in flight
72.9 / 82.1; SIGKILL then restart 29.7 / 33.4; binary swap 48.3 / 58.5; restore 144.5 / 149.6; the daemon's own
serving 20.04 / 27.63); L1 start p50 7.20 ms; turn frames 5 and 9, plain p50 74.9 ms, tool call 157.5 ms. Pushed
09:58:36, the branch deleted, done line 09:58:51; the five issues closed with the hash. The store stays at format 23.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6; 21bf5454 is this join's own merge). Only the
owner's judge commands change; nothing the daemon decides on its own changes. `theseus judge replay` counts a Noul
answer right by its lean (its fixed and broken counts, the per-judgment lists and `--errors`' selection): replays made
before the install over-counted rightness on five of loop.v1's six questions, so any worth relying on is to be re-run.
The nightly learning loop always used this rule, so its proposals, holds, placements and rollbacks do not move, and a
version already placed is judged the same. `theseus judge audit` sends its requests on the runtime. `theseus judge
prove` leaves a task a learned loop version judged out as `learned_version` and names its placement; no learned loop
version stood on Eddie's daemon on 2026-10-05, so nothing reads differently until the loop places one. Health after
the restart: `theseusd check` exit 0; secrets 9 ready 1.05 s; startup serving at 45.3 ms (store 4.6, kernel 36.9 ms;
the load 11 to 19, not a quiet reading); Discord ready; the judge's live packs as before; memory live on the `baseline`
arm; voice ready; `cgroup: delegated`; the unit active, `NRestarts` 0; no error or warning in the journal. Eddie's
store moved from format 22 to 23 at its first write, after the install's backup (soul-import's bump, Item
201).

**Divergences.** The mixed case is left out as `learned_version`, the session's decision (R22: keep it). The window
line names only the latest placement and a count, not each one. vh67's "a turn in each session" is the loop end's own
decision, `plan_loop_end`, not whole turns.

**Known gaps.** R22's "For Eddie", each recommended: keep the mixed rule; keep the one-line window and fix
theseus-clbx (P3: name the placement standing when the window opens); theseus-nwa5 (P2): `pack.promote` of a root
warns while a learned version of its lineage stands in shadow, naming it, and §2.17's next revision decides whether a
shadow learned version should record beside an acting root rather than displace it; theseus-fner (P3), the nice
test keyed on the thread's name. theseus-nwa5 and clbx went to batch 10's learned-shadow task and theseus-fner to its
core-gaps task, both launched at 12:37. Main's bgg5 hazard was never shown live (a rig that saturates the blocking pool
first, such as several long proves over a ten-thousand-task store, would show it). theseus-core's AGENTS.md lines for
the prove's `learned_version` and the audit's runtime requests, owed by the review, were not written at the join.

