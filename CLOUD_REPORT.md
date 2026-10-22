# CLOUD_REPORT: tail-flakes

Branch `cloud/20261021-tail-flakes`, base `6918e3c1`. Five commits, one per Beads id, all test-only. No product code changed.
No timeout, `PATIENCE`, `secs` argument or sleep was raised, and the flaky list is untouched. One test config sets the judge's
own deadline high (see theseus-iubei). No new dependency, no store-format change.

## How far each proof went (read this first)

- This VM has 4 cores and a small disk allowance. The disk filled twice during the build, so I deleted incremental caches
  and old test binaries, and built with `CARGO_INCREMENTAL=0`. I did not run the whole `cargo build --workspace --all-targets` to the end.
- I did **not** run the full gate (`scripts/gate.sh`) or its suite phase. Run it on the owner's machine.
- What I ran per commit: the touched tests (alone, and their module or file), `cargo fmt --check`, and
  `cargo clippy -p theseus-core -p theseus --tests -- -D warnings` (clean, at the head).
- The load recipe's "5 rounds" was run only for theseus-iubei. For the others, the race was shown with a deterministic **plant**
  (the brief allows it); the exact plants are below. No 5-round before/after counts exist for 4vgw1, t0y6v, fumkt or f4tg1.
- Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0).
  Caveat: `origin/main` in this clone may be older than the real main.

## theseus-4vgw1 (f5ed4f0): lag between the row and the notification

- Found: the notification follows the frame that wrote the `wake.fired` row. A reader that sees the row can drain the channel first.
- Changed: `with_no_dm_it_lives_in_a_reminders_conversation_and_every_surface_hears_it` awaits `next_noticed` for the first
  notice. "Heard once" now means the next notice is the *other* reminder's and has a different wake id. Every old assertion is kept.
- Proof (key test): plant `std::thread::sleep(300ms)` in `Rec::announce` (`fact/mod.rs`) before a `WAKE_NOTICED` is sent.
  - Old test: **FAIL**. New test: **PASS** with the same plant. The other 6 reminder tests also pass.
  - Behaviour plant, announce returns without sending the notice: new test **FAIL** ("a wake.noticed within 20 s").
  - The plants were reverted and `touch`ed; `git status` clean.

## theseus-t0y6v (c270009): read-after-write lag, not a lost pause

- Decision: lag. `work.agenda` reads the in-memory board (`push/work/horizon.rs` `agenda`, which skips `w.paused_at_ms.is_some()`).
  The push applies a frame on the board after the commit (`push.rs`, the `b.apply(core, &f)` line).
- Changed: `board_caught_up(core, routine session)` (existing helper) after `routine.pause`, and after the delete and create that fill the cap.
  The `assert!(!paused...)` stays, on the read after the wait.
- Proof: plant `sleep(300ms)` before `b.apply` in `push.rs`.
  - Old test: **FAIL** at "a paused series has no occurrences". New test: **PASS**. All 10 `tests_horizon` tests pass.
  - Behaviour plant, `agenda` ignores `paused_at_ms`: new test **FAIL**.

## theseus-fumkt (f4e288e): coalescing confirmed (the adapter's design, not a lost report)

- Found: `coalesce` in `herdr.rs` drops a `pane.report_agent` that a later one in the same batch replaces. The report for
  "sending your message" is sent when the line is typed. The label goes back (`restore()`) only when the **task's** `turn.submit` is refused
  (`interactive.rs`), not when main answers.
- Plant: the fake herdr sleeps 1 s on the first `pane.report_metadata`. The old test timed out waiting for 3 reports
  (it saw 2 because the two shared a batch). Hypothesis confirmed.
- Changed: the task's refusal of the line is held until the fake herdr has recorded the "sending" report (a channel from the fake
  herdr's answer closure). The two reports can then never share a batch, so all three reports and their order are guaranteed,
  and every old assertion is kept. The hold is unbounded; a missing report fails the existing waits.
- Proof: new test **PASS** under the same plant that failed the old one. Behaviour plant, `rep.restore()` removed in `interactive.rs`: **FAIL**.
  Whole `herdr` file: 19 of 19 pass.

## theseus-f4tg1 (bdde831): slow sink, plus a negative timed by a clock

- Found: the `judge.call` row is the judge sink's. The sink waits a 2 s window, then a quiet stretch between turns. The judgment is
  settled before `people.extracted` is written. Plant: a 25 s sleep in the sink writer's loop (`judge/sink.rs`) reproduces the exact
  message "0 of 1 judge.call rows" on the old test.
- Changed: `flush_sink()` after `people.extracted`, then `until_rows` (it now waits only on a write already in progress).
  The 500 ms vacuous "shared place gets nothing" sleep is replaced by waiting until no live decision is running (`until_not_deciding`;
  an exchange end inserts its session into `deciding` synchronously and removes it when done).
- Proof: old test **FAIL** under the plant (the other 7 people tests also fail under it, as expected). New test **PASS** under it.
  Behaviour plant, the shared-place class check disabled in `live.rs`: new test **FAIL**.
  The `judge::people` module passes 19 of 19. I edited neither `extract.rs` nor `run.rs`.

## theseus-iubei (bc710db): the judge's own deadline, and a shared 20 s wait

- Found under the load recipe (6 busy loops at nice 0, tests at nice 19, 5 rounds, on the base): **5 to 8 of the 15 tests failed in every round**.
  - Seven tests failed with the same wait, "no the task's turn in 20 s". That is `tests_task_brief::run` (not in my files), a 20 s clock for the task's first turn to end.
    Round 1: `a_cold_switch_waits_for_a_second_agreeing_verdict`, `a_checkpoints_text_is_bounded_for_a_task_of_long_loops`,
    `a_task_failing_for_twelve_loops_asks_three_times`, `one_agreeing_verdict_then_a_disagreeing_one_leaves_it_up`,
    `a_task_steps_back_to_its_base_after_two_agreeing_verdicts`, `a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes`,
    `the_cap_stops_moves_up_and_not_the_way_down`. Later rounds name others.
  - After fixing that wait, `a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes` hung in 5 of 5 rounds. Debugging showed
    `route_answers` was 2 at the gate (1 unloaded): the judge's default `judge.total_secs` (5 s) ended the held verdict before the starved
    loops reached the gate. That is the product's clock, not a lost event.
- Changed: `task_run` waits for the task's execution to complete (`test_support::until_event`: no deadline, ended by nextest's
  kill on a hang). The slow-verdict test awaits the gate, the asked checkpoint and the answer by events, with no `timeout` and no `until_within`.
  Its rig sets `judge.total_secs = 3_600` so only the test releases the verdict. **Design choice for the owner**: that is a longer
  product deadline in one test's config. The alternative is a shorter gate (fewer loops after the ask), which is still wall-clock bound.
  I did not touch `tests_task_brief.rs` (other lanes use `run`; its 20 s wait will flake the same way elsewhere) or `checkpoint_step.rs`.
- Proof: after the change **15 of 15 pass in 5 of 5 rounds under the same load** (about 74 s a round, one slow). 15 of 15 unloaded.
  No planted revert was run for this one beyond the before/after of the loaded rounds. Its assertions are all kept.

## What to run on the owner's machine

- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (not run here).
- Load recipe for any of them, e.g. `for i in 1 2 3 4 5 6; do sh -c 'while :; do :; done' & done` (note the pids), then
  `nice -n 19 cargo nextest run -p theseus-core -E 'test(/tests_reminders::/)' --no-fail-fast` and
  `nice -n 19 cargo nextest run -p theseus --test herdr`. Kill the loops by pid.

## Follow-ups

- `tests_task_brief::run` waits 20 s by the clock for a task's first turn (the same failure as iubei in other files).
  Switch it to `until_event` too.
- No fix was needed in `checkpoint_step.rs`, `extract.rs` or `run.rs`.
- `work.agenda` does not promise read-your-writes. Tests can use `board_caught_up`. I would not add a product barrier.
- Other flaky tests seen: none by name.
- Docs: nothing for the spec or `status.md`. `test_support.rs` gains `until_event`.
