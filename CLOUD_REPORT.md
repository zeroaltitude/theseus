# CLOUD_REPORT: horizon-board (HZ8, theseus-nuhsz)

Branch `cloud/20261013-horizon-board`. One step, committed whole (the feature crosses protocol, core, CLI, TUI and bench, and no
smaller piece builds alone): `bfc30eac`, plus a last commit for a routine's row id and this report.

## What I found
- Base has work-board, routines, watches. `lib.rs` of the protocol was at its ceiling, so the seam is a submodule of `work.rs`
  (`work/horizon.rs`); `work.agenda` is added on an existing method line (lib.rs unchanged in length).
- The horizon doc (§5.8 to §5.10) and the ping rows H-3, H-8..H-13 are not in the repository: I built from the task's summary.
- Two latent board facts I had to fix: a series wake that fired was marked ended (same id continues), and a frame built every
  view before installing any (a routine reads its runs' views), so executions are now built and installed first.

## What changed
- Protocol: `WorkKind::{Routine,Watch,Project}`; `WorkView` `purpose,next_at_ms,series,paused,routine,watch,project`;
  `Horizon`, `LocalDay`, pure `horizon_of`/`bucket_at`; `work.agenda` types (cap 200); answers for a failed routine
  (run now, pause, details), a met watch (done, keep watching), an overdue milestone (open, move the date);
  `notices::answers_for`; `StatusShortResult.next_due_ms/routines_failed`; `digest_line` (pure, for work-digest's reader).
  TS regenerated and added. No store change, no format bump.
- Core: `push/work/horizon.rs` (views, routine state machine, watch end by rows, lazy merged agenda via `Repeat::next_after`,
  paused skipped, window jump with occurrence numbers), `rpc/agenda.rs`, snapshot appends the future views, `status.short` facts,
  a failed run speaks once through its routine.
- CLI: `theseus agenda [--watch]` (`--json`, hidden `--now`), `render/horizon.rs` (shared), status short/long show the next
  reminder and failed routines. TUI: Horizon section under the tree (`H`; `j k z p r x`, enter opens a routine). Sim: `bench agenda`.

## Proof
- protocol: 11 new horizon tests (every kind x times around midnight and the week edge; midnight move with no frame), wire fixtures.
- core `tests_horizon.rs` (5, real cores): views and the seed path, failed run -> NeedsYou with answers and one notice -> pause ->
  run now -> Ready, met watch Done, agenda weekdays/pause/cap, golden `tests/golden/horizon_board.txt` (stable over 3 runs);
  `push/work/tests_horizon.rs` (5): weekday and cron series across a US DST change keep wall time, far window numbering, until, cap.
- CLI: render golden + `tests/agenda.rs` golden; TUI 7 tests; cockpit lint/test/build pass.
- Planted reverts: `bucket_at` reading the server clock -> 3 protocol tests failed; agenda not skipping paused -> `tests_horizon::the_agenda...` failed;
  settle building wakes before installing exec views -> the failed-run test timed out (seen before the fix).
- Full suite (`cargo nextest run --workspace`): 5023 run. 43 failures: 33 known L1 (sandbox/theseusd::sandbox); 6 status tests that WERE mine
  (my extra `status.short` call in the watch reload; fixed, `-p theseus` now 472/472); `rendering_costs_little_per_hundred_lines`,
  `tests_kept::fs_read_of_a_long_file...`, `sim a_killed_store...` failed under load and pass alone. Clippy -D warnings, fmt, shape, cockpit, deny: clean.
- Bench: debug build on this 4-core VM, `bench agenda`: p50 ~8 ms (floor 0.6 ms), p90 ~10 ms: over 5 ms in debug; release not measured
  (disk allowance). Re-run `--check` on a release-thin build. Per-frame push micro-bench not re-run (work after the lifecycle push phase: run `bench lifecycle --phases push`).

## Gate
The gate's door/keel phases fail on findings of the BASE, not mine: `lifecycle.rs:155 budget-raised seed` (its ack is signed by a key this VM cannot verify) and
two acks of 238ecead/1c2beb27. I did not bypass them; I ran fmt, shape, clippy, cockpit, the suite, generated-types and deny by hand.
`python3 scripts/keel-guard.py` lists only that one erosion. **Keel findings expected:** none from this branch.

## Live check for the maintainer
Scratch daemon (config with `[secrets] anthropic_api_key/zai_api_key = "env:X"`, web and discord off): `theseus remind "in 2h" X`, `routine add "weekdays 08:00" B --title T`,
`watches add Q --file /nonexistent --until exists`, `task start B --in-time 5h`, then `theseus agenda` and `theseus status --short`.
I ran this here: sections Today/This week/Recurring/Waiting on the world rendered, `status --short` showed `◐1 ⏰ 06:51`. Not run: a failing routine, `agenda --watch`, the TUI in tmux.

## Left open / choices for the owner
- "Dismissed" for a failed routine needs a record: not built; it clears on a good run, pause or delete.
- "Keep watching" names `watch.keep`, which has no verb (join note). Overdue milestone answers/`project.move` wait for `projects`; ProjectView is declared, empty.
- Digest: `digest_line` has no reader (work-digest not on main); a routine run's end is the task's own notice.
- A watch's `next_at_ms` is its expiry (next check lives in the checker's memory). Routine 30-day counts use the ended runs the board keeps (50 per root);
  skipped counts since the board began; routine month spend is the execution's spend.
- TUI sends `work.watch` non-interactive (so owner-ask counts are unchanged). Docs to update: status.md/spec Part III (HZ8), the horizon design's §5.10 layout is mine.
