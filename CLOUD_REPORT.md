# Cloud report: thread-board (theseus-5i8i)

Branch `cloud/20261013-thread-board`. Code commit `9b5bb9d8`. No store format bump, no new config key, no new dependency.

## What I found
- Main had T1 (thread-record) and the work board, not T2 to T4. So the routing ask and the T3 seal card do not exist yet; only `ThreadInfo.seal_pending` does.
- No surface drew the work board in a grouped way: the TUI and cockpit read `executions.watch`, not `work.watch`. I added a `work.watch` consumer to each.
- discord-board has not joined (only the per-place task board, `courier/board.rs`). Discord is not done. A thread is private always, so a shared-place board would never show one; the owner's-DM board is the seam.

## What I changed (all in 9b5bb9d8)
- Protocol: `WorkView.thread` (wire field), `ThreadGroup` (in `work_watch.rs`), `WorkWatchResult.threads`, `WorkChanged.threads`; `work_threads.rs` holds the one grouping/counting rule and header wording. TS regenerated; new fixture `watch_result_threads.json`.
- Core: `push/work/threads.rs` (groups, in-place restamp, stale resend), `Threads::on_change` feed, `Push::set_threads`, `Core::thread_groups` (stats read once per watch), `work_board.rs`. A guarded connection gets no group.
- CLI `theseus work` groups by thread; `--flat` is today's list; `--watch` prints group changes.
- TUI: Threads block, `T` key, fold/open, seal as a needs-you line, seals added to the header count.
- Cockpit: `/threads`, `/threads/:slug` (card, working set, parent/split_from/merged_into links, recent turns, open work), `threadboard.ts` (pure), `threadwatch.ts`, time machine `foldThreads` over `thread.*` rows (the work under a thread is the present's; the view says so).
- Counting: needs you first within a group; a question counts once; a pending seal counts one; a paused thread keeps its count and leaves the board's; a done thread shows only while open.

## Proof
- New tests: protocol `tests_work_threads.rs` (10), core `rpc/tests_thread_board.rs` (4, includes no record read / no index row on a group update, and a guarded surface told of none), CLI `work.rs` (3, two widths), TUI `tests_threads.rs` (6, widths 80/40/30), cockpit `threadboard.test.ts` (11), daemon `theseusd/tests/thread_board.rs` (real daemon, two threads each with a task and a question, real `theseus work`, `--flat`, pause leaves the count).
- Gate (`THESEUS_GATE_NO_BENCH=1`, `THESEUS_KEEL_BASE=d0527ec1`): fmt, shape, features, clippy, cockpit lint/test/build, reader rule pass. Suite: 4,542 tests; the only failures are the 33 known L1/sandbox ones (root VM). The gate's later phases (generated TS, benches) were not run separately; TS is committed, benches skipped by NO_BENCH. `theseus-sim bench lifecycle --check` was NOT run. No plain-turn path changed (no new frames or timers), but unmeasured.
- Planted reverts, each caught: task left ungrouped (`new.thread = None`) -> core `a_threads_conversation_and_its_task_carry_the_thread...`; sealed title without lock -> protocol `a_sealed_threads_title_has_a_lock` and cockpit `a sealed thread's title has its lock`; paused counted -> `a_paused_thread_leaves_the_boards_count`; a store read in `set_threads` -> `a_watch_update_reads_no_store`. Files restored and touched.
- Keel: the guard against the stale `origin/main` ref reports one erosion that is not this branch's (the work-board join's `lifecycle.rs` ack, unverifiable signature here). With `THESEUS_KEEL_BASE=d0527ec1`: 0 findings. **Keel findings expected: none.**

## Live check for the maintainer
- Real-daemon test already automates it: `cargo nextest run -p theseusd --test thread_board` (passes).
- By hand on a scratch daemon: `theseus thread new tides --title Tides`, same for another, `theseus ask --thread tides ...` with a task and a write that asks; then `theseus work` (a group each, "Threads: 2 live, 2 need you"), `theseus tui` (press `T`), and the cockpit at `/threads`. Headless light/dark screenshots of the cockpit and a TUI session in tmux were NOT taken on this VM.

## Left / uncertain
- Discord pinned board: not done (see above). herdr seam: the groups ride `work.watch` and `work.changed` (`threads`), so a follower can open one pane per thread from the stream; nothing in herdr changed.
- Routing-ask needs-you item: waits on T4. If T4 makes its card a `Question` view it groups automatically (views are stamped by root); the seal has no answer verb until T3.
- Group stats (context tokens, today's dollars) are read at each watch and thread write, not per turn; last activity is live (from the conversation view). Warm = last activity within 5 minutes.
- Docs to update at review: spec Part III item, status.md; AGENTS.md of protocol, core, CLI, TUI and cockpit are updated here.
- The VM's disk filled twice during builds; I cleaned incremental caches (`CARGO_INCREMENTAL=0`).
