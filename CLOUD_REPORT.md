# CLOUD_REPORT: thread-discord, T6 (theseus-sjzm)

Branch `cloud/20261013-thread-discord`. Base: the branch's `cloud task` commit on main 0baeed6 (store format 33, unchanged: no stored field was added; `ThreadInfo.surfaces` already existed).

## What I found
- T1 (thread-record) had joined; T2, T3, T4 and the persona rows had not. So the reply lookup, the tags and the explicit address are mine, behind one function T4's joiner can call: `Shared::thread_of_reply(message) -> Option<slug>` (runtime/thread_bind.rs; `Replies` maps message to turn to thread).
- An outbox target is one per session, and a thread's session is virtual (no target). So a bound place cannot be a plain "second session of the place": it takes the thread's session and its posts in memory (`Outbox::place_bound`), leaving the place's own session unsuperseded, which is what makes `unbind` possible.
- A core judge on a Discord thread that is not a place classes it shared; the binding judges such a press from the parent channel (which it has checked is private).
- Disk: this VM ran out of space once; I deleted stale workspace rlibs and use `CARGO_INCREMENTAL=0`.

## What I changed (commits 4e8a2ef, 1e31efb, 89a1748, d831232)
1. **Bind a channel as a thread** (`/thread bind slug:`, `/thread unbind`, `[[discord.thread]]`): a private channel the bindings file names, or a Discord thread under one (or under `discord_channel`). A shared one refuses. The thread's `surfaces` records it (`thread.edit { surfaces }`); one Discord surface per thread. A bound place's messages run in the thread's session; `/new` there explains instead of retiring it.
2. **DM tags**: a thread's reply in the DM opens `-# → <title>`; none in a bound place.
3. **Reply addressing + DM turns**: `thread:<slug> text` (provisional until T4's explicit form) or a reply to a tagged message runs the turn in the thread and is answered in the DM (`turn.submit { thread, speaking { discord, answer_at } }`; no live stream in the DM, the post arrives at the turn's end). A paused thread addressed this way says how to resume.
4. **DM pointer**: `-# from the DM: <80 chars> · <jump link>` as a quiet notice (body `quiet`, never pings) in the thread's place; kept until the thread has a surface.
5. **One Discord thread per activity thread** (`[threads] discord_channel`): made on `thread.changed { opened }` (text channel: public thread; forum: post with first message), run as a dynamic private place, recorded in surfaces; pause/done archive, resume/reopen unarchive, an edit renames, 🔒 first when sealed. Helper in `runtime/guild_thread.rs` (the one for other rows to use). Tasks' board and cards go there because the thread's session targets it.
6. **Persona**: a text part in a place that runs a thread with a persona is sent through a channel webhook under the persona's name (found or made once per lane), else `**Name**: ` first; a DM's is the bold name under the tag. A reply to a webhook's message counts as a reply to the bot (`Replies`), also in mention-only places (a defect fixed in code I was in).
Core/protocol: `thread.changed` notification (all thread facts + `ThreadOpened`), `ThreadEditParams.surfaces`, `Core::thread_surfaces`, `Core::thread_place_session`, `TurnSubmitParams.speaking`, `TurnRequest.answer_at` (read first by `stage_reply`), `PlaceRule::bind_more`, `Outbox::place_unbound`, config keys with template lines and tests; `stage_reply`/`unread_relays` moved to `turn/reply_step.rs` (turn.rs was 9 lines over its ceiling; it is now 3436). Stand-in: threads, forum, webhooks, `say_reply`, `command`.
Defect I introduced and fixed: the syncer task held the binding past a stop (`stop_checkpoint` caught it); it now ends at the stop.

## Proof
- New tests: `tests_gateway/threads.rs` 12 (all pass), core `rpc/tests_thread_surface.rs` 4, config 2, unit tests in thread_bind/guild_thread.
- Planted reverts (each failed its test, file restored and touched): shared channel bound via config (`the_config_binds_a_channel_when_its_thread_exists`; the interaction path is also refused by the core's judge, so `may_run_threads` is defense in depth); core `thread_place_session` private check (`a_place_runs_as_a_thread...`); `answer_at` ignored and its check removed (`a_threads_reply_is_posted_where_the_owner_spoke`, both); the pointer for a message spoken in the thread itself (guild guard removed, `the_dm_tags_a_threads_reply...`); tag removed; pointer ping (quiet removed); pause not archiving; persona always bold; reply to a webhook message not counted (`a_personas_reply_is_signed...`).
- Plain turn frames: `bench turn --check`: plain 4, tool 8, unchanged. `bench lifecycle --runs 10 --check` (debug build, this VM): every startup/shutdown/restart/swap budget ok; the one miss is "health, after the seed" p95 5.2 ms vs 5 (budget measured on the owner's machine; unrelated path).
- Gate (`THESEUS_GATE_NO_BENCH=1`) before the last fix: keel, fmt, shape, features, clippy, cockpit (after `npm ci`), test build, reader rule ok; suite 4460 passed, 35 failed: 33 are the known L1 failures (sandbox contract, theseusd sandbox), `stop_checkpoint` (mine, fixed in d831232 and passing alone), and `tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, whose fix (theseus-wpv2g, b495389) is not in this branch's base; it fails the same way without my change's code paths. The gate was not rerun after d831232 (phases after the suite not run).
- `python3 scripts/keel-guard.py`: ok, 0 findings. **Keel findings expected:** none.

## Live check (maintainer, `#theseus-test`)
1. Scratch daemon, bindings: `#theseus-test` private + the owner's DM; config `[threads] discord_channel = "<a channel or forum id in the guild>"`.
2. `theseus --socket S thread new tide-gauges --title "Tide gauges"`.
3. In `#theseus-test`: `/thread bind slug:tide-gauges` -> "runs the thread"; send a message -> reply, no tag, pings. `theseus thread show tide-gauges` lists the surface.
4. In the DM: `thread:tide-gauges what now` -> reply opens `-# → Tide gauges`; `#theseus-test` gets a silent `-# from the DM: what now · <link>`; reply to that reply -> same thread again.
5. `/thread unbind`; then send the first message to a second thread from the CLI (`theseus ask --thread x hi`) -> a Discord thread appears in `discord_channel` named by its title; `theseus thread pause x` archives it, `resume` unarchives.
6. `theseus thread edit x --persona Ada` (or `thread.edit`), message in its place -> sent through a webhook named Ada (Manage Webhooks) or `**Ada**: ` otherwise.

## Left / uncertain
- Doc changes for the maintainer: spec Part III item and `docs/status.md` for T6; AGENTS.md of discord, core, protocol and sim are updated here.
- Merge: every `TurnSubmitParams {..}` literal gained `speaking: None` and every `TurnRequest {..}` literal `answer_at: None` (~95 files, mechanical; a later branch's new literal needs the line). T4 may add its own `discord` origin to `turn.submit`; reconcile with `speaking`.
- A DM-spoken thread turn has no live stream; its approval cards go to the thread's place (nowhere if the thread has none, then the CLI/web answers). A retried or continued turn answers at the session's place, not the DM.
- Dynamic places (Discord threads) have no guild in health; `/thread bind` of an unlisted channel outside a private parent is refused (trusted guilds are not read). Restart revival (`revive_thread_lanes`, `start_threads`) is covered by reading, not by a restart test.
- `bench lifecycle` and the gate's benches were not run at release profile.
