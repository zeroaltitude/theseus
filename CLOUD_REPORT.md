# CLOUD_REPORT: p3c-steer-keep-going (theseus-ikr75)

Branch `cloud/20261024-p3c-steer-keep-going`, base `ac298666`. One code commit: `c3efebbf`.

## 1. For the maintainer

- Store format: 51, untouched. No config key. `STEER_SHARE`, `STOP_SHARE` and `MIN_LOOPS_WITHOUT_PASS` are unchanged.
- Ceilings: `turn.rs` did not grow (3498 lines; I changed two lines in place). `progress_step.rs` is 584 lines and
  `tests_turn_progress.rs` is 474. `scripts/shape.sh` says ok.
- No OWNER CALL and no STOP. Keel guard is clean (section 7). The DM golden is unchanged (section 8).
- **The gate script could not judge this branch** (section 6). That is not caused by this change.
- Rule (b): spend steers only a turn that has a deadline and whose limit only notifies. `Signals.guarded` is
  `spend_limit_mode = "ask"`. The time share steers and stops as before.
- Rule (c), option 1: until the turn's first answered `fs_write`, `fs_edit` or `fs_patch`, a loop with no command adds
  nothing to `since_pass`. After it, a loop whose reads all answered and opened a key not seen before resets
  `since_pass` and `failing` like a passing command.
- Why option 1: a bug hunt is mostly reads before the first fix, and option 2 would let a failing suite be hidden by
  a later read. A failed command is still a failed check, and a background command is still no check.

## 2. The notes (verbatim)

Steer, a check failing (`failing > 0`):

> [The harness: this turn has used 9 of its 15 minutes, and no check has passed in its last 5 loops (3 failed). This is not a stop: if work is left, keep going. Say in a line what is left, then take the smallest step that can make the failing check pass.]

Steer, nothing failing (checks passed earlier, or none have run):

> [The harness: this turn has used 9 of its 15 minutes, and no check has run in its last 5 loops. This is not a stop: if work is left, keep going. Say in a line what is left, then go on with the next item.]

The stop note is unchanged. The `turn.progress` narration now says "that this is not a stop" in place of "to replan or narrow".

## 3. The rule

`decide(signals, steered)`:
- `since_pass < 3` gives Nothing.
- `time >= 0.85` gives Stop, whatever the mode.
- Otherwise, when not steered yet, it gives Steer if `time >= 0.5`, or if `spend >= 0.5` while `time.is_some() && !guarded`.
- Anything else gives Nothing.

How the mode reaches the rule: `TurnRunner::progress` is now a method (`&self`). It reads
`self.cfg.kernel.spend_limit_mode` each loop and sets `Signals.guarded`. The call site in `turn.rs` changed in place
from `Self::progress` to `self.progress`. I chose a flag over `spend: None` so the `turn.progress` row still reports
the spend. The row gained a `guarded` field.

Reading as progress lives in `State::end_loop`, a pure method the unit test drives. `Commands::note_use` takes the
whole `ToolUse` (via `Checkpoints::answered`, one call site) and records answered edits, read errors, and read keys
(tool, path, pattern). The seen set is on `State` and holds keys only, capped at 4096. One consequence: a grep for a
new pattern counts as new coverage.

## 4. The 10-24 shape

Under "ask", no deadline, the suite passed earlier, spend past half, 3 or more loops of reads since:
- At the base, `decide` gives Steer on spend alone, because `spend >= 0.5` was enough.
- Now it gives Nothing, because the turn is guarded and also has no deadline.
- The same turn under notify with a deadline at half its time still steers, with the new words.
- The same turn under notify with no deadline also gives Nothing.

## 5. Tests and plants

Counts: 24 tests match "progress", all green. Whole `theseus-core` suite under `TZ=America/Phoenix` with nextest:
4498 passed, 25 skipped.

- Table test `the_rule_steers_then_stops_only_without_a_recent_passing_check` keeps its name and every row's intent.
  I rewrote `(None, Some(0.5), 4, 2)` to Nothing and gave it the sibling `(Some(0.1), Some(0.5), 4, 2)` to Steer, and
  added the notify, guarded and no-deadline rows from the brief.
- `the_notes_say_what_was_used_and_what_has_not_passed`: exact equalities for both steer notes, plus "This is not a stop"
  present and "smallest step" absent in the passing case.
- `reading_is_progress_until_there_is_something_to_check`: pre-edit reads add nothing; a failed edit is no edit; a new
  path resets; a re-read does not; a read error beside a new path does not; a grep on a read path does; a failed
  command still fails.
- `a_loops_check_comes_from_its_commands_alone` is unchanged and green.
- Whole-core, in `tests_turn_progress.rs`:
  - `spend_alone_does_not_steer_a_guarded_turn_with_no_deadline`: the 10-24 shape under "ask". It also asserts the
    session spent at least $3 of the $6 limit, so the silence comes from the rule.
  - `the_same_turn_under_notify_with_a_deadline_is_steered_once`
  - `spend_alone_does_not_steer_a_turn_with_no_deadline_under_notify`
  - `a_turn_that_has_not_edited_yet_is_not_steered_for_reading`
  - The `STEER` marker is now "This is not a stop: if work is left, keep going".

Planted reverts (each file restored and touched, `git status` clean afterwards):
- **P1 and P4** (`spend_steers = true`): red were the table test, `spend_alone_does_not_steer_a_guarded_turn_with_no_deadline`
  and `spend_alone_does_not_steer_a_turn_with_no_deadline_under_notify`.
- **P2** (dropped both new `None` arms in `end_loop`): red were `reading_is_progress_until_there_is_something_to_check`
  and `a_turn_that_has_not_edited_yet_is_not_steered_for_reading`. My first version of the unit test copied the logic
  and the plant stayed green, so I moved the logic into `State::end_loop` and the test now drives it.
- **P3** (old wording back): red were `the_notes_say_what_was_used_and_what_has_not_passed`,
  `a_failing_turn_past_half_its_deadline_is_told_to_replan_once` and `the_same_turn_under_notify_with_a_deadline_is_steered_once`.
- "Red at the base": the base does not compile against the new tests (`Signals.guarded` and `note_use` are new), so
  the plants are the proof of behaviour.

## 6. Gate

- `cargo fmt --check`: ok.
- `cargo clippy -p theseus-core --all-targets -- -D warnings`: clean.
- `TZ=America/Phoenix cargo nextest run -p theseus-core`: 4498 passed, 25 skipped.
- `scripts/shape.sh`: ok.
- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in its keel phase, before any build**. The "door"
  phase judges this tree with origin/main's `gate.sh` (`2d1d8d6f`), a newer main than my base. It reports erosions
  that sit between `ac298666` and that main: three removed tests in `crates/theseus/src/persona_cmd.rs`, the keel files, and
  unverifiable-signature acks. None is in my diff. The maintainer should rerun the gate after the join. I did not run
  the later phases of `machine_checks` or the theseusd tests. `grep Replan` finds the old text only in the two files
  I changed.
- None of the known VM failures (L1, goldens clock, `a_filtered_page_equals_the_scans_answer`, `python3s_repl`) appeared
  in the theseus-core run.

## 7. Keel

`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` printed nothing and exited 0.

## 8. DM golden

`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` printed nothing, exit 0.

## 9. Join notes (all additive)

- `crates/theseus-core/AGENTS.md`: one hunk, lines 3008-3017, inside the progress-check paragraph. It replaces the
  "steer note (replan or narrow)" clause and adds the guarded rule and the reading rule. Nothing in "One long
  instruction" or the tools paragraphs is touched.
- `turn.rs`: line 2076 (`self.progress(...)`) and line 3186 (`answered(&uses[r.index], ...)`).
- `turn/checkpoint_step.rs`: `answered` (about lines 89-101), signature and one call.
- `fact/progress.rs`: a `guarded` field in the row (line 37) and the narration (line 60).
- `turn/progress_step.rs`, `tests_turn_progress.rs`: mine alone.

## 10. What the verify rerun should look for

- No `turn.progress` row of decision `steer` before the spend limit under "ask" with no deadline. A deadline-free
  turn should get no steer at all.
- After the limit, the turn goes on, and ends only when the model ends it, or at the budget question. Compare wall
  time, dollars and bugs fixed with the 4 of 13 baseline.
- If a steer does fire (a deadline at half its time), the next message should continue the work rather than run the
  suite and stop.

## 11. Follow-ups

- A finish check or coverage ledger for open-ended asks (a later row). Today a model that decides it is done can
  still end its turn; this change only stops the harness from nudging it to.
- `fs_read` of a new path counts as coverage even if the model never uses it; a hunt that reads everything once never
  steers after its first edit. Say if the owner wants a cap.
- The steer text appears in `docs/status.md` or the spec nowhere I checked; the maintainer should check Part III for
  "Replan or narrow".

## 12. Data floor

Nothing new is sent to a model or routed anywhere; the change is harness-side counting and one note's wording.
