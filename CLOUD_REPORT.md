# CLOUD_REPORT: tui-task-return (theseus-mu0e6)

## Step 1: the TUI's way back from a task (commit cb9c57b1)

**Found.** A task is a session, so the board already opened it (Enter) and `i` already typed to it. Missing: a way back to the
parent, the inspector link, words for the steer, and handling of a message for a task that has ended. `p` was already the standing
records' key; `joinable` and `task_redirect_line` were absent from `proto/work.rs`.

**Changed.**
- `theseus-protocol/src/work.rs`: `joinable(&WorkView)` and `task_redirect_line(title, short, why)`, as in the shared contract.
  WorkView has no `Blocked` state: a blocked task is `NeedsYou`, so `joinable` matches Running, Queued, Waiting, NeedsYou.
- `theseus-tui/src/task_return.rs` (new): breadcrumb (`task a1b2c3 of <parent> · p parent · I inspector`, replacing `ses …id` for tasks);
  `p`/Backspace focus the parent (in a conversation `p` still opens the standing records, Backspace does nothing); `I` sends
  `turn.inspect` for the pane's newest turn and prints the URL; the input prompt says `task a1b2c3 reads it at its next step>`, and a sent
  line adds `↪ the task reads this at its next step`; on an ended task `i` prompts `task a1b2c3 reported; to main>` and Enter sends to
  the parent with the redirect line and follows it; a `turn.submit` to a task refused as ended (NOT_FOUND or `execution_*` class,
  as Discord reads it) is re-sent to main ("the task ended first").
- `detail.rs`: `Detail::turn`, the newest turn from history nodes and events. `app.rs`: `Purpose::SubmitTask`, `Purpose::Inspect`,
  `request_failed`. `ui.rs`: header, prompt, help. Two older tests (`tests.rs`, `tests_names.rs`) waited for the header text
  `ses …tide01`; they now wait for `task tide01 of DM`. No budget, ceiling or assertion about behaviour was loosened.
- `AGENTS.md` of the crate updated.

**Proved.**
- `cargo nextest run -p theseus-tui -p theseus-protocol` (TZ=America/Phoenix): 457 run, 457 passed, 1 skipped. New: 7 in
  `tests_task_return.rs`, 1 in `tests_work_join.rs`.
- Planted reverts (each restored, `touch`ed, `git status` clean): `p` focuses nothing → `p_and_backspace_return_to_the_parent` and
  `a_task_pane_names_its_parent_and_the_way_back` fail; `ended_task` always None → `i_on_an_ended_task_sends_to_main` fails;
  `task_is_over` always false → `a_message_that_loses_the_race_goes_to_main` fails.
- `cargo fmt --all --check` ok; `cargo clippy -p theseus-tui -p theseus-protocol --all-targets -- -D warnings` clean.
- No protocol types changed (functions only), so `cockpit/src/protocol.gen/` is unchanged.

**Live check (maintainer, needs a scratch daemon with a running task).** Run `theseus-tui` in tmux at 80x24 and 160x48 on the scratch
socket; open a task row with Enter: the header reads `task xxxxxx of <parent> · p parent · I inspector`. `I`: a footer/pane line
`inspect <url>`. `p` or Backspace: the parent's pane, footer `back in <parent> · task xxxxxx running`. `i`, type, Enter: a `↪ the task
reads this at its next step` line (steers fully only with row task-join-core). Let the task end, `i`: prompt `…reported; to main>`, and the
message lands in the parent with `[Written to the task “…” (…xxxxxx), and the task has reported] `.

**Uncertain / for the owner.** `p` loses its standing-records meaning inside a task pane (it is still `?`-listed); the standing list is
rarely wanted there. `I` shows the link only; the TUI has no way to open a browser. The steer words are a client's promise: until
task-join-core lands, the daemon queues rather than steers. Docs to change at review: spec Part III item, `docs/status.md`.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` FAILED in its door/keel phase before any build, not on this change: the keel
guard refuses three Keel acks in the base history (`d424da19e4`, `238ecead06`, `1c2beb2747`) because their signatures do not
verify here (`%G? is E`: this VM lacks the signers' keys), then lists the erosions those commits carry (lifecycle.rs seed,
keel-guard.py, test_keel_guard.py). None is in my diff. I ran the checks by hand instead (fmt, clippy, tests above); the full suite,
cockpit build and deny phases were not run. The disk filled once during clippy; I cleared `target/debug/incremental` and went on.
