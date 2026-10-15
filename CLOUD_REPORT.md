# CLOUD REPORT: chat-ask (theseus-wy6v.1)

Branch `cloud/20261013-chat-ask`. Built on a **debug** build for the unit work; the F10 runs below are on
`scripts/build.sh --profile release-thin` (so S1b and B1 count).

## What I found
- A question's `y` already worked in the chat (`work.answer`); the dock showed no diff, no command, no expiry.
- `execution.stop` kills every dispatched call of the execution, background jobs included. Esc through it alone
  fails S7c (the earlier survey died). **A daemon change was needed** (below).
- `origin/main` in the clone was stale (9fa84735), so the keel guard read 156 commits as mine and failed on an
  unverifiable ack from history. `git fetch origin main` fixed the range (merge-base fcdef5e0). Not a code finding.
- The VM disk allowance (~39 GB) filled twice during a gate; I deleted `target/debug/incremental` and big test
  binaries. Gate runs used `CARGO_INCREMENTAL=0`.

## What I changed
- `9b5ed837` chat: the question in the dock (`chat/ask.rs`, `chat/diff.rs`), `n` note, `v` whole call, Esc/Tab focus,
  `a`/`e` grant keys from the offer (scoped-grants had joined), `action.confirm` author `theseus chat` (AUTHOR now
  `theseus chat` for every chat request), the finished edit's diff and `⎿` result lines kept in the transcript
  (`events.rs`), Esc stop (`stop.rs`), the queue (`queue.rs`, ↑ pulls back, Ctrl-Enter), help keys, F10 arm s3/s4.
- `6395438c` stop: `ExecutionStopParams.keep_jobs` (serde default false); `Kernel::stop_execution_keeping`;
  core `stop_execution_with`; jobs with a short id and the session's terminals are left running. Two struct
  literals touched outside my area: `crates/theseus/src/cmd.rs` and `crates/theseus-discord/src/runtime/halt.rs`
  (`keep_jobs: false`). Cockpit `ExecutionStopParams.ts` regenerated. No store change, no format bump.
  Arm s7 uses Esc.
- (this commit) `crates/theseus-tui/AGENTS.md`, this report.
- Existing tests rewritten, same assertion counts, because the question's words changed ("[y] approve · [n] decline"
  became "y yes · n no…", `work.answer` became `action.confirm`): `chat::tests::a_question_shows_in_the_dock_and_y_answers_it`,
  `y_is_text_when_the_line_is_not_empty`, `a_pasted_y_or_q_is_text_and_a_pasted_newline_sends_nothing`.

## Proof
- `cargo nextest run -p theseus-tui`: 169 passed (new: 19 in `tests_ask.rs`, 3 in `diff.rs`, 1 in `keys.rs`);
  kernel `a_stop_keeping_jobs_leaves_a_background_job_running`.
- Rig tests: hunk numbered and marked at 80 and 120; colour marks as SGR; long line continues behind `↪` with no
  character lost; `y` sends action.confirm (author `theseus chat`), one frame for the question; "yes, but…" after Esc is a
  message (queued while the turn waits, sent when it ends); `n` note; Esc stop + "What should it do instead?";
  Esc with only a background job sends nothing; queue edit and Ctrl-Enter; diff stays after the edit.
- Planted reverts (restored, `touch`ed, `git status` clean):
  1. `y` meets typed text (drop the empty-line and typing guards): failed `y_is_text_when_the_line_is_not_empty`,
     `n_takes_a_note_…`, `a_character_that_is_no_answer_…`, `a_message_that_starts_with_y_is_a_message`.
  2. Esc stopping the background job (drop `!keep.contains(c)`): failed kernel
     `a_stop_keeping_jobs_leaves_a_background_job_running`. (The chat's `keep_jobs: true` is asserted in
     `esc_during_a_turn_stops_it_…`.)
  3. diff dropped after the edit: failed `the_finished_edits_diff_stays_in_the_transcript`.
- Gate (`THESEUS_GATE_NO_BENCH=1`): keel ok, fmt, shape, features, clippy, cockpit, build, reader rule pass. Suite
  fails only on theseus-sandbox contract tests and theseusd `sandbox::*` (the known L1-as-root set). One load flake
  in the first run, `theseus-core tests_work::the_substrate_scenario_…`, passes alone. Protocol types: no diff after
  the run beyond the committed TS. Benches not run (as told).
- Keel guard: `keel: ok (… 22 files changed, 0 findings acked)`. **Keel findings expected: none.**

## F10 (release-thin, stand-in model; main's 11 of 42 is the 2026-10-12 live run)
| Step | main | branch 120x40 | branch 80x24 |
|---|---|---|---|
| S1 | 1 | 4 | 4 |
| S2 | 0 | 3 | 3 |
| S3 | 3 | 4 | 4 |
| S4 | 0 | 4 | 4 |
| S5 | 0 | 2 | 2 |
| S6 | 2 | 3 | 3 |
| S7 | 1 | 3 | 3 |
| S8 | 2 | 3 | 3 |
| S9 | 0 | 0 | 0 |
| S10 | 1 | 1 | 1 |
| S11 | 1 | 1 | 1 |
| S12 | 0 | 0 | 0 |
| **match** | 11 | **28** | **28** |
| beats | 5 | 5 | 5 |
(Main's per-step counts are from the checks named in the task; the other gains come from the chat/term-render/
tool-words/scoped-grants already on main, measured by this run.) No check was changed. Failing and not mine: S2d
(a tool line's `✓ 0 ms` wraps onto the reply), S5c, S6b, S9, S10a/c, S11b-d, S12. `test_f10.py` passes unchanged.

Captures at 80x24 (S4.ask, after, S7.after) are in `/tmp/f10/theseus-80x24/screens/` on the VM; S4.ask shows
`? Edit tides.py · replace 1 line`, `tides.py:50 (+1 −1)`, the numbered hunk with `↪` continuation, "Theseus asks ·
expires in 14:59", the `e` grant offer and `y yes · n no, say what instead · v whole call · esc type`. S4.after
shows `● Edit tides.py  +1 −1` with the hunk under it. S7.after shows `stopped by you (esc)` and "What should it do instead?
(enter: nothing · ↑: edit your last message)" with the earlier survey still listed as running in the background.

## Live check for the maintainer
`python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out /tmp/f10` then
`python3 scripts/f10/scorecard.py /tmp/f10/theseus-*` : expect S3, S4, S7 at full marks. By hand on a scratch daemon in
tmux: ask for an edit, see the hunk; `y`; press Esc mid-turn: "stopped by you (esc)" and a background `python3 slow_survey.py` still
running (`pgrep -f slow_survey`).

## Left / uncertain / for the owner
- **Stop verified words**: with no running foreground call there is no verdict, so the line is just "stopped by you
  (esc)"; with a call it adds "verified: N processes ended" (tested with a fake verdict, B3 shows the real one).
- **Queue**: held in the client until the turn ends (a client crash loses it). When steer-queued joins, only
  `release_queue` changes. Ctrl-Enter works only where the terminal reports modifiers (the kitty set the chat already asks for).
- A message typed while a question waits is queued behind that turn (the turn cannot end until answered). Revisit if wrong.
- Doc changes for the maintainer: spec Part III item (execution.stop `keep_jobs`), docs/status.md.
- "Who asks": `ConfirmRequest.by` is always "operator" (who is asked), so the dock says "Theseus asks" or the task's title.
