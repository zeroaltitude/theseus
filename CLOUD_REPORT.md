# CLOUD_REPORT: tail-discord

Branch `cloud/20261021-tail-discord`. Store format unchanged (48): nothing here is stored. No new dependency. No file
over its line ceiling (`shape: ok` in the gate); `runtime.rs` gained no line (`mod seal;` became `pub(crate) mod seal;`).

## Commits
- 98bz.12: `16e96ba`, `250d5a4`, `34e9ee8` (the last two keep `thread_interaction` under clippy's 100-line limit)
- stpqg: `a8edd09`
- 0mq02 + x4s7g: `267d29b`, and the watch test's root fix after it
- 0prmm: `34c0f53`

## theseus-98bz.12: link card and `/thread link`, `unlink`
- Core: `count` stages `{"kind":"link_card","text","here","other"}`; the text is `🔗 Notes of «x» answered questions in «y» twice.` plus a last line `theseus thread link <here> <other>`. Updated in `rpc/tests_thread_link.rs`: the `proposals` filter (`onto_card` -> `link_card`) and an added assertion on `here`/`other`; the text assertion still held as written (it only checks the fragment and the `ends_with` line), nothing loosened.
- Discord: `runtime/seal/link.rs` (buttons, `parse`, `pair_of`, `Shared::link_press`, reusing `seal::failed`), `Buttons::Link`, `"link_card"` in the owner-DM list and card arm, one dispatch arm in `earned.rs`, `/thread link` and `unlink` (`a`, `b`; shared helper `thread_link_said`, and `thread_merge_said` moved out of the long match for clippy). Longest pair (40+40 slugs) gives a 90-char custom id, under Discord's 100 (tested).
- Tests and plants (each restored, `touch`ed, `git status` clean):
  - `tests_gateway::link::a_link_card_goes_to_the_owners_dm_and_links_the_two_threads` (key test): plant `Buttons::Keep` for the card -> FAIL; plant "yes calls nothing" -> FAIL.
  - `tests_gateway::link::not_needed_on_a_link_card_writes_nothing`, `slash_thread_link_and_unlink` (plant: dispatch arm renamed so `link` hits the fallback -> FAIL).
  - `rpc::tests_thread_link::a_pair_admitted_twice_is_proposed_once` (plant: old `onto_card` body -> FAIL).
  - `seal::link::tests::link_card_ids_round_trip` (unit), names test in `thread.rs` updated to include `link`, `unlink`.
  - The "someone else is refused" case uses a forged button in the shared #lab (as `seal.rs`'s test does): the core answers `🔐 Your answer did not count`.

## theseus-stpqg: `born` trimmed
`born: HashMap<String,u64>`; `add` (via `add_at(m, now)`) drops entries older than `DONE_SHOWN_MS + GRACE` with no `Made` row. Test `born_is_trimmed_but_never_reposts_a_task`; plants: never trim -> FAIL; trim regardless of age -> FAIL.

## theseus-0mq02 and theseus-x4s7g: `runtime/tasks.rs`
- `snapshot(ticket, r)`: an ended view older than `DONE_SHOWN_MS` is dropped; a ticket not newer than the last applied is ignored (returns no sessions). `ask` takes its ticket before `work.watch`. `Place::adopt` needed no change.
- Tests: `a_snapshot_drops_an_ended_view_older_than_done_shown` (plant: keep every ended view -> FAIL), `an_older_snapshot_never_undoes_a_newer_one` (plant: no ticket check -> FAIL).
- **Fixture change, to say plainly:** `a_snapshot_keeps_an_ended_task_it_does_not_carry`'s task ended at epoch ms 9, which the new rule rightly drops, so its fixture now ends the task a minute ago. Its assertion is unchanged. The brief said it must pass unchanged; it cannot with a fixture older than a day.
- `Bus::work_watched(root)` added; `tests_task_watch` now records, at each model call, whether the watch covers the lab place's session's execution (the roots `work.watch` names are execution ids, not session ids: my first probe used the session id and was always false). Plant: remove `Place::adopt`'s awaited `ask` -> FAIL. 10/10 stress runs pass.

## theseus-0prmm: `home_up`
Waits until the core was told the home (`outbox.report_target(dm).1.is_some()`, set by `register_task_homes` right after the state holds the channel) as well as the META record. `tests_task_home` with `--stress-count 10`: 10/10 iterations passed (7 tests each).
**Finding, production window:** in `ensure_task_home`, `find_task_home` writes the META record, then `home_lane`, `state.send_replace`, `register_task_homes`, and `wake_places` (only when the channel changed). A board drawn in the DM before the state holds the channel is not removed: `wake_places` redraws the place, which now goes through `board_lane` to the home, but the earlier DM message stays pinned. I did not trace the pin's cleanup further and did not fix it (more than a few lines). Follow-up: have the lane's idle/clear path edit the stale DM board, or hold the first board draw until `ensure_task_home` first answers.

## The gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: keel ok, shape ok, clippy ok, then the suite: 7493 passed, 49 failed. 34 are the known L1 failures (theseus-sandbox contract, `spawn_100`, theseusd sandbox). The other 15 failed only in the full run (the VM's disk filled repeatedly during my session, and the full suite ran under load): `rpc::tests_ordered::a_stop_after_an_input_reaches_its_running_turn`, `tests_horizon::the_agenda_expands_a_series...`, `tests_war_room::a_guests_comment_reaches_no_session...`, theseusd's `job_handles`, `mcp_server` (two), `planted_secrets`, `reaping` (two), `job_approval` (three), `outbox`, `push`, `restart_handle`. All 15 pass on rerun alone (11 + 4 in two runs). I did not run the benches phases (`THESEUS_GATE_NO_BENCH=1`) or the phases after the suite beyond `theseus-protocol` (196 passed); no protocol type changed. `tests_board::commands::tasks_shows_this_places_snapshot...` failed once under load in a targeted run and passed alone.

Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output.

## For the maintainer
- Re-run the full gate on the owner's machine (a loaded 4-core VM with a small disk is where the 15 failed).
- Live check: with a scratch daemon and bindings, make threads `a`, `b`, and run `/thread link a: b:` in a private channel (expect `🔗 \`a\` and \`b\` are linked.`), then stage a pair twice through recalls and watch the DM for the card with **Link these threads** / **Not needed**.
- Docs to change: `crates/theseus-core/AGENTS.md` ("Not yet: a card with Link / Not needed buttons and a Discord `/thread link`" and the `onto_card` sentence in the linked-threads item), `crates/theseus-discord/AGENTS.md` (the `/thread` subcommands, the link card, `born`'s trim), spec Part III item for theseus-98bz.12.
- Rollback for the card: existing `[threads] link_proposals = false`.
