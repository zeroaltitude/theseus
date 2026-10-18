# CLOUD_REPORT: chat-start-pause (theseus-ypnxt.4)

Branch `cloud/20261017-chat-start-pause`. Code commit `f3d2ae0f`.

## What changed
- `chat/task_new.rs` (new): `/task new BRIEF`, `make that a task`, `/pause [ID]`, `/resume [ID]`, and the answers. `/task new` with no brief uses the owner's last message (`Chat::last_said`, set at submit and from replayed user nodes of the main conversation only; never the model's reply). No brief and no last message: says so, sends nothing.
- `chat/commands.rs`: `/task` -> `task_command` (`new` is the only new word; `/task` and `/task ID` call `task_cmd` as before), `pause`, `resume`, new `/task` help line.
- `chat/state.rs`: `Purpose::{TaskStart, Hold, PickerTasks}`, `last_said`, and the make-that check in `submit` (before `view_submit`, so it works inside a task's view). `events.rs`, `task_view.rs`: routing of the new purposes.
- `new_task.rs`: `start_params` is the one `task.start` params shape, shared by the TUI's `N` and the chat. `P` (`hold_key`) toggles pause/resume on the selected task row (paused read from the work overlay); `row_key` merges `I`/`P` into one arm in `app.rs` to stay under clippy's 100-line limit. Help overlay has one new line (`ui.rs`).
- `chat/task_picker.rs`: rows are board tasks, then ended tasks from one `task.list {session_id}` read as the picker opens (none per key). Typing searches title and report first line, case blind. `j`/`c`/`a` stay commands until a query starts; `/` starts one; Esc clears the query, then closes. Ended task opens read-only.
- `AGENTS.md` of theseus-tui: one paragraph.

## Tests and plants
`chat/tests_task_new.rs` (20 tests), `tests_new_task.rs` (+1). `cargo nextest run -p theseus-tui`: 343/343 pass. `cargo clippy -p theseus-tui --all-targets` clean; `cargo fmt --check` clean.

| Plant | Test that failed |
|---|---|
| drop `home` from `task.start` | `task_new_starts_a_task_under_this_conversation_with_no_turn`, `make_that_a_task_takes_the_owners_last_message_as_the_brief` |
| make-that matches by prefix | `make_that_a_task_please_is_an_ordinary_message` |
| always pause | `resume_with_an_id_resumes_that_task`, `resume_alone_finds_the_paused_one_among_running_tasks` |
| act on first of two | `with_two_running_tasks_and_no_id_nothing_is_sent_and_both_are_listed` |
| filter title only | `typing_in_the_picker_narrows_it_by_title_and_report` |
| `task.list` per keystroke | `opening_the_picker_reads_task_list_once_not_per_key` |
| TUI `P` always pauses | `shift_p_pauses_a_running_task_and_resumes_a_paused_one` |

Not planted: "send it as a chat message" (the `/task new` test asserts no `turn.submit`) and "take the model's reply as the brief" (the test has the model answer and asserts the brief is the owner's message); I did not run these two reverts.

## Transcript (test rig, picker after typing `quay`)
```
tasks · search: quay
▸ ◐ 1abc1f Survey the north quay · running <1m
  ✓ 3abc3f Rigging check · done <1m          (matched on its report line)
↑↓ move · enter inspect · j join · c cancel · a answer · esc close
```
`/task new Survey the north quay` prints `◐ started Survey the north quay (e4f9a1) · /task e4f9a1 to look, /join e4f9a1 to talk to it`; `/pause` prints `Pausing the task 1abc1f`.

## Budgets
No core, compiler or tool change, so no first-request text moved. The turn bench and owner-config fixture were NOT run.

## Overlaps
`app.rs` (one arm, `P`/`I`), `ui.rs` (one help line), `state.rs` (field, three purposes, a 3-line check). Words: "RPC's words" for pause: `task.pause` returns no text, so the chat words it itself (`Pausing the task X`, or `... is paused already`).

## Proof nothing was lost
All 323 existing theseus-tui tests pass unchanged. `/task`, `/task ID` and the picker's old keys are pinned by existing and new tests. Note: typing `/task` (no space) hits the `/tasks` menu entry, as before; the test types `/task `.

## For the joiner
- **The gate did not pass here.** `scripts/gate.sh` stopped in its door/keel phases: origin/main moved to `9fa84735` since this branch's base `40aa83cd`, so main's keel guard flags 13 erosions that are main's own changes, not this branch's. `python3 scripts/keel-guard.py --base 40aa83cd --head HEAD --quiet` is empty. I ran fmt, clippy and the crate's tests by hand; the rest of the gate (workspace suite, deny, shape, benches) was not run.
- `j` on an ended task from the picker joins is refused; but `j` inside the read-only view of a task the board lacks can still join it (existing behaviour for cold-opened tasks).
- Ended-task rows come from `task.list` filtered client-side (C2); switch to task-history's params after it joins.
- Spec/status not edited: Part III item for ypnxt.4 and the `/help` line should be recorded.
