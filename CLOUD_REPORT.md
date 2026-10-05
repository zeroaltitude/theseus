# Cloud report: spreading activation as a recall source, and the `+activation` arm (theseus-6fn.12)

Branch `cloud/20261005-activation`, from `main` at 3085f71 (with the task commit 2759b8f). Started 03:00 UTC,
report written 06:30 UTC. Four steps, each its own green commit; this report is the last commit.

| Step | Commit | What |
|---|---|---|
| 1. The projection | 3cf7429 | `recall/adjacency.rs`: the graph the spread walks, folded from the record |
| 2. The arm | 2da6314 | `+activation`: the science, the seam, the spread before the pipeline, `memory search --arm` |
| 3. Surfaces | 9b0be7b | the manifest's share, the `recall.activate` span, health's memory block, two metrics |
| 4. The exam | b3f9abc | `Arm::Activation`, its daemon, the `+activation − baseline` pair; the arm's tests hardened for load |

## Differences from the design (the code and AGENTS.md won)

- **Edges have no weight in the store.** The weight comes from the EDGE's kind and route (`adjacency::mapped`):
  `derived_from` via `report`, `brief`, `publish`, `arrangement`, `claim`, and also `glide` (38b, on main but not in
  the brief's list) and `graduate` (19c's, still read), weigh 0.6; via `recall` maps to the `Recall` kind, which
  weighs 0 by construction (`EdgeWeights::weight`), not by data. A route this build does not know (31b's `synthesis`,
  or an empty route on a pre-hco compilation edge) spreads nothing and is counted `unmapped` (health shows it).
  `supersedes` (from the newer node to the older) is 1.0 toward the newer, 0.2 back. `contradicts` is not written by
  anything yet, so nothing maps to it.
- **Entities.** Read from each node's `memory.labeled` row (`about`). A node the memory pass never labeled (a store
  from before 31a, the exam's written past) has **none**: I left them out rather than ask the tender after serving.
  The new node is not labeled until after its turn, so its seed edges are its projection edges (its neighbour) plus
  **the query's entities as the index's hits matched them** (`IndexHit.entities_matched`): the tender is asked
  nothing more inside the deadline. Any query entity no hit holds is in no node the entity source found, so little is
  lost. A search has no new node: its query seeds with those entities alone (key `?query`).
- **A `Recall` node is no one's neighbour** in the position chain: otherwise a message and its reply would be two
  hops apart through the recall between them, and exposure would relay activation. Its `derived_from` edges are kept
  (as `Recall`, weight 0).
- **A common entity's bound.** An entity in more than `cap` nodes is not expanded at a spread. `cap` is computed from
  the spread's numbers (`adjacency::cap`): the largest df whose one edge carries a 1.0 seed over the threshold, 1,095 at
  the defaults. What it changes: activation that two or more such entities (or one such entity plus another path)
  would have summed over the threshold is lost; for example two nodes sharing two entities each in 2,000 nodes would
  get 2 × 0.7 × 0.1316 = 0.18 unbounded, and nothing bounded. The math's own tests (theseus-memory) are unchanged.
- **Additions skip what the turn already holds.** The at-most-20 nodes the index did not return are the strongest
  reached ones that are not in the turn's context (`in_context`, as the scene reads it), not at or after `as_of`, and
  of a kind the index indexes (no tool call, arrangement, or recall). Leaving in-context nodes out is not a filter
  bypass (the `in_context` filter would drop them anyway); it keeps the turn's own neighbours from taking the 20 slots.
  Every other filter, the place rule first, runs on them as on any candidate.
- **The fusion term** is the tender's weighted reciprocal rank: `weight / (60 + rank)` with weight 1 (as each of the
  tender's sources), in `Activated`'s data and digest. A hit the spread reached gains it once per chunk; a seed never
  gains it (the spread never returns a seed).
- **The projection is built** after serving when `[memory] arm = "+activation"` and the mode is canary or live
  (`Core::warm_activation`, from theseusd's after-serving list), or by the first search that names the arm, within
  its 2 s deadline. A turn that finds it unbuilt starts the build and goes on (`building`). It is **kept current at
  each spread** (`Projection::refresh` folds the NODE, EDGE and `memory.labeled` records written since, by kind), not
  by a hook on every write: a refresh is a few index reads inside the spread's blocking task.
- **The arms' seam**: `MemoryArm::Activation` (serde `"+activation"`, `baseline`'s sources, `MemoryArm::named`),
  `Memory::science_for(arm)` (a match: `baseline` for every other arm), `Scene.science` (read by `manifest_ranked`,
  `refill` and the rerank's `Recalled`), `Scene.activation`, `MemorySearchParams.arm`. 31b and 32a add arms beside it:
  their merges add a variant and a match arm each.
- **The cockpit** has no Memory view on main (no `.tsx` reads `RecallManifest`, `memory.search` or health's memory
  block); only its generated types changed. The design's "Observatory: a Memory section" has no successor yet.
- **The recall in front of the model runs on the heap** (`Box::pin(self.recall_live(..))`): with the arm's state, the
  turn's future overflowed the 2 MB test thread in `tests_output::the_cores_output_matches_its_golden` (it passes
  with 16 MB; main passes at 2 MB). Boxing it brings the turn's future back under. Worth knowing: the turn's future
  was within a few hundred bytes of that limit before this step.

## Step 1: the projection (3cf7429)

**Found.** EDGEs are `{kind, from, to, via, at_ms}`, scoped `in:<to>`; positions come from the NODE records in WAL
order; `memory.labeled` rows are LEDGER rows tagged `k:memory.labeled` (the page index), with a fallback scan of the
ledger while the index's shape is built.

**Changed.** `crates/theseus-core/src/recall/adjacency.rs` (new): `Projection` (interned node ids, each node's edges
and entities, each entity's nodes, each session's last node, unpaired tool calls and results), `build`, `refresh`,
`view` (the `Adjacency<String>` a spread walks, expanding shared entities from the entity's list), `cap`, `mapped`,
`stats` (nodes, edges, entities, memberships, unmapped, an estimate of bytes, the position it holds through).

**Proved.** `tests_activation.rs`, 4 tests: each edge on a small store (positions, the tool pair, each of six routes,
an unknown route counted and spreading nothing, `same_entity` both ways, `supersedes` both ways, shared entities with
df 2 and 3); a node reachable only through a `Recall` node never activated (from the recall, from the reply, and
from the recall's former neighbour); a projection kept current three records at a time equal to one built whole
(every node's edges and every node's spread, bit for bit); the common entity's bound (1,096 vs 1,095 nodes).
Planted reverts, each restored with `touch` and `git status` checked:
- `recall` route mapped to `DerivedFrom` (a recall's edge given weight): `the_projection_holds_each_edge_on_a_small_store`
  and `a_node_reachable_only_through_a_recall_is_never_activated` failed.
- `supersedes` walked toward the older node at 1.0 (ToNewer and ToOlder swapped):
  `the_projection_holds_each_edge_on_a_small_store` failed.

## Step 2: the arm (2da6314)

**Changed.** theseus-memory `activated.rs` (new): `Activated`, `baseline` but for `activate` (which calls `spread`
with its `SpreadParams`, at most the budget given), plus `weight`, `seeds` (10), `adds` (20), `term`, and a digest
over all of them (`activation@<16 hex>`). theseus-core `recall/activation.rs` (new): `Adjacent` (the projection behind
a mutex, built flag, stats for health), `Memory::activated` (seeds, the spread on the blocking pool bounded by what is
left of the index's deadline, the term and `sources.activation` on reached hits, additions read from the store);
`recall.rs` (the seam, `Begun.new_node`, the activation's share of what was admitted in `fill`); `turn/recall_step.rs`
and `turn/rerank_step.rs` (the scene's science by arm; the spread before the pipeline in `recall_live`);
`rpc/memory.rs` (`memory.search`'s `arm`, `Core::warm_activation`); config (`MemoryArm::Activation`, the template's
line); protocol (`RecallActivation`, `RecallManifest.activation`, `MemorySearchParams.arm`, generated TS); CLI
(`theseus memory search --arm`); theseusd (the after-serving call).

**Proved.** `tests_activation_arm.rs` (through the whole core, the index a stand-in): B ("Commit 3f9a2c1 also raised
the retry limit to 9."), reached only through the commit it shares with A, is admitted under `+activation` with an
activation rank of 1, a score of 0.7 / ln 3 and no index rank, its words reach the model; under `baseline` it is
never a candidate; a search with `arm: "+activation"` has it and with `"baseline"` lacks it; `+retention` and `none`
are refused. A turn never waits for the build (`building`), and the next turn spreads. An index that answers as the
deadline ends leaves the spread nothing (`deadline`) and the turn goes on with the index's answer. A shadow turn
under the arm sends the same request bytes as memory off, and builds nothing. The place property test over generated
stores with the arm on (16 cases; the index finds one note, activation reaches the rest through a shared entity):
every note it adds from a session the turn may not draw on is dropped for `place`, every other passes that filter,
and the model never sees one it may not draw on. Existing recall, rerank and memory-arm tests unchanged and green.
Planted revert: added nodes read as `Place::Private` in `manifest_ranked` (activated nodes let past the place filter):
`the_place_rule_holds_for_what_activation_adds` failed ("Cli asked of AliceDm").

## Step 3: surfaces (9b0be7b)

**Changed.** The manifest's `sources` count `activation` and each item carries `sources.activation` (rank and the
activation it holds): JSON in the row, no format bump. `theseus memory search --arm +activation` prints "activation
ranked N of the M admitted, K found by it alone · from S seeds reached R (B of the index's hits, A added) · T ms over
X nodes, Y edges", or "activation did not run (building): …". A `recall.activate` span inside `recall` (after the
index's stages, with outcome and counts); the recall span's `activation` attribute; the narrative line's clause.
Health: `HealthResult.memory` (`MemoryHealth`: mode, arm, and `AdjacencyHealth`: state, nodes, edges, entities,
unmapped, bytes, through) and `theseus health`'s line `memory: live · arm +activation · adjacency 1200 nodes, 3400
edges, 80 entities, 3.0 MB, through @9876`. Metrics: `theseus.recall.activate_ms` (histogram by outcome) and
`theseus.recall.activated` (by stage: `added`, `admitted`). theseus-protocol's lib.rs went to 2,709 lines for the
health field: its ceiling in `scripts/long-files.txt` is raised, with the reason.

**Proved.** `tests_activation_arm::the_trace_and_health_show_the_spread` (the span, its attributes, the narrative,
health's block after a turn; none of it under `baseline`), `telemetry/tests_recall.rs` (the two metrics through the
OTLP receiver), the CLI's line tests in `render/memory.rs`.

## Step 4: the exam (b3f9abc)

**Changed.** `Arm::Activation` (`"+activation"`, its own daemon), `daemons_for` names it, the report's `ARMS` and the
pair `+activation − baseline`. `--arms` keeps its default (`none,bm25,baseline,oracle`); `+activation` is opt-in.
The integration test `tests/arms.rs` now runs every arm, a real `+activation` daemon among them: its rows name
`activation@…`, it admits the gold and passes both items both runs. Also in this commit: the arm's core tests no
longer depend on the machine's speed (their recall deadline is the longest, 5 s; the build wait 90 s; the bound
test's labels in one frame), after a run under load showed two of them hit the 250 ms default.

**What items would show activation.** In the exam's written store no node is labeled (no memory pass ran over its
past) and no EDGE is written, so activation can only spread by position (0.3) and tool pairs (0.8): it adds the
neighbours and results of the index's top hits. Items whose answer sits next to what the query matches would show
it: an **episode** (the failure the query names in a tool result, its fix in the next message), a **fact** asked
in one turn and answered in the reply, a **procedure** whose command is the tool call beside its result. Shared
entities need labels: an exam store whose fixture writes `memory.labeled` rows (or one run through the memory pass)
would add **items linked only by an entity** (the live check's commit), and **superseded** items need the
`supersedes` EDGE written (with it, the older node spreads 1.0 to the correction). `docs/m6-ablation-plan.md`'s
next version would add: the `+activation` arm and its pair against `baseline` at the same budget, a feature row
("activation (`+activation` against `baseline`)") in the decision table (I did not add it to `FEATURES`, since the
plan names the features), and those item families. The replay (`theseus-exam replay`) does not replay `+activation`:
it would need the projection folded only up to each turn's position.

## Edits outside my own modules (small, for the merge)

- `crates/theseus/src/main.rs` (the CLI's, a shared file): `memory search --arm`, three lines.
- `crates/theseusd/src/main.rs`: `core.warm_activation()` in the after-serving list, beside `warm_labels`.
- `crates/theseus-protocol/src/lib.rs` (at its ceiling): `HealthResult.memory` and its doc; the ceiling raised to
  2,709 in `scripts/long-files.txt`, with the reason. `ts.rs`: the new types on the existing memory line.
- `crates/theseus/src/render.rs` (near its ceiling): one call, `memory::push_health`.
- `crates/theseus-core/src/turn/rerank_step.rs`: its science only (the scene's, by arm), and the two new arguments.
- `crates/theseus-core/src/telemetry/tests.rs`: three helpers made `pub(super)` for `telemetry/tests_recall.rs`.
- `crates/theseus-memory/src/science.rs`: `Baseline::canonical` and `fnv1a` made `pub(crate)` for `Activated`'s
  digest; nothing else of the math changed.
- After step 2's gate I set the new place property test's `failure_persistence: None` (a run had left a
  `proptest-regressions/` directory); fmt, clippy and the test ran again, not the whole gate.

## The live check (the maintainer's)

A scratch daemon on a fresh state dir, with `theseus-index` beside `theseusd`, `[index] weights_dir` an empty
directory, Discord and the web off, a GLM key or `theseus-sim fake-model --rules`:

```sh
D=$(mktemp -d); mkdir -p $D/state $D/weights
cat > $D/theseus.toml <<'EOF'
# … the scratch daemon's usual [server]/[model]/[profiles] lines, socket and state in $D …
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = true
weights_dir = "<D>/weights"
[memory]
mode = "live"
arm = "+activation"
EOF
theseusd --config $D/theseus.toml --socket $D/s.sock --state-dir $D/state &
export THESEUS_SOCKET=$D/s.sock
theseus health                                   # memory: live · arm +activation · adjacency … nodes, … edges
theseus ask "The Kestrel relay was fixed by commit 3f9a2c1."          # session A
theseus ask "Commit 3f9a2c1 also raised the retry limit to 9."        # session B
sleep 5                                           # the memory pass labels A and B (commit:3f9a2c1)
theseus --json ask "What fixed the Kestrel relay?" | jq -r .session_id   # session C
theseus memory recalled <C>
```

1. `theseus memory recalled <C>` shows the recall with `arm +activation`, science `activation@…`, the line
   "activation ranked … found by it alone ≥ 1", and B's message admitted with `(activation #N)` and no `bm25`/`entity`
   rank; A admitted by the index.
2. `theseus memory search --arm baseline "What fixed the Kestrel relay?"` lacks B;
   `theseus memory search --arm +activation "What fixed the Kestrel relay?"` has it, with its activation line.
3. `theseus health` names the projection's nodes and edges (and its entities, at least the commit).
4. `theseus --json health | jq .memory` gives the same as JSON; the turn's trace (`theseus history --json` or the
   ledger's `turn.trace`) holds a `recall.activate` span.

If B is missing in step 1, `theseus memory recalled <C> --json` says why in `activation.outcome` (`building` on a
first turn after the start, before the build finished: ask C again; `deadline` if the index took the whole 250 ms).

## What is left, uncertain, or for the owner

- **Unlabeled nodes have no entities.** Eddie's store from before 31a and the exam's written past get position and
  tool-pair edges only. Asking the tender for every unlabeled node's entities after serving (batched, at nice) is
  the obvious next step if the exam should measure shared entities.
- **No hook on writes.** The projection folds what was written since at each spread, under the projection's lock, on
  the blocking pool. A long gap between spreads (a busy daemon whose sessions run under another arm) means a bigger
  fold at the next spread; it is bounded by the spread's deadline (the turn goes on, `deadline`), and the fold finishes
  in the background for the next one.
- **RSS.** `stats().bytes` is an estimate (interned ids, edges at 8 bytes, entity memberships at 4, the maps with
  slack). The lifecycle bench does not configure the arm, so §9's RSS rows do not include it; a bench row with the arm
  and a large store is still to add.
- **The cap's choice** (an entity past 1,095 nodes is not expanded) loses sums of small contributions; see above.
- **`in_context` additions are skipped** before the filters (see above): the owner may prefer them counted as drops.
- **Docs for the maintainer**: the spec's Part III item for 32b's wire-in, `docs/status.md` (the arm, health's memory
  line), design m6-memory.md §2.7 (the routes table: `glide`, `claim`, `arrangement`; the unknown route; the cap;
  entities from `memory.labeled`; `Recall` nodes out of the position chain) and §2.13 (health's `memory` block,
  the two metrics), and `docs/m6-ablation-plan.md` (above).

## The gate

Every commit's gate was `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (incremental
off: see below). Each failed only in its suite phase, only on the known L1 cases, and I ran the phases after the suite
by hand (`protocol types`, `bench turn --check --runs 5 --burst 0`, `cargo deny --offline check`), all passing.

| Commit | fmt, shape, features, clippy, cockpit, test build, reader rule | suite | after the suite |
|---|---|---|---|
| 3cf7429 | pass | 2,490 run: 2,457 passed, 33 failed (L1), 1 flaky (`theseus-sim the_kernel_holds_its_invariants_under_seeded_faults`, on the flaky list, passed its 2nd try) | protocol types ok; turn bench plain 5 frames, tool-call 9 (budgets 5, 9); deny ok |
| 2da6314 | pass | 2,499 run: 2,466 passed, 33 failed (L1) | the same |
| 9b0be7b | pass | 2,502 run: 2,469 passed, 33 failed (L1) | the same |
| b3f9abc | pass | 2,503 run: 2,470 passed, 33 failed (L1), 1 flaky (the same sim test, passed its 3rd try) | the same |

The 33 failures, each run: theseus-sandbox's contract tests (19), its bench's `spawn_100`, and theseusd's `sandbox`
tests (13): L1 refuses a root daemon's job without a job cgroup on this VM (theseus-pv6i). Nothing else failed.

**On this VM, and worth knowing:**
- **Disk.** The first gate of step 2 filled the session's disk: `target/debug/incremental` had grown to 17 GB, and
  the suite failed 91 tests and timed one out, the job tests first (the disk had 0 bytes free when I looked). I deleted the incremental
  directory (rebuildable) and ran every later gate and test with `CARGO_INCREMENTAL=0`.
- **The golden test's stack.** Step 2's first clean gate failed `tests_output::the_cores_output_matches_its_golden`
  with a stack overflow (its whole turn runs on the test thread's 2 MB stack). It passes with `RUST_MIN_STACK` at 16 MB,
  and passes at 2 MB on main; boxing the recall in front of the model fixed it (step 2). Its golden lines did not move.
- **Under load** (four `while :; do :; done` loops at nice 0 beside the test at nice 19, killed by their pids): the
  arm's tests, `tests_activation_arm::` and `tests_activation::` (11 tests), passed 5 runs of 5 after step 4's
  hardening (each run about 65 s). Before it, two of them failed in one run of five on the 250 ms default deadline,
  and the bound test (then 2,191 frames of labels) passed nextest's 120 s kill in every run. Two existing tests,
  `tests_recall::a_stalled_index_never_holds_the_turn` and
  `tests_recall_node::a_stalled_index_holds_a_canary_turn_no_longer_than_its_deadline`, fail under that load in every
  run, and fail the same way on main (2759b8f) under the same load (both failed in each of the three runs whose output I kept): the load starves a 4-core VM, not a change of mine.
  Neither is on the flaky list; they pass in every gate.
- `cargo deny fetch` worked, so the deny phase ran with a fresh advisory database.
