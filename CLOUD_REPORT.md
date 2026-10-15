# CLOUD_REPORT: discord-commands (theseus-a5uds)

Branch `cloud/20261013-discord-commands`. Sonnet 5.5, cloud VM (4 cores, a disk allowance that filled twice).

## Step: the task commands (one step; commits e87257d3 and the one after it)

**Found.** `commands()` was already moved out of `runtime.rs` by discord-board (it is `runtime/slash.rs`), and Stop-asks-twice, the task view and the `wa:` buttons already existed. `task.retry` and `execution.dismiss` judged the caller with no `DiscordOrigin`, so a Discord press could never be judged as the presser. A `Shared.rpc` `confirm.list` gives one `may_answer` per presser, which is what `/answer`'s autocomplete reads.

**Changed.**
- `/tasks [show:]` (ephemeral snapshot, `↻` edits in place), `/task name:` (autocomplete from the task model, needs-you first, at most 25, `new` first; Details with Stop → `[Stop d4e5f6] [Keep it]` → `task.cancel` as the presser, and Follow), `/answer [question:]` (autocomplete over `confirm.list` as the presser, kept 3 s), `/quiet [for:]` (one META key `discord.quiet:<target>`, restored when the place starts), `/cancel` autocomplete, the message command "Make this a task", the board's `➕ New task`, and Retry/Dismiss on the board and in Details (`task.retry`/`execution.dismiss` gain an optional `discord` origin; the CLI passes none; TS regenerated).
- Answers beyond buttons: more than five answers are a string select (`ws:`); "Decline with a note…" and an ask's "Something else" open a modal (`wm:`) with a text input, and for an ask a file upload. A text file's text goes after the note (read as any inbound file; a file that is not text is named as not read). Not stored as an attachment: `work.answer` has none and the task forbids a store change; say if you want a protocol field.
- Follow: own `session.watch` connection, private places only, silent, 3 a place, all of a place's follows edit at most once in 2.4 s, sealed with the report line.
- Interaction callbacks go on `Shared.interact`, a client with no rate limiter.
- Files: `runtime/task_cmds.rs` (named so it does not clash with `slash::commands`; horizon-discord can add to it, or rename it `commands.rs` at the join), `answer_cmd.rs`, `follow.rs`, `start_task.rs`, `render/snapshot.rs`, `render/answer_v2.rs`; sim: `fake_discord/commands.rs` (`slash`, `autocomplete`, `message_command`, `submit_modal`, `pick_in_reply`, and `theseus-sim discord interact` for a live check).
- No store change. No new config key. No new dependency.

**Proved.**
- `cargo nextest run -p theseus-discord`: 368 passed (14 are the new `tests_board/commands.rs` end-to-end tests through a real core and the fake gateway; plus unit tests in `snapshot`, `answer_v2`, `follow`, `start_task`, `task_cmds`). Three full runs in a row stable after I made the failed-task test run a moment first (a task that fails in 0.1 s races the board's watch).
- Autocomplete ranks needs-you first, ≤ 25; five keystrokes make no `work.watch`/`confirm.list`/`work.answer` call (`rpc_client::calls()`); the median keystroke is under 50 ms on the fake. `/answer`'s autocomplete reads the daemon once for four keystrokes.
- Stop needs the second press; Follow mirrors, is silent, refuses a second follow of one task, and seals with `🏁 … cancelled`; a seven-answer ask is answered through its select and the DM card settles; a modal with a note and a file lands in the record; a decline's note goes with the decline; a modal answer from a person who may not is refused to them alone and the question keeps waiting; New task and Make this a task start tasks with the right budget and report, and the quote holds the session (`external::VIA_QUOTE`); `/quiet` silences and restores and keeps one META key; a failed task's Dismiss.
- Planted reverts: autocomplete calling `work.watch` per keystroke, and the `/answer` list's TTL set to 0, each failed `task_autocomplete_ranks…` and `answer_autocomplete…`; `quotes: vec![]` failed `make_this_a_task…`.
- Write count (10 tasks, 200 transitions, 2 follows): the board is edited at most once in 2 s (30 a minute, held by `two_hundred_transitions_edit_the_board_at_most_once_in_two_seconds`), two busy follows together at most 25 a minute (`two_busy_follows_cost_a_place_25_edits_a_minute`, paused clock): worst minute about 55 plus the creates.
- `cargo fmt`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -D warnings`, cockpit lint/test/build: clean.

**The golden and tests I rewrote** (not deleted): `render/golden/board_v2.txt` (the New task button, a failure's row); `card_v2`'s many-choices test (now one select); `task_view`'s stop-button ids (Follow joined); `tests_board`'s stop press (the button is `Stop <short>`); `voice.rs`'s command-list test (21 commands, the new four last).

**Live check.** On this VM I ran the in-process rig only (the above). I did not get a cross-process rig with four tasks running: the disk allowance ended it. `theseus-sim discord interact` (body `{"op":"slash",…}`) is there for it. The owner's check for `#theseus-test` is below.

**Left / uncertain.** The full gate never ran to its end here (below). Docs to change at review: spec Part III item and `docs/status.md`; `crates/theseus-discord/AGENTS.md` is updated. `render.rs` 2915/3009 and `runtime.rs` 3459/3500 lines.

## Keel findings expected
None from my diff. `scripts/keel-guard.py` prints, for the base range, `lifecycle.rs:155: budget-raised "seed" => Some(250.0)` and two refused acks (`238ecead06`, `1c2beb2747`: "signature does not verify", the VM has no key), none in files I touched.

## The gate
`scripts/gate.sh` fails in `keel` (above) and its door. I ran the later phases by hand: fmt, shape, clippy (workspace, `-D warnings`), cockpit: clean. Suite: the whole workspace could not be built here (the disk allowance filled twice: `No space left on device`). I ran `-p theseus-protocol -p theseus-discord -p theseus` and core's `retry` tests instead; their result is at the end of this file. Not run: the workspace suite, `deny`, the benches, `tests/sim.rs` (needs a built `theseusd`).

## The owner's check in `#theseus-test` (real Discord, the owner's keys)

Start a daemon on a fresh state dir with the owner's config and bindings (a DM and `#theseus-test` bound private), this build, `[discord] live_board` on (the default), and `[tools.defer] always_loaded = ["ask"]` so the model may call `owner_ask`. Then, as the owner:

1. In the DM: `START: run these four background tasks, please` with a model that makes four tasks (or start four with `theseus task start`): one asking a question with six options, one that runs long (`sleep 300`), one that writes outside the roots (an approval), one that runs a few minutes.
2. `/tasks` → an ephemeral snapshot of the DM's tasks with `↻ Refresh` and "Open a task…"; `/tasks show:working` lists only the running ones; ↻ redraws it in place; nothing is posted in the channel.
3. `/task name:` and type `lo`: the choices narrow as you type, the task that needs you first, `➕ New task` first of all. Pick the long one: Details shows its doing, calls and `[Stop] [Follow]`.
4. **Follow**: a silent message appears in the DM that mirrors the task's tool lines and words, edited in place (watch: about one edit every 2 to 3 seconds, never a ping); stop the task (`Stop` → `[Stop d4e5f6] [Keep it]` → Stop) and the mirror ends with `🏁 … cancelled`.
5. `/answer` → the asking task's card, ephemeral. Its seven answers (six choices and "Something else") are one select: pick "3." and the needs-you card in the DM settles ("✅ Answered by …").
6. Ask again (rerun the asking task), `/answer`, pick **Something else**: a modal with a text box and a file upload; write words, attach a small `.txt`, submit: the model reads both. On an approval card use `Decline with a note…`.
7. From `#theseus-test` as someone who is not an owner (or with the channel unbound from private), press a `wa:` button or submit an answer modal: only that person is told it did not count; nothing is posted in the channel.
8. `/quiet for:30m` → "🔕 Quiet until …", the board header says so, new needs-you cards are created silent; `/quiet for:off` ends it. Restart the daemon during a quiet: it is still quiet.
9. Right-click a message › Apps › **Make this a task**: the modal shows the message's text as the quote; set a budget and "Silently to the board" and submit. The task starts with `$N`, and `theseus task show` (or the cockpit) shows its session as holding outside text. `/task name:new` and the board's **➕ New task** open the same modal without a quote.
10. A failed task (stop a model key to make one fail): the board row shows `[↻ Retry …abc123] [✓ Dismiss …abc123]`; Details has them too. Dismiss removes it from needs-you everywhere; Retry opens a linked task.

## Test results (final tree, `CARGO_PROFILE_DEV_DEBUG=0`)
`cargo nextest run -p theseus-protocol -p theseus-discord -p theseus`: 1071 passed, 1 skipped, 0 failed. `cargo nextest run -p theseus-core retry`: 20 passed. The rest of the workspace suite was not run (disk allowance).
