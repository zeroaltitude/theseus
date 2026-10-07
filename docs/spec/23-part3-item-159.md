# The Ship of Theseus, chapter 23: Part III, A4's Items 159 to 170 ([index](README.md))
### Item 159. Activation: spreading activation over an adjacency projection as one more recall source, under the `+activation` arm (theseus-6fn.12; step 32b's wire-in, roadmap row 59; the sixth cloud batch's activation session, fired 2026-10-04 20:00 from 3085f71a, Opus 5.5; 3cf74295, 2da63147, 9b0be7bb and b3f9abc2; reviewed 00:53 to 01:54 by local reviewer R5, stack M, on retention's review merge; joined 03:01 at dca30347, a signed merge onto a9442c81, by the stack-M joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** M6's second arm (§5.5a): spreading activation, the weighted spread over typed edges that `theseus-memory`'s
math already had, put into recall as one more ranked source beside the index's BM25, entities and vectors, so that a
node the index did not return but a neighbour of what it did return can be recalled. Like retention (Item 157),
it is an arm, read only by a daemon or a search that names it.

**What landed** (46 files, +2,809 −83 without the paperwork; the merge 42 files, +2,794 −130; no new package; no store
format change).
- **The projection** (3cf74295; theseus-core `recall/adjacency.rs`, new, 475 lines), folded from the record and never
  stored: interned node ids, each node's edges and entities, each entity's nodes, each session's last node, unpaired tool
  calls and results. Edges have no weight in the store: the weight comes from the EDGE's kind and route
  (`adjacency::mapped`). Neighbouring turns by position weigh 0.3; a tool call and its result 0.8; `derived_from` via
  `report`, `brief`, `publish`, `arrangement`, `claim`, `glide` (38b) and `graduate` (19c) 0.6; `same_entity` 1.0;
  `supersedes` 1.0 toward the newer node and 0.2 back; via `recall` it maps to the `Recall` kind, which weighs 0 by
  construction, not by data, so exposure never spreads activation. A `Recall` node is no one's neighbour in the position
  chain (else a message and its reply would be two hops apart through it). A route this build does not know (31b's
  `synthesis`, or an empty route on an old compilation edge) spreads nothing and is counted `unmapped`. Entities are
  read from each node's `memory.labeled` row (`about`); a node the memory pass never labeled has none. An entity in more
  than `cap` nodes is not expanded at a spread; `cap` is computed from the spread's own numbers, the largest df whose one
  edge carries a 1.0 seed over the threshold: 1,095 at the defaults.
- **The arm** (2da63147). theseus-memory `activated.rs`: `Activated`, `baseline` in every verb but `activate`, its
  numbers (the weight, 10 seeds, 20 additions, the fusion term) in its digest `activation@<16 hex>`. theseus-core
  `recall/activation.rs` (new, 456 lines): `Adjacent`, the projection behind a mutex, built after serving by
  `Core::warm_activation` when `[memory] arm = "+activation"` in canary or live mode, or by the first search that names
  the arm; and `Memory::activated`, the spread before the pipeline, on the blocking pool, bounded by what is left of the
  index's deadline (`deadline` past it). The seeds are the new node's projection edges plus the query's entities as the
  index's hits matched them (`IndexHit.entities_matched`), so the tender is asked nothing more inside the deadline. A
  reached hit gains the tender's weighted reciprocal rank term, `weight / (60 + rank)` with weight 1, once per chunk; at
  most 20 of the strongest reached nodes the index did not return join the candidates, never one the turn already holds,
  none at or after `as_of`, none of a kind the index does not index, and every filter, the place rule first, runs on
  them. The projection is kept current at each spread (`refresh` folds the NODE, EDGE and `memory.labeled` records written
  since, by kind), not by a hook on every write. A turn that finds it unbuilt starts the build and goes on (`building`).
  The seam is retention's: `MemoryArm::Activation`, `science_for`, `Scene.science`, plus `Scene.activation`,
  `Begun.new_node` and `MemoryArm::named`.
- **The recall in front of the model runs on the heap** (`Box::pin(self.recall_live(..))`): with the arm's state, the
  turn's future overflowed the 2 MB test thread in the core's golden test, and it had been within a few hundred bytes of
  that limit before.
- **Surfaces** (9b0be7bb). The manifest's `sources` count `activation`, and each item carries `sources.activation` (its
  rank and the activation it holds), JSON in the row. `theseus memory search --arm +activation` prints "activation ranked
  N of the M admitted, K found by it alone · from S seeds reached R (B of the index's hits, A added) · T ms over X nodes,
  Y edges", or why it did not run. A `recall.activate` span inside `recall`; the narrative line's clause; health's
  `memory` block gains `AdjacencyHealth` (state, nodes, edges, entities, unmapped, bytes, the position it holds through),
  printed `memory: live · arm +activation · adjacency 1200 nodes, 3400 edges, 80 entities, 3.0 MB, through @9876`; and two
  metrics, `theseus.recall.activate_ms` (by outcome) and `theseus.recall.activated` (by stage: added, admitted).
- **The exam** (b3f9abc2): `Arm::Activation` on its own daemon and the pair `+activation − baseline`, opt-in, not in
  `--arms`' default; the arm's core tests no longer depend on the machine's speed (a 5 s recall deadline in them, a 90 s
  build wait) after a run under load showed two of them hit the 250 ms default.

**How it is proven.**
- **The session's tests.** `tests_activation.rs` (4): each edge on a small store (positions, the tool pair, six routes,
  an unknown route counted and spreading nothing, `same_entity` and `supersedes` both ways, shared entities at df 2 and
  3); a node reachable only through a `Recall` node never activated; a projection kept current three records at a time
  equal to one built whole, bit for bit; the common entity's bound (1,096 against 1,095 nodes). `tests_activation_arm.rs`
  (7, through the whole core with a stand-in index): a note reached only through the commit it shares with another is
  admitted under `+activation` with activation rank 1, a score of 0.7 / ln 3 and no index rank, and its words reach the
  model, while under `baseline` it is never a candidate; a turn never waits for the build; an index answering as the
  deadline ends leaves the spread nothing; a shadow turn under the arm sends the same bytes as memory off and builds
  nothing; the place property test with the arm on (16 cases): every note activation adds from a session the turn may
  not draw on is dropped for `place`, and the model never sees one; the trace and health show the spread. Under the load
  recipe the arm's 11 tests passed 5 runs of 5 after step 4's hardening. The session's planted reverts (a recall's edge
  given weight; `supersedes` walked toward the older node; added nodes let past the place filter) each failed its test.
- **R5's review** on retention's review merge (d4f795d1 on 1176bdb8): 352 of 352 in 75 s, the golden turn at the default
  2 MB stack. Four new plants: a turn building an unbuilt projection inline, not caught, but a second, unreachable guard
  (the first is held); no entity cap, caught in 90 s; a shadow daemon building the projection, caught in 144 s; the
  nodes the turn holds taking the addition slots, **not caught**: theseus-syxg below.
- **Live, at the review** (the stand-in model, no spend, BM25 and entities, `mode = "live"`, `arm = "+activation"`): the
  projection built after serving; C's recall named `activation@9983d3470d9d1227` and "activation ranked 1 of the 2
  admitted, 1 found by it alone · from 2 seeds reached 1 (0 of the index's hits, 1 added) · 0.4 ms over 6 nodes, 8
  edges": A's reply, reached by position from A's message, admitted as `(activation #1)` with no index rank; 1.5 ms of
  recall each. Health's one `memory` line carried the adjacency, and its JSON both projections. The report's
  shared-entity story did not show live, and not for this branch's reason: the memory pass labeled B's message
  `about: ["commit:3f9a2c1"]` and A's `about: []`, so A and B shared no entity; the suite holds the shared-entity
  spread on a stand-in index.

**The join** (stack M's second; reviewed 01:54, and its review's addendum at 02:16 after retention's join). The joiner
locked at 02:17:02 and queued behind lane files' second join and lane speed, dry-running on each main while it waited.
Since retention's join made `INSTRUMENTS` 28, the metrics.rs conflict lost the line `resolve.py` expected, so the
joiner wrote `metrics-pre.py` (the same keep-both) to run before it. The merge at 02:37:20 on a9442c81: 22 conflicted
files, rerere replaying 20 of R5's resolutions; `resolve.py` (50 blocks, every one the seam both arms built word for
word, keep-both with retention's line first; long-files.txt takes main's line with no raise, theseus-protocol's
`lib.rs` at main's 2,717) and then `joinfix.py`'s seven fixes, all R5's: one health `memory` block carrying both
projections (one `HealthResult.memory` field, one `Core::memory_health`, one `memory:` line); one CLI line;
`MemoryArm::named` listing `+retention` (merged clean, and wrong: without it a `+retention` search was refused); the
exam's counts (`Arm::ALL` 6, `PAIRS` 7) and the tests' arm names; ts.rs naming `MemoryHealth` once; the CLI's
`recall_lines` split into `arm_lines` under clippy's 100-line limit (103 merged); and `INSTRUMENTS` counted, 30. No new
join fix. The staged tree equalled its dry run on a9442c81 file for file. Before the gate, at the default 2 MiB stack:
R5's filter 353 of 353, and the rest of the core and the CLI 1,095 of 1,095, no overflow. The signed merge dca30347
(a9442c81 and 43544a9b) at 02:52:03, 42 files, +2,794 −130. Its gate (02:52:12 to 03:00:17; 150 s waiting for the lock
behind two review steps): 2,624 of 2,624 (19 skipped); lifecycle ok on the first run, cold start p95 22.7 ms; the plain
turn 5 frames at p50 75.6 ms and the tool-call turn 9 at 158.1, inside the recent spread (the gate's turn bench runs
memory in shadow, so it measures only `Begun.new_node`'s scan, not the live turn's walks or the arm's spread); resident
memory 92.9 MB after the burst. Restore (no budget) read p50 168.6 and p95 350.5 ms against about 141 and 155 at the
three gates before, its copy phase slower on a slower disk (the WAL's cold read 423 MB/s against 515 to 638, fdatasync
12.3 ms): not attributed, since activation does not touch restore. Pushed; the branch deleted on origin; the done line
at 03:01:22; theseus-6fn.12 closed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change and no new key: `+activation` is a value of retention's
`[memory] arm`, off unless named. Health's one `memory:` line carries both projections; two metrics and a span are new. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** Edges carry no stored weight; routes map to weights in code. Entities come from the memory pass's
labels, not from the tender. The projection is refreshed at each spread rather than hooked on writes. An entity past
the computed cap is not expanded (a design choice M6 did not state). Additions skip what the turn already holds.

**Known gaps.** theseus-syxg (P2): nothing holds the rule that additions skip what the turn already holds; without it,
on a long session the turn's neighbours would take the 20 slots and activation would add fewer unseen nodes, silently
(the code is right). theseus-3edq (P2, filed at the join): the projection's build walks every page with no PSI pace;
the page walk is shared with `refresh`, which runs inside a turn's deadline and must never wait, so the build needs a
pace flag of its own before `+activation` meets the owner's store. Under any live arm a turn walks its kept transcript twice
for `in_context` (folding them is a cleanup). Unlabeled nodes (a store from before 31a, the exam's written past) have no
entities, so there activation spreads by position and tool pairs only; asking the tender for their entities after
serving is the next step if the exam should measure shared entities. The cap loses sums of small contributions through
common entities. The lifecycle bench does not configure the arm, so §9's RSS rows do not include it. The replay does
not recompute `+activation`. `via = "synthesis"` spreads nothing, rightly, but counts as `unmapped`; mapping it to zero
explicitly would keep that count honest (optional). R5's recommendations for the owner, each how the code works now: keep
the cap; keep skipping in-context additions.

### Item 160. Consolidation: co-recalled clusters into cited syntheses, Jev's `citation.v1` in shadow, the `+synthesis` arm, and a private place that draws on every session (theseus-6fn.10, with theseus-1is6; step 31b, roadmap row 57; the sixth cloud batch's consolidation session, fired 2026-10-04 20:00 from 3085f71a, Opus 5.5; c0b9535c, 94618d81, b5ce8698 and 90856ea6; reviewed 01:56 to 02:36 by local reviewer R5, stack M, on activation's review merge; joined 03:50 at fd9235d0, a signed merge onto dca30347, by the stack-M joiner; store format 18; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** §5.4 keeps one part of "dreaming": a memory job that clusters nodes the ledger shows are recalled together,
has a model propose one synthesis per cluster, has Jev check every sentence against its sources, and stores the
accepted ones as `Synthesis` nodes, scored in shadow and shown only through a bounded canary. Step 31b builds it as
M6's third arm, `+synthesis`, after retention (Item 157) and activation (Item 159). The place
rule's half, theseus-1is6 (P2), was found by the batch-6 task writer on 2026-10-04 and given to this session: M6's
§2.15 and the place rule the owner approved on 2026-10-03 (Item 76) say a private place draws on any session, but
recall's `Place::may_draw_on` (30a) admitted only private places' sessions to a private place.

**What landed** (68 files, +3,594 −111 beside the cloud files; the merge 69 files, +3,572 −116; no new package). Steps
1 to 6 of the task landed as one commit (94618d81), with the place rule's before it and two follow-ups found in review
and in the planted reverts after it.
- **The place rule** (c0b9535c, theseus-1is6): `Place::may_draw_on` is `(Private, _) => true`, so a private place
  draws on every session; a shared place still draws only on its own. Both property tests (the pipeline's and the
  rerank's) now assert both directions.
- **The node.** `Body::Synthesis { text, sources, check, stage, cluster, profile, model, cost_usd }`, origin `agent`,
  author `consolidation`, id `syn_…`; `CitationCheck` (`Unchecked { why }` or `Supported { least, judgment, mode }`)
  and `Stage` (`Shadow` or `Arm`). Its doc says it is an encyclopedia entry, never a procedure. It lives in a harness
  session opened at the first kept synthesis (its META key `memory.session`), which has no target and so reads private,
  with `derived_from` edges to each source `via = "synthesis"`, so `theseus reach <source>` lists it. The compiler
  renders nothing for it, and the memory pass never labels it (90856ea6). **Store format 16 → 17 on the branch** for the
  new body, which replaces no layout, so no old-layout sample is owed.
- **Clusters** (`theseus-memory/src/consolidate.rs`, pure): node pairs admitted together in at least 3 distinct turns,
  joined by union-find into components of 3 to 8 nodes; none whose digest a `synthesis.proposed` row with text already
  holds, and no synthesis or recall node among the sources. The run reads the newest 5,000 `recall.shadow` and
  `recall.ran` rows by kind through the store's pages, so shadow recall counts and there is data before any canary.
- **The run** (`consolidate/run.rs`, `tender.rs`). `memory.consolidate { dry_run? }`, one of the owner's runs (judged
  as `JudgeRun`), and `theseus memory consolidate [--dry-run]`; a nightly tender at `[memory] consolidate_hour` (4),
  started after serving only with memory on, the learning tender's pattern (a missed night once, never within 10
  minutes of a start). The plan runs on a `learning` thread at nice 19, paced to about 5 % of a core; the calls and the
  frames' waits are tasks on the runtime (b5ce8698: the first version's `block_on` panicked on a stop during a wait).
  The profile is `synth_profile = "session"` by default: the profile every source's session last used, and sources
  that disagree wait (`profiles_disagree`). A cluster with external text is never sent. The request asks for at most 120
  words, every sentence ending with its notes' numbers, the sources clipped to 1,500 characters, `max_tokens` 600. The
  call is reserved at the catalog's prices and settled at its usage, as the audit's are, under `synth_limit_usd_per_day`
  (0.50, summed from the day's rows, so a restart keeps it); a failed call is booked at its reservation and its cluster
  left for the next run. Each kept synthesis's node, edges and rows are one frame, written between turns through the
  memory pass's handshake: `Turns.writing` became a count of writers, so the pass's frame ending never uncounts
  consolidation's. Rows, scoped `memory`: `synthesis.proposed`, `synthesis.checked` (`supported`, `unchecked` or
  `rejected`, the least probability, the pack's mode, the unsupported pairs) and `synthesis.scored`. A rejected
  synthesis is rows only, no node.
- **Jev's check**: `citation.v1`, a new pack in shadow (`Builder::Citation`, one Noul per sentence and cited source,
  "does the source support what the sentence says?", at most twenty pairs). The deterministic checks come first (not
  empty, at most 120 words, every sentence cites, every cited number a source), and a failure rejects before Jev is
  asked; any pair under 0.5 rejects; every pair answered at 0.5 or more is `supported`; the judge off, its budget spent
  or a pair unanswered leaves it `unchecked`, admitted by no arm. A shadow pack's verdict does qualify a synthesis for
  `+synthesis`, since that arm is itself the operator's experiment; the node records the pack's mode.
- **Shadow scores.** For each recall row that admitted two or more of a synthesis's sources: would the arm have
  selected it, and at what rank. Rows keep no query, so it is scored as its best admitted source, ranked just ahead of
  it (`basis: "best_admitted_source"`).
- **The `+synthesis` arm.** `MemoryArm::Synthesis`; `science_for` gives `WithSyntheses { base, checked }` (baseline's
  every verb, id `baseline+synthesis@<baseline's digest>`); `MemoryScience::synthesis(node)` is the seam the
  pipeline's filter reads, dropping a synthesis as `arm` (an arm that admits none) or `unchecked`, after `recursion` and
  before `threshold`, with the place filter still first. Every other arm leaves the harness session out before the
  index's top k (`Memory::begin(arm)`'s `exclude_sessions`). The checked set is read once per daemon and kept.

**How it is proven.**
- **The session's tests:** `tests_consolidate.rs`, 13 whole-core tests (690 lines): a cluster becomes one checked
  synthesis, and a dry run changes neither the WAL's last position nor the model's requests; external text never
  synthesized; spend stops at the day's cap with nothing sent; disagreeing sessions wait; a failed call leaves its
  cluster; no frame lands inside a turn (a counted turn held open: nothing written for 1.5 s, written after); Jev
  rejecting an unsupported sentence (`unsupported: ["s2:2"]`); an uncited sentence rejected before Jev; without Jev,
  `unchecked`; a live `+synthesis` core admitting a checked synthesis while a shared place's turn drops it for its place;
  and nothing shown in shadow (a shadow turn's request equal before and after a synthesis is stored). Six planted
  reverts, each failing its tests; the first frame plant passed at first, because the session-opening wait masked the
  synthesis frame's, and b5ce8698 split the test so each wait has its own. Under the load recipe, 9 timing tests 5 runs
  of 5.
- **R5's review** on activation's review merge: 618 of 618 after join fix 6 (617 before it), the golden turn at the
  default 2 MB stack among them; **4 of 4** new planted reverts caught: the first writer to finish uncounting every
  writer, the day's cap unchecked, an external cluster synthesized, and the harness session left among the index's top
  k for an arm that admits no synthesis.
- **Live, at the review.** Keyless, on the stand-in model: three sessions stated a relay's port, log path and nightly
  restart, three asked about it; `memory consolidate --dry-run` found one cluster in 3 turns; `memory consolidate` wrote
  the cited entry `unchecked` ("the judge or citation.v1 is off") for $0.00024 with its three rows; `reach` listed it;
  under `+synthesis` a search dropped it for `unchecked`, and `baseline` never listed it. **With GLM and Jev**:
  glm-5.3-flash headed its entry with a title ("Kestrel relay. The Kestrel relay is a service that listens on port 7714
  [1]. …"), the deterministic check rejected it as "sentence 1 cites no source" before Jev was asked ($0.000162), and the
  cluster is never proposed again: **theseus-8edz** (P2). A first try, with the op CLI missing from the unit's PATH,
  showed the failure path live: booked at its reservation, its row with no text, the cluster left. That phase spent
  cents at most.

**The join** (stack M's third). Task-board's join held the queue at 02:42 and was expected to take format 18, so the
joiner previewed that case and wrote `format-hand.py` (a main past another bump: base 16, main 18, branch 17, which
`resolve.py`'s `renumber()` stops on). Task-board's gate went red at 03:23 and it was parked (Item 163),
so main stayed at 17. Locked at 03:07:11; the merge at 03:32:20 on dca30347: 21 conflicts, rerere replaying 19; then
`format-hand.py` (a no-op on this main), `resolve.py` (theseus-store's `store.rs`, where both sides had bumped from 16,
keeping main's note and putting 31b's after it; long-files.txt), and `joinfix.py`'s six fixes, all R5's: `hit_of` and
`Memory::activated` take `Synthesis` and `+synthesis` (E0004); `MemoryArm::named` lists `+synthesis`; **the nightly run
is background work**, on `on_idle_thread` (`SCHED_IDLE`) after a PSI wait (`pressure::quiet_blocking_unless`), as
linux-io's (Item 151) nightly learning run is, while the owner's `memory consolidate` keeps
`on_low_thread`; `Asker.retention` in 31b's two pipeline tests; `Memory::begin`'s exclusion covering every arm; and
activation's place property test reading the new place rule. **The format renumbered at the join to main's plus one,
18**: the constant and its note, the write's pin in the core's store test, theseusd's `versions.rs` (writes 18, refuses
19, reads formats 2 to 18) and the core's AGENTS.md line. The test strings matter most, since lane files' own bump had
changed them to 17 and git merged them clean and silently wrong. The staged tree equalled the dry run on dca30347 file
for file. The signed merge fd9235d0 (dca30347 and c0b0ad0b) at 03:43, 69 files, +3,572 −116. The warm 4 m 11 s;
theseus-protocol's tests 30 of 30 with protocol.gen unchanged; R5's filter 623 of 623 and the rest of the core, the CLI
and theseus-store 1,050 of 1,050, at the default stack. Its gate (03:43:22 to 03:49:24; 47 s waiting for the lock):
2,646 of 2,646 (19 skipped); lifecycle ok on the first run, cold start p50 21.9 and p95 29.5 ms, the slowest of its
runs against a budget of 50 + 7, read as one noisy run (the tender starts after serving); restore back at 138.0 and
150.9, which supports reading activation's as that gate's slower disk; the L1 start p50 6.20 ms; the plain turn 5 frames
at p50 75.8 ms and the tool-call turn 9 at 159.2; resident memory 90.7 MB after the burst. Pushed; the branch deleted;
the done line at 03:50:01; theseus-6fn.10 and theseus-1is6 closed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). Install #4 moved the owner's store from format 16 to 20 at its first write (18 is this join's),
after the install's backup. Three new keys in `[memory]`: `synth_profile = "session"`, `synth_limit_usd_per_day = 0.50`, `consolidate_hour =
4`; `+synthesis` is off unless `[memory] arm` names it, and no other arm puts a synthesis before a model. With the owner's
memory mode live, the nightly run would spend by default, up to $0.50 a day, losing for good every cluster the model
titles until theseus-8edz is fixed; so install #4 set `[memory] synth_limit_usd_per_day = 0` on his config, by his 12:03
"Take your recommendation" (morning notes sections 39 and 42), and consolidation's writer stays off until 8edz lands. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The harness session is `memory.session`, not M6's `sys:memory`. A synthesis carries no label meet:
labels are gone, and its place is its harness session's, private. `synth_profile` defaults to `session`, not `glm`.
The score's basis is the best admitted source, since rows keep no query. The memory pass's writer flag became a count
rather than routing consolidation through the pass's queue. `JudgeRun` was reused for the run's gate rather than a new
act. Six steps landed as one commit.

**Known gaps.** theseus-8edz (P2): an entry headed by a title is rejected as uncited, and its cluster is never proposed
again. An unchecked synthesis is never checked again (its cluster's digest is in a row), so a daemon that gets Jev later
keeps its old ones unchecked; a re-check pass in the nightly run is the next step. The memory pass may still link a later
message to a synthesis (`same_entity`), and `baseline` would then drop the synthesis as a duplicate under `+synthesis`;
excluding the harness session from `index.neighbours` closes it. A checked synthesis is an eligible rerank candidate, so
rerank.v1 may see its text (private, as its sources). The exam has no recall rows to cluster, so `+synthesis` there needs
a run under `baseline` first and Jev. The cockpit has no Memory view and lists no synthesis. R5's recommendations for
the owner, each how the code works now: keep a shadow verdict qualifying for `+synthesis`, but require a live one before it
is ever a canary's arm; keep `session` and watch `profiles_disagree`; keep the $0.50 cap and the hour 4.

### Item 161. Detour-recall: a trivial detour sends no recall, writes none, and its row says `detoured` (theseus-n7nc, lane context-honesty's fix B; the `detour-recall` lane, a subagent of the DM thread, Opus 5.5, 2026-10-05 02:47 to 03:58, from origin/main; 969c099f and 9cb5af90, with `main` merged in at 4a36a685; joined by the lane at 03:50 at 62de744f, a signed merge onto fd9235d0; reviewed 04:20 to 04:22 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Lane context-honesty (Item 156) found it and filed it as its fix B: after a "thank
you!" that route.v1 judged trivial and detoured to the small model, the next reply carried the note the greeting's
recall had found, a turn late, read as part of the greeting. On the first loop `recall_first` runs before the compile;
under a live memory mode `recall_live` packs, puts the `Recall` node in `t.recall.pending` with its record and edges in
`t.recall.rides` and the note count in `t.recall.count`, and announces the `recall.ran` row at once. On `trivial` the
detour recompiles with empty sources, so its request has no note, but `call_model` took the rides anyway. So the
`Recall` node and its `derived_from` edge were written in the plan frame; the reply's footer (`🧠 N recalled`) counted
notes the model never saw; the next turn rendered the node in its tail, merged into the greeting's user message; and
that node put the note "in context", so the next turn's own recall dropped it as `in_context`. From the code, not
tested: the memory pass would also have attributed the node's items (`memory.used` rows, which retention folds) for
notes the model never saw. Lane speed (Item 158) made it common: every greeting now detours.

**What landed** (969c099f, in the files fix B names; 9cb5af90, three lines of theseus-core's AGENTS.md).
- `turn/route_step.rs` (+45 −4): `RouteState::deciding()`, true when the inbound point asked route.v1 (`wait` set) or
  the first compile is reading its verdict (`defer_persist`). `compile_routed`'s routed first loop moved, line for line,
  into `compile_first`; `compile_routed` calls it and then `recall_routed(t)`, so that runs once on every exit: the
  first compile kept, a switch's compile, a detour's, a failure, a fault.
- `turn/recall_step.rs` (+49 −2): `Recalled.held`, the `recall.ran` rows of recalls made while `deciding()`.
  `recall_live` holds its row then, and records it at once otherwise, as before. `recall_routed`, on a detour
  (`t.route.keeps` set): takes the pending node, finds the held row by its `recall_id`, sets `outcome = "detoured"` and
  `why` ("route.v1 judged the message trivial, and the detour's request carries no recall"), and clears `rides`,
  `drops` and `count`; then records every held row. A held row with no pending node (nothing admitted, `paused`,
  `deadline`) keeps its outcome, since the detour dropped nothing the model would have seen.
- `fact/recall.rs` (+20): the narrative's `detoured` line, "Recall (live) admitted 3 notes (1,140 tokens), but the turn
  took a trivial detour, whose request carries no recall: none reached the model."
- **Why the row waits rather than being written twice.** It used to be announced before the compile, so by the time
  the detour was decided it sat in the store's waiting rows, and a second `recall.ran` for the same recall would list it
  twice in `memory.recalls` and count the turn twice in the exam's replay. A turn whose inbound point asked route.v1
  nothing (the judge or route.v1 off, a continuation) records the row exactly as before; one that asked it holds the row
  and records it unchanged unless the turn detoured, in the same frame as before (the plan frame), after `route.decided`
  and `loop.started` instead of before them. A compaction's recall inside the first compile is held too.
- **No store format change.** `RecallManifest.outcome` is a string, and `detoured` a new value of it, as `paused` was
  at 30b; an older build reads such a row without error.
- `turn.rs` is unchanged (3,511 of its 3,523 lines): its `take` finds no rides after a detour. The rest of
  context-honesty's small-talk design (its A, C and D) was left: recall still runs on small talk.

**How it is proven.** Three tests in `tests_route.rs`, on its rig (the fake Jev scripting route.v1's mode, two fake
providers, `[memory] mode = "live"` with a stand-in index answering one invented note for every message, the verdict's
wait at 5 s so load never makes one late; no live model, no Jev): **a trivial detour writes no recall and its row says
`detoured`** ("thank you!" judged trivial at 0.95: no note in the request, `recalled` 0, no `Recall` node, one row,
`detoured`, 1 admitted); **after a detour the next request carries no stale note**; and **a routed turn still writes and
sends its recall** (`sophisticated`, a switch to Opus compiled again on its profile, and `chat`, the first compile kept:
each sends the note after the message, counts 1, writes one node and one `ran` row). Plus the narrative line's unit
test. Four planted reverts (`revert.py`, each restored byte for byte after): **A**, the whole fix reverted, failed tests
1 and 2, and its request showed the incident's shape, the greeting's first user message holding `[{"text": "thank
you!"}, {"text": "[Recalled by the harness: 1 note from earlier sessions, …"}]`; **B**, an over-broad fix dropping every
routed first compile's recall, failed tests 2 and 3; **C**, held rows never recorded, failed 1 and 3; **D**, nothing
held, failed test 1 on `ran`, not `detoured`. A targeted run of 78 theseus-core tests (route, recall, recall node, memory
arm, rerank, retention, compaction, activation) passed. The lane gate (03:18:31 to 03:29:36, at nice 19): 2,627 of
2,627; the plain turn 5 frames (p50 75.0 ms), the tool-call 9 (158.2).

**The join** (by the lane, under `lane-detour-recall-join`, taken 03:29:53 behind task-board's and consolidation's
locks). Activation's join (dca30347, which changed `recall_live`'s start and the recall fact's span) was merged into the
lane as 4a36a685 with no conflict. Task-board's gate went red at 03:23 and it did not join; consolidation joined at
fd9235d0, and two seconds after its done line, at 03:50:03, `watch-join.sh` made the guarded merge: `git merge --no-ff
-S` of `lane/detour-recall` (9cb5af90) onto fd9235d0, **62de744f**, no conflict (dry runs with `git merge-tree` showed
none on dca30347 or fd9235d0, and none added against any queued branch). Shape ok. The warm 1 m 30 s, clippy clean. The
gate (03:52:34 to 03:57:48; the lock after 6 s): 2,649 of 2,649 (19 skipped); lifecycle p95 cold start 21.0 ms, from the
config copy 35.4, clean shutdown 51.4, SIGKILL and restart 27.5, binary swap 51.9, each within its budget; the L1 start
p95 5.78 ms; the turn bench's frames unchanged, plain 5 (p50 77.6 ms) and tool-call 9 (p50 164.8). Pushed; the worktree,
branch and target removed (the lane branch was never pushed); the done line at 03:58:28; theseus-n7nc closed. At the
review (04:20 to 04:22) the DM thread checked origin/main and its parents, the gate's exit, the four reverts' logs (each
"fix restored byte for byte: True") and the code: `compile_routed` calls `compile_first` and then `recall_routed` on every
return, Ok or Err. Not checked: a turn whose future is dropped inside `compile_first` would lose its held row; before, it
was deferred to the next frame, so likely no worse.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No config and no store change. After a "thank you!" that detours to the small model, the
next reply no longer carries the note the greeting's recall found, and the footer's "N recalled" counts only notes the
model saw. It ships with lane speed, since every greeting now detours. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** None from fix B. The narrative line is an addition, so the narrative never says "admitted 3" for notes
the model never saw.

**Known gaps.** The protocol's doc comment on `RecallManifest.outcome` lists `ran`, `deadline` and `unavailable`, and
omits `paused` and now `detoured`; adding them regenerates the cockpit's `RecallManifest.ts` (left for a code change). Lane
route-gaps (theseus-d13v, not yet joined) holds the first compile's `context.compiled` and `loop.started` rows the same
way, until the call uses them; the two holds could share one flush point. Lane situations (35a) filters earlier `Recall`
nodes out of a detour's request, the other half of the same idea.

### Item 162. Tiering: payload stubs and a bounded heat cache, so a turn decodes only what it reads (theseus-6fn.13; step 33, roadmap row 61; the sixth cloud batch's tiering session, fired 2026-10-04 18:08 from d5a4b808, Opus 5.5; 34c8dfd8, 5492bb43 and 719d6f79; reviewed 02:29 to 03:50 by local reviewer R5, stack M, on consolidation's review merge, its planted reverts run again at 03:07 on a rebuilt target; joined 04:43 at 23f132f7, a signed merge onto 62de744f, by the stack-M joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** There was no arena and no cache across turns: every turn decoded every node of its session, payload and all,
however long the session (M6's design note, §2.10). At 1.4 MB there was nothing to demote to cold storage; the real
cost was per-turn decoding in long sessions. The session measured it first, on main's debug build: a turn on a
2,000-node session took a wall p50 of 326.5 ms, and the daemon held 200.1 MB after ten such turns. Step 33's thin slice
was stubs, a bounded heat cache, and `decay_sweep`'s hints ordering eviction, with bench rows to measure them; S3-cold
segments and the presence filter wait for a store that needs them.

**What landed** (45 files, +2,204 −185 at the join; three code commits).
- **The bench rows first** (34c8dfd8, theseus-sim only): `bench turn --session-nodes N [--result-bytes B]` writes one
  session of N nodes before the daemon starts and measures turns in it (wall time by both clocks, frames, nodes decoded
  per turn from health's count, "not counted" on a build without one, and memory with the index tender's); `bench idle
  --active N` runs a turn in N sessions after the parked window. History columns `turn_long`, `decodes_long`,
  `rss_long`, `rss_tender`, `rss_active`.
- **The heat cache** (5492bb43, `theseus-core/src/node_cache.rs`): `Arc<Node>` by WAL position, one per store, shared
  by every handle, and every node read goes through it (a turn's transcript, the nodes its frames write,
  `session_nodes`, `get_node`, and a recall's source through `Store::node_at`). Bounded by `[memory] node_cache_mb` (64;
  0 off; the config check refuses more than 16,384), counted in record bytes plus 128 a node; past the bound it evicts
  to 7/8 of it, `decay_sweep`'s hints first, then by last touch, touch count and touch order. Nothing on the start path
  fills it. Health gains `store.node_cache` (`NodeCacheHealth`: bound, bytes, entries, hits, misses, decodes,
  evictions, failed) and a CLI line, `node cache: 3.0 of 64 MB, 812 nodes; 95% hits of 1000 reads, 50 decoded, 0
  evicted`; metrics `theseus.node_cache.bytes` and `theseus.node_cache.reads` by outcome.
- **Stubs** (719d6f79, `theseus-core/src/stub.rs`): `Transcript = Vec<(u64, Stub)>`. A stub holds the record's id,
  kind, origin, turn and a summary's last position, taken from a **peek** (a serde struct of those five, so the parser
  skips the payload), keeps the record's bytes, and derefs to its `Node`, decoded at the first touch through the cache
  or read again by position. Every whole-transcript reader of the compiler, recall, compaction, the tool runner and the
  memory pass reads stub fields before a body; readers that walk from the end, or need every text (the exam's replay),
  rehydrate what they touch. The debug check that the kept transcript equals the store's compares positions and record
  keys from a scan, decoding nothing (a debug build used to decode the whole session at every read).
- **A failed rehydration** logs the node and its position, counts in `failed`, and reads as a harness message `[node …
  at @… could not be read: …]` (the bytes peeked, so a decode failing after that means a corrupt store).
- **`context.compiled` gains `decoded`** (nodes the compile read past their stub) **and `stubs`** (left undecoded),
  skipped when zero so old rows round-trip; the core's output golden changed with them.
- theseus-core's AGENTS.md names **the deref trap**: a closure typed `|n: &Node|` applied to a stub decodes it
  silently (`render_request`'s `section` closure did, decoding every node, until it took `&Stub`).
- No new package; **no store format change**.

**How it is proven.**
- **The session's tests** (`tests_tiering.rs`, `stub::tests`, `node_cache::tests`): a 60-message session's second
  turn decodes exactly its new nodes, and the whole session again with `node_cache_mb = 0`; one script on two cores,
  cache 64 and 0, recall live (a recall of another session's note, a tool call, long turns until the session compacts,
  two after) gives every request byte-equal with ids and times normalized, its last row with `stubs > 0`; the
  transcript check holds through a ring and a recompile; a recall source from before a compaction floor renders the
  same bytes with the cache on and off; a proptest keeps eviction under the bound. The plain turn stays at 5 frames and
  the tool-call at 9. Under load, 5 of 5 runs, 12 of 12 each. Three planted reverts, each caught: a read that decodes
  every node again; eviction that ignores the bound ("258 bytes over a bound of 215"); a stub rendered without
  rehydration (six tests).
- **The session's rows** (4 cores, debug, both builds alike), `bench turn --session-nodes 2000 --runs 10`:

  | | main | this branch |
  |---|---|---|
  | wall p50 / p95 (the first turn compacts) | 326.5 / 3,240.8 ms | 217.1 / 3,114.9 ms |
  | daemon p50 | 284 ms | 197 ms |
  | nodes decoded per turn | the session whole (not counted) | 2,003, then 2 each |
  | frames | 8, then 5 | 8, then 5 |
  | memory after the turns (daemon + tender) | 200.1 + 47.8 MB | 158.0 + 48.0 MB |

  `bench idle --sessions 10000 --active 50`: 246.8 against 244.4 MB, daemon plus tender, under §9's 1 GB.
- **The review** (R5, stacked on retention's, activation's and consolidation's review merges over e6f90af3): 4 files
  and 9 keep-both blocks (`resolve.py`) and five join fixes (`joinfix.py`): (1) **the deref trap, which the stack had
  walked into**: activation had moved `scene()`'s transcript walk into `TurnRunner::in_context`, called twice a live
  turn, and the moved copy merged clean with the old `&n.body` walk, decoding every node after the floor every turn;
  the kind check went in as tiering wrote it; (2) `stub::Kind` gained consolidation's `Synthesis`, without which every
  synthesis record would fail its peek; (3) the memory pass leaves a synthesis's stub undecoded; (4) `INSTRUMENTS`'
  length counted from its entries; (5) refusal-fallback's `RequestSpec.fallback` in tiering's test. 1,559 of 1,559
  tests, twice, the golden turn at the default 2 MB stack among them.
- **A poisoned target, caught.** The first plant run was void: main's A-side bench build at 02:42, built into the
  review's target, had overwritten the stack's `theseus-memory`, and the plant script counted a build failure (exit 101)
  as a catch. The batch-6 harvest wake fixed the script at 02:57 (a 101 prints VOID); R5 deleted every workspace crate's
  artifacts, rebuilt, and found the rebuilt `theseusd` and `theseus-sim` **bit-identical** to the stack's binaries
  frozen before the poisoning. The plants again: the deref trap in `in_context`, caught by the cache-off test at "the
  summarized range stays stubs"; the cache never hitting, caught (64 decodes, not 2); **a synthesis's stub kind read as
  a summary, not caught**, the code right and the test missing (theseus-q0qe, P2).
- **FAST, the whole stack against e6f90af3** (03:38 to 03:42, one hold, palindrome order, frozen debug builds; IO PSI
  rising from 0.7 to 16.4): plain turn 5 frames on both, median p50 81.1 against 79.6 ms; tool-call 9 frames, 170.6
  against 172.3; the lifecycle's medians equal or better but SIGKILL and restart (+5.5 ms), both arms missing budgets
  under that IO; the long session about 10% faster (means 335 against 302 ms) and about 25 MB smaller.
- **Live, keyless** (03:49, a scratch daemon on the stand-in model): fourteen long turns gave rows with `decoded` 0,
  then 1, 3, 5 … 21, and health read **"node cache: 0.3 of 64 MB, 28 nodes; 89% hits of 251 reads, 28 decoded, 0
  evicted"**. The session never compacted (the window override did not take), so stubs kept past a summary were not
  seen live; the suite holds that case.

**What the session found.** The index alone could not serve the compiler: its walks need each node's body kind, a
summary range's end, its origin and its turn, which the index does not keep (below). A turn that rings or compacts
decodes the whole session once, since the ring renders each candidate cut from the earliest. `decay_sweep`'s baseline
science hints only nodes idle 30 days, so in a running daemon heat decides. The bound counts record bytes, not the
decoded heap: 64 MB of records may be 100 MB or more resident.

**The join** (the stack-M joiner's part 4, on the DM thread's 04:28 follow-up). `may-build.sh` said yes before each
heavy step (C: 24.3 GB free, no other tree building). A dry run on 62de744f (detour-recall's join) resolved the 4
files, kept `INSTRUMENTS` at 32 and the format at 18, and held every line ceiling. Lock and merge in one guarded call at
04:28:47: `git merge --no-ff` of `cloud/20261005-tiering` (2f7b3622) onto 62de744f, rerere replaying 3 of 4 conflicts and
`resolve.py` the fourth (`telemetry/metrics.rs`), the cloud files removed, `joinfix.py` 1 to 5, no fix of the joiner's;
the staged tree equalled the dry run file for file. Signed 23f132f7 at 04:36. Before the gate: R5's filter 1,585 of
1,585 at the default stack, the golden matching, and the CLI and theseus-sim 142 of 142. **The gate** (04:36:32 to
04:43:16, after 97 s waiting behind stack T's A/B): 2,659 of 2,659 (19 skipped); lifecycle ok on its first run, cold
start p50 21.3 and p95 31.4 ms (under 50 plus 7), SIGKILL and restart 25.1 / 25.7; L1 job start p95 7.35 ms; the turn
plain 5 frames at p50 76.0 ms, tool-call 9 at 152.3. Pushed; the done line at 04:43:44; the branch deleted;
theseus-6fn.13 closed; R5's tree and its 9.5 GB target removed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). One new key, `[memory] node_cache_mb = 64`. The cache is the store's read path, not
memory's, so it serves on the owner's daemon whatever his memory mode. Health gains the `node cache` line; `context.compiled`
rows gain `decoded` and `stubs`. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** **A read is a scan plus a peek**, not positions from the index alone (`positions_in_scope`) as §2.10
had it: carrying the five fields as index terms would be a projection change (a rename and a rebuild after serving),
and the step expected no store change. **The cache is keyed by WAL position, not node id**: a node never changes, so its
position names its bytes for good. A failed rehydration reads as a placeholder where a whole read used to fail.

**Known gaps.** theseus-q0qe (P2): nothing holds that `stub::Kind::of` agrees with the peek's kind for every `Body`
variant. The review's recommendations for the owner, none decided at the join: keep the bound in record bytes and have
health say "of records" (not yet changed); accept a ring's one whole decode; leave the payload scan until the
long-session row shows it dominating; let `+retention`'s science give `decay_sweep`'s hints when that arm is measured.
Not yet run: the release-build `bench idle --active 50`, and a live compaction keeping a summarized range as stubs. A
merge that adds `match &n.body` over every node compiles and works, but decodes the session.

### Item 163. The task board: claim leases, one pinned board a place, `/tasks`, a layer-1 card that names the change, and the cockpit's task graph (theseus-ext.14, with theseus-83qm; step 39b, roadmap row 71; the sixth cloud batch's task-board session, fired 2026-10-04 20:00 from 3085f71a, Opus 5.5; 95bbf690, 33699044, 75253bf3, ae51f2e7, 1155d267 and e241d761; reviewed 2026-10-04 23:55 to 2026-10-05 01:45 by local reviewer R6, stack T, on e6f90af3; a first join at 03:01 on dca30347, red in the suite and parked; joined 04:53 at c1c493af, a signed merge onto 23f132f7, with theseus-83qm's fix ef37f325 cherry-picked signed on it, by the stack-T joiner's second take; store format 19; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Step 39a (Item 125) made a task a record, with versions and CAS, but left out the claim, and
nothing in chat showed the graph. 39b is the rest of M7 §2.4: a claim lease, so a second session cannot take work
already taken (§3.5's "claim lease with expiry"); the board, one living message per place; `/tasks`; the layer-1 card,
which must say what the change is before the operator accepts it; and the task graph in the cockpit.

**What landed** (57 files, +2,246 −64 at the merge; theseus-83qm's fix one file, +275 −42).
- **Claims** (95bbf690; `task_graph/lease.rs`, new). `TaskRecord.claim: Option<TaskClaim { by: exe_…, session,
  until_ms }>`; `claim_at(now)` reads a claim past its `until_ms` as free, and `shown()` clears it for every surface.
  `task.claim { id, version }` is a layer-2 edit: it names the version it read and moves it by one. The holder claiming
  again renews (no version move, `renewed: true` on its row); the holder's `task.update` and `task.split` renew in the
  frame they write; `task.close`, or a task session's report closing its record, ends it. A held task refuses another
  execution's claim **before** the version is compared, whatever version it names: `blocked: claimed by session <short>
  until <HH:MM>. Task … is another session's for now: …`, the call's error result, with no row. Another session's
  other edits still apply under CAS and leave the claim. `[kernel] task_lease_minutes` (30, at least 1).
- **The due pass.** `Leases` keeps the claims that hold, in memory (task id → until), built once after serving
  (`Core::warm_leases` in theseusd's after-serving list, before the driver starts, its lock held across the read so a
  claim that lands meanwhile is noted after); each edit's claims are noted after its frame. On the driver's tick, beside
  the questions' expiry, `free_expired_leases_if_due` frees a lapsed claim under the task's lock: the record without
  its claim and a `task.lease_expired` row in one frame, the version kept (a lease's end is not an edit), then
  `task.changed`. Facts `task.claimed` and `task.lease_expired` with narrative lines; the view's line and `theseus tasks`
  show a claim while it holds.
- **Store format 16 → 17 in the branch, 19 at the join**, with a format-16 plan item as a layout sample.
- **The board and `/tasks`** (33699044 and e241d761; theseus-discord's `runtime/board.rs`, `render/board.rs`,
  `courier/board.rs`, new). A task's **home** is its root's origin session, or, for a task session (which no place
  routes), that session's own record's home, so a task session's plan items show on its parent's board. The router
  hands each `task.changed` to its home's place, which sends the place's tree to its lane as one live upsert (id tail,
  title, state, owner, the claim while it holds): made at the place's first change, coalesced, never replayed. The lane
  pins it once (a refusal logged once a lane) and, after a restart, at its first board write, finds the bot's pinned
  message that begins `📋 **Task board**` and edits it, or makes a new one; never on the start path. `/tasks` shows the
  place's records as a tree with states, owners, claims and versions, then today's task sessions. **The binding hears
  every session** (e241d761): `task.changed` joined the wide notifications and the binding calls `executions.watch`
  once its places are bound, since a change made by a CLI session, a task session, or a lapse under another session's
  claim had never reached the place's own watchers.
- **The layer-1 card** (75253bf3). `ConfirmRequest.change: Option<TaskChange { task, title, field, before, after }>`,
  built wherever a question is built, so `confirm.requested`, `confirm.list` and the card carry it, and worded once:
  "Change the acceptance of tsk_… (title)? Before: … After: …", or "Abandon tsk_… (title)? Before: accepted After:
  abandoned". Discord's card opens with 📝 and carries **Accept** and **Decline** on `confirm:approve:` and
  `confirm:decline:` ids, so a press is judged as any card's; `theseus confirm` prints the same question.
- **The cockpit's task graph** (1155d267): a Task graph panel in Actions (the records as a tree, with a waiting change
  in the card's words and its ConfirmCard) and `?taskgraph=1`, the same records in React Flow (`&task=` for the one
  picked); under the time machine it says so and the card is off.
- **The fake model's tool ids** (ae51f2e7, theseus-sim): each id now carries the time and a count. The live check found
  that an accepted change never applied on the rig because every turn's calls reused `toolu_fake_0…`, so the
  continuation read the new call as already answered; a fake's defect, not the core's.
- **theseus-83qm's fix** (ef37f325, `render/board.rs` only). The board had kept a place's 25 **oldest** lines, so once a
  place had had 25 tasks new ones showed on neither the board nor `/tasks`, and it was O(N²) in the task records (140 ms
  at 5,000). Now the open roots come first, newest first, then the closed; past 25 lines the closed lines are cut first,
  every kept line keeps its parent above it, and the last line counts what was left out ("and 5 more, all closed;
  `theseus tasks` lists them all"). The walk uses maps built once, linear in the records: a place's board at 5,000
  records went from 126 to 197 ms to 3.3 ms.

**How it is proven.**
- **The session's**: `tests_task_claims` (two claimers, one blocked for any version; a renewal keeps the version and a
  close ends the claim; a lapsed lease freed at `until` and not at `until − 1`, in one frame, version kept), 5 of 5
  under load; `tests_outbox` for the board (one message, edited in place and pinned; a refused pin; a lane after a
  restart editing the pinned board and making none; a change from a session the binding does not watch reaching its
  home's board); the card's words and its ids; the cockpit's lint, 63 tests (4 new) and build, and `/actions` and
  `?taskgraph=1` in headless Chromium with no console error. Six planted reverts, each caught. A live run of all four of
  the brief's steps on the Discord rig.
- **The review** (R6, 01:21 to 01:45 after a pause at its long-run mark): two keep-both conflicts in `resolve.py`, no
  join fix; **577 of 577** of the branch's tests and their neighbours; **8 of 10 planted reverts caught**, the two not
  caught (the holder's `task.update` not renewing; `shown()` keeping a lapsed claim) gaps in the tests with the code
  right (theseus-8xqt, P3). Live on the Discord rig, all four steps, with **Accept pressed on the Discord card** (one
  `task.change_accepted`, the record v2 with the new acceptance), and the cockpit's panel and graph in headless
  Chrome. A stale target was found on the way (the bench's A build, from a throwaway worktree, had written main's
  artifacts into the review's target) and fixed by touching the 57 changed files and rebuilding.
- **FAST** (the review's probes and A/B, in one hold under neighbour IO, PSI 8 to 13%): nothing before serving; a tick's
  due pass walks only the claims held (224 ns with none, 35 µs at 1,000 among 20,000 records); `warm_leases` reads every
  task record (0.32 ms empty, 21 ms at 2,000, 197 ms at 20,000); the turn bench's frames identical (5 and 9) and its p50s
  within noise; the lifecycle's misses the same phases in both arms. The binding now hears 3 `execution.changed`
  (1,260 bytes) for each turn of a session no place shows.
- **The fix's own**: three new tests (an open task behind 29 closed shows first; the cut takes closed lines first; the
  board's `homed` equals the core's for every session, counts asserted), 6 of 6 planted reverts caught, theseus-discord
  121 of 121; R6's probe, before and after: 1,000 records 4.9 to 10.1 ms → 0.5 to 0.9, 5,000 records 125.9 to 196.9 ms →
  3.2 to 3.3.

**The join, first take** (the stack-T joiner, 02:41 to 03:35). Lock and merge at 03:01:31 on dca30347 (activation's
join): three conflicts, keep-both, the format renumbered to 18; one join fix, theseus-discord's `render.rs` line ceiling
(lane speed raised it to 2,995 and the branch to 2,935, each fitting alone; merged it holds 3,001). Warm clean; signed
2e819be7, and the fix as 0ca28888. **The gate went red in the suite** (03:17:58 to 03:23:03): 2,641 of 2,642, theseusd's
`a_kill_mid_task_then_a_restart_finishes_it_and_reports_once` timing out, "no the task complete in 40 s", where it had
passed in all 170 earlier gate logs at about 5.7 s; afterwards it passed 2 of 2 alone and 24 of 24 four at a time. A
suite red allows no re-gate: main was reset, both commits parked, nothing pushed. The DM thread (03:59) asked for an
experiment before any re-gate.

**The A/B** (03:59 to 04:39, its own worktree and target; A dca30347, B 0ca28888, frozen binaries; full debug logs with
the test's marks): 50 of 50 runs passed in three conditions, quiet, a disk stalling 300 ms a flush, and the test at nice
19 beside 32 busy loops, B's restart-to-complete p90 1.00, 1.00 and 0.98 times A's. At a 220 ms flush delay the red
came back **8 of 8 in both arms**, with the gate's own message: a job's result that lands in the spool between a
restart's spool drain and the harness loop's notify bind waits for the 60 s heartbeat, older code (theseus-74lt; fixed
by lane restart-notify, Item 166). `warm_leases` took 2 to 6 ms in every condition, widening that
window by about 5 ms.

**The join, second take** (lock re-taken at 04:44:00 once tiering's done line landed, the old done line reworded so the
lock read open). The merge on 23f132f7: five conflicts, the three of the first take, with the format renumbered to
**19** (lane files took 17, consolidation 18), and the core's store pin and theseusd's versions test, which both bumps
had changed (join fix 2, `joinfix_pins.py`: each block keeps the branch's side, checked to say 19 or 20 and none of
main's); join fix 1 again. Warm clean at 04:47:12, no semantic conflict. Signed c1c493af, then `git cherry-pick -S`
of 0ca28888 as ef37f325. **The gate** (04:48:03 to 04:53:26): 2,677 of 2,677, the kill-restart test 5.68 s; lifecycle
cold start p50 20.6 and p95 21.9 ms, SIGKILL and restart p95 27.0, swap 55.1, clean shutdown 56.1; L1 job start p50
5.38 ms; the turn plain 5 frames at p50 74.0 ms, tool-call 9 at 152.9. Pushed (23f132f7..ef37f325); the done line at
04:53:57; the branch and the parked branch deleted; theseus-ext.14 and theseus-83qm closed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). Install #4 moved the owner's store from format 16 to 20 at its first write (19 is this join's),
after the install's backup. New:
`[kernel] task_lease_minutes = 30`, and the template's commented `# "task.claim" = "notify"` under `[policy.tools]`. The
binding watches every execution; `warm_leases` runs after serving, before the harness loop and the driver. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The claim carries `session` beside `by`, so the refusal names the session without a read. A claim
moves the version; a renewal and a lease's end do not. The refusal comes before the version compare. The board is one
message a place, not a thread, with no buttons (filed); the binding hears every execution rather than the core
publishing a task change to its home's session (R6: the core's way would put a scan of the task records on the turn
path). The graph is a panel in Actions, not a route.

**Known gaps.** The owner's, recommended by the session and R6 and not built: refuse a non-holder's `task.close` while a
claim holds, the one edit that silently ends the holder's work (one check in `close`). theseus-8xqt (P3, the two test
gaps). `until HH:MM` truncates to the minute. After `/new` in a place, the old session's tasks have no routed home and
their changes draw nothing. The board, `/tasks` and a layer-1 question read every task record per change, and the
router's per-change `home()` walk stays (about 0.5 ms at 5,000 records); the core's own O(N²) `homed` is left, used only
by tests. `warm_leases` grows with plan items, on the way to the driver's first tick.

### Item 164. The learning loop: the owner's labels rewrite a pack's wording as a learned version, a replay checks it, and the ladder places it (theseus-0j2.12, with theseus-bgg5; step 25f; the fifth cloud batch's learn-loop row, launched with the sixth batch, fired 2026-10-04 20:00 from 3085f71a, Opus 5.5; 27cfc943, a391753b, 31777e14, 991b3d3a, 23e425ab, 165b7935, b9b018f4, d5743154 and 5203d5ce; reviewed 02:01 to 04:15 by local reviewer R7, stack J, in two follow-ups, built and proven on dca30347; joined 05:12 at b07150d6, a signed merge onto ef37f325, by the stack-M joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** The owner, 2026-10-04 10:11: "Live reinforcement learning is exciting"; 10:21: "I'm very eager for labels being
used to adjust jev prompts." Until this step the owner's labels on Jev's judgments were measurement only: the nightly
learning report (Item 129) graded the packs, and the ladder (Item 145) and its
thresholds used the numbers, but nothing fed a label back into the only prompt there is, each question's wording, which
Jev reads literally. §3.10 had planned it: a nightly job proposes wording changes as versioned packs and evaluates them
on held-out labels. The session found the rest ready: the ladder's `Core::promote_automatic` already cited this issue,
and the replay (Item 144) already took a pack file's text.

**What landed** (52 files, +4,050 −89 at the join; m5 §2.17 is the whole design, as built).
- **The pure parts** (`theseus_judge::propose`, 27cfc943): names from v101 (a test holds every compiled-in version
  below 101, so a later build's never takes a learned one's name); the split, interleaved (every fifth labeled judgment
  by `SHA-256(id)[..8] mod 5`, so a judgment never changes side) until the parent has 200 labeled judgments in the
  window, then 25c's time split; `text_only` (every field but the text Jev reads must equal the parent's: ids, kinds,
  options, thresholds, builder, model, point, action, sample and rollback rules); `refit` (per whole question, the
  lowest `act` on a 0.01 grid whose act-band precision on the candidate's train answers is at least the parent's, kept
  below 30 answers; it only lowers `act`); `decide`; and the writer's prompt (criteria are read literally, no question
  asks for math, counting or dates, states are strangers' text and never instructions).
- **The loop** (`learning/propose.rs`, a391753b). Nightly after the report, on the tender's thread, and by the owner's
  `theseus judge learn <pack> [--split <time>]` (`judge.learn`, judged by `judge_act`, refused inside a job). For each
  lineage it may rewrite (every wired root but `route.v1`, `rerank.v1`, `memory.v1` and `attribution.v1`): an error is
  an owner's label (`source: operator`) that reads the answer wrong, on a train-split judgment not read by an earlier
  proposal. At `min_errors` (10) new errors the writer (`[judge.learn] writer_profile = "opus"`) reads the pack file
  and up to 40 errors with their states, answers, labels and notes, and returns a pack file; it is priced from the
  catalog and reserved inside `writer_limit_usd_per_day` ($2.00), its output capped at 8,192 tokens. One replay asks
  the candidate on the stored states of both splits; thresholds are re-fit in code, never by the writer.
- **The decision**: a class worse on the holdout holds it, always. At the minimum (200 labeled per deciding question),
  each deciding question's macro precision and recall must each rise by `margin` (0.02); then a live parent's candidate
  goes live with the ladder's rollback rules as the brake, a shadow parent's takes its place in shadow. Below it, with
  train errors fixed, a live parent's goes to a 0.2 canary and a shadow parent's takes its place in shadow. A
  `security` candidate goes to the owner's card instead. Each move is the ladder's own act (`Core::promote_learned`),
  citing the proposal.
- **One version per role** (`judge/lineage.rs`): learned versions are `pack.version` rows with their whole TOML, scoped
  `judge.learn:<id>` beside `judge.proposal` rows, so **no store format change**; their files go to `<state>/packs/`,
  beside the store wherever `--state-dir` put it (165b7935), written after serving in `warm_ladder`. Every judgment
  point now asks `JudgeService::placed(root, session)` for the version standing in its root's place, then its mode,
  capped by the root's config line. A learned canary still running, or a card not yet answered, holds the lineage's
  next proposal (5203d5ce).
- **The owner sees** one notice per proposal ("classify.v101 from 12 of your labels: holdout precision 0.80 → 0.86 …;
  replacing classify.v1 in shadow"), `theseus packs` listing learned versions, and the cockpit's **Versions panel** in
  Judgment (each lineage, its modes and sources, the line diff between any two at `?va=`/`?vb=`, promote, and reject as
  `pack.rollback { off: true }`). The writer's file keeps its layout, so a diff is its wording (23e425ab).
- **Config:** `[judge] holdout_days` (14), and `[judge.learn]` with `enabled = true`, `writer_profile`,
  `writer_limit_usd_per_day = 2.0`, `min_errors = 10`, `max_errors = 40`, `margin = 0.02`, and the join's `min_holdout
  = 5`. The branch's docs commit d5743154 wrote m5 §2.17, step 25f, §2.9's "proposes nothing" and §5's Q10.

**How it is proven.**
- **The session's**: 14 tests, the pure ones (versions above every embedded one; the split stable at about one in five
  and switching at 200; a reworded criterion a candidate, a moved id or a new builder not; the re-fit 0.71 on the
  worked case; each decision) and eight `tests_learn_loop` with the fake Jev scripted per state (nine errors propose
  nothing, ten once, and the holdout never reaches the writer; a better candidate replaces a shadow parent and a
  rollback restores it; a class-worse candidate is held; a live parent's goes to the canary; the day budget stops a run
  with the errors kept new; a security candidate waits on the card). Five planted reverts, each caught; 3 of 3 runs
  under load. An offline end-to-end on a scratch daemon: ten sessions, every `classify.v1` judgment labeled, then
  `theseus judge learn classify.v1 --split <ms>` proposed classify.v101 ("holdout precision 0.50 → 1.00, recall 0.50 →
  1.00 (kind)", writer $0.0002, replay $0.0001), whose diff was its version line and the `control` criterion's
  rewording; the next bare "stop" was judged by v101 as `control` 0.95, and the rollback put v1 back.
- **The review** (R7; its follow-up 2 stopped at the disk guard at 02:15, when three overlapping builds took C: from 41
  to 12.9 GB; follow-up 3 waited on `may-build.sh` before each heavy step): built clean on dca30347 with two conflicts by
  `resolve.py` (the cockpit's export list; `learning/tender.rs` on lane linux-io's PSI-paced form) and five join fixes
  (`joinfix.py`): (1) **the loop paced between lineages by its thread's CPU time**, not the wall clock (a lineage's wall
  time is mostly its requests' network waits, and the branch's form would have slept 19 times those minutes in a sleep
  no stop interrupts), starting none once a stop begins; (2) **`[judge.learn] min_holdout = 5`**: fewer labeled holdout
  judgments send and move nothing, since an empty holdout made "no class worse" vacuously true; (3) §2.17's docs; (4)
  **the writer's request and the replay's calls spawned on the runtime**, not run through `block_on` from the
  SCHED_IDLE thread, whose pool thread would inherit its policy (theseus-bgg5, with a probe test); (5) a new field in
  main's newer initializers. **488 of 488** tests; **5 of 6 planted reverts** caught, the sixth (`placed` ignoring a
  canary's arm) a test gap with the code right (theseus-vh67, P2).
- **FAST** (frozen debug builds, one hold, A B B A, a quiet machine): plain turns 5 frames, p50 73.3 and 81.6 ms on main
  against 78.5 and 74.1 merged; tool-call 9 frames, 175.6 and 169.8 against 166.5 and 165.8; the lifecycle inside every
  budget on both. `placed` costs 232 ns a judgment with no learned version and 2.2 µs with one placed, beside a Jev round
  trip. Nothing new before serving.
- **Live, offline**, two scratch daemons (fix 4 in and planted out): each nightly run, at nice 19 in SCHED_IDLE,
  proposed classify.v101 and placed it in shadow, its file beside the store; the rollback answered "shadow →
  rolled_back by owner". **With real keys** (GLM 5.3 Flash, real Jev, the Opus 5.5 writer capped at $0.35): real Jev
  judged all eight train messages right, so the loop proposed nothing ("0 new errors on the train split; it takes 2")
  and the writer was never asked; a bug in the review's script kept its fallback from relabelling.

**The join** (the stack-M joiner's part 5, on the DM thread's 04:28 follow-up). Lock taken at 04:44:49 and queued
behind task-board's second take. Dry runs on 23f132f7 and ef37f325 found a third conflict (consolidation's
`MEMORY_CONSOLIDATE` beside the branch's `JUDGE_LEARN` in `rpc/judge_runs.rs`, both kept) and, by counting every
`[…; N] = [` in the 14 files both sides changed, **two lists the build would refuse**: `RUNS` declared 4 holding 5, and
the CLI's `OPERATORS` declared 20 holding 21. Consolidation and the loop had each grown them by one entry from the same
base with the same edit to the length, so the line merged clean into a compile error neither merge-tree nor a
string-level dry run shows: **join fix 6** (`joinfix-5.py`) sets each length to its entries' count. Merge at 04:54:05
on ef37f325, rerere replaying two conflicts; `resolve.py`, R7's fixes 1 to 5 and fix 6; the staged tree equal to the dry
run. Signed b07150d6 at 05:00. Warm clean (04:57:39); R7's suites 495 of 495 and the rest of theseus-core 1,005 of 1,005
at the default stack. **The gate** (05:01:02 to 05:11:30, after 284 s behind a batch-7 review build): 2,693 of 2,693;
lifecycle ok on its first run, cold start p50 21.6 and p95 27.4 ms; the L1 job start p50 8.61 ms (5.38 at task-board's
gate, on a path the loop does not touch, with a reviewer's build inside the hold); the turn plain 5 frames at p50 78.8
ms and tool-call 9 at 156.7, inside the last dozen gates' spread (the turn bench runs with the judge off). Pushed; done
line at 05:12:23; the branch deleted. theseus-0j2.12 left open with the hash and what remains; theseus-bgg5 left open
for its audit at nice 19.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change. `[judge.learn]` is on by default, and the owner's config names
`[profiles.opus]` (since install #2), so **the nightly writer can spend, up to $2.00 a day, once his labels give a
version 10 new train errors and 5 labeled holdout judgments**; most nights it sends nothing. The owner's 12:03 yes leaves it
on. `[judge.learn] enabled = false` or `writer_limit_usd_per_day = 0` would stop it. New surfaces: `theseus judge
learn`, `<state>/packs/`, learned versions in `theseus packs`, the Versions panel, and a notice per proposal. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**What the session found.** Opus 5.5 at its full output cap reserves $2.57, past the day's $2.00, so the writer could
never have run: its output is capped at 8,192 tokens. `report::graded` gives a Noul's label truth, not whether its lean
was right; the loop has its own `right`, and the same slip is in 25d's replay (`learning/replay.rs`, its per-judgment
`fixed`/`broken` and `--errors` wrong for Nouls; the one-line patch is in the report). `theseusd --state-dir` moves the
store but not `[server] state_dir`, so 25c's `<state>/learning/<date>.json` follows the wrong one; not changed here.
Re-serializing the writer's file through `toml` rewrote every line, so the version and thresholds are now set in their
own lines. A replay of a learned `classify` candidate would have left every judgment out (25d refuses a stored state
for a pack with a dynamic question), so it asks the candidate without the builder's items.

**Divergences.** `min_holdout`, the CPU-time pacing and the runtime-spawned requests are the review's, not the brief's.
Learned versions are ledger rows and files, not a second registry.

**Known gaps.** Health's judge lines and the Judgment table show a learned version as `not wired`; `judge.list`'s
"disagrees" and the report's question kinds read compiled-in packs only; the security card has no diff; no method lists
proposals; health does not say when the writer's profile does not resolve (theseus-0j2.12, left open for these).
theseus-vh67 (P2): no test holds a canary's control arm on its parent. theseus-bgg5 (P2): the owner's audit at nice 19
on main. The real-key writer check is owed: rerun the review's fixed script to see Opus 5.5 write a candidate that
passes `text_only` and the loader, under $0.50. The 25d and 25c slips above. At the minimum sample the decisions are
proved in the pure tests only. The owner's promotion of a learned version reads as `forced` until a learning report
covers that version.

### Item 165. The cockpit's Budgets, Ledger and Policy tabs: money and its questions in one panel, the whole ledger followed, and every tool's posture layer by layer (theseus-ext.15; step 42b, roadmap row 74; the sixth cloud batch's cockpit-tabs session, fired 2026-10-04 20:00 from 3085f71a, Sonnet 5.5; 669ead46, 33c0ae15, 1d3427e2 and 24413ae7; reviewed 00:45 to 02:10 by local reviewer R6, stack T, on task-board's review merge; joined 05:48 at 1a08a40e, a signed merge onto b07150d6, by the stack-T joiner, after a first gate red in lifecycle and one re-gate; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Step 42a (Item 130) gave the protocol two reads, `budget.list` (each open execution's money,
its carves and resets) and `policy.explain` (each tool's posture, layer by layer), but nothing in the web UI read them:
Money's Budgets panel read `execution.list` and `action.list`, the Ledger polled `ledger.tail` for its newest 20,000
rows every 5 s, a second copy of the ledger, and the tightenings and their undo lived inside Boundaries' gate. 42b was
M7 §2.6's three tabs, on the push, with no polling.

**What landed** (cockpit only: 16 files, +1,078 −136; no Rust, no protocol type, no package).
- **Budgets, in Money** (669ead46, and 24413ae7 for its place). `components/Budgets.tsx` and the pure `lib/budgets.ts`:
  `budget.list`'s rows, each session's tasks indented under it with their carves ("its parent holds $X for it"), where
  each limit comes from in words, held, held unknown, available, lifetime, burn per hour (the last hour's
  `provider.call` rows, scaled to an hour, a parent's including its tasks'), the last reset (`?budget=<session>`), the
  questions waiting as the Actions view's own `ConfirmCard`, the totals by the daemon's rule (money adds the top rows
  only, so a task is never counted twice; lifetime adds every row), the judge's day line, and the AWS hands from health
  with a runaway said first. It is full width above the activity river, where a waiting question is never past a
  scroll. `budget.list` and `policy.explain` joined the pushed reads, and the push now reads again on
  `policy.tightened`, `policy.untightened` and `session.trusted`. Money's range, measure and table moved into the
  address.
- **The Ledger on the shared history** (33c0ae15). It reads `useHistoryRows()`, the whole ledger, followed, and the
  20,000-row poll is gone. `lib/ledgerview.ts`: the filter the list uses and an export built from it, so the export is
  by construction the rows shown; saved filters in the browser's storage (the address's filter keys only, tolerant of
  junk); follow keeps the newest rows at the top and, scrolled away, pauses with "N newer · follow paused · jump to the
  newest". The time brush, the row picked and follow are in the address. The histogram's `Math.min(...rows)`, which
  throws past about 120,000 rows, became a loop.
- **Policy** (1d3427e2; `views/Policy.tsx`, `/policy`, in the nav with the key `g` then `p`): `policy.explain` for the
  CLI and each bound place (class, ceiling, hold), or one session (`?session=`); each tool's result and the layers that
  raised it, the strictest first, a tool opening its layers in the gate's order with their conditions (`?tool=`); the
  tightenings with their undo (confirmed first, off under the time machine), now one component shared with
  Boundaries; and a link to Systems' approval channels.
- Each view says when it shows only the present under the time machine, and its acts are off there.

**How it is proven.**
- **The session's**: 16 new node tests (budgets 6, ledger view 5, policy view 5; the suite 75 of 75), three planted
  reverts each caught (totals over every row, "expected 3.5, actual 4.5"; burn over all time; an export of every
  row), and headless Chrome on scratch daemons: the Budgets totals equal to `theseus budgets`', a new
  turn moving them with no reload, a budget question's card ("Reset to $0 and continue" / "Keep waiting") from
  `[kernel] spend_limit_usd = 0.0006`; a saved filter surviving a reload and an export of 5 rows, all `startup.*`; a CLI
  tighten and its Undo on `/policy`, with 7 layers and the conditions for an opened `proc.run`.
- **The review** (R6; the static and cockpit checks 00:45 to 01:07 in a throwaway worktree, then its follow-up 01:44
  to 02:10, the lock's done line at 02:08:11): merged on task-board's review commit, two keep-both conflicts
  (`bindPush`'s one `if`, which takes task-board's `task.changed` and these three policy methods; cockpit/AGENTS.md),
  replayed by rerere; none against main alone; no join fix. The stack's build clean; the cockpit's lint, **81 of 81
  tests** and build; **7 of 7 npm planted reverts** caught (burn's window edge, the resets' moment, the session filter,
  follow's count, a saved filter's keys, the tightening workaround, the strictest first). Live on scratch daemons:
  `/money`, `/policy`, `/ledger`, `/boundaries`, `/systems` and `/actions` with no console or page error; follow took
  a CLI turn's rows (1,107 to 1,124) with no reload; a CLI tighten showed in the tightenings list within 273 ms, and
  its Undo, a second tighten and an untighten as quickly.
- **FAST** (the page's own work; nothing on the daemon's start or turn path): on a scratch daemon with a 151,509-row
  ledger, in CPU-drawn headless Chrome, the first row drew 507 ms after the navigation began and the whole ledger was
  in the list in 15.4 s, the views working on what had landed meanwhile; a search keystroke moved the count in 85 to
  170 ms (the filter serializes each row at every keystroke, linear in the ledger); the tab peaked at a 127 MB heap and
  a 650 MB renderer and settled to about 40 MB and 430 to 515 MB.

**What the session and the review found.** 42a's `explain.rs` sets the `tightening` layer's `raised` from the whole
decision, not the tightening, so with no workspace root, or wherever an approve list raises a call, every tool reads
"not tightened" yet raised (theseus-19t8, P3, confirmed live: 16 of 26 tool rows on a rootless daemon); the view counts
a tightening only when the layer names who tightened, a workaround the planted revert holds. And `/policy`'s
explanation went stale after a tightening made elsewhere: the page reads `policy.explain` again on `policy.tightened`,
but the core sends that by `publish_all`, which reaches only connections that watch a session, so live a CLI tighten
left `proc.run` reading "open" 20 s later, "approve, raised by tightening" after a reload (theseus-n9wa, P3).

**The join** (the stack-T joiner's second take, after task-board's and learn-loop's joins). Lock taken at 04:48:17 to
hold its place; learn-loop's older lock went first. Merge at 05:12:41 on b07150d6: rerere replayed both of R6's
resolutions and `resolve.py` found nothing left; 16 files, all under `cockpit/`; package-lock.json main's; the Rust
warm (05:13:48) and the cockpit's (`npm ci --offline`, lint, tests, build, 05:14:53) clean. Signed 1a08a40e. **Gate 1**
(05:15:31 to 05:29:26) passed its suite, 2,693 of 2,693, but **missed in lifecycle**: run 1 had one cold-start outlier
(p95 310.9 ms, the other nine near 20), and the gate's own rerun saw every fsync-bound phase rise together (clean
shutdown p50 84.3 ms against 30.6 to 33.6 at the last three gates, swap p95 822.5) while the load rose from 3.4 to
27.8 as reviewers' builds resumed: the neighbour-IO pattern, on paths a cockpit-only branch does not touch. By the
brief, the joiner re-warmed and **re-gated once** (05:31:10 to 05:47:56, 607 s of it waiting for the lock): 2,693 of
2,693, one flaky (the simulator's seeded-faults test, on the named flaky list as theseus-81ig, passed on its second
try); lifecycle cold start p95 22.8 ms, clean shutdown p95 48.8, SIGKILL and restart 27.7, swap 54.4; the turn plain 5
frames at p50 77.2 ms and tool-call 9 at 156.3. Pushed; the done line at 05:48:31; the branch deleted;
theseus-ext.15 closed. Batch 6 was then all on main but situations, which waited for the owner.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). The cockpit's build ships with the daemon: Money's Budgets panel, the followed Ledger,
and `/policy`. No config, no store change. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** Budgets is a full-width panel in Money, not a tab of its own, and Ledger is the existing view on the
shared history; `/policy` is the one new view. The approval channels and trusted users stay in Systems, linked from
`/policy`, not moved. A budget reset is one click on the card, as in Actions (R6: the reset is itself a question the
daemon asked). The views live in `cockpit/`, since the cockpit replaced `web/` (Item 86).

**Known gaps.** theseus-n9wa (P3): put `policy.tightened`, `policy.untightened` and `session.trusted` in the wide
notifications, as task-board did for `task.changed`. theseus-19t8 (P3): `explain.rs`'s tightening flag, then drop the
view's workaround. The search filter serializes every row at each keystroke: if it drags at the owner's store's size,
build each row's search text once, as it lands. Budgets' burn walks every `provider.call` row once per budget row
every 10 s, and an unfiltered export builds one string of the whole ledger. Boundaries' undo stays live under the time
machine, as before.

### Item 166. A job's result spooled during a restart's startup is taken at the harness loop's bind, not a minute later (theseus-74lt; the `restart-notify` lane, a subagent of the DM thread, Opus 5.5, 2026-10-05 05:19 to 06:46, from b07150d6; 2c4b52fe; joined by the lane at 06:42 at 62ec4199, a signed merge onto 1a08a40e; reviewed 06:49 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Task-board's first join gate went red on theseusd's kill-restart task test ("no the task complete in 40 s"),
and the stack-T joiner's A/B (Item 163) found the cause in older code, in both arms alike: 8 of 8 runs
red at a 220 ms flush delay, with and without task-board. A restart drains the completion spool before serving, while
a job's wrapper may still be fsyncing its result; the wrapper then renames the result into place and sends its one
notify, which the killed daemon's leftover socket refuses (best effort by design: "the spool is the truth and the
reconciler will find it"). `harness::run` binds the notify socket only after `after_serving`'s inline steps, and then
skipped its first tick as "startup already reconciled", so the next look at the spool was the heartbeat, 60 s later.
Quiet, the window between the drain and the bind is about 22 ms; under an IO stall it is half a second to a second, and
the gate's restart ran at load 23 beside other trees' builds. It was in the owner's installed daemon too: a job that ends
while the daemon is starting could wait a minute for its result.

**What landed** (2c4b52fe, 3 files, +135 −1).
- **`harness::run`** (`theseus-core/src/harness.rs`): the first tick, which fires at once, is now a heartbeat, run on
  the blocking pool before the loop parks: `tick.tick().await;` then `spawn_blocking(move || c.heartbeat("bind"))`, in
  place of the skipped tick. The heartbeat runs strictly after the bind, so a result renamed into place before it
  (its notify refused, or sent before the socket existed) is taken there, and one renamed after it reaches the
  listener as before; a notify arriving during the heartbeat waits in the bound listener's backlog. Nothing falls
  between the two.
- **Off the start path:** `harness::run` is spawned by `after_serving`, after serving. The bind and the startup's
  drain stay where they were, and the kernel's notify stays best effort. A heartbeat with nothing to do writes nothing
  (204 to 363 µs in the rig's logs, `heartbeat: nothing to do why="bind"`); one that takes a result also wakes the
  driver, so the driver takes it at once rather than at its next 500 ms tick.
- **One more effect:** a full heartbeat also fires due wakes, so a wake that fell due while the daemon was down is now
  queued by whichever runs first, this heartbeat or the driver's first tick. `fire_due` decides under the execution's
  lock and the turn that takes the wake writes its one `wake.fired` row, so the outcome is the same either way.
- **Not taken:** binding the socket before the startup's drain (that puts the bind on the start path), or binding it
  first thing in `after_serving` (narrower, but the drain still runs before serving, so the window stays open).

**How it is proven.**
- **A deterministic test** (`tests_harness::a_result_spooled_before_the_loop_binds_is_taken_at_the_bind`, 124 lines):
  a core does its startup with an empty spool; an execution waits on a dispatched `proc.run` job; the job's result is
  written into the spool as a wrapper does, its notify finding no socket; then `harness::run` starts, `heartbeat_ms`
  asserted at its 60 s default; within one second of tokio's paused clock the result must have left the spool, the
  job settled `Succeeded`, and the execution be `Queued` with it. A heartbeat on the blocking pool holds the paused
  clock, so the bound measures the loop's own waits, never the machine's load. 0.31 s alone, 0.35 s in the gate.
- **The planted revert** (main's skipped first tick), run twice, at 05:48 on the working tree and at 06:22 on the
  commit: the test failed each time, "the result still waits in the spool after 1s"; the file restored byte for byte
  with a fresh mtime.
- **Live, on the slow-disk rig at 220 ms** (append and fdatasync 241 to 256 ms; one frozen test binary, the arm's
  frozen daemon put at the path it bakes before each run; P F F F F F F P, 06:15:59 to 06:21:00, load 13 to 25): the
  fixed daemon passed **6 of 6**; in the 3 runs that hit the window (the restart's drain found nothing) the bind's
  heartbeat took the result 972 to 998 ms after the restart's kernel began accepting events. The planted build went
  red **2 of 2**, with the gate's own message. The rig's before and after snapshots were identical.
- **The lane gate** (06:02:39 to 06:12:45, niced, benches off): 2,694 of 2,694; the restart tests at their usual
  times (kill-restart 5.66 s, wake-due-while-down 10.16 s).

**The join** (by the lane, under `lane-restart-notify-join`, taken at 06:23:49 in one guarded call with the merge, queued
behind two batch-7 step reviews' locks until 06:27; bench-stack's and sim2's joins queued behind it). `git merge
--no-ff -S` of `lane/restart-notify` (2c4b52fe) onto 1a08a40e (cockpit-tabs' join) at 06:27:43, **62ec4199**, no
conflict; every file far under its line ceiling. Warm clean (06:32:11). **The gate** (06:32:11 to 06:40:17): 2,694 of
2,694 (19 skipped); lifecycle within every budget, cold start p50 21.5 and p95 27.2 ms, clean shutdown 32.1 / 51.1,
SIGKILL and restart 30.7 / 32.2 (25.8 / 27.7 at cockpit-tabs' gate), swap 48.0 / 57.3; L1 job start p50 5.76 ms; the
turn plain 5 frames at p50 75.1 ms and tool-call 9 at 154.0. The restart's 30.7 ms is not this change: the bench
SIGKILLs each restarted daemon at its first health answer, before `harness::run` starts, and every start phase before
serving was slower in this gate by the daemon's own clock (serving 19.77 against 17.48 ms over 51 starts), the
machine's batch-7 builds; a lifecycle A/B of the two frozen builds never got the gate lock and was not run. Pushed at
06:42:41; the done line at 06:42:47; the worktree, branch (never pushed) and target removed; theseus-74lt closed.
At the review (06:49) the DM thread checked 62ec4199's parents and signatures, the gate log's exit 0 with 2,694 of
2,694, the lock's JOINED line, and the issue closed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No config and no store change. After any restart, an install's included, a result that
lands during the startup is taken about a second after it, where it could wait a minute. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** None from the joiner's proposed fix: one heartbeat at the bind, rather than moving the bind.

**Known gaps.** A lifecycle A/B on a quiet machine, if wanted (`rig/lifecycle-ab.sh`). §3.15's sentence and Item 8's
record say "the driver's first tick queues" a due wake; that is true unless the bind's heartbeat gets there first, and
§3.15 gains a note (Item 8 stays the record of its day).

### Item 167. Efficiency in every benchmark trial: a sampler of the harness's CPU and RAM apart from its work, one record for every arm, a measured Claude Code arm, and Pareto reports (theseus-7gir.12; the bench program's efficiency track; the seventh cloud batch's bench-efficiency session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; c41e43bf, fdeeda6c, ad6048a6, 44cf178e, 952e8a9f, b568b2a0, 3791637f, fd2fe6d6 and 590a54fc; reviewed 05:01 to 05:34 by local reviewer R11, stack B; joined 06:50 at 4bcaeef6, a signed merge onto 62ec4199, the first of stack B's three merges, by the stack-B joiner; nothing installed: `bench/` only)

**Why.** The owner, 2026-10-04 16:36: benchmark efficiency against performance, RAM and CPU as well as tokens. The first
full Terminal-Bench run (Item 148) published each arm's score and dollars, but no CPU or RAM, and Harbor's
input count folds the cache writes in. The bench program's efficiency track asked for one record per trial, for every
arm: tokens by class (input, cache read, cache write, output), dollars, wall time, model and tool calls, and the
harness's own CPU and peak RSS **apart from the commands it runs**, sampled inside the task's container; and for each
run, solved per dollar, tokens per solved task, cache-hit share, harness CPU per tool call and peak RAM, with Pareto
charts of score against dollars, tokens and RAM.

**What landed** (12 files, +2,683 −19, all under `bench/`; Python, standard library and Harbor only).
- **The sampler** (`bench/harbor/sampler.py`, one file, Python 3.6 or later), started in the task's container around
  the agent's run. Every 250 ms it reads `/proc/<pid>/stat` ticks, `cutime` and `cstime` included, so a reaped child's
  time is kept (a tree process's `cutime` grown by more than what vanished under it goes to the work). It sorts the
  agent's process tree into the **harness**, the **job wrappers** (a third class) and the **work**, and the rest of the
  container as outside. A process started through `/proc/self/exe` is named by its `argv[0]` (theseusd's job wrapper's
  comm is `exe`; found by a live run). On cgroup v2 it reads the container's cgroup, and the work's CPU is the cgroup's
  total less the harness's, the wrappers', the sampled outside processes' and its own (`cpu_from: cgroup`); without one,
  the samples' (`cpu_from: samples`). `memory.peak` covers the container's whole life, install included, so it is
  kept as `container.memory_peak_kb` and never used for a peak. Its own cost, measured: 0.31% of a core at 250 ms in a
  23-process namespace. With no `python3` in the image it writes `unavailable` and the trial goes on.
- **The record** (`bench/harbor/efficiency.py`), one shape for every arm, in `agent/efficiency.json`. Theseus's tokens
  split by model from each history answer (what they don't account for goes to the turn's model; a refusal fallback's
  two models are billed apart), and its model calls are the trace's `provider` spans. Claude Code's spend comes from
  the stream-json `result` event's `modelUsage` first (its own bill per model, all four classes, a background model's
  calls included), else its session log with each message id once, else Harbor's trajectory.
- **Theseus's adapter** runs the sampler in `run_script` and `stop_script`, uploads it at install, and writes the
  record in `populate_context_post_run`. **`MeasuredClaudeCode`** (`bench/harbor/claude_code_agent.py`) is a thin
  subclass of Harbor's `ClaudeCode`, measured the same way.
- **The report** (`bench/report/efficiency.py`): per arm, solved per dollar, tokens per solved task, the cache-read
  share, harness CPU per tool call and peak harness RSS; the Pareto front as `pareto-{dollars,tokens,ram}.svg`;
  `trials.csv`. An old job with no records reports Harbor's dollars, its cache writes from each trial's own files, and
  "not sampled" for CPU and RAM.

**How it is proven.**
- **The session's**: bench/harbor 50 tests (7 need Harbor, all 50 in Harbor's 3.12 venv) and bench/report 8: parsers
  on fixture `/proc` text; classes on a fixture tree (a reaped child no sample saw, a CLI reaping its daemon, an orphan,
  a reused pid); on the host, a busy child's work CPU equal to the kernel's own count within 0.05 s; the report against
  numbers worked by hand. Five planted reverts, each caught (work counted as the harness's; a reaped child's time
  dropped; cache writes folded into input; a non-dominated arm dropped from the front; the `exe` naming). A local run on
  the debug binaries and the stand-in model: harness 0.05 s of CPU at 76 MB, the wrapper 22 MB, the work 4.5 s.
- **The review** (R11; no Rust built): no conflict; one join fix (theseus-t412): the sampler test's host bound, 1% of a
  core over the whole host's `/proc`, failed on this machine's 157 to 187 processes (2.0 to 3.8%), so it scales with
  the process count (`0.01 * max(1, procs / 25)`); in a task's container the sampler costs 0.15 to 0.4%. Suites 50 of
  50 and 8 of 8 under both Pythons; **6 of 6 planted reverts** caught.
- **Live** (R11, $0.1088 of stack B's $0.5702): the sampler in a real Docker container on cgroup v2, `cpu_from:
  cgroup`. The report over the first full run's 534 trials reproduced docs/benchmarks.md exactly: 128 of 178 solved
  and $24.72, 131 and $24.23, 145 and $22.65, with CPU and RAM "not sampled". One paid `fix-git` trial per arm (on b5's
  own build and profile, since no copyable build knew today's bench profile), both solved:

  | | Theseus | Claude Code 2.1.288 |
  |---|---|---|
  | dollars | $0.0552 | $0.0536 |
  | model calls, tool calls | 7, 13 | 7, 6 |
  | tokens: input, cache read, cache write, output | 16, 45,969, 11,756, 1,663 | 14, 109,395, 8,374, 1,080 |
  | harness CPU, peak RSS | 0.33 s, 25.8 MB (2 processes, 5 job wrappers) | 1.05 s, 247 MB (`claude`) |
  | harness CPU per tool call | 25.4 ms | 175.0 ms |
  | cache-read share of input | 79.6% | 92.9% |

  Claude Code's record read `spend_from: result_event` with all four classes, so the `modelUsage` keys the session
  wrote from memory are right.

**What the session found.** Harbor's converter already counts each Claude Code message once, keeping its last usage,
and the record does the same. Harbor 0.23 needs Python 3.12. `schedstat` loses exited threads, so CPU is read in ticks.

**The join** (the stack-B joiner, 06:26 to 06:55: one bench-only join of three signed merges, with `bench/`'s own
gate). The lock `cloud-bench-stack-join` was taken at 06:28:03 and queued behind lane restart-notify's join. A dry run
on 62ec4199 gave the same `bench/` tree as the review's (c32353c1), with nothing outside `bench/`. **4bcaeef6** at
06:43:39: no conflict, the join fix applied, 12 files, `bench/` equal to R11's review commit; then bench-async's and
bench-recall's merges (Items 168 and 169), and `HEAD:bench` was c32353c1 as reviewed.
**The gate:** `bench/`'s four suites under the host's Python and Harbor's venv, all green (harbor 50, report 8, async 35,
recall 47 with none skipped, on main's own `target/debug` and today's bench profile), and a scrub of every added line
with 10 hits, all known false kinds (an invented host, SVG namespaces, loopback addresses), and 0 for names, paths and
key shapes. No Rust gate, no model spend. Pushed 62ec4199..e4d09068; the three branches deleted; the done line at
06:50:52; theseus-7gir.12 closed. The joiner also found bench-async's stand-in daemon leaking one process per suite run
and stopped 15 of them (theseus-6xre).

**The install.** Nothing: `bench/` is a harness, with no daemon code, config key or store format.

**Divergences.** The sampler was committed before the record, which reads its summary. Wrappers are a class of their
own. "Harness CPU per tool call" uses the harness alone.

**Known gaps.** theseus-1xxi (P3): `efficiency.machine()` subtracts the sampler's own CPU twice, so the work's cgroup
CPU reads low by it (live, 0.268 s against the samples' 0.36); and an old trial with no files counts 0 model calls, not
unknown. theseus-t412 (P3): the real fix for the test bound. With `hidepid` on `/proc`, other users' processes vanish
from the samples. The owner's, costed by R11 and not run: the full measured run, both arms over terminal-bench@2.0, about
$47 and 6 hours, after 1xxi; publishing efficiency rows in docs/benchmarks.md only from a sampled full run. The report's
`--arm` takes one job directory, so b5's per-task layout needs a directory of symlinks (worth a line in
`bench/README.md`).

### Item 168. Novel benchmark 4, the async bench: six task families where concurrency is the point, a Theseus arm on its own daemon, a Claude Code arm fed through a FIFO, and a scorer (theseus-7gir.16; the bench program's fourth novel benchmark; the seventh cloud batch's bench-async session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; ed7d73f9, edae6ddc, 69d2644a, 08b9a87f, 8db94d0d, 64562755 and 2cd066ea; reviewed 05:16 to 06:02 by local reviewer R11, stack B, on bench-efficiency's review merge; joined 06:50 at 32efb965, a signed merge onto 4bcaeef6, by the stack-B joiner; nothing installed: `bench/` only)

**Why.** The owner, 2026-10-04 16:36: "We also have a simultaneity aspect, with async tools and tasks, and could have an
async bench we also test against". Terminal-Bench gives one instruction and waits for the end, so nothing in it rewards
doing things at once, answering while working, or stopping cleanly. The async bench is containerised tasks where
concurrency is the point, run through Harbor with a driver that injects a message mid-trial, each arm using its own
async means (Theseus's jobs and completion wakes; Claude Code's background commands).

**What landed** (62 files, +10,986, all under `bench/async/` but one function in `bench/harbor/theseus_bench.py`).
- **Six task families** (`bench/async/tasks/`, each a Harbor task: `task.toml` with its family, injection and timeouts;
  an instruction that says what to do, never how; a `python:3.12-slim` image with the tools; an oracle; a verifier):
  **parallel** (independent parts whose aggregate is checked), **wait-tax** (a slow job and work to do meanwhile),
  **interrupt** (a second request while a long job runs), **fan-out** (with partial failure), **cancel** (a migration
  stopped mid-way, its workers too), and **contention** (concurrent deposits that must not lose an update).
- **The ledger the verifier reads** (`tools/asyncbench.py`, one library synced into every image and verifier; a test
  fails on a stale copy): JSONL outside the working directory, each record with its seq, kind, tool, step, pid and the
  process's start time (so a reused pid is caught), wall and monotonic times, the drawn duration and the scale, in a
  sha256 chain. The check fails a broken chain or a gap, time going backwards, an end sooner than its drawn duration,
  or a step run at another scale than the verifier's, so an agent that changes the scale fails. The chain catches an
  edit, not a forgery by an agent that rewrites the whole file (the agent is root in these containers).
- **The driver** (`driver.py`, `async_agents.py`): an injection fires on the ledger's first `start` of the long job plus
  its delay, else at a time after the agent started, and is written to the ledger with the trigger that fired before
  it is delivered. **TheseusAsync** runs a real daemon for the whole trial (`theseus_bench.daemon_script`, beside
  `run_script`), with `proc_sync_secs` at the product's default of 60, so a long command goes to the background and its
  result comes back as a continuation; the injection is a second `ask -s`, which queues behind a running turn. A trial
  settles only when nothing is running, queued, outstanding or waiting and no wake is pending, each wait the daemon's
  own (`theseus wait --until settled --after <position>`). **ClaudeCodeAsync** rewrites Harbor's one command so the
  CLI's stdin is a FIFO (`--input-format stream-json`): the instruction is the first line, the injection a second, and
  the input closes once a `result` follows the last message.
- **The scorer** (`score.py`): success; wall over the ideal from the ledger's durations; the wait tax (calls and tokens
  inside the slow job's window); responsiveness (the injection to the second request's end, or to the migration's first
  stop); orphans; duplicated effects; CPU and RAM where an efficiency record exists; cost. An injection that never
  reached its agent makes the trial "not measurable", not a failure.

**How it is proven.**
- **The session's** (35 tests): every oracle earns reward 1 at scale 0.01, overlapping its steps; one planted wrong
  effect per family earns 0 with the problem named (a missing part, a guessed token, an abandoned job, an effect twice,
  an orphan left, a lost update); an edited value, a dropped line, a rebuilt chain and another scale all fail. The
  driver against a fake environment and a stand-in `claude`; an end-to-end on the workspace's own daemon and the
  stand-in model, the interrupt family: the long job went to the background, the injection fired by event and ran at
  once, the job's continuation came, reward 1. Four planted reverts, each caught; three of three runs clean under load
  once the tests waited on ledger events instead of sleeps.
- **The review** (R11): merged on bench-efficiency's, one keep-both conflict (`theseus_bench.py`'s docstring); suites
  35 of 35 under both Pythons; **5 of 7 planted reverts caught**, the two not caught test gaps with the code right (a
  pending wake in `settle`, never given one; `ClaudeCodeAsync.run`, which no test runs; theseus-6xre, P3).
- **Live** (R11, $0.1019): the oracle earned **reward 1 on all six families** in real containers at full scale (4 min
  2 s; wall over ideal 1.0 to 1.1, no orphans, no duplicates). Theseus on Claude Sonnet 5.5 (b5's build): **interrupt**
  reward 1, $0.0320, 9 calls, settled, the trigger by event, wall over ideal 1.07, wait tax 5 calls and 30,771 tokens,
  **responsiveness 46.4 s**; **cancel** reward 1, $0.0423, no migration process left, **responsiveness 56.5 s**. Both
  numbers are `proc_sync`'s 60 s: the first turn waits on its command until it goes to the background, and the injected
  ask queues behind it. Claude Code 2.1.288 on interrupt: reward 1, $0.0277; the stream-json message shape was
  accepted, and **the CLI took the injected message mid-turn** (one result, `queued_turn_count` 0), acting on it when
  its foreground `train-model` returned (**147 s**). But the trial ran to Harbor's 900 s timeout.
- **The bug that timeout showed** (theseus-70vi, P2): the closer grepped `^{"type":"result"`, and Claude Code writes
  the result's `type` after other keys, so it never closed the CLI's input. **The join fix** drops the anchor (safe in
  JSONL: a nested string's quotes are escaped, and `"type":"tool_result"` does not match) and makes the stand-in
  `claude` write the real key order, so the FIFO test fails without the fix (its planted revert was caught).

**The join** (stack B's second merge, 32efb965 at 06:44:02; the shared join and its gate are in Item
167). rerere replayed R11's docstring resolution, `resolve.py` said "already resolved", and the
join fix applied to `driver.py` and `test_driver.py`: 62 files, +10,986, `bench/` outside `recall/` identical to the
review's. The async suites in the gate: 35 of 35 on the host (4 skipped) and under Harbor's venv. theseus-7gir.16 stays
open: OpenClaw's arm is not built, and CooperBench and BFCL's parallel categories are to run beside it.

**The install.** Nothing installs from `bench/`.

**Divergences.** The injection is a second `ask -s`, queued behind a running turn, since that is what the daemon does
with it. The interrupt instruction does not say a second request is coming (R11: not knowing is the point; say beside
the scores that Theseus's number is `proc_sync`'s product default). The cancel family's oracle plays its part without
receiving the message.

**Known gaps.** theseus-z5ty (P2), the seam with the efficiency record: TheseusAsync inherits bench-efficiency's
`populate_context_post_run`, which records the first `ask` only ($0.0168 and 2 calls where the ledger had $0.0423 and
8), runs no sampler, and the scorer reads top-level CPU and RAM keys the nested record never has; one follow-up step,
before the full run. theseus-70vi's live rerun of one ClaudeCodeAsync trial (about $0.03) is owed. theseus-6xre (P3):
the two test gaps, a stand-in race seen once under load, and the stand-in daemon's leak found at the join. A trial past
1,000 model calls would be under-counted. The owner's, costed and not run: the six families on both arms, about 24 trials,
about $1 and 30 minutes, after z5ty.

### Item 169. Novel benchmark 5, the incidental-recall bench: a seeded progression of facts said in passing, probed at measured distances, with Theseus and Claude Code drivers and a scorer (theseus-7gir.17; the bench program's fifth novel benchmark; the seventh cloud batch's bench-recall session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; 908d32b6, 2915f311, bbda2334, b255a871, 48af0bed, 2ac2ec3f, 30ec2842, 027d9d73 and b0397246; reviewed 05:36 to 05:45 by local reviewer R11, stack B, on bench-async's review merge; joined 06:50 at e4d09068, a signed merge onto 32efb965, by the stack-B joiner; nothing installed: `bench/` only)

**Why.** The owner, 2026-10-04 16:37: "a recall benchmark that is about reference out of the way facts given a context
progression". theseus-exam's families are Theseus-only and have no salience, no progression and no indirect probes. This
bench replays one scripted, multi-session progression identically to each arm, with facts seeded at known points, some
central to the topic and some incidental (said once, in a tool's output, an error message, an aside or a side remark),
and probes them at controlled distances (turns, a topic shift, a compaction, a session boundary, days, after a
supersession): directly, indirectly (a task that silently needs the fact), and by abstention (a fact nobody said).

**What landed** (10 files, +3,976, all new under `bench/recall/`; Python, standard library only).
- **The format** (`progression.py`, `checks.py`): sessions with a date; turns with the user's text, a block, a role, an
  estimate, the workspace edits made before them, and marks; facts with a kind (port, path, version, host, ticket,
  date), a value, a salience, a family, a carrier, a source, a delivery marker and what supersedes them; probes with a
  fact, a kind, a planned bucket and a check. Distance is measured from where an arm actually compacted: a compaction at
  turn m lies between a fact at f and its probe at p when f < m ≤ p. A validator refuses a fact probed twice, a probe
  before its fact, a value said outside its turn, or an abstention's subject said anywhere. `checks.py` is
  theseus-exam's `check.rs` language in Python.
- **The generator** (`generate.py`): SplitMix64 ported from `rng.rs`, so a seed gives the same bytes; topic blocks whose
  central facts are about the topic and incidental ones about something else; a script that showed a value rewritten
  before the next turn, so it cannot be read again; supersessions; abstentions beside a same-kind distractor. **Smoke:**
  two sessions of 15 turns three days apart, a mark at turn 10, 8 probes. **Full:** three sessions of 200 turns, a mark
  at turn 120 of each, **204 probes in 34 cells, 228 facts**. It prints the budget at the catalog's price: for seed 7 on
  Sonnet 5.5, $0.48 an arm for the smoke and $11.25 for the full. The window is the smallest whose request budget holds
  each session's turns before the mark with 15% to spare (Theseus keeps the output cap and 4,096 tokens back): 35,000
  for the smoke, 124,000 for the full.
- **The drivers** (`drive.py`): **Theseus** on a scratch daemon with `theseus-index` beside it, configured as the
  exam's daemons are, `[memory] mode = "live"` with the arm named (`none`, `bm25`, `baseline`, `+synthesis` …), one
  session per progression session and each turn an `ask`; compactions read from `context.compacted` rows by ledger
  position; at the end it saves history and recalls, stops the daemon, and fails if anything still names the run.
  **Claude Code** by `claude -p --output-format json` with a scratch config dir, a session id kept across `--resume`,
  and compaction by `--autocompact` at 100k and above, or `/compact` after each mark's turn below it, counted only where
  its session log has a `compact_boundary`. A turn past its timeout is killed with its children, and Theseus's session
  stopped.
- **The scorer** (`score.py`): each probe by its own check (an indirect probe by its file, never the arm's words); the
  accuracy curve by salience and distance, with its half-life in turns and in tokens; confident-wrong, stale, citing
  where and when, dollars and milliseconds per probe, and accuracy by kind and carrier; `report.md`, `curve.svg`,
  `scores.json`.

**How it is proven.**
- **The session's** (47 tests): the check language as `check.rs` reads it; SplitMix64's reference stream and the smoke's
  pinned digest (`d30943acf7bfd1ac`); every cell filled, each fact before its probe; no name or value from the exam's
  file; a known curve's half-life worked by hand (20 turns); a stand-in `claude` driven turn by turn; the Theseus driver
  end to end on the workspace's binaries and the stand-in model (30 turns, every fact delivered, nothing left
  running). Five planted reverts, each caught; the whole suite under load once. By hand, the real `claude` 2.1.289
  against the stand-in model: the flags parse, print mode runs `/compact`, and the smoke delivered 9 of 9 facts.
- **The review** (R11): no conflict, no join fix; 47 of 47 under both Pythons, and 47 of 47 with none skipped on a host
  build; **5 of 5 planted reverts caught** (strict abstention, seed 7's digest, strict stale, an exam word among the
  invented people, the half-life's interpolation).
- **Live** (R11, $0.3594): `generate.py --seed 7 --size smoke` gave digest d30943acf7bfd1ac twice. **The smoke on
  Theseus** (`--memory-arm baseline`, Claude Sonnet 5.5, a throwaway container): 30 turns, **9 of 9 facts delivered**,
  compacted at turns 11 (a compaction) and 25 (a ring), nothing left running, 2.5 minutes. The scorer: recall 83% (5 of
  6), abstention 50% (1 of 2), half-life not reached; read as a person would, recall 6 of 6 and abstention 2 of 2. The
  Claude Code arm was not run: its driver runs the CLI on the host, which needs a container built for it.
- **What the smoke showed** (theseus-523y, P2, before the full run): the mark's bulk build log is a context overage on
  Theseus (the generator put it at 6,944 tokens at four bytes a token; the compiler estimated 24,338 for the request,
  34,074 at its upper bound, against 22,154 of room), so turn 10 was refused, nothing sent, and the compaction came at
  turn 11; the admission check missed "I can't tell" and "found no matches"; and the strict stale rule failed a right
  answer that gave the new port and retracted the old one.

**The join** (stack B's third merge, e4d09068 at 06:44:14, clean, a new directory, no join fix; the shared join and its
gate are in Item 167). In the gate the recall suite ran **47 of 47 with none skipped**, on main's
own fresh `target/debug` and today's bench profile, which closed R11's follow-up of running the binary tests against
today's profile. theseus-7gir.17 stays open: its runs at each memory milestone are part of the issue, and OpenClaw's arm
is not built.

**The install.** Nothing installs from `bench/`.

**Divergences.** Six families: five of theseus-exam's for their kinds and checks, and `said` for the user's own words.
The check language has no `calls` subjects, since the arms name their tools differently. Stale is strict (the old value
must be absent). The index is off for the `none` arm, as the exam's daemons have it.

**Known gaps.** theseus-523y's three fixes, then the full on Theseus alone first: about $8 to $11 and two to three hours
an arm (the owner's). R11's recommendations, not decided: widen the admission check and keep strict abstention; accept a
retracted stale value, counting the old value named in a column of its own; keep indirect probes answerable from an
arm's own notes, reported per arm; decide whether a ring trim moves a probe's bucket; build OpenClaw's driver after the
checks settle. The full size is untuned against a live run, and Claude Code's arm needs a throwaway container. Offline,
Theseus never compacts against the stand-in model, so its compaction path is proved by the scorer's tests and the live
smoke only.

### Item 170. sim2: the kernel simulator drives `/stop`, tasks under a parent, wakes due in a busy turn and the outbox through crashes, and its coverage check no longer flakes (theseus-celu.35, with theseus-81ig; the seventh cloud batch's sim2 session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; 3dad9a68 and 55cca822; reviewed 05:00 to 05:52 by local reviewer R8, stack K, on ef37f325; joined 07:16 at 3dba509d, a signed merge onto e4d09068, by the stack-K joiner after a 10-run stress with no retry; installed 2026-10-05 13:07 at 60b43fb6, install #4, in `theseus-sim` only)

**Why.** The kernel simulator (§8; races since Item 41, wakes since 37a) did not drive four things the kernel had
gained: the operator's `/stop`, tasks under a parent with their carves and reports, a wake that falls due while its
turn runs, and the outbox's posts. No task had a parent, so no report wake ever ran. And the gate's one kernel-sim test
was flaky (theseus-81ig, on `.config/nextest.toml`'s retry list): a raced run reproduces only up to its
first race, so its coverage counts, "series put back" and the racing thread's transactions, hung on later races, and a
gate could see "no series was put back" (cockpit-tabs' gate 2 retried it, Item 165).

**What landed** (10 files, +1,470 −56: `crates/theseus-sim` and `.config/nextest.toml` only; the kernel untouched).
- **theseus-81ig** (3dad9a68). `the_kernel_holds_its_invariants_under_seeded_faults` now runs `--seed 1 --seeds 2
  --steps 300 --p-race 0` and reads every coverage count from that run, which no second thread touches, asserting that
  no turn was raced. A new `the_kernel_holds_its_invariants_with_raced_turns` runs the old raced command and checks only
  that every invariant held, that turns were raced, and that the racing thread ran transactions. For that last count to
  be fixed by the seed, **a run's first race always appends a `RaceOp::Frame`**: a small change to what the sim does,
  made on purpose. `--verbose` prints each race's ops. The retry override is gone.
- **sim2's four operations** (55cca822), each a module of `kernel_sim/`:
  - **`/stop`** (`stops.rs`): `stop_execution` between turns, inside one (5% of conversations' turns), on the racing
    thread, `stop_call` of one call, and a task's stop, which must be refused and write nothing; crashes after each.
    After a stop the execution is open and not cancelled, keeps its session, limit, spend, **tasks and wakes**, its
    planned and authorized actions are declined (Cancelled), and its dispatched non-provider calls are told to stop and
    settle once the backends are killed; a stopped turn's `plan_action` and `set_wake` are refused, and its end leaves
    it waiting on input. From the ledger in WAL order: nothing of a stopped turn runs until it parks or is cancelled.
  - **Tasks** (`tasks.rs`): 15% of turns open a task, some running the same call again (found, nothing written), with
    depth-one and nothing-to-carve refusals. Invariants: the parent's carve for each task equals what the task can
    still spend; an ended task's report sits on its open parent until a turn reads it, once; `report_wakes` holds
    exactly the unread, not-cancelled reports of `wake_parent` tasks. A task's budget question is always declined in
    the sim (below).
  - **Wakes due in a busy turn** (`wakes.rs`): 15% of plain turns move the clock past the soonest wake before ending, and
    an end that would park a free execution with a wake due must leave it queued; right after the heartbeat's reconcile
    no open execution is due.
  - **The outbox** (`outbox.rs`): two in five turn ends stage a reply in the end's frame, and a fake binding plans
    notices outside a turn and delivers posts to a fake channel keyed by the post's id (Discord's nonce), refusing a
    tenth of first sends; crashes around each transition at twice the usual rate. Invariants: every OUTBOX record the
    WAL gains is one the sim wrote (no cancel, stop or reconciler writes a post); no post among actions or outstanding
    work; at most one copy of a post in the channel; a second settle writes nothing; at the end every post Succeeded
    with one copy or Failed with none.
  - `counts.rs` adds the run's sim2 counts to its TOTAL line, and the gate's `--p-race 0` run asserts each operation
    ran (on seeds 1 and 2: 17 stops, 10 tasks, 5 reports read, 7 wakes due in a turn, 50 posts staged and 54 planned,
    16 crashes around a post).

**How it is proven.**
- **The session's**: release runs of 40 seeds × 1,000 steps at both race rates, all invariants held (at `--p-race 0`
  293 stops, 278 tasks, 136 reports read, 222 wakes due in a turn, 4,900 posts settled, 822 crashes around a post); the
  deterministic run twice, identical; 50 runs of both kernel tests at nice 19 beside busy loops, 50 passed; four planted
  kernel bugs, each found within ten seeds (a stop leaving a declined action Authorized; `task_ended` not reaching the
  parent's reports; a second settle changing a post; a binding sending under a fresh key, two copies in the channel).
  No kernel bug was found.
- **The review** (R8): clean merge onto ef37f325, no join fix; **51 of 51 theseus-sim tests with no retry**; **2 of 2
  planted kernel bugs** caught by both kernel-sim tests at the gate's own 300 steps (the stop's declined action; the
  lost report). R8 read the invariants against the kernel and noted that the busy-turn and due-scan checks take the
  kernel's own predicates as their oracle, so a bug inside `due_now` would pass them (the rule has its own unit tests).
- **On the whole stack K** (sim2 with kernel-fixes, Item 173; release, nice 19): 200 seeds × 1,000
  steps held every invariant at `--p-race 0` (200,400 checks; 1,318 stops, 1,222 tasks, 617 reports read, 1,033 wakes due
  in a turn, 24,231 posts settled, 3,971 crashes around a post) and at 0.3 (3,163 raced turns, 797 transactions on the
  racing thread), every sim2 count above 0 at both rates. The sim test binary at nice 19 beside 16 busy loops: 10 of 10
  passed (the run stopped after 10 of its planned 20 to give the machine back to the joins).

**The join** (the stack-K joiner, 06:26 to 07:17). Lock taken at 06:29:39 behind lane restart-notify's and stack B's;
in one guarded call, a clean `merge-tree` dry run and the merge at 06:50:57 on e4d09068: no conflict, `resolve.py` a
marker check, no join fix, 10 files as reviewed. Warm clean (06:51:59). **A stress before the gate**, since R8's
loaded check had not run yet: both kernel-sim tests ten times at nice 19 with `--retries 0`, **10 of 10**, medians 13.0
and 11.6 s, under a load falling from 25 to 9. **The gate** (06:55:11 to 07:15:34; 908 s waiting while R8's 200-seed
release runs held the lock shared): **2,695 of 2,695**, no flaky retry, the two kernel-sim tests 11.11 and 9.69 s;
lifecycle cold start p50 20.3 and p95 20.9 ms, from the config copy p95 43.6 (one run, under its 57.1 limit), SIGKILL
and restart 24.2 / 26.1; L1 job start p50 5.33 ms; the turn plain 5 frames at p50 74.2 ms (one p95 sample at 124.1,
a metric with no limit) and tool-call 9 at 149.2. Pushed (about 07:15:45); the done line at 07:16:12; the branch
deleted; theseus-81ig and theseus-celu.35 closed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). The daemon is unchanged; `theseus-sim` ships with the install script's binaries, so
its `kernel-sim` is sim2's. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** Steps 2 to 5 are one commit, since the operations share one turn-end helper. The forced first-race
`Frame` and the always-declined task budget question are the session's choices, both for the owner (morning notes; R8
recommends accepting the first, and for the second keeping the product rule, an operator's approval trusted, and
teaching the sim a matching rule: a parent's allowance grows by each approved task reset). Held posts and the credential
request are driven by neither.

**Known gaps.** The sim's run time about doubled (7.9 and 7.2 s idle against main's 2 s; R8: keep it while the sim
tests are not a gate's longest pole, and lower the binding's 0.3 a step first if they become it). "Reports that land
together start one turn" is checked only as a turn or a stop between two report queues. The deadline's stop of a whole
tree (theseus-g11i) is not reached by the sim. Two kernel comments still say `/stop` drops wakes (`wakes.rs`'s
`drop_wakes`, `kernel.rs`'s `cancel`); `stop_execution` keeps them, as the sim now checks.

