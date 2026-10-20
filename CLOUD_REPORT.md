# CLOUD REPORT: onto-wire (theseus-xzxqv with theseus-s1vkl)

Branch `cloud/20261019-onto-wire`. Work commits: `ea9e28f`, `642466f`, `50b0030`, `ec08c29`, `45589d4`, `4b52f5d`. This report is the last commit (not for main).

## Gate result

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `45589d4`: door, keel, fmt, shape, features, clippy (-D warnings), cockpit, test build and the reader rule passed. The suite failed. The failing tests, by cause:

- **L1 on a root VM, not mine (about 66 tests):** `theseus-sandbox::contract` (clauses 01-12, egress_18b_*, exit_status, scratch, sigterm...), `theseus-sandbox::bench spawn_100`, and `theseusd::sandbox` (26). L1 refuses a root daemon with no job cgroup. They fail the same way at base.
- **`theseus::herdr a_line_sent_on_to_main_does_not_leave_the_pane_sending`:** timing flake. It failed in the gate at 20 s and passed when I reran the herdr tests alone.
- **`theseusd::judge` (3 tests): mine, fixed in `4b52f5d`.** These were `a_start_with_the_judge_on_builds_nothing_of_it`, `a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes` and `a_kill_mid_block_books_the_rest...`. The rig turned off only the other inbound packs, so `onto.v1` asked Jev and health's pack line list lacked `onto.v1: off`. After the fix the 4 tests of `binary(judge)` pass. I did not rerun the whole gate afterwards, because the suite takes about 25 minutes and the deadline was near.
- Earlier, in my own runs, six `theseus-discord` tests (`tests_task_threads::*`, `tests_war_room::a_shared_place_refuses...`) time out at about 101 s. I verified they fail identically at base with `git stash -u`. They did not show in the gate's failure list.

Other things to know:

- The disk filled twice (the shared allowance). I deleted `target/debug/incremental` and the big test binaries, and used `CARGO_INCREMENTAL=0`.
- The keel guard first flagged two `#[allow(clippy::too_many_arguments)]` in `decide.rs`. They are removed (`45589d4`, an `At` struct) and the keel then said `ok`.
- The row checks I ran by hand: `cargo fmt --all --check`, `scripts/shape.sh`, clippy on the five packages with `-D warnings`, `tests_onto` in core and judge, `tests_first_request`, and the example-config template tests. All were green on the working tree before `4b52f5d`.

## Steps

### 1. `onto.v1` pack, builder, miner, scripted answers (`ea9e28f`)
- **Found:** `onto.v1` had no pack. The `Source` enum had no dynamic sources for the ontology.
- **Changed:** `theseus-judge/packs/onto.v1.toml` (Choice per kind, `<kind>_name`, per-item `still_<kind>` Nouls, `kind_of_thing`, `personal`); `builders/onto.rs` (`OntoInput`, the picker: session, then thread, then word match, then recency, at most 20, with `new` last); `mine.rs` (titles, paths, hosts, capitalised words, at most 10); `FakeJev::script_onto`; goldens (`inbound.onto.state.json`, `onto.v1.request.json`).
- **Proved:** `theseus-judge` `tests_onto` (state, picker, miner, round trip, three bands, suggested names) and the loader-rules count 18→19. Plants (replayable with `/tmp/plant_p8.json`):
  - P8: `builders/onto.rs`, the sort key `(!a.session, !a.thread, *ha == 0, Reverse(*ha))...` becomes `(*ha == 0, Reverse(*ha))...`. `tests_onto::the_picker...` fails on the candidate order.
- **Live check:** `theseus judge get <id>` for an inbound judgment shows `onto.v1` questions in `context.onto`.
- **Left or uncertain:** the seeded `project` kind is Given with transport-only assigners, so it is not asked until a row lets Jev assign it. Images are the multimodal row's.

### 2. `onto.v1` in the inbound batch, the sink, the footer, cards, naming, undo (`642466f`, `ec08c29`, `45589d4`, `4b52f5d`)
- **Found:** the batch carried `classify.v3` and `role.v1`. `route.v6` (which the turn waits on) must not change. Packs batch only when their state JSON is byte-identical.
- **Changed:**
  - `judge/onto/{mod,decide,stage,name}.rs`: a pure `decide` run twice (at landing for the footer and the naming call, at the sink for the writes).
  - `judge/inbound.rs`: `route.v6` gets the plain state, and `request = "own"` moves `onto.v1` to a third request.
  - `judge/sink.rs`: `stage_into`/`Board::stage`/`Staged`, so the records, the `onto.filed` row and the cards ride the judgment's own frame under the ontology writer lock.
  - `rpc/ontology_machine.rs` (`Machine`), `rpc/ontology_kinds.rs` (`ontology.undo`, `ontology.kind.add`, `ontology.kind.retire`), `rpc/judge_latency.rs`.
  - Ledger kinds `onto.filed`, `onto.named`, `ontology.kind` (`onto.named` is also in `day_ceiling::SPEND_KINDS`, now 16 entries).
  - `config/ontology.rs` and the example template: `judge=true`, `act=0.80`, `confirm=0.55`, `name_profile="haiku"`, `request="batch"|"own"`.
  - `TurnSubmitResult.filed` and the footer `filed: topic › parent › name (new)`.
  - A mint card with undo, and a confirm-band card that writes nothing.
  - Generated TypeScript regenerated and committed.
- **Proved:** `theseus-core tests_onto` (key test `a_new_topic_at_the_act_band...` first; confirm band; below band; undo; operator wins; off switch; the turn's wait; naming cap; kinds; latency), `tests_judge`, `tests_inbound`, `tests_continue`, `rpc/judge_latency` unit tests, the config's `tests_ontology`. Plants (specs in `/tmp/plant_p1..p7.json`, runner `plant.py` in the scratchpad):
  - P1: `judge/onto/stage.rs`, `Ok((records, rows))` becomes `Ok((Vec::new(), rows))`. `tests_onto::a_new_topic_at_the_act_band` fails (no records in the frame).
  - P2: `stage.rs`, `act: core.cfg.ontology.act,` becomes `act: core.cfg.ontology.confirm,`. `tests_onto::a_confirm_band_answer` fails (the confirm answer writes).
  - P3: `theseus-ontology/src/machine.rs`, `(Record::Category(c), None) => Record::Category(c.retired(at_ms)),` becomes `...Record::Category(c.clone()),`. `tests_onto::undo_through_the_rpc` fails (the mint stays).
  - P4: `judge/onto/mod.rs`, `&& self.core().is_some_and(|c| c.cfg.ontology.judge)` becomes `&& self.core().is_some()`. `tests_onto::off_the_batch` fails.
  - P5: `judge/inbound.rs`, `join3(routed, rest, alone).await` becomes sequential awaits. `tests_onto::the_turns_wait...` fails (wait over 1.0 s with route live and a slow batch).
  - P6: `decide.rs`, remove `.filter(|(_, m)| m.origin == Origin::Jev)`. `tests_onto::the_operators_memberships...` fails (an operator entry is dropped).
  - P7: `name.rs`, `held.is_some_and(|n| n.day == today)` becomes `... == "never"`. `tests_onto::the_naming_call...` fails (called twice).
  Each plant was reverted and the file touched (fresh mtime).
- **Live check for the maintainer, on a scratch daemon (own `--config`, `--socket`, `--state-dir`):**
  1. `theseus --socket <s> ask "Let's plan the quarterly compliance audit for Redwood"` in a private place; reply footer should say `filed: ...` (or `theseus judge list --pack onto.v1`).
  2. `theseus --socket <s> ontology list` shows the minted category; `added_by` is `jev:<judgment id>`.
  3. `theseus --socket <s> ontology undo <judgment id>`; the list returns byte for byte.
  4. `theseus --socket <s> ontology kind add ...` and `... kind retire ...`.
  5. `theseus --socket <s> judge latency --pack inbound --since <date>`.
- **Left or uncertain:**
  - A message routed early by the thread router files the home session, not the thread's session.
  - The footer shows only on outbox and Discord replies.
  - A late footer is a `notice` post.
  - No store format bump (still 46).
  - A sparse config must not show `[ontology]`; the template tests hold this.

### 3. CLI and Discord (`50b0030`)
- `theseus ontology undo|kind add|kind retire` (`onto_cmd.rs`; `client::OPERATORS` 88→91, refused inside a job). Discord renders the `onto_card` post (`runtime/onto_card.rs`, `Buttons::OntoUndo` as `onto:undo:<write>`) and the `filed` footer line.
- **Proved:** the CLI's operator-list tests, the discord render tests. Not a full Discord live check; maintain it with a real DM.

## Docs that should change (I did not edit them)
- Spec Part III: a new item for xzxqv/s1vkl and the version line in `docs/spec/01-front.md`; `docs/status.md` (updated line and recently landed); the R3 row in `docs/design/roadmap-v2.md`.
- `crates/theseus-core/AGENTS.md` and `crates/theseus-protocol/AGENTS.md` are already updated in the commits.
- The WIRED doc comment in `judge/mod.rs` still does not mention onto being live (comment only).

## After the join, for the maintainer
1. `scripts/build.sh --profile release-thin`, install the four binaries (see AGENTS.md, "Installing").
2. Watch a day of DMs: memberships, mints and their cards (`theseus ontology list`, `theseus judge list --pack onto.v1`).
3. Try `theseus ontology undo <judgment id>` on one mint.
4. Run `theseus judge latency --pack inbound --since <date>` daily for a week. If the batch p95 grows by more than 50 ms, set `[ontology] request = "own"`.

## Design choices the owner should hear about
- Jev's memberships are written at the act band only, in the judgment's own frame, origin `jev`, `added_by jev:<judgment>`. A confirm-band answer only asks.
- A list full of operator entries refuses Jev's (a refused step with `disagrees: true` in `onto.filed`); a full list drops the weakest Jev entry.
- A `new` with a name that an existing category of the kind already has files under that category (no double minting).
- `ontology.kind` is a new ledger kind, and R2's `ontology_machine_write` was refactored into the `Machine` struct to share undo rows.
