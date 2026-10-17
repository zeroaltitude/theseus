# CLOUD_REPORT: discord-task-join (theseus-esxyb)

## Step: the owner joins a task from Discord (commit 1f4e2295)

**Found.** `Place::task_message` sent every message for a *running* task to the parent conversation. The DM had no way to reach a task. The redirect line was Discord's own wording.

**Changed.**
- `theseus-protocol/src/work.rs`: `joinable(&WorkView)` (Running, Queued, Waiting, NeedsYou as "blocked") and `task_redirect_line(title, short, why)`, exactly as the contract gives them. The line now reads "Written to the task", not "Written in the thread of the task".
- `runtime/threads.rs`: `task_message` calls a new `Place::join_task`. The owner (`place_rule.owners`) always goes to the task. A guest while it runs, or any message once the task is over (`execution_*`), goes to the parent with the shared line, and the place says so. The pointer now goes to the place's own target (`say_to`), so it also works in a DM. A new thread's first line is `↩ back to <#parent>`.
- `runtime/follow.rs`: the mirror's id is kept (`Follows::note_mirror`, 64 in memory). A reply to it is `Inbound.task` and goes to `Place::task_reply`, then `join_task`. The footer reads `reply here to talk to the task`. A non-reply is still the conversation's.
- `inbound.rs`, `runtime.rs` (+3 lines), `thread_bind.rs` (the dispatch arm, `say_to`), the crate `AGENTS.md`.
- No store format change and no new protocol type, so no TypeScript to regenerate.

**Proof.**
- `cargo nextest run -p theseus-discord -p theseus-protocol`: 608 passed (1 slow), 1 skipped.
- Reader rule: `-p theseus-core tests_registry` 15 passed.
- `cargo fmt --check`, `scripts/shape.sh` and `cargo clippy -p theseus-discord -p theseus-protocol --all-targets -D warnings` are clean.
- New tests: `tests_task_threads::the_owners_message_joins_a_running_task_and_a_guests_is_redirected` (also checks the jump link), `..::the_owners_message_for_an_ended_task_goes_to_the_conversation_with_the_shared_line`, `tests_board::commands::a_reply_to_the_follow_mirror_joins_the_task_and_an_ended_ones_goes_to_the_conversation` (also checks the footer), and `work::tests::a_task_is_joinable_until_it_ends_and_the_redirect_line_has_one_wording`.
- Planted reverts, each restored and `touch`ed:
  - Putting `running` back for the owner (`if running && !owner` to `if running`): the owner test timed out waiting for the message to reach the task.
  - Disabling the mirror-reply dispatch arm: the mirror test failed ("reply reached the task" timed out).

**Live check for the maintainer (needs real Discord).**
- In a private guild channel with `task_threads`, start a task that runs a long command.
- As the owner, write in the task's thread. The task's session should gain the message (`theseus history <task session>`), and the parent should not.
- As a guest, write in the same thread. The parent should gain `[Written to the task “…” (…abc123), and the task is running] …`.
- In the owner's DM, press Follow on a task, then reply to the mirror. The task should read it. After the task ends, reply to the sealed mirror: the DM's conversation should gain the line `…and the task is over]`.

**Uncertain, and for the owner.**
- **Steering a *running* task needs task-join-core, which is not on this base.** The client now sends the owner's message to the task whatever its state. Without core rule 1 (`steered()` accepting `Class::Task`), the kernel queues it behind the running turn. If that turn ends the task, the queued message is refused. My tests therefore prove routing on a waiting or blocked task (the gate task backgrounds its job, and the DM test uses the asking task). I could not assert `TurnSteered` here. That test belongs after the core row merges.
- Once the core row merges, the owner check is the core's. The binding judges the owner by `places.owner`, else the DMs' users, and sends no `DiscordOrigin` on this `turn.submit`.
- The `-# ↪ the task reads this at its next step` acknowledgement was not added for Discord, to avoid a line per message.
- I did not edit `render/task_view.rs` or `task_cmds.rs`: nothing needed it.
- Docs: Part III should record the owner-join rule and the mirror reply. The spec's chapters were not touched.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in the door/keel phase, before any build.** Main's copy of the keel guard flags three "erosions" (`lifecycle.rs` seed, `keel-guard.py`, `test_keel_guard.py`) and refuses several `Keel:` acks because their signatures cannot be verified here (`%G? is E`: no signer keys on this VM). None are in my diff, which touches only the Discord crate, `work.rs`, `tests_work.rs` and `AGENTS.md`. I ran the later phases by hand instead: fmt, shape, clippy on the two crates, the two crates' suites, and the registry. The workspace-wide clippy, deny, cockpit and benches phases were not run. The disk allowance filled twice, so I deleted `target/debug/incremental` (in the repo) and `/tmp` gate dirs.
