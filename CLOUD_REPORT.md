<!-- CLOUD_REPORT.md: the report of cloud/20261016-chat-task-view. Drop it with its commit at the merge. -->

# chat-task-view (theseus-rrkb7): inspect, join and return in the chat

Branch `cloud/20261016-chat-task-view`, from `main` at 3e1a7ed0. Two steps, two commits, plus this report.

## Step 1: the shared contract (572ecfac)

**Found.** `joinable` and `task_redirect_line` were absent from `crates/theseus-protocol/src/work.rs`.

**Changed.** Added exactly the two functions in the brief, same names, signatures and words. `WorkState` has no `Blocked` variant: "blocked" is `NeedsYou`, so `joinable` is `Running | Queued | Waiting | NeedsYou`. Other rows must add them identically.

**Proved.** `tests_work.rs::a_task_is_joinable_while_it_can_take_input_and_the_redirect_line_is_one_wording` covers each state and the exact line. Planted revert (drop `NeedsYou` from the match): the test failed; restored and touched.

**Left.** Discord still says its own copy of the words ("Written in the thread of the task …", `theseus-discord/src/runtime/threads.rs` ~765). The brief's line says "Written to the task", so Discord's row changes its wording when it moves to the shared function. I did not touch Discord.

## Step 2: the chat, the CLI (7162d4ac)

**Found.** The chat keeps one session. `ctrl-t` printed the tree. A task's own session could be neither read nor written from the chat. `Chat::session` is read by every event handler (`ours()`), by `submit_call`, by the history replay, and by the viewer's `focused`.

**Design.** A shown task is the chat's view, never stored. While it is shown, `Chat::session` is the task's, so history, turns, tool calls, notices' `focused` and the dock all work unchanged. The conversation's own state (session, execution, profile, model, cost, mode, plan, live turn, question, queue, turn ids, replay cursor, last inspect link) moves into a `Stash` inside the view. Its events keep arriving (the watch stays) and are only counted: replies and questions, for the return line.

- `chat/task_view.rs` (new): the view, the stash, open, return, the keys of an inspected task (`j`, `i`, `f` on an empty line), the commands, `/stop` with a confirm, the redirect, the header and the prompt.
- `chat/task_picker.rs` (new): ctrl-t's picker.
- Small edits: `state.rs` (fields, key routing, `Purpose` variants), `events.rs`, `commands.rs` (`/task`, `/join`, `/back`, `/follow`, `/inspect [ID]`; added after `stop` so the `/` menu's first six are unchanged), `dock.rs` (header provider first; prompt; picker takes the input's place), `stop.rs` (Esc in a view goes back, never stops), `tasks.rs` (two hooks), `reopen.rs` (one `pub(super)`), `mod.rs`, `main.rs` (`--task ID`, `--inspect`).
- CLI: `chat --task ID [--inspect]` in `main.rs` and `chat_cmd.rs`. `--plain` maps to `watch <id> --interactive`, and `--inspect --plain` to a non-interactive follow.

**Behaviour, per the brief's table.**
- **Picker.** ↑↓ move, Enter inspects, `j` joins, `c` cancels (second `c` confirms, any other key keeps the task), `a` gives the keys to the task's question, Esc closes. `/tasks` still prints the tree.
- **Inspect** is read-only. A typed line is kept and the screen says why. `j` joins. `i` prints the inspector link: the last ended turn's, or `turn.inspect` for the task's last replayed turn.
- **Join** sends `turn.submit {session_id: <task's>, author: "theseus chat"}` at once. It is never queued while the task's turn runs, and it carries no profile. The prompt reads `1abc1f ›`, and each line prints `↪ the task reads this at its next step`.
- **Return** is Esc on an empty line or `/back`. The conversation is restored and its history read again, then: `── back in main · 1abc1f still running · main: 1 reply while you were away ──`. In a task's view Esc never stops anything. `/stop` asks once, and the second `/stop` sends `task.cancel`.
- **Task ends while joined.** `🏁 1abc1f reported · what you type now goes to main · esc goes back` is told once, and the prompt becomes `main ›`. The next line returns to the conversation and goes to it prefixed by `task_redirect_line(title, short, "the task is over")`. Racing the end: a `turn.submit` refused with `NOT_FOUND` or `data.class` starting `execution_` takes the same path, so the line is never lost.
- **Notices.** `focused` is the task's session while it is shown, so the task on screen rings nothing and the conversation's own questions still do.
- **Cold open.** `chat --task ID` lists sessions, finds the task and its `parent_session_id`, and opens the task without first replaying the conversation. Esc then returns to that parent. A task with no parent says so and quits.

**Proved.**
- `cargo nextest run -p theseus-tui -p theseus -p theseus-protocol`: 1072 passed, 1 skipped (TZ=America/Phoenix).
- New tests, 19 in all:
  - `chat/tests_task_view.rs`: 18 tests plus a header golden, `tests/golden/chat_task_view_100.txt` (inspect, joined, ended).
  - `main.rs`'s `args_tests`.
  - The CLI's `tests/chat.rs::chat_task_opens_a_task_joined_and_inspect_reads_it`.
- Planted reverts (restored and touched each time; `git status` clean after):

| Break | Test that failed |
|---|---|
| `return_main` keeps the task's session | `esc_goes_back_and_the_next_line_goes_to_the_conversation` |
| join sends to `main_session()` | `j_joins_from_the_picker_and_a_line_goes_to_the_tasks_session` |
| Esc with a typed line returns | `esc_in_a_task_never_stops_anything_and_slash_stop_asks_twice` |
| `/stop` cancels at once | the same test |
| an ended joined task still sends to the task | `a_task_that_ends_while_joined_sends_the_next_line_to_main_with_the_shared_line` |
| inspect accepts input | `enter_in_the_picker_inspects_the_task_read_only` |
| a refused line is not redirected | `a_line_that_races_the_end_is_not_lost_it_goes_to_main` |
| `focused` is main | `the_task_on_screen_rings_nothing_and_the_conversations_question_still_does` |
| away replies not counted | `the_return_line_says_what_main_did_while_you_were_away` |
| the picker cancels on the first `c` | `c_in_the_picker_cancels_on_the_second_press_only` |
| cold open takes the task as main | `chat_task_opens_the_task_joined_with_its_conversation_behind_it` |
| `joinable` without `NeedsYou` | step 1's test |

  My first planted revert of "join sends to main" used `self.session`, which is the task's inside a view, so it was not a real revert and the test passed. I redid it with `main_session()`, and the test then failed.
- Not run under load: no timing is involved. These are paused-clock tests, and a nice-19 run with four busy loops was not asked of this row.

**Live check for the maintainer.** I could not run one: it needs a daemon with a running task and a model.
```
# on a scratch daemon of this build (own --config, --socket, --state-dir):
theseus --socket $S ask --session <conv> 'start a task that sleeps then reports'   # or: theseus --socket $S task start 'wait, then report'
theseus --socket $S tasks                                  # note the task's short id, say 1abc1f
theseus --socket $S chat <conv>                            # ctrl-t: the picker lists the task; Enter shows its transcript, header "── ⚙ task … ── j join · i inspector · Esc back to main"
#   j, type a line, Enter: it prints "↪ the task reads this at its next step". With task-join-core merged it is steered at the task's next loop boundary; without it, the daemon queues or refuses it (see below)
#   Esc: "── back in main · … ──" and the conversation's prompt; the next line goes to main
theseus --socket $S chat --task 1abc1f                     # opens joined; add --inspect for read-only; Esc returns to the parent
theseus --socket $S chat --plain --task 1abc1f             # the watch loop on the task's session
```
Let the task report while joined. The prompt should become `main ›`, and the next line should appear in the conversation as `[Written to the task “…” (…1abc1f), and the task is over] …`.

**Uncertain, and for the owner.**
- Joining a running task depends on task-join-core, so on today's `main` the daemon may queue or refuse the line. A refusal is shown as `✗ not sent: …`, and an ended-class refusal is redirected to main. I could not test a real daemon.
- The `turn.submit` answer to a steered line is of an unknown shape. The chat reads it only when it parses as a `TurnSubmitResult`.
- The header's clock is `coarse` minutes (`running 4m`), not `4:12` as in the brief's sample, to keep the screen quiet by construction. The one deadline is `view_repaint_at`.
- Header words, not `🏁 … reported: <first line>`. The existing `✓ <title> reported: <first line>` event line already prints the report, so my `🏁` line only says that the prompt moved. In an inspected task, `f` is `task.retry`, which the daemon accepts only for a failed task, and the refusal is shown. A fuller "linked follow-up" would be a `task.start` row.
- Inspect keys (`j`, `i`, `f`) work only on an empty line in an inspected task, where nothing can be typed. Joined, they would swallow the first letter of a message, so there the commands are `/inspect`, `/follow`, `/back`.
- Not done (outside my files or rows): `theseus open turn --task ID`; `theseus tasks`' `chat --task` hint (it changes the tree goldens); the board TUI's `p` and Backspace. `/inspect ID` here opens the task view rather than printing a link.
- Docs for the maintainer, not edited here: the spec's Part III item, `docs/status.md`, and §3.3's chat row (as built above).
- `crates/theseus-tui/AGENTS.md` and `crates/theseus/AGENTS.md` are updated.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in `door`/`keel`, and nothing else ran.** The keel guard reads the base's history and refuses three old acks ("its signature does not verify (%G? is E)", commits d424da19, 238ecead, 1c2beb27), because this VM has no keys in `scripts/keel-signers`. It names erosions at `crates/theseus-sim/src/lifecycle.rs:155`, `scripts/keel-guard.py:59` and `scripts/test_keel_guard.py:59`. None of those files is in my diff, so this is the environment, not the change. I ran the later phases by hand:

- `cargo fmt --all -- --check`: ok.
- `scripts/shape.sh`: ok.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. This is the `-q -D warnings` form the gate runs.
- `cockpit`: `npm run lint`, `npm test` and `npm run build` ok.
- `cargo nextest run --workspace --no-fail-fast`, TZ=America/Phoenix: 6190 run, 6154 passed, 34 failed, 2 timed out, 47 skipped.
  - 32 are the known L1 failures: `theseus-sandbox` contract and `bench spawn_100`, and `theseusd` sandbox (root, no job cgroup).
  - `theseus-core tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future`: failed under load, and passed alone (1.7 s).
  - 2 timeouts in `theseus-discord tests_task_threads` (`a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it`, `a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`): they hang at 120 s alone too. I stashed my work and the first still hangs on `main` as cloned, so it is not mine. They are not on the flaky list.
  - Registry (`tests_registry`) and the protocol's TypeScript test pass inside the suite. No protocol type changed, so `protocol.gen/` did not change.
- `cargo deny --offline check`: advisories FAILED on `zip` (RUSTSEC-2026-0367, -0368, -0369), bans, licences and sources ok. `Cargo.lock` is unchanged by me and I added no dependency. It is the freshly fetched advisory database.
- Not run: the lifecycle, jobs and turn benches (`THESEUS_GATE_NO_BENCH`, and the VM's clock). Nothing here touches the start path or a turn's frames.

Disk: this VM's writable allowance filled twice during the suite (`target/debug/incremental`, then duplicate test binaries). I deleted only `target/` build artefacts, and ran the suite with `CARGO_INCREMENTAL=0`.
