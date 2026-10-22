# CLOUD_REPORT: tail3-discord (theseus-wy8pt, theseus-ysp2k, theseus-we57k)

Branch `cloud/20261021-tail3-discord`, base `29f5dcad`. Store format unchanged (48); no stored field, no config key, no new dependency. No ceiling grew past its limit (`runtime.rs` untouched).

## theseus-wy8pt: a read forum post leaves within the hour after its day (062ce6be)
- **Found.** The archive ran only on the task channels' sweep (start, then daily), so a read post left 24 to 48 h after its end. `TaskPosts::seen` marked only rows with `ended_ms` set, so a Clear done pressed before the worker wrote the end was lost.
- **Tick's shape.** `ARCHIVE_TICK` (1 h) in `forum/archive.rs`; `Shared::archive_tick` spawns `every(period, stopped, run)`, which sleeps then runs, until `outbox.stopped()`. It is spawned by `sweep_task_channels` (one line). `sweep_once` still calls `archive_read_posts` (the existing tests call `sweep_once` and rely on it; the start-up run is also useful), so the tick only adds runs. A read post now leaves at most an hour after its 24 h.
- **Read rule.** A post is also read when `Tasks::cleared_since_end(task)` holds: some place whose roots hold the task cleared at or after the task's own `ended_at_ms` (the view's end, not the post's `ended_ms`, which the worker writes later, so "clear at t, end written t+1 s" counts). It is computed at each tick/sweep for finished unread posts and kept in the existing `read_ms`, so the answer survives the view leaving the model. No new field.
- **Tests and plants.**
  - `forum::archive::tests::a_read_post_leaves_within_the_hour_after_its_day`: tokio paused clock; `every` with `ARCHIVE_TICK` runs once after 3601 s; a post read and 24 h + 1 min old is `due` at the next tick. Plant: `ARCHIVE_TICK` = 24 h: fails (`left: 0` runs). Note: this is a unit test of the loop and the due rule, not the fake-Discord rig: a paused clock with real sockets jumps to the daily timer on idle, so the rig cannot tell the two cadences apart.
  - `tests_forum_archive::a_clear_before_the_end_still_counts_as_read` (rig; `read_ms` taken off the row, the board model given the ended task through `Tasks::ended_for_tests`): unread until a clear, archived after. Plant: cleared rule off (`.filter(|_| false)`): fails at `tests_forum_archive.rs:169`.
  - `runtime::tasks::tests::a_clear_after_the_tasks_end_has_seen_it` (model).
- The existing forum-archive and task-channel tests pass unchanged.
- Also here (it shares `tasks.rs`): the comment at `Tasks::snapshot`'s re-apply site (ysp2k finding 2).

## theseus-ysp2k: the DM's stale task board (17d00de5)
- **Option taken.** The lane's cleanup (smaller than holding the first draw, which would need to know whether a home is wanted and pending). `ensure_task_home`, when the home first changes, sends the owner's DM lane `LaneMsg::BoardMoved`; `Lane::board_moved` drops board ops waiting and forgets the board's id, `drop_stale_board` (in the lane's loop after `v2_tick`) deletes the message (a 404 is done). No "moved to" one-line form exists in the code, so the message is deleted rather than edited (a V2 message cannot be edited to plain text anyway).
- Module-doc sentence on the core-side watch-scope race added in `task_home.rs`.
- The fake Discord gained `DELETE /channels/<c>/messages/<m>` (`theseus-sim/src/fake_discord.rs`, small).
- **Test and plant.** `courier::tests_v2::the_dm_board_drawn_before_the_home_answers_does_not_stay` (lane level: the real daemon's `Shared` is not reachable from the rig, so it is not driven through `ensure_task_home`). Plant: `board_moved` forgets the id without keeping it for deletion: fails at `tests_v2.rs:320` (board still there).
- Left: a place that drew to the DM in the instant between the state holding the home and the message may leave one more board (documented in the crate's AGENTS.md). A board pinned in the DM by an earlier run is not found (as before).

## theseus-we57k: the voice merge's bounds (ca406186)
- (1) `Vad::opened_at()` (first tick, pre-roll included); `awaits_its_rest`'s open-VAD arm counts only a VAD that began at or before `last.closed + merge`. Test `an_open_vad_after_the_window_does_not_hold_the_turn` (transcription delayed 3 s, a new utterance opens at 4 s): turn at 4.7 s. Plant (arm back to any open VAD): turn at 7.7 s, fails.
- (2) **Carry**, not assert: `join` sets `last.over = next.over` (`goes_on` guarantees `last.over` is none), so a barge-in is never folded away; an assert would crash a call on a case nothing forbids. Unit test `engine::tests::a_joined_utterance_keeps_the_later_ones_over`; plant (line removed): `left: None`.
- (3) **Doc only**: the hold gates the whole line so turns stay in the order utterances closed; a correct per-speaker bound would reorder turns. Bound documented on `awaits_its_rest` and in `crates/theseus-voice/AGENTS.md`: past the close by `merge_ms`, or by the end of an utterance the speaker began within it. No test (no code change).
- `an_unfinished_utterance_waits_for_its_rest` passes unchanged.

## Proof
- `cargo nextest run -p theseus-discord -p theseus-voice -p theseus-sim`: 862 run, 860 passed; the two failures (`the_judge_benchs_every_pack_arm...`, `the_restore_rows_store...`) need the `theseusd` binary, which that `-p` build lacks; both pass in the gate's workspace build.
- `cargo fmt --check` and `cargo clippy -p theseus-discord -p theseus-voice -p theseus-sim --all-targets -- -D warnings`: clean (the gate's workspace clippy phase also passed).
- Gate: `CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=29f5dcad TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit passed; suite: 7638 run, 7604 passed, 34 failed. **All 34 failures are the known L1 ones** (theseusd `sandbox` tests; the L1 refusal under root, theseus-pv6i), none in the touched crates. The phases after the suite (protocol types, lifecycle, jobs, turn, size) were not run: no protocol type changed and benches are off by `NO_BENCH`. A first gate run died on a full disk (incremental caches); `target/debug/incremental` was removed and `CARGO_INCREMENTAL=0` used.
- **Keel.** `origin/main` in this clone is stale (`1d092ded`), so the gate's default keel base flags base commits that are not mine (`lifecycle.rs`, `keel-guard.py`); I ran it with `THESEUS_KEEL_BASE=29f5dcad`. `python3 scripts/keel-guard.py --base 29f5dcad --head HEAD --quiet`: empty output, exit 0. Against the stale `origin/main` it is not empty, for that reason.
- Commits are unsigned (no key on the VM).

## Live check (the maintainer's)
Start a scratch daemon with `[discord] task_home` and a bound DM on a fresh state dir; let the DM draw its board (a task running) before the home is found (e.g. start with the home channel's creation delayed): expect the DM's pinned board message gone and one board in the home. For the archive: a finished, read post whose `ended_ms` is 25 h ago in META `discord.task_posts` is archived within an hour of the start, with no restart.

## status.md sentence (for tail3-google)
"A read forum post of a finished task now leaves the forum within an hour of its 24 h (an hourly archive tick; a Clear done pressed before the post's end was written still reads it); the owner's DM no longer keeps a task board drawn before the task home stood; the voice merge's open-VAD hold is bounded by the merge window and a joined utterance keeps its barge-in."

## Follow-ups
- A rig-level test of `ensure_task_home` removing the DM board needs the daemon's `Shared` exposed to tests.
- A DM board pinned by an earlier run, with the home recorded, is never cleaned (outside this row).
