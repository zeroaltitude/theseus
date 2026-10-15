# CLOUD REPORT: route-persona, T10a (theseus-c1uz)

Branch `cloud/20261013-route-persona`. No store format change (nothing new is stored: the record is a `persona.engaged` ledger row).

## What I found / built
- Tail line, not header: with 2+ `[personas]` every persona's files are in the persona layer and the header is the built-in default whoever answers (`Config::persona_engaging`, `context_paths`, `owned`). The engaged persona is one line `<Name> answers this turn` attached after the cached blocks of the request's last message (`turn/persona_engage.rs`, like the surface's standing records). A switch changes no prefix byte. **Design choice for the owner:** with 2+ personas a persona file no longer *replaces* the default whole; it is added after it. One persona: unchanged.
- `route.v5` (judge crate): v4's two questions word for word + `persona` (`Source::Personas`, no_match `prior`, decides=false) + `seal` (yes/no, `Source::ThreadNow`, decides=false). State adds speaker, cards, thread title/goal/last 3 turns with who answered; absent otherwise, so stored states are byte-identical.
- `engage.rs` (pure): explicit address (name+comma/colon at start, `@name`, `/as name`) > owner correction/layer > Jev by band (act silent, confirm..act shown with why, below/late/shadow: prior) > prior (session's last engaged, in memory, else its own).
- Route line (who + why) on CLI/TUI (`render.rs`), Discord footer, cockpit (`routefooter.ts`, persona menu in `RouteFooter.tsx`). `TurnRoute.persona` (`RoutePersona`).
- Corrections: `route.correct { to: "persona:<key>" }` (`theseus judge correct persona:birch`, cockpit menu, Discord reaction of a persona's `emoji`, "that was for Birch", `/as birch` alone): labels the `persona` question, `Steer::Persona`, next message at once, close ones via the layer. "Birch, answer that" is an address (same session, only the tail line differs).
- Seal: `judge/seal.rs` calls `Core::thread_recommend_seal` on a `yes` at confirm+ in a thread, only when `[threads] seal_recommend = true`.
- New config (all with defaults + template lines): `[threads] persona_engage = "shadow"|"act"` (default shadow), `[threads] seal_recommend = false`, `[personas.x] shareable = false`.

## Deviations (revisit)
- Ladder: a shadow-only route.v5 would have unwired live mode routing (rung is per pack), so route.v5 is adopted live as v4 was (`decision of 2026-10-13`), and "shadow first" for the persona is `persona_engage = "shadow"`. The maintainer flips it to `act` after the holdout. The pack's baseline stays `session_profile` (not `thread_prior`; baseline is per pack).
- The persona decision waits on the existing route wait (`[routing] max_wait_ms`), not `route_wait_ms`: the compile does not name the persona, so no new timer.
- The explicit-address `persona.engaged` row has no judgment id (decided before the verdict); no system-label rule was added.
- Limits: prior and "answered by" history are in memory (lost at restart); the thread router's early inbound call asks the persona question without thread context; `/as` as a Discord reply corrects the session's last routed turn, not the replied-to one.

## Commits
9d980c02 judge pack; 91095edc core/surfaces; plus a late-verdict test commit; this report last.

## Proof
- `cargo nextest` theseus-judge 151/151. theseus-core+judge+protocol: 2964/2967 before my test rewrites; the 3 failures were tests of the old 2-persona layout (`tests_persona` x2, `config::tests::context_files...`), rewritten (same assertion count, now tail-line/all-files), passing. `tests_engage` 8/8, `engage::tests` 6/6.
- Full gate with `THESEUS_KEEL_BASE=6cbb6201` (see keel): fmt, shape, features, clippy -D warnings, cockpit lint/test/build, test build all pass; suite: only the known root-VM L1 `theseusd::sandbox` failures (66 FAIL lines, all sandbox). Later phases run by hand: turn bench: 4 frames per plain turn (all turns), first-request token budgets ok; `cargo deny --offline` could not run (needs network, fetch not done); protocol TS regenerated and committed.
- `theseus-sim bench lifecycle --runs 10 --check`: MISSED twice, only on the push's seed (1.2 vs 2 ms p50 ok, p95 2.5 > 2) and health reads (p95 7.9 > 5); startup/serving phases fine. These paths are untouched by me and this is a debug build on a 4-core VM; maintainer should rerun on the owner's machine.
- Planted reverts (restored and committed tree clean): engaging layout off -> tests_persona x2, config context_files, template test fail; address() disabled -> `an_address...` + `an_explicit_address...`; confirm band acting silently -> `the_bands...` + `jevs_pick_engages_by_its_bands`; shared filter ignored -> `a_shared_place_offers...`; persona label skipped -> `a_correction_labels_steers_and_teaches`; seal flag ignored -> `a_seal_yes...`; carried verdict used -> `a_late_persona_verdict...`; engaged line moved into the system block ("a persona in the header") -> `a_switch_changes_no_prefix_byte` + tests_persona; persona file text as Jev's card -> `the_personas_files_are_in_the_prefix_and_never_in_jevs_state` only. (A persona-in-play name in the header was caught by `sessions_of_one_persona_share_the_cached_blocks`.)
- Call count: `the_persona_question_adds_no_call`: 2 Jev connections with and without personas; the route request carries `route.v5/persona`.

## Offline persona track (bench/threads/th_persona.py, seeds 11-13, 143 msgs)
Prior 0.57/0.54/0.57 overall, 0.08/0.03/0.06 on the ~35 switch messages; cards-keywords 0.52/0.52/0.51 overall, 0.58/0.57/0.59 on switches. The stand-in answerer (noise 0.2) scores 0.82/0.77/0.80 and says nothing about Jev. **No Jev key here: no Jev numbers.**

## Live check for the maintainer
1. `python3 bench/threads/th_persona.py --seed 11 --out /tmp/p --dump-inputs --jev-probe target/release/jev-probe` (adapt inputs to `personas`), compare to the baselines above.
2. Set `[threads] persona_engage = "act"` with two personas, in `#theseus-test` send 20 messages over four specialties: expect >=85% engaged right, every `Name,` / `@name` honoured (`persona.engaged` how=explicit), `theseus judge correct persona:<key> --session S` then the next message `how=corrected`, and no cache rewrite at a switch (ledger `provider.call` cache read stays high).

## Keel
Findings of mine: none (`THESEUS_KEEL_BASE=6cbb6201`: 0 findings, 64 files). The default gate fails the keel phase on an inherited finding: `crates/theseus-sim/src/lifecycle.rs:155` budget-raised in base commit 1c2beb2747 (its Keel ack signature does not verify). **Keel findings expected:** none from this branch.
Docs to change at review: spec Part III item, status.md, `docs/design` (route.v5, the layout change above).
