# CLOUD_REPORT: horizon-discord (theseus-j90w2)

Branch cloud/20261013-horizon-discord. Work commits: 986774c8 (the feature), 83a66733 (a test wait, the guide). No store change, no new key, no protocol type.

## What I found
- A routine's own conversation is not a place, so the Discord surface cannot watch it or read it in `work.agenda`. Routines come from `routine.list` (and `routine.runs` for the 30-day record); wakes in the places' sessions (Home is the DM's) come from `work.agenda`.
- The binding hears no event when a routine is made from the CLI or a wake is set by the model, so the snapshot also reads once a minute (and at every session change, `wake.noticed`, work change, and its own posts and commands).
- **A met watch's Done cannot be answered**: its wake is gone, `work.answer` says "no reminder is named". "Keep watching" has no verb. So the met-watch post pings and has **no button**. Needs a core row (a fired record for a met probe, `watch.keep`) before it can.
- Overdue-milestone post: nothing in core produces one yet (`milestone_answers` has no verbs), so none is built.
- Pre-existing: `theseus-sim` test `the_restore_rows_store_is_the_same_with_the_cancel_row_selected` fails on the base without my change too.

## What I changed
- `render/horizon.rs` (pure words: Coming up, routine line and buttons, project line, /agenda text), `render/board_v2.rs` (three sections, folded with the rest to 40 components and 4,000 characters; a routine counts four).
- `runtime/horizon.rs` (the snapshot loop, post buttons `hz:<answer>:<wake>` through `work.answer`, a routine run's report head, the status line's reminder), `runtime/horizon_cmds.rs` (/agenda, /routine add|list|pause|resume|run, /snooze, /watch add|list, /cancel autocomplete; modals; a command check only from the DM and approved once), small edits in tasks.rs, presence.rs, live_board.rs, courier.rs, render.rs, slash.rs, prompt.rs (three items made pub(super)), orders.rs (one chain line), runtime.rs (+4 lines, now 3437 of 3500).
- Core, small: `watch/run.rs` adds `wake_id` and `purpose: watch.met` to a met watch's notice body (so it pings).
- theseus-sim: `FakeDiscord::modal`, `autocomplete`, `Reply.choices`, `fake_gateway::modal_submit`.
- Edited existing test literals only to add fields (`Parts`, `Counts`) and the voice command position (`all[19..]`, 21 commands).

## Proof
- `cargo nextest run -p theseus-discord -p theseus-sim -p theseus-protocol` (minus sandbox): 605 of 606; the one is the pre-existing sim test above. `-p theseus-core -E 'test(watch)'`: 55 of 55.
- New: 9 gateway tests (`tests_gateway/horizon.rs`: /agenda sections; a shared place sees no routine/agenda/choices/board; the DM board has Coming up, Routines, buttons; /routine add modal makes a routine that runs and speaks as the routine; list's Run now and Pause act; a failure posts with 3 buttons once a day and Pause settles it; a met watch posts once and pings; snooze autocomplete from memory then acts; a command watch refused outside the DM), unit tests, a property test (any board with the future fits 40 components / 4,000 characters, unique ids), and the status line test.
- Planted reverts: (1) `routine_pings` always true: the failure test fails (`once a day` posts 2). (2) /agenda's private check forced true: the shared-place test fails. Both restored and touched.
- Under load (nice 19 beside four busy loops), the 9 gateway tests passed twice; the first loaded run caught a test that read the pause's answer too early, fixed in 83a66733.
- fmt, clippy -D warnings, shape: pass.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in the **door/keel** phase before any build, on findings that are not mine: the branch base is behind origin/main as cloned, two older main commits carry a `Keel:` ack whose signature cannot be verified here (no keys), listing `theseus-sim/src/lifecycle.rs` "seed" budget. I ran the later phases by hand: fmt, shape, clippy, and the suites above. Not run: cockpit, the full workspace suite, deny, benches. No keel finding of mine; **Keel findings expected: none.**

## Live check for the maintainer
1. `scripts/build.sh`, then `theseus-sim discord proof --theseusd <bin>` (still passes: the proof is untouched). 2. On a scratch daemon with the fake Discord (fresh state dir): `theseus routine create --title Digest --schedule "every 1m" --brief "say hi"` with a model that fails once; expect, in the DM, one `✗ Digest failed` post with Run now / Pause / Details, no second the same day, a `🔁 **Digest** · <time> · run N` report after a good run. 3. `theseus watch add` a file watch, create the file; expect one `👁 … → …` ping with no buttons. 4. `theseus remind` for today; expect the bot status `⏰ HH:MM` and the board's Coming up and Routines. 5. `/agenda` in the DM and in a shared channel (the latter says it is yours alone). I did not run a real daemon here.

## Uncertain / for the owner
- The status line's `⏰` is only recomputed at a read; across midnight it can show yesterday's or miss today's until the next minute's read.
- Pressing a post's button settles only that Discord message (as a reminder does); an answer from the CLI does not edit it.
- Docs to change at review: spec Part III item, status.md; join note for `discord-emoji-catchup`'s digest (agenda lines can use `FutureParts`).
