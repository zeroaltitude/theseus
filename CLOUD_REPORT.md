# Cloud report: the memory pass, attribution, and `memory.v1` in shadow, step 31a (theseus-6fn.6)

Branch `cloud/20261004-memory-pass`, from `8a9d4ee` (main at `760553f` plus the task commit). Started 10:03 UTC,
report written 13:13 UTC. All six steps are built, each its own commit, plus a fix the scratch daemon found and one more test.

| Commit | Step |
|---|---|
| `18d94da` | `index.entities`: the extractor's rules asked of the tender (part of step 2) |
| `441d9ef` | 1 and 2: the pass after each turn, off its path, and its table-driven labeler |
| `ae58b54` | 3: the gate, `same_entity` and `supersedes` edges over `index.neighbours` |
| `ed0a3e1` | 4: attribution, `memory.used` rows with their outcome |
| `eb08cbd` | 5: `baseline`'s second version reads the edges and prefers the newer node |
| `9456a4f` | 6: `memory.v1` and `attribution.v1` in shadow through `JudgeService` |
| `4aaaf0a` | fix: the gate reads the index's `mode`, not its `state` (found on a scratch daemon) |
| `e79c9f6` | test: the two new builders in the judge crate's huge-input cap test |

How the gate ran here: `TZ=America/New_York THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before every commit. The TZ is
because `tests_output::the_cores_output_matches_its_golden` pins a wake's UTC offset sign (`-#:#`); in this VM's UTC
it reads `+#:#` and fails on `main` too (it passes under any western zone). Every phase passed but the suite, and the
suite failed only on the root VM's known sandbox tests (theseus-pv6i: 19 `theseus-sandbox::contract`, 1
`theseus-sandbox::bench`, 13 to 15 `theseusd::sandbox`); I ran the phases after it by hand each time (`protocol types`:
the generated TypeScript staged; `deny`: `cargo deny --offline check`, all ok; the benches separately, below). Details
at the end.

---

## 1 and 2. The pass and the labeler (`441d9ef`, with `18d94da`)

**Found.** `JudgeService::after_turn` is called once, at the end of `TurnRunner`'s turn, after the session hold drops.
The index's entity rules are `theseus-index/src/entity.rs` (regex); the core has no regex and the tender never links
the core.

**Changed.**
- `crates/theseus-core/src/memory_pass/` (new): `MemoryPass`, a field of `TurnRunner` built with the core (it reads and
  starts nothing until a turn ends with memory on); `turn.rs` gains only the field and the call
  `self.pass.after_turn(res)` beside the judge's. The call queues the session on the pass's task and returns.
- Eligible (`memory_pass::eligible`): `UserMessage`, `AssistantMessage`, `ToolResult` with text, origin not `harness`.
  Never `Recall` (the exclusion is on the body, whoever wrote it), never a `ToolCall`. 30c's `Summary` is not a `Body`
  variant on main yet: add it to `eligible` and `shape_of` at its merge.
- The labeler (`memory_pass/labels.rs`): tables `KIND_RULES` (decision, then preference, operator only), `ACKS`
  (transient), `INSTRUCTIONS` (procedure: a code block with an instruction), `FACT_WORDS`, `CORRECTIONS` (the
  operator's corrections, which the gate reads), `VOLATILE_WORDS`, `COUNTED`; durability by kind and author; trust is
  DD5's `external` (`external` or `own`); `about` is the entity field.
- **One extractor: the tender's** (`18d94da`). A new tender method, `index.entities { texts }` (at most 256), runs each
  text through `entity::entities`. The pass asks it once a pass for every text it needs. No copy of the rules in the
  core. Without a tender, rows say `entities_unavailable` and why.
- Rows: `memory.labeled`, keyed by node, **scoped `memory:<session>`**, not the design's `memory`: 30b's label set
  (`recall/labels.rs`) scans the whole `memory` scope, and a session's next pass reads what it did with one scan of its
  own scope. Say this in §2.8's table.
- Frames: the pass's own, one per 32 nodes or 2 s after the first waits, a node's records never split; before each
  frame the WAL must be still for 500 ms (at most 60 s). That keeps the frames out of a measured turn: the turn
  bench's turns are about 50 ms apart, so the pass waits until the bench's run ends.
- A crash: what is done is read back from the rows, once a session per daemon; nothing scans on the start path.

**Proved.** `memory_pass::tests::only_the_eligible_are_labeled_and_a_recall_never_is`,
`a_labeled_row_carries_the_tables_labels_and_the_indexs_entities`, `at_most_one_frame_per_32_nodes_or_2_seconds` (paused
clock: nothing at 1.9 s, one frame at 2.4 s; 70 nodes as 32, 32, then 6 when the window runs out),
`with_memory_off_the_pass_does_nothing`, `labels::tests::the_labelers_rules_follow_the_table` (17 rows),
`volatile_values_are_named`, `trust_and_about_come_from_the_node`, and theseus-index's
`the_socket_names_the_entities_of_texts_with_the_one_extractor`.
- **Planted revert:** `eligible` with `Body::Recall { .. } => true`: the recursion test failed (`a recall is never
  labeled`). Restored, touched, green. A first plant that only moved `Recall` into the eligible arm did not fail: the
  empty-text guard also keeps a recall out (`text_of` of a recall is empty). The test does not rely on that.

## 3. The gate (`ae58b54`)

**Changed.** `index.neighbours` with `k = 10` and `as_of` = the node's position, so only earlier nodes; neighbours
limited to the kinds the pass labels and to other turns (a reply restating its own question is not a duplicate).
`MemoryScience::gate` decides; its thresholds are the science's data (`Baseline.merge_cosine` 0.92,
`supersede_cosine` 0.75), now readable as `gate_thresholds` for the row. Edges are 12a's EDGE with `via = "memory"`:
`same_entity` from the new node to the duplicate, `supersedes` from the correction to the older node, scoped into the
older. `memory.gated` (keyed by node, beside its labels) holds the decision, the neighbours with cosines, the science
id, the thresholds, or why it did not run. No new field: no `MANIFEST_FORMAT` bump (new edge *names* in the existing
record; an older build reads them as unknown, as `reach.rs` already handles).

**Waiting for vectors: wait within a bound, then leave the node for the next pass.** A refused neighbours call is
asked again after 250 ms, doubling, for at most 4 s a pass (all its nodes together); a node still without a vector is
left whole, labels included, for its session's next pass. An index with no vectors at all (`mode = "bm25_only"`) or no
tender: the row says `unavailable` and why, no edge, the node labeled.

`node.reach` reads the two kinds and follows neither (a likeness is not a copy).

**Proved.** `the_gates_thresholds_make_the_right_edges` (0.95 merges; a correction at 0.80 supersedes; a correction at
0.70 and a plain node at 0.80 store), `a_node_not_yet_embedded_waits_and_an_index_without_vectors_is_said`.

## 4. Attribution (`ed0a3e1`)

**Changed.** `memory_pass/attribution.rs` (pure) and `recalls.rs`. For each item of a `Recall` node of an ended turn:
`used` if one of its entities (the tender's, of the excerpt the model saw: the source over its frozen range) is among
the reply's or a call input's, or 8 of its words in a row are in the reply. The row says which (`by`). Outcome, at the
pass after the session's next input: `corrected` when the operator's next message is a correction (the labeler's rule)
overlapping the item (a shared entity, or 2 content words of 4+ letters or with digits), `ok` for any other operator
message, `unknown` when the next input is not the operator's (a wake, a task's report). An unused item is written at
once with `outcome: null` (exposure is no review); a used one waits for the next input. Rows: `memory.used`, scoped
`recall:<session>`, one per item.

**Proved.** `an_item_is_used_by_an_entity_or_a_run_of_its_words` (and seven words are not a run),
`a_used_items_outcome_follows_the_next_message`, `recalled_items_are_attributed_and_their_outcome_follows`.

## 5. `baseline`'s next version (`eb08cbd`)

**Changed.** `Baseline.version`: 2 is the default and `prefers_newer`; 1 keeps 30a's parameter line and its digest
(`baseline@46038939f14a4f49`, pinned in a test). Version 2's digest differs, and every recall row's `science` names it.
In `theseus_memory::recall`, after the filters (place first, unchanged) and before the rank: the older side of a
`supersedes` is dropped as `superseded`, and every node of a `same_entity` group but its newest as `duplicate` (chains
resolve), each only when the newer node is a kept candidate or already in context. The links reach the pipeline in
`Asker.links`; the core reads them from the EDGE records into each candidate (`recall::links`, scope `in:<id>`), and
only for a science that reads them. Two new drop reasons, with narrative words.

**Proved.** `each_filter_drops_with_its_reason` (every reason met, the two new ones too), `the_newer_node_is_preferred`
(supersedes, a chain kept to its newest, an absent newer node drops nothing, version 1 reads no link),
`the_id_names_the_parameter_set`, and through a whole core
`a_correction_supersedes_the_fact_and_recall_prefers_it` (the live check's story, offline). Recall's place property
tests still pass (`recall::tests::the_place_rule_holds_for_every_pack`,
`tests_recall::the_place_rule_holds_over_generated_stores`).
- **Planted revert:** `prefers_newer` returning `false`: `the_newer_node_is_preferred` failed (`["old", "a", "b", "c",
  "new", "lone"]` against `["c", "new", "lone"]`), so did `each_filter_drops_with_its_reason` and the core test (3
  notes admitted, not 2). Restored, touched, green.

## 6. `memory.v1` and `attribution.v1` in shadow (`9456a4f`)

**Changed.**
- `crates/theseus-judge/packs/memory.v1.toml` (`kind` Choice over the labeler's kinds with `other` as no-match;
  `corrects_earlier` and `volatile` Nouls) and `attribution.v1.toml` (one `relied_on` Noul per note, at most 6).
- **Point: a new one, `memory_pass`** (after a turn ends, off its path); neither `loop_end` nor `exchange_end` is when
  the pass runs. **Builders: `memory` and `attribution`** (`builders/memory.rs`). New closed-set variants:
  `Baseline::Rules` (the pass's deterministic half) and `Source::Notes`.
- States hold only what the session's model saw: `memory` has the node's text as stored (a result as shown, scrubbed
  and capped), its role, a result's tool, and the message before it; `attribution` has the operator's message, the
  reply, and each note's excerpt as rendered. The core's scrubber runs on every state (`ScrubWith`).
- `crates/theseus-core/src/judge/memory.rs`: `JudgeService::at_memory_pass`, two `WIRED` lines in shadow, sampled by
  node or recall id (`[judge.packs."memory.v1"] sample`), reserved from the shadow budget, recorded by the sink
  (`judge:memory`, `judge:attribution`). No trace marks (no turn waits). `[judge]` off or a pack off: nothing asked.
  The pass asks `memory.v1` for each node it labels, and `attribution.v1` once per recall, at the pass after its own
  turn.
- Tests that pinned health's pack list now name all three (`tests_judge.rs`, `theseusd/tests/judge.rs`); a small
  `rig_with_secrets` beside `tests_recall::rig_with`.

**Proved.** The judge crate's loader rules on all ten packs, the builders' golden states and requests (new goldens
`memory.state.json`, `attribution.state.json`, `memory.v1.request.json`, `attribution.v1.request.json`), and with the
fake Jev: `tests_jev::the_pass_asks_memory_v1_and_attribution_v1_in_shadow` (one `memory.v1` per node labeled, one
`attribution.v1` with a Noul per note, answered, priced, shadow, states holding what the model saw, no key in them;
health lists both) and `without_the_judge_the_deterministic_half_runs_alone` (judge off, or both sampled at 0: no
call, labels written).

This commit also fixes a race in step 5's core test: B's neighbours were set after the turn had already handed B to
the pass. The stand-in now refuses unknown nodes as the tender does before it embeds, so the gate waits for them.

## The fix the scratch daemon found (`4aaaf0a`)

A scratch daemon of this build, with the real tender and no model files, wrote no `memory.labeled` or `memory.gated`
row. The gate read the index's `state` (`ready`), but `bm25_only` is its `mode`, so every refusal looked like "not
embedded yet": each pass waited 4 s and left every node. Now `PassIndex::mode` reads `IndexStatus.mode`.
`tests_tender::the_memory_pass_reads_the_tenders_mode` asks the adapter against tests_tender's stand-in (ready,
`bm25_only`). **Planted revert:** reading `state` again failed it (`"ready"` against `"bm25_only"`); restored, green.

Rerun of the scratch daemon (`/tmp/scratch`, fresh state dir, the fake model `theseus-sim fake-model`, Discord and web
off, `[memory] mode = "live"`, `arm = "baseline"`, `recall_deadline_ms = 2000`, `[index] weights_dir` empty): every
node labeled; a message naming `crates/larkspur/src/main.rs at d069c4c` got `about =
[commit:d069c4c, crate:larkspur, file:main.rs, path:crates/larkspur/src/main.rs]`, volatile `commit`; each gate row
`unavailable: the index answers BM25 alone …`; a question's live recall admitted four notes, and after the next
message `memory.used` rows said `used: true, by: [run], outcome: ok` for the three the reply quoted and `used: false`
for the 8081 note. No supersedes edge without vectors, as designed. Stopped with `theseus shutdown`, the fake model by
its pid.

## Benches and load

- `a_plain_turn_stays_within_its_frame_budget`: passes in every gate (its rig has memory off). With memory on,
  `tests_recall::shadow_writes_no_frame_and_changes_no_request_byte` and the pass's own frame test hold it.
- `target/debug/theseus-sim bench turn --check --runs 20 --burst 0`, three runs on the final builds: plain 5 frames
  every run (budget 5), tool-call 9 every run (budget 9). The bench config keeps `[memory]` in shadow, so the pass
  runs in it; no pass row appears in any measured turn's frames.
- Under load (four busy loops at nice 0, the tests at nice 19; loops killed by pid): the memory and recall tests
  (`test(memory_pass) or test(/recall/) or package(theseus-memory) or test(tests_tender) or test(the_place_rule)`, 94
  tests): 5 of 5 runs green, 94 of 94 each.
- Under the same load, the judge side (`test(tests_jev) or test(tests_judge) or package(theseus-judge)`, 126 tests),
  3 runs: both `tests_jev` tests passed every run. Failing under this load, and not mine: the judge crate's timing
  assertions `builders::security2::tests::the_builder_is_fast` and
  `builders::tests::every_builder_stays_under_its_cap_on_huge_inputs` (its `took < 500 ms` on the existing packs; it
  ran before `e79c9f6` added mine) in all 3 runs, and `tests_judge::a_tiny_day_limit_pauses_shadow_with_one_row` once.
  All pass unloaded, and in every gate.

## The live check (the maintainer's)

A scratch daemon on a fresh state dir; Discord and the web UI off; the tender with model files; a GLM key; optionally
Jev's key. Config note, sparse (with the vault's or env secrets as you run them):

```toml
[server]
state_dir = "/tmp/m6-31a/state"
socket = "/tmp/m6-31a/sock"
[discord]
enabled = false
[web]
enabled = false
[model]
live = "glm"
[index]
weights_dir = "~/.cache/theseus/models"
[memory]
mode = "live"
arm = "baseline"
recall_deadline_ms = 2000   # a debug build
[judge]
enabled = true              # step 3 only, with jev_api_key in [secrets]
```

```sh
T="theseus --socket /tmp/m6-31a/sock"
theseusd --config /tmp/m6-31a/theseus.toml &          # note its pid
$T health | grep '^index'                                # ready · hybrid, once the model loads
A=$($T --json ask "The staging port of the Larkspur service is 8081." | jq -r .session_id)
sleep 3                                                  # the tender embeds A's nodes
B=$($T --json ask "Correction: Larkspur's staging port is 8082, not 8081." | jq -r .session_id)
sleep 8                                                  # the pass: window 2 s, still WAL, vectors
$T --json ledger -n 500 --kind memory.gated | jq '.rows[].data | {node_id, decision, to, neighbours}'
```
1. B's correction (its `user_message`) shows `decision: "supersedes"`, `to` = A's message node, its top neighbour at
   0.75 or more. If it says `store` with a top cosine under 0.75, the threshold needs calibrating for Nomic (§2.3 says
   thresholds are per model): note the cosine. If B's node is missing, its vector was late: send B another message
   and look again (the next pass takes it). `theseus --json reach <A's node>` lists no copy for it (a likeness).

```sh
C=$($T --json ask "What is Larkspur's staging port?" | jq -r .session_id)
$T memory recalled $C        # B's correction admitted; A's 8081 note dropped as superseded (or below it)
$T ask -s $C "Thanks."
sleep 6
$T --json ledger -n 500 --kind memory.used | jq '.rows[] | select(.session_id=="'$C'") | .data'
```
2. The reply says 8082; `memory recalled` shows B admitted and A `superseded`. The `memory.used` row for B's item says
   `used: true` (`by` names an entity or `run`), `outcome: "ok"`.
3. With `[judge] enabled` and the key: `$T judge log` lists `memory.v1` (one per node labeled) and `attribution.v1`
   (one for C's recall) with their costs; `$T health` shows `memory.v1: shadow`, `attribution.v1: shadow`.

Stop it with `$T shutdown`.

## What is left, uncertain, or for the owner

- **Thresholds on Nomic.** 0.92 and 0.75 are the design's starting points; nothing here measured Nomic's cosines for a
  correction against its fact. The live check's step 1 is the first reading.
- **The 4 s vector wait** is a guess at the tender's embed lag after a frame; a node later than that waits for its
  session's next pass, so a session's last turn may stay ungated until it speaks again. A sweep after serving could
  take such nodes; the task said never on the start path, so none is built.
- **Attribution's outcome needs the next input.** An exchange that ends never writes its used items' rows. A pass at
  an execution's end (or at the next day's report) could write them `unknown`; not built.
- **The pass's quiet rule.** It yields to turns by waiting for 500 ms of WAL silence, at most 60 s. A daemon busy for
  over a minute gets the frame anyway. It is what keeps the frames out of the turn bench's measured turns, and is
  simpler than knowing when turns run; the owner may prefer an explicit "no turn in flight" signal.
- **`memory.labeled` and `memory.gated` are scoped `memory:<session>`**, not `memory` (above). 25c's nightly report
  reads them by kind, so it is unaffected; say it in §2.8.
- **The labeler's rules are deliberately small** (word tables, no model). `memory.v1`'s shadow rows are there to show
  where they miss.
- **`index.entities`** is a new tender method: the tender binary must be the branch's (installed beside `theseusd`);
  an older tender answers "no method", and the rows say `entities_unavailable`.
- **The cockpit's Memory view** should show, per session: each node's labels (kind, durability, volatile with why,
  trust, `about` as chips), the gate's decision with its neighbours and cosines and any edge (a link to the other
  node, and `superseded`/`duplicate` drops in recall's list), and each recall's items with `used`, `by`, and outcome;
  beside them, `memory.v1`'s and `attribution.v1`'s answers from `judge.call`, so a disagreement is one glance. And
  per daemon: counts of supersedes and same_entity edges, gated `unavailable` by reason, and the share of recalled
  items used.
- **Docs to change (the maintainer's):** design m6 §2.6 (eligibility as built, the wait and the next pass, the quiet
  rule, attribution's `unknown` rule), §2.8 (the scopes; EDGE with `via` and no `{weight, by}` payload; the two new drop
  reasons), §3.1 31a's row (the new point `memory_pass`, builders `memory` and `attribution`, baseline `rules`);
  m5's pack list (two packs); crates/theseus-core/AGENTS.md (a "memory pass" bullet under Recall:
  `memory_pass/`, `judge/memory.rs`, `fact/memory.rs`), crates/theseus-memory/AGENTS.md (`superseded` and
  `duplicate` in the filter order; `Baseline.version`), and Part III's item. docs/status.md's roadmap row 57.
- Merge notes: 32c's `+rerank` adds a `WIRED` line, a pack and a builder beside mine (keep both); health's pack-list
  assertions in `tests_judge.rs` and `theseusd/tests/judge.rs` then need its line too. 30c's `Summary` joins
  `eligible`. `Point`, `Builder`, `Baseline`, `Source` gained variants other lanes may also add to.

## The gate's result

Final code head `e79c9f6` (gated, as was every commit before it): every phase passed (fmt, shape, features, clippy, cockpit, test build, reader rule) but the
suite, which failed only on the root VM's sandbox tests (theseus-pv6i). Then `protocol types` ok and `deny` ok
(advisories, bans, licences, sources). Benches skipped by `THESEUS_GATE_NO_BENCH`; the turn bench ran separately (above).

Other failures seen across the thirteen gate runs, none in code this branch touches:
- `tests_output::the_cores_output_matches_its_golden` under UTC (the golden pins a negative UTC offset; passes under
  `TZ=America/New_York`, which every counted run used).
- `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` failed all three tries once (C6's second run):
  on the flaky list (theseus-81ig: "no series was put back", a coverage count under load); all invariants held; the
  kernel sim does not link the core; it passed alone under UTC and New York and in every other gate.
- `theseusd::bench_profile theseusd_check_passes_on_the_bench_profile_with_no_vault` failed once (C5's gate): its L1
  self-test said "a workspace root, /, is the root"; passed 3 of 3 alone; it runs `theseusd check`, no turn or pass.
- One C6 run had dozens of job tests fail at once: the disk was full (18 GB of `target/debug/incremental`). I removed
  that directory (rebuildable, inside the repo's `target/`), and the rerun was clean.
- `tests_judge::a_jev_that_is_down_opens_the_breaker_and_later_judgments_skip` failed once in a targeted run beside
  other tests; it passed 4 of 4 alone, and in every gate.
