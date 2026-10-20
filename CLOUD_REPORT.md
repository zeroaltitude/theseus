# CLOUD_REPORT: dm-cards-fix (theseus-27p6w)

Branch `cloud/20261019-dm-cards-fix`, base `ae7724f`.

## Step 1: a due question's card always posts (the pacer paces the buzz)

**Found.** As the brief said: `escalate_questions` returned 0 when the window was closed, and the question expired (15 min) before a window (30 min) reopened.

**Changed** (commit 1b4c9ed, test-count fix-up after it): `needs_you.rs` posts every due question on its tick; `loud = window open || prompted`; with `loud` false every post body gets `"silent": true`. `outbox.rs` (live `posted` and the index reread) takes the pacer window only for a covered post that is not silent, so a silent card never takes it, also after a restart. Doc lines fixed in needs_you.rs, pacer.rs, config/notify.rs, theseus-core AGENTS.md. `dm_cards = false` follows the same rule (one-line post, silent when closed, body otherwise `needs_you::body_of`).

**Proved.** `tests_m3/ask/due.rs`: the key test (window closed 5 min earlier, approval card posts within the tick, `silent: true`, card kind approval, `pacer.last_ms` unchanged; second tick posts nothing; a fresh `Outbox::new` reread knows it posted and took no window; with the window open it pings and takes it) and the `dm_cards = false` test. The two old window tests (`unprompted_pings_go_one_a_window...`, `with_dm_cards_off_the_rest...`) now assert a silent post with the window untouched; assertion counts kept (keel guard prints nothing, `--base ae7724f --head HEAD`).
Plants (each restored and `touch`ed, `git status` checked):
1. needs_you.rs, before `let prompted = ...` add `if !self.outbox.pacer.open(now, pace) { return 0; }` -> `due.rs:18` "carded within the tick" fails (left 0), also the ask-again test and the old window tests.
2. needs_you.rs, delete the line `&& !self.outbox.pinged(&a.correlation_id)` -> `due.rs:31` (second tick posts again) and `ask.rs:285` ("once") fail.

## Step 2: "Ask again" on an expired card

**Mechanism chosen.** No new protocol method, no new store record kind, no format bump. The expiry's settle post body gets `"again": true` when the question's execution is not terminal (`Outbox::closed`, outbox.rs). Discord shows one button, "Ask again" (`Buttons::Again`, custom id `again:<question>`, runtime/ask.rs), on the classic needs-you card. The press is `work.answer` with option `ask_again` (`protocol::answer::ASK_AGAIN`), routed in `rpc/answer.rs` to the new `rpc/ask_again.rs`: judged as an answer (`judge_act(Act::Answer)`), accepted only for a question that closed by expiry (owner_ask: its `ask.answer.*` META says expired; others: resolution "declined by expiry"), then `Kernel::create_wake` due now, purpose check, run turn, note `The owner asked again for: <card's what>` (idempotent id, so a second press says "already asked again"). A wake is the session's existing way to take a turn from outside, so a live conversation or a waiting task plans the call again. The press also marks the session prompted (`Outbox::prompted`, in memory): its next question is due at once and pings whatever the window says. Why this over `turn.submit`: it needs no session id on the press path, uses the owner judgment already in `work.answer`, and works for task sessions.

**Proved.** core `due.rs`: an expired approval settles with `again: true`; an MCP-surface press is refused (REFUSED, no wake); the owner's press sets one wake with the note, a second says already asked; a hand-run turn plans the call again and its card posts at once, not silent, with the window closed; a cancelled execution's expired settle has `again: false`; an approved question cannot be asked again. Discord `tests_gateway/cards.rs::an_expired_card_offers_ask_again_and_the_owners_press_asks_it`: the expired card carries exactly `["Ask again"]`, the owner's press clears it (callback type 7, "asked again") and the wake exists. Plant: outbox.rs `body["again"] = json!(false && live.is_some())` -> `due.rs:108` fails.

## Checks
- `cargo fmt --all --check`, `bash scripts/shape.sh`: ok. `cargo clippy -p theseus-core -p theseus-discord --all-targets -- -D warnings`: clean.
- nextest core filter `needs_you|pacer|notify|card|expir|ask|layouts|registry|output|first_request|digest|initiative`: 536 passed.
- `nextest -p theseus-discord`: 564 run, 557 passed, 7 failed, all pre-existing on this VM: `tests_task_channels::a_short_task_gets_the_thread...`, `tests_task_threads::{a_shared_channels_task_thread..., a_shared_forum..., a_task_in_a_guild_channel..., react::in_a_shared_channel..., the_owners_message_for_an_ended_task...}`, `tests_war_room::a_shared_place_refuses_a_share...`; each times out waiting for a task thread; I confirmed one fails identically on the base with my changes stashed.
- keel guard: empty.
- Full `nextest -p theseus-core -p theseusd` (first run that linked): 3,804 run, 3,712 passed, 92 failed, **all 92 in theseusd, none in theseus-core**. They fail at 0.2 s with `NotFound` spawning the `theseus` CLI, which that command did not build (the known L1/root failures are among them). I built the CLI and tried the theseusd rerun, but the VM's disk allowance ran out twice (ENOSPC, then a linker crash), so **the theseusd rerun did not complete**. The theseusd suite is therefore unproven on this branch; theseusd does not read the changed code except through the outbox, which theseus-core's suite covers.
- The full gate line (`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`) was **not run** (disk).

## Live check for the maintainer
1. Start a scratch daemon (fresh state dir), bind a test DM. Post any `needs_you`/`digest`/`initiative` (or a first question) so the pacer window is closed.
2. Have a session ask an approval (`fs.write`) and leave it. About 60 s later the DM shows a **silent** needs-you card with buttons (no buzz); `theseus history` lists it; pressing Approve runs it.
3. Let one question expire (15 min, or `[kernel] confirm_ttl_secs` low): the card edits to "Expired" and shows **Ask again**. Press it as the owner: the button goes, "asked again" shows, the session takes a turn with the note, and its new card appears at once and pings.

## Left / uncertain / for the owner
- **Design note.** The 2026-10-13 rule (one buzz a window) is kept: a silent card is no buzz. Alternative, not built: a question that would expire before the window opens pings anyway.
- A prompted question (after Ask again) pings regardless of the window and, because it pings, takes the window. The prompted mark is in memory (a restart forgets it; the question then waits as any).
- "Ask again" is offered while the execution is non-terminal **at the moment of expiry**; if the session ends later, the press is refused with "its session can take no more turns".
- Only the classic needs-you card (the DM's) gets the button. A V2 approval card (`card_v2::closed`, `detail.v2`) still settles with no button; adding it needs a button in `closed` and a route for `again:` ids through `live_board::task_press`.
- The pre-existing 7 theseus-discord failures above.
- A non-owner cannot press in the owner's DM in the stand-in, so the refusal is tested in core (MCP surface), not through Discord.
- The first commit mixes both steps (the outbox, needs_you and test files are shared); I did not split it. A follow-up commit restores the pacer test's assertion count for the keel guard.
- Docs: theseus-core's AGENTS.md "always-on channel" paragraph is updated for the pacing; it does not yet mention Ask again (suggest one sentence there and in theseus-discord's AGENTS.md "needs-you cards": `again:<question>`, `Buttons::Again`).
- No store format bump, no new dependency, no protocol type added (one `ASK_AGAIN` const in protocol/answer.rs).
