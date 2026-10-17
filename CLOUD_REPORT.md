# CLOUD_REPORT: voice-tasks (theseus-bh3t.3)

Branch `cloud/20261017-voice-tasks`. Work commit 486df352.

## What I found
- The voice status's one work watch (`voice_status::run`) already hears the whole board; the feed hangs on it (no second watch).
  It ran only when `[voice] status` was on, so it now runs when the status or `speak_tasks` is on (`Handle.board` keeps the
  board's line and the status writes tied to the status alone; `put` is now an `Option`).
- Views carry no `place`, so "the owner's task" is read from where the task reports (`outbox.target(task session)`): a shared
  place's task, or a shared voice place, says nothing. As `Tally` already assumes, the board is the owner's.
- `task.cancel` takes no Discord origin and the core does not judge it (like `/stop`); `work.answer` is judged (owner, private).
- The decline option's id is `decline` (not `deny`/`reject`); "deny" maps to it. An `ask`'s options are `choice:N`, by position.

## What I changed (486df352)
- New `dc/runtime/voice/tasks.rs`: `Tasks` (said-once set of 256, pending question 5 min, past/seed rule), `matched` (the table),
  `Place::voice_answered`. Tests `voice/tests_tasks.rs` (12).
- Hooks: `voice.rs` (mod lines, `voice_turn` calls `voice_answered`, `start_status` builds the feed, `board_line` checks `board`),
  `voice_status.rs` (`Watching` struct, feed in `watch`/`changed`).
- `[voice] speak_tasks = true` in `VoiceConfig` and the template; AGENTS.md of theseus-discord has a bullet.
- Spoken: question `"<title> needs you: <what>. Say approve or deny."` (approval: the tool's name, never its arguments; ask: the
  first sentence and the numbered choices); end `"<title> is done: <1-2 sentences>"`/`failed:`/`was stopped.`. 220 chars, code/links/paths cut.
- A task cancelled by voice has its end marked seen, so "Stopped." is not followed by "X was stopped.".
- Ack words: "Approved." / "Declined." / "Option two." / "Stopped."; refusal "I can't take that here." plus the core's words as a notice.

## Proof
- `cargo nextest run -p theseus-discord -E 'test(tests_tasks)'`: 12/12 (3 repeats). Whole crate: 455/456; the one timeout,
  `tests_task_threads::a_task_in_a_guild_channel_gets_a_thread_...`, times out alone on the base too (stash test): not mine.
- Core subset (`test(example) | test(config) | test(voice) | tests_registry`) green with the discord run: 492/493 at the time, the one
  failure was the stop test's race fixed afterwards.
- Real core state, not mocks: the "second one" test asks a real `owner_ask` over a CLI turn and reads `action.confirm_answered`
  (`choice:1`) and an empty `confirm_list`; "stop it" cancels a real `task.start` task (kernel state `Cancelled`).
- Planted reverts (each restored and touched): speaks twice (`mark` always true) -> 3 tests fail (report_once, the_past, spoken_choice);
  speaks with no call (no `unwatch_work`) -> `nothing_is_said_once_the_call_has_ended`; private check off -> `nothing_is_said_in_a_shared_voice_place`
  and `stop_it_cancels...`; ordinal off by one -> `the_matcher_reads_whole_utterances_only` and `a_spoken_choice_settles...`.
- `cargo fmt --check`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -D warnings`: clean.
- A bare "yes": `heard.rs` is private to theseus-voice, so no test of mine drives the engine. By its rules a "yes" over/just after
  speech is a backchannel (no turn); in the clear it is Words and reaches `voice_turn`, where `matched("yes")` is None (tested), so it is an
  ordinary turn. The spoken question therefore asks for "approve".

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in **keel/door** before my code is judged: three `Keel:` acks on
earlier commits do not verify here (`%G? is E`: no signing keys on this VM) and 3 erosions in the base range (lifecycle.rs seed,
keel-guard.py, test_keel_guard.py) are not mine. I ran fmt, shape, clippy and the tests above by hand. Not run: cockpit, deny, bench,
the full workspace suite, the known L1 failures.
Disk: the VM filled up once (`target/debug/incremental`, 12 GB); I deleted it and built with `CARGO_INCREMENTAL=0`.

## Live check (maintainer)
`/join` the test voice channel from a private place; start a task that asks (approval); hear "<title> needs you: wants to run ...";
say "approve"; the card settles and you hear "Approved."; say "stop it" on another to hear "Stopped."; let a task finish and hear its sentences.

## Open / design notes
- A task whose report goes to the voice place's own session is not announced at its end (its wake turn is spoken by `voice_heard`);
  its questions are.
- A dotted word like `notes.md` is cut and its sentence stop kept, so a sentence can end oddly ("wrote the notes to.").
- No store format change, no protocol change, no new dependency.
