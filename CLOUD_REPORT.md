# CLOUD_REPORT: fact-recall-a (theseus-2ced)

Branch `cloud/20261011-fact-recall-a`, one step, commit `ccb2e7c`.

## Step: `fact.retract`, the walk over reach, correction notes, one card

**Found.** `node.reach` already follows copies over `derived_from` (recall writes `via = "recall"`, publish `publish`,
consolidation `synthesis`), and `memory.used` rows (scope `recall:<session>`) say whether a recalled item was used.
Nothing walked the exposure and acted. Also found and fixed (a defect in code I was in): consolidation's clusters were
built from every recalled node, labeled wrong or not, so a wrong node could be synthesized again. It now leaves any node
the label set excludes out of the co-recalls (`consolidate/run.rs`), which is also how a stale synthesis's cluster comes
back "without the node".

**Changed (ccb2e7c).**
- Protocol: `crates/theseus-protocol/src/retract.rs` (`FactRetractParams/Result`, `RetractExposure`, `exposure_kind`),
  method `fact.retract`, ledger kinds `fact.retracted` and `fact.corrected`, TS regenerated (types added to the existing
  memory line of ts.rs).
- Core: `retract.rs` (the act, the sort, `Notes`), `fact/retract.rs`, `turn/retract_step.rs`, `Act::Retract`,
  dispatch (`rpc_acts` in rpc/memory.rs, one arm shared with `memory.label`: dispatch sat at clippy's 100-line limit),
  a `corrections` field on `Memory`, one line in `turn.rs`'s `catch_up`, the courier's `jev_post` arm gains `retraction`.
- CLI: `theseus retract <node-or-handle> <why...>` (`crates/theseus/src/retract.rs`: `table_lines` is the replaceable
  function), `OPERATORS` 31 -> 32.
- Docs: nearest AGENTS.md of core and protocol.

**Design choices to hear about.**
- *Which mechanism for the note:* neither recall's notes nor place warnings. A retraction queues the note in one META
  record `retract.notes` (new key, no format bump) and the next turn of that session writes it as the harness's own
  user-role node (same shape as a wake or report relay, author `harness:retraction`) in a frame of its own that also
  clears the queue, so it is written once. Writing straight into a running session from the RPC would race the turn's
  transcript handle. A turn with none pending costs two atomic loads (`Notes::waiting`).
- *Which exposures get a note:* recall copies whose `memory.used` says used, **or says nothing yet** (the memory pass
  runs after the turn; unknown is treated as used, and the table says so), the node's own session when its context held
  it, a report relay. Seen-only (`used: false`), retired sessions and imported ones are listed, no note. Tasks (a session
  record with `task`) go on the card, never get a note.
- *Card:* one post, kind `retraction`, to the owner's DM through `stage_to_operator` and the courier's `jev_post`
  (plain text with "Re-run?", no buttons); none when no task used the node or no DM is bound (the table still says so).
- *Published copies* are labeled `wrong` as well (they carry the node's text, recall would serve them otherwise).
- *Handles:* `[m:a1b2c3]` / `m:a1b2c3` is read as an id ending until provable-memory-a's handles join (then
  `retract_target` in retract.rs is the one place to change).
- *Place rule:* `FactRetractParams.session_id` names the session the request is run for; its place shapes the answer
  (a shared or unreadable place gets `hidden` rows, `place = "a private place"`, no gist if the node's session is
  private). The table is stored whole in the `fact.retracted` row and shaped on the way out.
- *Idempotence:* a second call answers from the stored row (`already = true`), writing nothing.
- No store format change (a ledger kind pair and a META key; `MemoryLabel` rows rode the same way).

**Proved.**
- `cargo nextest run --workspace -E 'package(theseus-core) and test(tests_retract)'`: 12 passed (planted fact in
  sessions that used it, a task that used it and one that only saw it, cited by a synthesis: four exposures sorted;
  next turn carries the note once and a second turn does not repeat it; synthesis `stale`, out of recall; one card;
  recall drops the node as `labeled_wrong`; second retraction writes nothing, by id, ending and handle; shared-place run
  hides private text; refused act writes nothing; a copy of a copy is found; two tasks share one card; no DM or no task
  means no card; the queue is read back from the store; consolidation proposes a stale synthesis's cluster again
  without the node).
- Planted reverts, each restored and `touch`ed (`git status` clean after):
  - label skipped -> `a_retraction_finds_every_exposure_and_sorts_each_right` and
    `a_stale_syntheses_cluster_is_proposed_again_without_the_node` fail;
  - walk cut to generation 1 -> `the_walk_goes_past_the_first_generation` fails;
  - note never queued -> `the_live_sessions_next_turn_carries_the_note_once`, `the_note_is_the_harnesss_own_node`,
    `a_waiting_note_is_read_back_from_the_store` fail;
  - a card per task (each post indexed) -> `two_tasks_share_one_card` fails (my first revert, which staged extra posts
    without indexing them, was *not* caught: `posts()` only sees indexed ones, so I redid it indexing each);
  - consolidation's wrong-node filter off -> `a_stale_syntheses_cluster_is_proposed_again_without_the_node` fails.
- CLI's `the_operators_methods_are_refused_inside_a_job` loops `OPERATORS`, so `fact.retract` is covered.
- `python3 scripts/keel-guard.py`: `keel: ok ... 0 findings`.
- No load runs: nothing here is a race (a node's note is delivered under the queue's mutex and one frame).

**Live check (done on this VM, scratch daemon, `/tmp/live2`, stand-in model, BM25-only index, `[memory] mode = "live"`).**
Seeded "Kestrel relay ... port 7714", recalled into two sessions and a task (`theseus ask`, `START ...` through the
stand-in's `task_create`), then:
- `theseus retract msg_...ac0621 "the relay moved to port 7715"`: 13 ms wall (store of a few hundred rows), table of
  three `seen` rows (the BM25-only stand-in has no entity extractor, so the memory pass wrote `used: false` for every
  recall: honest, not a bug);
- second `theseus retract 4ac0621 ...`: "already retracted ... Nothing was written now.";
- retracting the user's message in the session that had a second turn: table row `session ... a correction note goes
  into its tail at its next turn`; its next `ask` wrote the note (`theseus history` shows `Correction: “Fact for the
  record ...” was marked wrong on 2026-10-11: port moved to 7715`; `theseus ledger -k fact.corrected` has one row);
- `theseus memory search` then lists both nodes under `dropped ... for labeled_wrong`.
Not shown live: a task row and the card (needs a bound Discord DM, and the stand-in's attribution reads "not used");
the tests cover both.

**For the maintainer, on the owner's store** (scratch daemon on a copy of the store, never the owner's):
```
theseusd --config <scratch.toml> --socket <s> --state-dir <copy>          # [memory] mode = "live" for recall copies to exist
theseus --socket <s> memory search "<a known fact>"                        # pick a node id from the hits
time theseus --socket <s> retract <node> "<why>"                           # table; the walk should be well under 1 s
theseus --socket <s> retract <node> "<why>"                                # "already retracted ... Nothing was written now."
theseus --socket <s> ask -s <a listed session> "next"; theseus --socket <s> history <it> --full | grep Correction
theseus --socket <s> ledger -k fact.corrected
```
Only sessions with recall copies from `live` or `canary` mode have edges: shadow-mode recalls write none, so those
exposures are invisible to `node.reach` (and so to this).

**Left / uncertain.**
- Doc changes for you: spec Part III item and status row; theseus-protocol's notices doc may list post kinds
  (`retraction` is new; I did not edit it, discord-calls owns that file).
- `OPERATORS` array length and the courier's match arm will conflict with siblings' additions: trivial.
- `tests_registry` passes (reader rule): `fact.retract` read by the CLI, both new ledger kinds written.
- Quarantine, the CLI's full exposure table and the cockpit tree are `fact-recall-b`.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit
(lint, test, build), test build, reader rule all passed. Suite: 3757 run, 3724 passed, 33 failed, all the known root/L1
failures (theseus-sandbox `contract`, theseusd `sandbox`, none outside them). Then the later phases by hand: protocol
types (regenerated TS committed), `cargo deny fetch` then `cargo deny --offline check` ok, `theseus-sim bench turn
--check` ok (5 / 9 / 15 frames), `theseus-sim bench lifecycle --runs 10 --check`: first run `LIFECYCLE OK` (cold start
p50 28.4 ms, shutdown p50 16 ms, SIGKILL restart 31 ms, swap 32 ms); a second run missed `clean shutdown ... a job
running` at p95 918 ms (max of one sample, p50 16.2 ms) on this loaded 4-core VM, other phases ok; nothing here touches
the start path. The jobs bench's L1 row was not run (L1 refuses a root daemon here).
