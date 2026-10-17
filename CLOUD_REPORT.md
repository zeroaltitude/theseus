# CLOUD_REPORT: discord-reactions (theseus-bh3t.2)

Commits: e175ba56 (the step), then a test fix and this report.

## What I built
- **Core**: `task.pause` / `task.resume` (`rpc/task_pause.rs`, params in `theseus-protocol/src/task_pause.rs`, two names on the
  `TASK_CANCEL` line). Owner only (`Act::Session`). The hold is META `task.paused.<session>` plus an in-memory set
  (`toolrun/waits/pause.rs`, one atomic load when none is held; read back from the keys at the first ask). Rows `task.paused` /
  `task.resumed` (two ledger kinds, no store-format change) mark `WorkView.paused` on the work board.
- **Park**: `TurnRunner::paused_at` at each loop top (`turn/join_step.rs`, one call in `turn.rs`); `task_end` keeps the wait.
  **Divergence**: a held task waits on `Wake::DueAt{PAUSED_AT_MS}` (year 2100), not `Wake::Input`: the kernel re-queues a task
  parked on input with no wakes (`task_unparked`), which looped forever. Labels/agenda treat that time as "paused".
- **Resume** clears the hold; if parked it writes one operator node `(resumed by the owner)` and `kernel.wake`s the execution
  (divergence: not `turn.submit`, which would block the RPC).
- **Discord** (`runtime/task_react.rs`, one call in `route.rs`, one `mod` line in `runtime.rs`): ⏸ ▶️ 🛑 🔁 👀 📌 as the owner in a
  private place; others/unknown emoji/shared place: nothing. ✅ after an action, a quiet reply on refusal. 🛑 posts the board's
  own V2 confirm; Yes is the existing `tb:yes:`. 🔁: `task.retry` for failed; cancelled/done (core refuses) start anew via
  `task.start` from the brief. 👀 mirrors in the owner's DM; 📌 `project.create` + `project.attach`.
  `project.attach` gained optional `author`/`discord` (wire only) so the core can judge the owner.
- **Pre-added controls** on the thread's first message (`say_to` now returns the post id; courier calls `Shared::noted`) and the
  Follow mirror, in order; ▶️/⏸ flip when a reaction pauses/resumes. Task messages: any message in a task thread, mirror, thread
  first message, `report:` line. Board message and `/task` Details stay buttons.
- **Fake Discord**: `fake_discord/reactions.rs` (PUT/DELETE `@me`, `reactions()`, `react()`), 2 router lines.
- `state_words` says `⏸ paused`; TS regenerated.

## Proof
- Core: `rpc::task_pause::tests` 3/3 (park without a second model call or report, resume reports, owner-only/ended refused, hold
  survives a restart, board `paused`).
- Discord: `tests_task_threads::react` 9 tests (controls pre-added in order; pause/resume; stop asks then Yes; retry failed
  (linked) and cancelled (new); follow; pin twice; guest/unknown emoji; shared channel).
- Planted reverts: owner check removed -> `a_guest_an_unknown_emoji...` FAILS; park disabled -> `a_held_task_parks...` FAILS;
  🛑 cancels directly -> `stop_asks_in_the_place...` FAILS. Private-check revert **not proven**: its test uses a shared-channel
  task thread, which hangs on this VM (see below).
- Not done: runs under load.

## Gate (`THESEUS_KEEL_BASE=c6f62abf TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`; needed because origin/main moved)
fmt, shape, features, clippy, cockpit, test build, reader rule: ok. Suite failed only on: ~33 known L1 tests; two
`tests_horizon` tests (pass alone; load); and three Discord timeouts in shared-channel task-thread tests:
`a_shared_channels_task_thread_is_shared...` and `a_task_in_a_guild_channel_gets_a_thread...` (the first times out the same way on
the base commit c6f62abf on this VM) and my `in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing` (flaky, 1 pass in 3 here).
Later phases (protocol types, benches, deny) did not run; deny fetch was not checked.

## Live check
Start a two-step task from a private channel; react ⏸ on the thread's first message: board/state says paused, the task parks and
posts no report; react ▶️: it finishes and reports. 🛑 shows the confirm, Yes stops it.

## Left / for the owner
No CLI verbs for pause/resume. Reaction removal not done. Flip of the ▶️/⏸ pair only follows reactions, not pauses from other
surfaces. AGENTS.md notes for theseus-core and theseus-discord not written. Resume's wake has a small race if the turn is mid-park.
