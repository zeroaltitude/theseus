# CLOUD_REPORT: chat-board-gaps (theseus-xock5)

Branch `cloud/20261016-chat-board-gaps`, from main 3e1a7ed0 (store format 45; no format change here).
Started 06:47 UTC, 2026-10-17. Four gaps from the row, in two commits.

## Step 1: cost on footer rows, horizon kinds in `/tasks` (82973b73)

**Found.** `task_tree::footer` drew `mark id title elapsed activity`, with no spend. `task_tree::tree` kept only roots of kind
Conversation or Task (the filter at `task_tree.rs` ~355 in the row), so a routine, a watch or a project on the board never
showed in `/tasks` or `theseus tasks`.

**Changed.** `crates/theseus/src/render/task_tree.rs`:
- `Cells` gains a `cost` cell and `Layout` a `cost_w`. The column is present only when some shown row has spent something,
  so `tree` (which passes `with_cost: false`) and every existing golden are byte-identical. The footer passes `true`.
  Money is `theseus_protocol::usd` (`$1.20`, `$0.0042`); a task with no spend gets a blank cell.
- `listed(kind)` (Task, Routine, Watch, Project) replaces the Conversation/Task-only filter, for roots and for children of a
  conversation. `ahead()` words them: `routine · next Fri 09:00`, `routine · paused`, `watch · <label>`, `project · <label>`.
  Their rows do not enter `Counts` (the footer's counts stay the tasks').
- Deferred tasks were already Task rows (their label carries the time); I did not change them.
- Docs: `crates/theseus/AGENTS.md`, `crates/theseus-tui/AGENTS.md` (the "Gaps closed" paragraph).

**Proof.**
- `cargo nextest run -p theseus -p theseus-tui` (TZ=America/Phoenix): 900 run, 900 passed. The three tree goldens did not move.
- New tests: `render::task_tree::tests::the_footer_shows_what_a_running_task_has_spent_and_the_tree_does_not` (golden
  `tests/golden_tree/footer_cost_100.txt`), `a_routine_a_watch_and_a_project_are_rows_but_no_task`, and in the chat
  `chat::tests_tasks::a_footer_row_says_what_its_task_has_spent`, `tasks_lists_a_routine_a_watch_and_a_project_beside_the_tasks`.
- Planted reverts (one build, all planted together, each failed the tests named for it, then restored, `touch`ed, `git status` clean):
  - footer `with_cost` set to false: the footer golden test and `a_footer_row_says_what_its_task_has_spent` failed.
  - root filter back to `matches!(Conversation | Task)`: `a_routine_a_watch_and_a_project_are_rows_but_no_task` and
    `tasks_lists_a_routine_a_watch_and_a_project_beside_the_tasks` failed.

## Step 2: the chat reads the shared seen file: welcome digest, `seen`, `away_ms` (8718c78f)

**Found.** `chat/tasks.rs` built `Viewer { seen: 0, away_ms: 0 }`, so a question shown elsewhere rang again and a chat opened
after an absence told nothing. The chat had no seen-file wiring and no terminal focus events.

**Changed.** New `crates/theseus-tui/src/chat/welcome.rs`; small hooks in `state.rs`, `tasks.rs`, `events.rs`, `run.rs`, `mod.rs`.
- `Cfg::seen_path` (`Seen::default_path()` in `mod.rs`; `None` in tests).
- Opening: if the file's look has a position and is at least 60 s old (`AWAY_MS`, the board's number), the connect also sends
  `work.digest {since}`. The reply becomes one line under the banner (`◆ 2 reported, 1 answered by you since 14:02 · theseus
  digest`; held until the banner has printed), and the look moves to the digest's end. Reconnect: after a loss of 60 s or more
  the chat asks since the last board position it applied (`Board::position`, kept from the snapshot and each `work.changed`).
  An older daemon's refusal says nothing (`Purpose::Welcome` is silent on error). No file look (first start): nothing is
  asked, and the first board snapshot is recorded as the look.
- `Effect::Look(path, position, at_ms)` writes the look through `seen::record_looked` on the blocking pool; `finish()` writes
  the final look on leaving.
- `Chat::viewer(&Notice)`: `seen` = the file's displayed position for the work's execution (a Question view maps to its parent
  task), re-read per notice so another client's writes count; `away_ms` = time since the later of the chat's last key or paste
  and the file's last look, zero under 60 s.

**Design choices the owner should hear about.**
- The chat only reads displayed marks; it does not write them. If it wrote one when it told a task's question, the daemon's
  `work.notice` for the same position would then count as seen and the chat's own bell would never ring. A question counts as
  seen when the board TUI, `theseus history/watch/confirm` showed it.
- The chat has no focus events (guard.rs enables raw mode and paste only), so "away" is the quiet of the keyboard combined with
  the machine's last look, not terminal focus. Turning on focus reporting would touch `guard.rs`/`term` bytes and the pty tests;
  I left it.
- The digest is machine-wide (`since` only), like the board's, not scoped to the conversation.

**Proof.** Same 900-test run. New file `chat/tests_welcome.rs`:
`opening_after_an_absence_tells_what_happened_in_one_line` (request `{since: 500}`, line under the banner, file look moves to 900),
`a_first_start_is_told_nothing_and_its_board_is_the_first_look`, `a_look_a_moment_ago_is_no_absence`,
`a_connection_back_after_a_long_loss_asks_since_the_board_it_knew` (15 s: nothing; 2 min: `{since: 640}`),
`a_question_already_shown_elsewhere_does_not_ring_twice` (displayed 30: position 21 silent, 31 rings once),
`the_operator_away_hears_the_conversations_own_question` (present: silent; 15 min without a key: rings).
Planted reverts (restored and `touch`ed): `seen: 0` in the viewer failed `a_question_already_shown_elsewhere_does_not_ring_twice`;
`away_ms: 0` failed `the_operator_away_hears_the_conversations_own_question`; dropping `welcome_call()` from the connect failed
`opening_after_an_absence_tells_what_happened_in_one_line`; dropping `reconnect_call` failed
`a_connection_back_after_a_long_loss_asks_since_the_board_it_knew`. No run under load: none of these is a race.

## Live check for the maintainer (not run here: no daemon with real state)

On a scratch daemon (own `--socket`/`--state-dir`, `XDG_STATE_HOME` pointing at a scratch dir):
1. `theseus task start "…" --title Probe` then `theseus chat`; the footer row for the running task should end `… <time> $0.xx  model …`
   once it has spent anything.
2. `theseus routine add "every 1d" "…"` (or any watch/project), then `/tasks` in the chat and `theseus tasks`: a `routine · next …` row.
3. Write a look into `$XDG_STATE_HOME/theseus/seen.json` with `looked: {position: <older board position>, at_ms: <an hour ago>}`
   (or run `theseus digest` once, finish a task, wait a minute), then open `theseus chat`: one `◆ … since HH:MM · theseus digest`
   line under the banner, and `looked` in the file moves forward. A fresh `XDG_STATE_HOME` prints no such line.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in the **keel** phase (and its "door" re-run) with 3 erosions
in the range from an old base to the tree: `crates/theseus-sim/src/lifecycle.rs:155`, `scripts/keel-guard.py:59`,
`scripts/test_keel_guard.py:59`, and three `Keel:` acks "whose signature does not verify (%G? is E)". None of those files is in
my diff; this VM has no keyring for `scripts/keel-signers`, so the acks on main's own commits cannot verify. That is not in the
known-failures list, so I ran the later phases by hand instead:
- fmt: `cargo fmt --all -- --check` clean. shape: `scripts/shape.sh` ok. clippy: `cargo clippy --workspace --all-targets -q -- -D warnings` clean.
- reader rule: `tests_registry` 15/15. protocol types: no protocol type touched, `cockpit/src/protocol.gen` unchanged.
- tests: `-p theseus -p theseus-tui` 900/900. I did not run the whole workspace suite (the known L1 failures would hide it); the only
  other users of `task_tree` are `task_lines.rs` and `tasks_tree.rs`, in `theseus`, whose tests are in that 900.
- cockpit phase: not run (nothing there changed). deny: `cargo deny --offline` cannot run (it wants to download crates); no dependency
  and no lockfile change in this branch, so nothing to check. Benches: not run (`THESEUS_GATE_NO_BENCH=1`).
- Disk: the 15 GB allowance filled once from `target/`; I cleared `target/debug/incremental` and used `CARGO_INCREMENTAL=0` after.

## Left / uncertain

- Deferred tasks in `/tasks` show as before; the plan's "deferred" horizon kind needed no code, and I could not confirm
  from the wire whether the chat's `work.watch {roots: [sid]}` returns a routine/watch/project that is not under this conversation.
  The tree draws any such view the board holds (a root, or a child of a conversation).
- Cost column uses `Cost.spent_usd` (spend since the last reset), as `theseus work` does for forecasts.
