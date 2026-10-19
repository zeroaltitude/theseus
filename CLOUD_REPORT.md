# CLOUD_REPORT: turn-end-facts (theseus-53ujg.1)

Branch `cloud/20261019-turn-end-facts`, base `bbc65530`. One implementation commit, `7f86a2c1`.

## Found
The brief's two habits are real in the code: nothing told the model, at the moment it mattered, which files a
command left beside its edits, or that it edited after its last command. A result said only
`[exit code 0 · in /dir]`.

## Changed (7f86a2c1)
- `toolrun/job/beside.rs` (new; `pub(crate) mod beside;` in `job.rs`, and `mod job;` became `pub(crate) mod job;` in
  `toolrun.rs`, a changed line, no new one): the launch listing (`beside::before`, via `theseus_store::blocking`; job cwd
  plus the directories of the turn's written files, at most 8, a directory over 512 entries skipped), the result line
  (`beside::line`, cached by correlation id, `.gitignore` honoured through `ignore::gitignore::GitignoreBuilder`, new
  entries no newer than `Completion::finished_at_ms`, ordered by mtime then name, `and N more` after 8), the
  per-execution union of new files and the last finished job's end. A batch's next step relaunches under the same id and
  gets its own listing and line. `job.rs`: one call at `launch`, one argument in the `format!("{header}…")`, one line in
  `remove_job_output`.
- `rewind.rs`: `Rewind` carries the execution id; `settle` notes `(path, created, at_ms)` per write in a static map;
  `written`, `forget_written`.
- `turn/nudge_step.rs`: `may_continue` (shared place, strict cap, job running) shared by `may_nudge` and the end note;
  `Nudge::sent`. `may_nudge` now checks the judge switches first, then `may_continue` (same outcomes, different order).
- `turn/end_note_step.rs` (new): the note, `files_left` for the result, and `Held`, whose `Drop` drops the turn's maps on
  every exit. `turn.rs`: `mod` line, the field, its init, one call after `stop_nudge`, `files:` in the result.
- `fact/turn.rs`, `fact/mod.rs`, `ledger.rs`: `TurnEndNoted` (`turn.end_noted`), and `turn.ended`'s row carries
  `files {changed, left, end_note}` counts for measurement.
- `theseus-protocol/src/files_left.rs` (new; `pub mod` line, `files` field in `lib.rs`; `ts.rs` types added to an
  existing line): `FilesLeft`, `PathList`, `EndNote`, `harness_lines` adds `Files: changed main.rs; left by commands: main, cmain`.
  `cockpit/src/protocol.gen/` regenerated (3 new types, `TurnSubmitResult`, `index.ts`).
- `[model] end_note = true` in `config.rs` and the template. `ignore = "0.4.33"` added to theseus-core (already in the
  lock; `Cargo.lock` gains only that dependency edge). `crates/theseus-core/AGENTS.md` has a paragraph.
- Existing expectations that moved, both for the new line/field: `tests_m3::a_job_past_its_output_cap_…` (its command
  creates `ran`, so the result now has `[new files: …/ran (0 B)]`), and 3 lines of `tests/golden/core_output.txt`
  (`turn.ended` ledger row, its outbox copy, its notification, each gaining `files`). Nothing else in the golden moved.

## Proof
- New tests (all pass): `tests_end_note.rs` (`a_turn_that_leaves_build_outputs_beside_its_file_is_told_once`,
  `a_commands_result_names_its_new_files`, `edits_after_the_last_command_are_named`, `a_turn_that_edited_nothing_is_not_told`,
  `no_end_note_with_the_switch_off`, `no_end_note_in_a_shared_place`, `a_turns_files_are_dropped_at_its_end`),
  `tests_stop_nudge::no_end_note_after_the_stop_nudge_continued`, `beside::tests` (6: names and stable line, eight and
  more, gitignored, 513 entries skipped and 512 read, no completion/no listing, newer than the job's end), protocol
  `files_left::tests` (2). `nextest -E 'test(end_note) or test(beside) or test(stop_nudge)'`: 32 passed. Protocol: 185 passed.
- The shared-place test seeds the facts (`rewind::seed_written`, `beside::seed`, cfg(test)): a shared place runs no
  command of its own, so it cannot produce them honestly.
- Planted reverts (each restored, `touch`ed, `git status` clean):
  | plant | failed |
  |---|---|
  | end note skipped (key test, plant 1) | `a_turn_that_leaves_build_outputs_beside_its_file_is_told_once` |
  | no once-a-turn guard (key test, plant 2) | same test, by **timeout at nextest's 120 s**: with no guard the turn loops forever, so it fails as a hang, not an assertion |
  | `[new files]` line dropped | `beside::tests::a_commands_result_names_its_new_files`, `tests_end_note::a_commands_result_names_its_new_files` |
  | "a command finished before" condition dropped | `edits_after_the_last_command_are_named` |
  | shared check dropped from `may_continue` | `no_end_note_in_a_shared_place`, `tests_stop_nudge::a_shared_place_is_never_nudged_and_the_owners_dm_is` |
  | switch ignored | `no_end_note_with_the_switch_off` |
  | nudge not consulted | `no_end_note_after_the_stop_nudge_continued` |
  | `.gitignore` ignored | `gitignored_outputs_are_not_named` |
  | big directory read in part | `a_big_directory_is_skipped_not_read_in_part` |
  | turn maps not dropped | `a_turns_files_are_dropped_at_its_end` |
- `tests_m3::a_plain_turn_stays_within_its_frame_budget` and `tests_first_request.rs` passed unchanged in the suite. The
  end note adds no frame beyond the nudge's shape and no first-request text. **Not measured here:** the gate's turn
  bench (`THESEUS_GATE_NO_BENCH=1`; budgets are the owner's machine's). A plain turn's added work is a few map lookups at a
  stop and one `readdir` set per `proc.run` launch and result.
- Keel: `python3 scripts/keel-guard.py --base bbc65530 --head HEAD --quiet` printed nothing (rc 0). This clone's
  `origin/main` is stale (`96c592c0a4`, older than the base), so the gate's keel phase failed on commits that are not
  mine; I ran the gate with `THESEUS_KEEL_BASE=bbc65530`.

## Live check for the maintainer
On a scratch daemon (own `--config/--socket/--state-dir`) with a model key and L1/L0 as usual, in a scratch directory:
1. Ask: "Write hello.rs printing hello, compile it with rustc to check, then stop." Expect the `rustc` result to show
   `[new files: …/hello (… KB)]`, one `harness:end_note` node (`theseus history`) reading `[The harness: this turn would end
   here. Left beside its edits: …/hello.]`, the model going on once, and the footer line `Files: changed hello.rs; left by
   commands: hello`. `theseus ledger | grep turn.end_noted` shows one row.
2. Set `[model] end_note = false`, restart the scratch daemon, repeat: result line and `Files:` line stay, no note.
3. In a Discord shared channel: no note, no `Files:` line.

## Left / uncertain
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=bbc65530`): door, keel, keel tests, fmt, shape,
  features, clippy, clippy-probe, cockpit, test build and reader rule pass. The suite phase (6,862 tests) fails only on:
  - 32 L1 tests (`theseusd::sandbox`, `theseus-sandbox::contract`, root VM; known, theseus-pv6i);
  - `theseus-discord` `tests_task_threads::*` (up to 4) and `tests_war_room::a_shared_place_refuses_a_share_…`:
    120 s timeouts under the suite's load; the 20 tests of those files all pass run alone;
  - `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`: failed twice (once in the gate,
    once alone right after a build; `dispatched` 2 or 3 where 1 is asserted: a model call dispatched for a moment, the
    test's own comment), then passed 9 of 9 reruns on my build, and passed on the base. Not on the flaky list; I read it as
    a load-sensitive race but did not prove that, so the maintainer may want to rerun it a few times.
  - The golden test (`tests_output`) passes with `TZ=America/Phoenix`; the post-suite phases (`protocol types`, benches) were not
    run as such (protocol's own suite, which holds the TS generation, passed).
- Design notes for the owner: a command running beside another job in the same directory may be credited with the
  other's new files; a job launched in one turn and finishing in a later one is not in that turn's union; a parked and
  resumed turn starts new maps (a note needs the whole turn's facts). The note's `left` requires the file to sit in a
  directory a file tool wrote to; the result's `files.left` lists every new command file that still exists.
- Docs that should change at review: spec Part III item and `docs/status.md` (the row), the template comment is in place.
- Disk: this VM's allowance filled twice; I deleted stale `target/debug/deps` executables and `incremental/` (inside the repo only).
