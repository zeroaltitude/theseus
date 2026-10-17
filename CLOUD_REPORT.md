# CLOUD_REPORT: task-offer-chip (theseus-bh3t.5)

Branch `cloud/20261017-task-offer-chip`; work commit `2d05d782`.

## What I found
- Core titles a task from the brief's first line, cut at 60 characters (`title_from`), so the modal needs no title field and `task.start` no `title`: the prefilled brief is the message itself.
- `turn/reply_step.rs` receives the turn's result, whose `route` already holds the mode; `Turn.route.result` is taken before it.
- `runtime.rs` and `protocol/lib.rs` needed no edit. The press reaches `more_press` through the existing `tb:` prefix.

## What I changed (2d05d782)
- **Judge**: `packs/classify.v3.toml` (v2 word for word plus `offer_task`, no `decides`); registered in `EMBEDDED`; golden `fixtures/golden/classify.v3.request.json`; pack count 23; shape list, rollback rules, reservation row; `classify_v3_is_classify_v2_and_an_offer_question`.
- **Core**: `CLASSIFY_PACK = "classify.v3"`; `offer_answer`; `JudgeService::note_offer` / `take_offer` (64 kept, in memory); `reply_step::offer_for` puts `"offer": {"brief"}` on the post. No `TurnSubmitResult`/protocol change, no store format change. Existing assertions were rewritten to v3, none deleted (`prove.rs` also reads v3's judgments).
- **Discord**: `Buttons::Sources` gains a third field (the offered turn), so the button joins the sources row (own row if 5 sources); `Tasks.offers` holds the briefs; `start_task::modal(quote, brief)`; `offer_press` (owner only, else ephemeral "Only the owner can start a task here."); `drop_offer` edits the button off after a submit. Modal custom id is `nt:<turn>`.
- **Sim**: `FakeDiscord::submit_modal_from` (a modal submit that carries its source message).
- Docs: core and discord `AGENTS.md` bullets.

## Proof
- `cargo nextest run -p theseus-judge`: 152 pass. `-p theseus-protocol` 165 pass, `-p theseus-sim` 95 pass.
- `-p theseus-core`: 3018 of 3019 pass; failing: `tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future` (also `the_agenda_expands_a_series...` once). They read the real clock and the series/agenda; I touched nothing near them. Not re-proved against a clean base.
- `-p theseus-discord`: 445 of 448 on the first full run; the 3 `tests_task_threads` timeouts passed alone (10/10) — load. My 3 tests pass.
- New tests: core `tests_offer.rs` (7: yes at Act, 1,000-char brief, Confirm/no, trivial and quick, shared place, judge not landed via `Held`, `task_create` made); Discord `tests_board/offer.rs` (3: button, modal brief and submit and button dropped, guest refused, shared place no button).
- Planted reverts (each restored and `touch`ed): `acts_true` -> always true: `a_yes_at_the_confirm_band_or_a_no_offers_nothing` failed. detour check off: `a_trivial_or_quick_turn_offers_nothing` failed. owner check off: `a_message_in_a_shared_place_offers_nothing` failed. courier ignores the offer: both press tests failed. Discord owner check off: `a_guests_press_is_refused_to_the_presser_alone` failed.
- fmt check, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -D warnings`: clean (one `too_many_lines` in `judge_inbound` fixed).

## Gate
`scripts/gate.sh` did not complete: the door runs `origin/main`'s newer gate.sh on this tree and its keel guard reports erosions that are main's own since my base (`lifecycle.rs` seed, keel-guard.py / test_keel_guard.py, moved compiler tests). None are files I touched. I ran the phases by hand as above; not run: deny, cockpit build step beyond `npm run build` (ok), benches (budgets untouched), theseusd integration tests, the turn and size steps, the full workspace suite in one go (disk allowance on this VM forced per-crate runs). Known-VM L1 failures were not seen in core.

## Live check for the maintainer
In the owner's DM write: "we should clean up the bench reports later and then re-run the async suite". Expect a reply with `➕ Make this a task` under it; press it: the modal shows the message as the brief; submit: `▶️ Started …` and the button disappears. A message with a question or "do X now" should show no chip.
`theseus-sim bench turn` should still show 5 frames for a plain turn (nothing added on that path).

## Left / uncertain
- Judging the reply's own text for offerable work is a new judge point; not built (follow-up).
- Chip appears only if Jev's batch lands before the reply is written (never waits); on a very fast turn there is no chip.
- Offer needs classify live at the act bar; in shadow it offers nothing (by design, "shadow before they act").
- `docs`: spec Part III item, status.md, and the template comment mentioning classify.v1/v2 at theseus.example.toml:1329 could name v3.
