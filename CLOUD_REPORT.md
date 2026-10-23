# CLOUD_REPORT: tail7-threads (theseus-7zs0f, theseus-bibny)

Branch `cloud/20261021-tail7-threads`, based on `ae420a49`. Store format 49, untouched (nothing here is stored).
No line ceiling was touched; no dependency added; no protocol change.

## 1. The steered held-compile test (theseus-7zs0f item 1), commit 405a92f4

**Found.** The rig can stage a steered turn after all. `correction_point` takes a session's pending steer
(`corrections.set_pending(sid, Steering)`) when the turn "asked" the route pack, and the rig's Jev scripts a sure `chat`
mode, so the turn asks. `compile_first` then does `t.route.steer.take()`, the compile overflows, and `held` passes
the steering to `record_route`.

**Changed.**
- New `a_held_compile_failure_records_its_steer` in `tests_hold_compile.rs`: a pending `Steering { Steer::Mode("chat"),
  follows: "quick", By::Owner }` is set on the session before the overflowing run; the single `route.decided` row must
  carry `source == "correction"` and `follows == "quick"`. No seam in `route_step.rs` was needed.
- The old test is renamed `a_held_compile_failure_records_its_wait` (its asserts are unchanged, only the name;
  it never checked steering). Its doc comment is still right.

**Proof.**
- `TZ=America/Phoenix cargo nextest run -p theseus-core hold_compile`: 5 passed.
- Plant: `held` passed `None` instead of `steering` to `record_route` (`route_step.rs:701`). Result:
  `a_held_compile_failure_records_its_steer` FAILED at `tests_hold_compile.rs:229`,
  `left: (None, None) right: (Some("correction"), Some("quick"))`. The other four stayed green. File restored and
  touched, `git status` clean.
- `cargo nextest run -p theseus-core --no-fail-fast`: 4124 passed, 25 skipped, 0 failed.

## 2. The sidebar's confirm list (item 2), commit 405a92f4

**What the list is for.** It is the whole daemon's. Evidence: `crates/theseus-tui/AGENTS.md` ("every session of a running
`theseusd` in one sidebar... the queue of what needs you, answered inline"); `Purpose::Questions` →
`confirm.list` takes `Value::Null` (no session scope); `Board::confirm_requested` keys questions by `session_id`
for every view on the board, and `Board::questions(sid)` returns them per session. It has no notion of "this
terminal's chat", unlike the chat's `ours_in`, which exists to keep another surface's turn events out of one
conversation's transcript. A placed-thread filter here would hide a question the owner must answer.

**Changed: no code.** New `tests_questions.rs`: `a_question_of_another_turn_in_a_placed_session_still_shows`
(snapshot of the harbour board, then a `confirm.list` answer carrying a question with
`turn_id = Some("turn_from_discord")` for `ses_dm0001`; `board.questions` must list it).
Plant: `confirm_requested` returns early when `c.turn_id.is_some()`. The test FAILED, the rest of the TUI suite passed;
board.rs restored and touched.

## 3. One ended-state list (bibny), commit 8a233d68

`pub fn ended_state(state: &str) -> bool` in `crates/theseus-tui/src/board.rs` (the module both call sites already
reach), used by `app.rs` (the cancel check's fallback to the execution's state) and `chat/task_picker.rs` (`ended`).
`WorkState::ended()` in theseus-protocol is a different type with different words (`Done`, `Failed`, `Cancelled`,
`Expired`; no budget exhaustion) and the two cannot call each other without widening one; both are left as they were.

Tests, one per site plus the list:
- `board::tests_ended::the_ended_states_are_the_four_and_no_other`;
- `tests_cancel::c_on_a_budget_exhausted_task_missing_from_the_work_view_says_finished` (the app.rs site);
- `chat::tests_task_new::the_picker_counts_a_task_that_ran_out_of_budget_as_ended` (the picker site).

Plant: dropped `budget_exhausted` from `ended_state`. With `--no-fail-fast` all three FAILED (372 other tests passed);
restored and touched. `cargo nextest run -p theseus-tui`: 375 passed.

## 4. The AGENTS.md wrap

In `crates/theseus-core/AGENTS.md`, the thread router paragraph's over-long line is broken after
`` `rpc/thread_flow.rs` `` and re-wrapped to the paragraph's width. No word changed; only that line moved.

## Checks

- `cargo fmt --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Keel guard, `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: **empty output, exit 0**.
- Gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran door, keel, fmt, shape, features, clippy,
  clippy-probe, cockpit, test build and the reader rule green. The suite: 7964 run, 7930 passed, 34 failed. All 34 are the
  known L1 failures of this root VM (theseus-sandbox `contract` and `bench::spawn_100`, theseusd `sandbox` tests,
  including `a_host_beyond_the_list_once_approved_is_outside_text`, which is a sandbox test too); no other test failed.
  I then ran what follows the suite by hand: the generated-TypeScript check (the suite's protocol test left the tree
  clean) and `cargo deny --offline check` (advisories, bans, licenses, sources ok). The benches are skipped by
  `THESEUS_GATE_NO_BENCH`. The first gate run died with "No space left on device" at the test build (the
  VM's disk allowance); I deleted test binaries and incremental caches under `target/` and reran.
- The herdr timing watch item passed. `python3s_repl_computes_on_the_screen` and `a_filtered_page_equals_the_scans_answer` did not fail.
- Commits: this VM's `git commit -S` produced commits that show as unsigned (`%G?` N: no signing key available here), so
  the maintainer's merge commit carries the signature.

## Live check for the maintainer

No behaviour of the daemon changed; the TUI change is a refactor with tests. To look:
`theseus tui` against a scratch daemon, with a task whose budget is exhausted: `c` on it says
"that task has already finished", and ctrl-t in `theseus chat` lists it among the ended tasks.

## For docs/status.md

"tail7-threads: a steered held-compile route row is tested (source `correction`, `follows`); the TUI sidebar's question
list is pinned as the whole daemon's, not filtered by placed thread; the ended-task states are one helper
(`board::ended_state`)."

## Left / uncertain

- 7zs0f item 3 (thread_grow before append) left alone, as the brief said.
- If the chat should one day hide a placed session's questions of other turns, that is the chat's `ours_in`, not the sidebar's.
