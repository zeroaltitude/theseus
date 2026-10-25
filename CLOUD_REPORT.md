# CLOUD REPORT: p3a-recall-floor (theseus-96cyh)

Branch `cloud/20261024-p3a-recall-floor`, base `ac298666`. Code commit `eb8b1d07`.

## 1. For the maintainer

- Store format: 51, untouched (the new field is `#[serde(skip)]`; no stored or wire byte changes; the cockpit's generated types are unchanged).
- No ceiling raised, no OWNER CALL, no STOP.
- **Defaults: left at `recall_floor = 0.0`, `recall_subject_bar = 0.0`.** The model-free sweep shows no setting lowers the admitted-distractor count without losing more than 2 points of gold (details in section 2). No change to `config/memory.rs` (it is also at its 633-line ceiling).
- Item 3 (subject beside a same-kind value) is built, but it can only fire for chunks the index gives entities to (host, path, crate, ...). The bench's facts ("the alder relay's port") carry no such entity, so it does not touch the bench's abstention probes. See sections 3 and 10.
- The brief's premise does not reproduce in this proxy: at 0/0 the pack admits the distractor's value on only 5 of 60 abstention probes (3 of 34 held-in, 2 of 26 held-out), not 28 of 60. Misleading answers of 28/60 came from the model reading near notes; the pack-level count here is a lower bound.

## 2. The sweep

How it ran: `python3 bench/recall/precision.py --run /tmp/rc/run --make 7:full --bin-dir target/debug --out /tmp/rc/out --settings <grid>`. Seed 7, size `full` (the bench's published size: 600 turns, 228 facts delivered, 204 probes, compactions at turns 121, 303, 321, 481, 520, 558), on the stand-in model (no key, no spend). Binaries from this branch's base, `cargo build -p theseus -p theseusd -p theseus-index` (debug). A second pass (`--settings 0.5:0,0.7:0,0.9:0,0.95:0,0.98:0,0:0.9,0.5:0.9`) widened the grid because the first showed no effect up to floor 0.4. Held-in = odd probes (34 abstentions, 68 answerable); held-out = even probes (26 abstentions, 76 answerable). "Distractor" = abstention packs admitting the same-kind distractor's value; "gold" = answerable packs still admitting the gold value; tokens = the pack's mean.

| floor | bar | in: distractor (of 34) | in: gold (of 68) | in tok | out: distractor (of 26) | out: gold (of 76) | out tok |
|---|---|---|---|---|---|---|---|
| 0.0 | 0.0 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.0 | 0.3 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.0 | 0.5 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.0 | 0.7 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.0 | 0.9 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.1 | 0.0 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.1 | 0.3 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.1 | 0.5 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.1 | 0.7 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.2 | 0.0 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.2 | 0.3 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.2 | 0.5 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.2 | 0.7 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.3 | 0.0 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.3 | 0.3 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.3 | 0.5 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.3 | 0.7 | 3 | 27 | 171.3 | 2 | 25 | 177.8 |
| 0.4 | 0.0 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.4 | 0.3 | 3 | 25 | 171.9 | 2 | 25 | 180.7 |
| 0.4 | 0.5 | 3 | 25 | 166.5 | 2 | 24 | 172.6 |
| 0.4 | 0.7 | 3 | 25 | 166.5 | 2 | 24 | 172.6 |
| 0.5 | 0.0 | 3 | 24 | 165.4 | 2 | 24 | 171.1 |
| 0.5 | 0.9 | 3 | 24 | 165.4 | 2 | 24 | 171.1 |
| 0.7 | 0.0 | 3 | 24 | 165.4 | 2 | 24 | 171.1 |
| 0.9 | 0.0 | 2 | 22 | 143.1 | 2 | 23 | 150.9 |
| 0.95 | 0.0 | 1 | 14 | 79.1 | 1 | 18 | 83.5 |
| 0.98 | 0.0 | 0 | 8 | 38.3 | 0 | 10 | 39.2 |

Why it cannot separate them: the fused score is a reciprocal-rank sum, and every one of an abstention pack's eight items sits at 0.89 to 1.00 of the ceiling (the raw rows: the distractor-bearing item is at share 0.8971 to 0.9683; gold-bearing items in answerable packs are at the same shares). A floor high enough to drop the distractor (0.9+) drops the gold with it: 0.9 removes 1 held-in distractor and 5 of 144 gold packs (3.5 points, past the 2-point limit); 0.95 and 0.98 empty answerable packs. The subject bar acts only on chunks with same-type entities and has no effect on distractor counts here (bar 0.5 happens to keep 2 more held-in gold and 3 fewer tokens, noise-sized and not chosen).

Chosen point: 0/0 (before = after). Held-out, once: 0/0 gives distractor 2/26, gold 25/76, 180.7 tokens. Nothing to report on a change.

Which setting would most likely meet the acceptance (misleading at or below 15/60, accuracy within 2 points)? None of this grid, by this proxy. The 28/60 → 15/60 target needs the model to stop reading near notes, which the pack-level counts cannot show. The only plausible levers are the header wording (section 3) and a subject signal richer than index entities (section 10).

## 3. The subject wording

- Where: new module `crates/theseus-core/src/recall/neighbour.rs` (`note`, `marks`, `apply`, `with_note`). `recall/render.rs` and `recall/relevance.rs` are at their ceilings, so neither is touched. `RecallItem.neighbour` (`theseus-protocol/src/memory.rs`, `#[serde(skip)]`, `ts(skip)`) carries the note in memory. `recall.rs` calls `neighbour::apply` after both `fill` sites (the run and `refill`); `turn/recall_step.rs::hold_node` joins it into the header with `neighbour::with_note`.
- The rule, the same as the subject bar's (`Floor::other_subject`): the question's entities that some candidate holds (`asked`, the union of `matched`); a chunk is a neighbour when it holds none of them and one of the type of an asked one. It does not depend on the floor or bar being on.
- Example, before: `a message in the CLI, 2026-10-01 (as of @7), session cd34ef "Staging"`; after: `... session cd34ef "Staging", about host db-stage.example.test, not the asked host db-prod.example.test`. A chunk that holds the asked entity, another entity type, or a question that named no entity: header byte-identical to the base.
- Compaction: a compacted session's recall goes through the same `hold_node`; a note's header is frozen when it is written and `render::render` prints that header, so the note is the same bytes in every later request and after a compaction. A recall assembled into a prefix (`recall_id`) reads the same items.

## 4. Tests

- `recall::neighbour::tests::a_same_kind_other_subject_is_named` and `the_neighbours_header_names_its_subject_and_the_askeds_is_unchanged` (new; the second asserts the asked subject's header is unchanged and the neighbour's carries the subject).
- P2 planted (the note dropped from `with_note`): `the_neighbours_header_names_its_subject_and_the_askeds_is_unchanged` FAILED (panicked at neighbour.rs:145); file restored and touched; `git status` shows only my changes.
- Test 1 (pin new defaults) and P1: **not applicable**, the defaults did not change. The existing test `the_floor_and_the_subject_bar_are_shares_of_the_best_score` is untouched and still asserts 0.0.
- Test 3: `bench/recall/precision.py` unchanged, so no new Python test.
- Not tested: the field in a live daemon (no entity-bearing question in the bench; see 9).

## 5. Gate

- `cargo fmt --check`: ok. `scripts/shape.sh`: ok. `cargo clippy -p theseus-core -p theseus-protocol --all-targets -q -- -D warnings`: clean. `scripts/recall-tests.sh`: exit 0.
- `TZ=America/Phoenix cargo nextest run -p theseus-core -p theseus-protocol -p theseus-memory -p theseus-exam -E 'test(recall)|test(memory)|test(neighbour)|test(config)|test(precision)|test(cockpit)|test(layouts)|test(registry)'`: 469 run, 469 passed (includes the protocol's generated-types test).
- The full `scripts/gate.sh` could not run to its suite: its door and keel phases judge this tree against origin/main, which has moved well past `ac298666`, and fail on commits that are not mine (the persona_cmd.rs test moves and keel-guard.py edits on main, plus acks whose signatures do not verify here). I ran the later phases' relevant parts by hand as above. I did not run the whole workspace suite, benches, or `cargo deny`.

## 6. Keel

`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`: no output, exit 0. No test removed or weakened; no golden edited.

## 7. DM golden

`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no diff.

## 8. Join notes

Files touched: `crates/theseus-core/src/recall/neighbour.rs` (new), `crates/theseus-core/src/recall.rs` (+5: `pub mod neighbour;`, two `apply` calls, one `marks`, the `neighbour: None` literal field), `crates/theseus-core/src/turn/recall_step.rs` (+1 call in `hold_node`), `crates/theseus-protocol/src/memory.rs` (+5, the skip field on `RecallItem`). `recall/render.rs` and `crates/theseus-core/AGENTS.md` are **not** touched, so the join with `p3a-recall-now` has no hunk there. The maintainer should add a paragraph on `neighbour.rs` to core's AGENTS.md at the join. `p3a-recall-now` constructing `RecallItem` literals must add the `neighbour` field or use `..Default::default()`.

## 9. What M5 should run

A recall-bench Theseus-only run at the default 0/0 plus this branch (header note on); it will show no change on the bench's abstentions unless the questions name an entity. To test the floor itself with a model, run one arm at `recall_floor = 0.9` (only point that moved distractors in the proxy) and read both misleading answers and recall accuracy; expect an accuracy cost of about 3.5 points.

## 10. Follow-ups

- The index has no "subject of a statement" entity (a service, a job), so the neighbour note cannot name "the cedar relay" for a port fact. A subject extractor in the memory pass would let the same function name those subjects (the call sites need no change).
- `bench/recall/precision.py` counts only whether the pack admits the value; the 28/60 figure needs the model's answer. A cheap model-in-the-loop abstention check on the stand-in is not possible (the stand-in is a perfect arm).
- Environment: the first full `cargo build --workspace --all-targets` filled the VM's disk allowance (debug, 19 GB); I rebuilt the three binaries with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.

## 11. Data floor

Nothing new is sent to a model or routed anywhere; the note is a few words inside the recalled item's header, in the same note and place as before.
