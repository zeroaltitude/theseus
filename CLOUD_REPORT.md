# CLOUD_REPORT: tail-herdr-cancel (theseus-dtvqa, theseus-xgfps)

Branch `cloud/20261021-tail-herdr-cancel`, from `main` at `6918e3c1`. Store format unchanged (48). No new dependency.
No file went over a line ceiling; `turn.rs` changed by exactly the one `inspect_url` line.

## Steps

1. **Stop cancels a task pane by its session (dtvqa), `7f8ec5f`.** `stop_for` returns `Stop::Cancel(session)`; the
   question's words keep the title. `herdr_slots` is untouched. `git grep 'tokens\["task"\]'` found one other reader,
   `stop`'s question words, which wants the title and keeps it. Tests: `stop_cancels_a_task_pane_by_its_session` (tokens built
   by `herdr_slots::slots` for a task named `[walkthrough] drift`), the old test's fixture (title in `task`, not an id), and
   `a_stop_cancels_the_task_a_pane_shows` in `tests/herdr.rs` (real binary, fake herdr serving `pane.get`, scripted daemon:
   `task.cancel` carries the session id).
2. **Esc closes the answer card (xgfps 2), `bd74932`.** `decide`: a line of only ESC bytes and whitespace is `Quit`; a line
   with an ESC among other characters is `Unknown` (never a decline note). Tab's rule is unchanged. Test `esc_closes_the_answer_card`.
3. **Esc closes the inspect popup (xgfps 1), `eedd38c`.** `view_escape` quits when the chat was started `--task ID --inspect`
   (`cfg.task == Some((_, false))`) and the view is not joined; a join and the picker's inspect keep Esc back to main. Two
   Ctrl-C already left via `interrupt`; the test checks it. Test `esc_ends_an_inspect_popup`.
4. **One prompt per question in a pane (xgfps 3), `3bc610c`.** Cause: `on_note` says the prompt line again whenever an event
   prints below a waiting question (by design, so the terminal ends on the prompt; `pane_answers.rs` holds that for
   `owner_ask`). Any call event under a plain approval therefore printed `approve? [y/N/t/note]` a second time. The earlier
   tests only pushed events that print nothing. Fix: in a herdr pane (`HERDR_ENV=1`) the prompt is not repeated. Test
   `a_pane_says_its_prompt_once_when_events_print_below_the_question` (new, at the end of `tests/herdr.rs`). I could not see
   the live pane, so which event did it there is inferred; the maintainer's live check below covers it.
5. **No overdraw (xgfps 4), `05036cc`.** `Notifier::new` takes its voice (the CLI's status watch passes `eprintln!`); the
   TUI's queues the words and the loop shows the last as its notice line (`Tag::Warn`). Test
   `a_notice_never_writes_to_stderr_under_the_tui`. **The stray `s`: not reproduced, left.** The existing 180x50 vt100 readback
   (`outside_text_draws_cell_for_cell`, with task rows in the sidebar) shows every row as meant; I did not find a cell-level
   cause. Likely the same stderr text landing on the frame, which this fixes; not proved.
6. **`Run null` and the empty head (xgfps 5), `9194555`.** A call whose input this view never saw (a late result's end has
   none) fell to `serde_json::to_string(Null)` = `null`. `summarize` now treats a null input as no argument: `Run a command`,
   `Wait for a job`. I could not tell from the code alone whether the live line was a `proc.run` late end or a `job.wait`; both now
   read right. Test `a_call_with_no_input_is_said_without_an_argument` (protocol) and `a_joined_tasks_job_wait_reads_as_a_wait`
   (TUI; note it sits in the `eedd38c` commit file, so that commit alone fails it until `9194555`). The `resumed · 0 turns ·
   untitled` head: it was the *main* conversation's replay head printed after returning from a task (the view is already taken,
   so `task_head` has nothing). The head is now dropped when `self.back` is set (`back in main` says it). No new test for the head:
   existing back-to-main tests pass; say so if you want one.
7. **Plugin path (xgfps 6), `d2ff7fa`.** `plugin_dir` returns `Option`: first of beside-the-binary checkouts, the build tree, the
   cwd's ancestors that holds `herdr-plugin.toml`. With none, `plugin_lines` says it found none and where it looked. Test
   `plugin_dir_names_only_a_directory_that_holds_the_plugin`.
8. **No inspect link without a web UI (xgfps 7), `318a53f`.** `CockpitConfig::resolve(port, web_enabled)`, `turn_link() ->
   Option<String>`; the one `turn.rs` line uses it. `turn_url` (used by `rpc/inspect.rs`) is unchanged and would still print
   the local base with web off: **`theseus inspect`/`open turn` can still show the dead link**, not widened. Test
   `no_inspect_link_without_a_web_ui`.

## Proof

Planted reverts, all ten tests run together on the planted tree (`0 passed, 10 failed`), then restored and `touch`ed:
stop_for back to `tokens["task"]` -> `stop_cancels_a_task_pane_by_its_session` (the KEY test) and `a_stop_cancels_the_task_a_pane_shows`
failed; ESC rule off -> `esc_closes_the_answer_card`; popup check off -> `esc_ends_an_inspect_popup`; `!in_herdr` dropped ->
`a_pane_says_its_prompt_once_...`; notifier voice `eprintln!` -> `a_notice_never_writes_to_stderr_under_the_tui`; null guard off ->
`a_call_with_no_input_...` and `a_joined_tasks_job_wait_reads_as_a_wait`; `.is_file()` dropped ->
`plugin_dir_names_only_...`; `turn_link` always `Some` -> `no_inspect_link_without_a_web_ui`. Restored files checked with
`git status`/grep. Without the plants the 40 selected tests pass.

Load runs: none (the new tests are not timing tests; the herdr ones wait on events).

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_PROFILE_DEV_DEBUG=0`: the disk allowance was spent
twice by debug builds). fmt, shape, features, clippy `-D warnings`, cockpit, keel passed (0 findings). The suite phase failed
only on: the known L1 tests (theseus-sandbox contract and bench `spawn_100`, theseusd `sandbox` tests) and two load flakes
that pass when rerun alone: `theseus-core judge::people::tests_seen::over_the_gate_exactly_one_extraction_runs` and
`theseus-kernel tree::a_wrapper_that_never_answers_is_killed_with_its_group_and_uncertain` (3.2 s; may be root/VM related).
Not run after the suite: lifecycle/jobs/turn/size benches (no-bench), `cargo deny` (its database was never fetched here,
`--offline` fails). `protocol_types`: `cockpit/src/protocol.gen` unchanged (`git status` clean; no protocol type changed).
Commits were made before the gate ran (it takes about 40 minutes cold here), then the gate ran on the head.

Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): printed nothing, no findings. Keel note:
`stop_is_cancel_for_a_task_and_halt_for_a_conversation`'s fixture changed on purpose (an id in `task` was never what a pane
holds); its other cases are kept.

## Live check for the maintainer

On a scratch daemon and scratch herdr: start a task, focus its tab, prefix+shift+s, `y` Enter: the task is cancelled. prefix+a,
then Esc: the card closes. prefix+i, then Esc: the popup closes, no chat opens. Also: a pane with a waiting approval and a
call event under it shows `approve? [y/N/t/note]` once. `theseus herdr plugin` from an installed binary prints a path that
exists (or says it found none and where it looked). With `[web] enabled = false`, a reply has no `inspect …` line.
In `theseus-tui` under tmux at 180x50 with a WSL-less host, nothing is drawn across the sidebar; check the stray `s`.

## Left / doc changes

Docs to update at review: `crates/theseus/AGENTS.md` (`notify/` says the missing-powershell note is on stderr: now the TUI shows
it on the notice line; `Notifier::new` takes a voice) and the herdr bullet (stop cancels by session; one prompt per question in a
pane). Open: `theseus inspect` link with web off (item 8); the stray `s` (item 5); the late notice line only appears at the next
event (the worker's note waits for the loop's next wake).
