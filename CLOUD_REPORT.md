# CLOUD_REPORT: task-join-core (theseus-slmj1)

Branch `cloud/20261016-task-join-core`, base 3e1a7ed0. One step, one code commit: `4d2263ae`.

## What I found
- Tasks already have an `Steer` open for their turns (`Steers::open` is not class-gated); only `steered()`'s `Class::Person` filter kept the owner out. Plain `turn.submit` carries no Discord origin, so the owner test uses `speaking` (the origin the binding already sends); a Discord call without it is not the owner (queues as today). The Discord row must pass `speaking` to join.
- `load_report` builds the parent's report node from the execution and the store, not from the outbox post, so "carried on the post" does not reach the node.

## What I changed (4d2263ae)
- protocol `work.rs`: `joinable()` (Running|Queued|Waiting|NeedsYou; NeedsYou is "blocked") and `task_redirect_line()`. Wording follows the brief ("Written to the task ...", Discord's old text said "in the thread of"). Discord's `task_message` uses the shared line.
- `EventSink.owner` (+ `from_owner`), set in `turn_submit` from `Core::owner_input` (`rpc/owner_input.rs`: `owner_in_private`, not `opened_from`, not a prompt). Rule 1: `steered()` accepts `Class::Task` when `sink.owner`.
- Rule 2: `turn/join_step.rs`: `owner_waits` (one atomic load via `Steers::any`, only at a task's turn end) and `task_end` (moved out of `finish`, which hit clippy's 100-line limit); the task parks on `Wait{Input}`.
- Rule 4: `Steers` counts the owner's messages per task (steered ones and owner-input turns); the end frame writes META `task.joined.<session>` (no store format change); `load_report` reads it; `Report::node_text` appends `(you joined: N message[s])`.
- Rules 3 and 5 need no kernel change (verified by tests). Docs: `crates/theseus-core/AGENTS.md` steer paragraph, `task.rs` header.

## How I proved it
- New: `tests_task_join` (3), `rpc::owner_input` (1), protocol `a_task_is_joinable...` (1).
  - owner mid-loop -> one `turn.steered`, message after the tool result, 1 task turn, one report, parent woke once and read `(you joined: 1 message)`;
  - last-call message -> task parks, takes it, 2 turns, report = second turn's message, then a further owner message fails `execution_complete`;
  - guest message -> nothing offered, no steer, refused `execution_complete`;
  - owner judgement across CLI/web/job/prompt/MCP/Unnamed/Discord (DM owner, shared channel, guest, no origin).
- Planted reverts: (b) `|| false && owner_waits` -> `a_message_during_the_last_call_parks...` FAILED (`execution_complete`); (a) restore `Class::Person`-only filter -> both owner tests FAILED (message never offered, 20 s timeout). Files restored and touched; `git status` clean of them.
- `cargo nextest run -p theseus-core -p theseus-protocol -p theseus-discord`: 3614 run, 3612 passed, 2 timed out under load (discord `tests_task_threads::a_shared_forum_...` and `a_task_in_a_guild_channel_...`); both pass alone (8/8 in 4.5 s). Existing steer and frame-budget tests pass. `cargo fmt --check`, `scripts/shape.sh`, and `cargo clippy --workspace --all-targets -- -D warnings` pass.

## Gate
`scripts/gate.sh` was not green: the door ran a stale cached copy of an older main (9fa84735c0) and its keel phase listed erosions in files I did not touch (compiler tests moves, lifecycle.rs, keel-guard) plus unverifiable signatures; it stopped there. I ran fmt, shape, clippy and the suites by hand instead. Not run: the cockpit, `cargo deny` and the registry/layout phases outside the suites above (no protocol types changed, so no TypeScript regeneration).

## Live check for the maintainer
1. Scratch daemon with a task running a long job; as the owner `theseus` chat, `turn.submit {session_id: <task>, input: "also check X"}`; `theseus history <task>` should show the message after the next tool result, and a `turn.steered` row.
2. Send a message while the task's final answer streams; the task should end in `Waiting` (not `Complete`), take it, then complete with the second message as its report.
3. The parent's next turn should show `(you joined: N messages)` after the report.
4. Discord: only works if the binding sends `speaking` on `turn.submit` to a task session (not done here; the Discord row).

## Left and uncertain
- The joined count is in memory until the end frame; a restart mid-join loses it (the line is then missing). A cancel path (`cancelled_report`) writes no count.
- The "queued on the hold" case of rule 2 is covered only for offers (steer on, which is the default); with `[kernel] steer_queued = false` a task still reports and the message gets `execution_complete`.
- A message arriving between a task's admission and its steer opening is queued, not offered, tiny window; a client falls back to rule 3.
- `load_report` now does one META point read per report (the brief said no read at report time).
