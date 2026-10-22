# CLOUD_REPORT: tail4-tui

Branch `cloud/20261021-tail4-tui`, from `main` at `8260dbab`. Store format unchanged (49): no stored field added.
Commits are unsigned (the VM has no signing key). No file went over its line ceiling; nothing was added to
`crates/theseus/src/render.rs`, `protocol/lib.rs` or any long file.

## theseus-v54l9: herdr up focuses the workspace (602ba4a)

- **Found:** `up` made the tabs and never touched focus. herdr 0.9.1's schema has `workspace.focus` with params
  `{workspace_id}` (`WorkspaceTarget`). It has no "is a client attached" call, but `workspace.list` gives each workspace
  `number` and `focused`.
- **Changed:** `herdr_up.rs` `show_workspace` runs after the base tabs (`--follow` from a shell) and after a one-shot `up`.
  It lists, returns if `theseus` is already in front, otherwise calls `workspace.focus` (`wire::focus_params`) and lists
  again. In front: `herdr: switched to workspace <n> (theseus)`. Not in front: `herdr: workspace <n> (theseus) is ready,
  but no herdr client shows it: run `herdr` to attach`. `--follow` also prints `the board tab is the follower, so
  `up --follow` returns now`. New flag `--no-focus`. The startup hook (`--boot`) and the `--here` follower never focus.
  A second `up` finds it in front and sends only reads, so `a_second_up_sends_only_reads` still holds.
  `docs/herdr.md`: one sentence.
- **Tests:** `herdr_up_focuses_the_theseus_workspace_and_says_so` (tests/herdr_up.rs: the fake herdr gained
  `workspace.focus`, a `focused` flag and a `detached` mode; covers the switch line, the no-client line, a second up,
  `--no-focus`, `--boot`); the contract test (`the_workspace_and_pane_builders_match`) checks `focus_params` against the
  schema; `every_method_the_code_names_is_in_herdrs_schema` passes.
  **Plant:** the `workspace.focus` call removed: the test failed at tests/herdr_up.rs:887 (`left == right`: no focus
  request was recorded). Restored and touched.
- **Live check:** inside a herdr with the `theseus` workspace not in front, from another workspace:
  `theseus herdr up --follow` should print `herdr: switched to workspace N (theseus)` and herdr should show it; from a
  plain terminal with no client attached it should print the attach line; `--no-focus` prints neither.
- **Uncertain:** "client attached" is inferred from `focused` after the call, since herdr's schema has no direct call.

## theseus-tlhjt: the board's `c` (f756b67)

- **Found:** `arm` always took `on_screen()` (the open session) and never looked at whether it had ended.
- **Changed:** `app.rs` `arm`: for `Arm::Cancel`, with the list on screen (`wide()`) the target is the row under the cursor
  (else the open session); a task whose work view `state.ended()` flashes `that task has already finished` and arms
  nothing. `s` and `S` are unchanged (one-line change if wanted). On a narrow screen showing only the pane, the open
  task is still the target.
- **Tests** (`tests_cancel.rs`): `c_on_a_finished_task_says_so_and_cancels_nothing`,
  `c_takes_the_cursors_row_while_another_task_is_open` (open Fetch, Esc, `k`, `c`: prompt names Count the boats, `c c`
  sends `task.cancel` for `ses_boats01`). **Plants:** the ended check disabled: the first test failed
  (tests_cancel.rs:23); `Arm::Cancel if false` (open task always): the second failed (tests_cancel.rs:41).
  `tests_work.rs` helpers `regatta_world` and `rig` became `pub(crate)`.

## theseus-hwv3t: the chat footer's job line (d0590ec)

- **Which hypothesis held: the first.** `push/last_line.rs` and `WorkBoard::sample` (push/work.rs) document that a sample
  "keeps its position and takes the sample's time, so a client applies a view by (position, time)". The CLI's
  `tasks --watch` does that; `chat/tasks.rs` `work_changed` compared `position` only, so every sample was dropped.
- **Changed:** `work_changed` compares `(position, at_ms)`. No daemon change, so positions stay monotone per view.
- **Test:** `the_footer_follows_a_running_jobs_last_line` (chat/tests_tasks.rs): a job view with the same position and a
  later `at_ms` moves the footer from `census step 2/9` to `census step 7/9`. **Plant:** the position-only rule back: the
  test failed (chat/tests.rs:188, the screen never showed step 7/9).
- **Follow-up:** the full-screen TUI's `Work::apply` (`theseus-tui/src/work.rs`) is also position-only. The owner saw the
  follow pane live, so I did not touch it; its sidebar job rows may freeze the same way.

## theseus-dowi0.1 item 2: answer card order (e5233fa)

- `herdr_answer.rs` `others_after(qs, shown)`: the other questions in `queue` order starting after the shown one;
  `context` lists them so, and `Decision::Next` takes the first. Test `the_listed_waiting_order_is_the_tab_order`.
  **Plant:** raw order: failed (herdr_answer_tests.rs:214). My first plant did not apply (rustfmt had reflowed the
  signature) and the test passed; I redid it and it failed as above.

## Gate and checks

- `cargo nextest run -p theseus -p theseus-tui` (TZ=America/Phoenix): **1094 passed, 0 failed.**
- `cargo fmt --check`: clean. `cargo clippy -p theseus -p theseus-tui --all-targets -- -D warnings`: clean.
- `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: **empty output, exit 0.**
- Full `scripts/gate.sh` (`THESEUS_GATE_NO_BENCH=1`): door, keel, keel tests, fmt, shape, features, **workspace clippy,
  clippy-probe and cockpit passed**; it **failed in "test build" with ENOSPC**. This VM's disk allowance (about 29 GB)
  cannot hold a workspace debug build with tests. So the suite phase and `machine_checks` after it did not run for the
  other crates; nothing I changed is in them. I ran with `CARGO_INCREMENTAL=0` after the first overflow. The maintainer
  should run the full gate on the owner's machine. `cargo deny fetch` was not run.

## For the integrator

`docs/status.md`: "`theseus herdr up` switches herdr to the `theseus` workspace (`--no-focus` to skip) and says which, or how
to attach; the TUI board's `c` refuses an ended task and takes the cursor's row; the chat footer follows a running job's
last line (views apply by position, then time); the answer card lists waiting questions in tab order."
`crates/theseus/AGENTS.md`'s `herdr_up` entry could name `--no-focus` and `show_workspace`.
