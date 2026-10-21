# CLOUD_REPORT: onto-r8-r6 (branch cloud/20261021-onto-r8-r6)

## Line ceilings
No file is over its ceiling. `crates/theseus-protocol/src/lib.rs` is untouched (the new types are in `judge_runs.rs`,
added to the existing `ts.rs` type-list line). `replay.rs` (core) gained two lines (`if p.onto` call, `onto: None`) and
`replay_result` was trimmed by one line (`..ReplayJudgment::default()`) to stay under clippy's 100-line limit.

## What changed, per part

### 1. The labelled replay over the store (theseus-nngor): 5739f9c, 6c8c886
- `theseus-judge/src/replay.rs`: `replay_onto` split into `onto_asks` and `onto_scored` (it still composes them, its tests
  unchanged) so the core can make calls one at a time and keep each `Judgment` for its row.
- `theseus-core/src/learning/replay_onto.rs` (new): `Core::onto_replay_run` / `onto_replay`, `onto_cases` (every private,
  non-task session with an `Origin::Import` membership in an onto kind, newest first by `last_active_ms`, at most `limit`,
  default 50; a session with no readable first human message is left out with its reason); refused without
  `[judge] backfill_consent` and over `replay_limit_usd` (estimate refused with the numbers, before any call); calls through
  `Caller` (`judge.call` rows, `purpose: replay`, scope `judge.replay:onto`, one `judge.replay` row, same frame).
- `learning/onto_undo.rs` (new): pure `pub(crate) undo_rates(rows, since_ms) -> Vec<UndoRate>` (Jev's writes = rows whose
  `undo.write` starts `jdg_`; kind = the rows' kind; a write counted once per kind; undone = an `ontology.undo` naming it) and
  `rows_since` (paged by kind). Default window `WINDOW_MS` = 7 days.
- Wire: `JudgeReplayParams {onto, limit}`, `JudgeReplayResult.onto: Option<OntoReplayReport>` (+ `OntoKindScore`, `UndoRate`);
  `theseus judge replay --onto [--limit N]` prints the Markdown (then cost and each left-out session), `--json` the result.
  Generated TypeScript regenerated under `cockpit/src/protocol.gen/` (no cockpit code). `judge::onto::onto_input` is `pub(crate)`.
- README: "The accuracy part of the +onto report" in `bench/threads/README.md`.
- Design notes: the cases' labels are the import's memberships only; a session's *other* memberships (operator, Jev) still show as
  the session's own in the state sent to Jev (only labels are hidden, as the pure `hide_labels` does). The seeded `resource` kind
  does not list `import` in `assigned_by`, so on a real store only topic labels exist today; the test adds `import` to it.
  While the index's shape is being built the undo rates come back empty; the replay itself still runs.

### 2. The first +onto report's plan and format (theseus-ri5up): abda7fa
`bench/plans/threadbench-t13-onto.json` (k 3, A1o and A1, T12's model/effort and the three progression digests, `comparison`
naming A3 and A5 as read from T12, budget 33 to 40, pause 80, spend limit 25 per arm run), `bench/threads/jev-overlay.example.toml`
(`[judge]` keys, one `op://` placeholder), `report.py`: `onto_run_text` ("What T13 needs"), A1o in the reproduction arms and the T12
block, `onto_section` (A1o minus A1; rates pooled over an arm's runs, others the mean), `onto_replay_section` and `--onto-replay`,
`check_slots` (a figure holding two arms of one palette slot raises, naming both; `charts.py` untouched). README paragraph.
**Flag for the owner:** the plan's 33 to 40 dollars is the task's figure; T12's plan puts four arms at k 1 at 35 to 60, i.e. 9 to 15 dollars
an arm run, so six arm runs may cost 54 to 90. The plan's pause line is 80; watch the first pair's bill.

### 3. The decisions column (theseus-2e6c1): 1c6f25e
New `bench/threads/th_decisions.py`; `th_score.decisions` delegates; `th_threads.judgment_row` keeps per answer `{q, band, chose}` plus
`turn`, `session`, `at_ms` (only when present, so old rows are unchanged); `report.decisions_section` shows judgments, share of
messages with a decision and a per-question table, "not recorded" for old runs. A decision = a decision question at the act band
(including an act-band `none`, which `chose` tells apart). Two small additive edits outside the named functions: `th_threads` writes
`turn_id` and `ended_ms` into each `turns.jsonl` record (without them attribution falls to "between turns"); `th_score.decision_class`
delegates. Dollars stay per judgment and modelled. Old-run rows score by judgment (existing tests pass unchanged).

### 4. Thread and project in the manifest (theseus-w0vyp): fed769f
`ontology::harness_given`, `Walk.harness` (+ `with_harness`), appended to the composition's `memberships` after `compose`; recorded
thread/project entries are stripped from `compose`'s input so an append neither skips nor doubles them. `TurnRunner::walk` reads the
thread board once (`thread_and_project`, also one `get_session` for the project: one extra point read per walk); `rpc/ontology.rs`'s session
listing shows them. `thread_topics` (now unused) was deleted from `thread_context.rs`. No stored data, no request byte change.
Walks exist only while the ontology holds a category, as before.

## Tests and planted reverts
- Core: `learning::tests_replay_onto` (6 tests, incl. the key `an_import_labelled_store_replays_through_onto`), `tests_manifest_harness` (2).
  Judge/protocol/CLI: existing replay tests; `judge_runs::tests::the_onto_replay_round_trips`; `render::judge_runs::tests::an_onto_replay_prints_its_markdown_then_its_cost`.
- Python: `test_th_decisions.py` (15, new), `test_th_onto.py` (+7), `test_th_threads.py` (+1, the T13 plan). Whole `bench/threads` suite: 104 OK.
- Plants (each restored, `touch`ed, git status checked):
  - A, labels from Jev-origin memberships: 4 of 6 `tests_replay_onto` failed (key test among them).
  - B, skip `hide_labels`: `an_import_labelled_store_replays_through_onto` failed.
  - C, drop the consent check: `the_onto_replay_is_refused_without_backfill_consent` failed.
  - D, count every `ontology.undo`: `undo_rates_count_the_owners_undos_per_kind` failed.
  - Collision check dropped: `Palette.test_a_figure_that_holds_a_1o_and_pi_is_refused_naming_both` failed.
  - Count per judgment (decisions): 5 `test_th_decisions` failed.
  - Part 4, drop the append: both `tests_manifest_harness` tests failed; give them to `compose`: both failed.
- Not shown on a planted revert: the CLI render test and the protocol round trip (pure shape tests).

## Gate and what could not run
- `scripts/gate.sh` (TZ=America/Phoenix, no bench) first run failed in clippy (`replay_result` 101 lines): fixed in 6c8c886. Second run:
  door, keel, keel tests, fmt, shape, features, clippy, cockpit all passed; **test build failed: the VM's disk filled** (No space left on device
  while linking whole-workspace test binaries). So the workspace suite, protocol-types, lifecycle/jobs/turn/size phases and deny did not run in
  the gate. In their place: `cargo nextest run -p theseus-core -p theseus-judge -p theseus-protocol -p theseus --no-fail-fast` (TZ=America/Phoenix):
  4,685 tests, all passed (no L1 failures were hit in those crates). `the_cockpits_types_are_generated_from_the_rust_ones` passed and its output is committed.
  No run under load was needed. The turn frame budget test is in that suite and passed.
- The maintainer should run: `scripts/gate.sh` and `cargo nextest run --workspace` (theseusd/sandbox L1 tests fail as root here).
- Keel guard (`python3 scripts/keel-guard.py --base 1d092dede7 --head HEAD --quiet`): empty output, exit 0. (`origin/main` does not exist in this clone; base is the cloned main.)
- Store format: none changed (47).

## Live checks for the maintainer
1. On the owner's daemon, with `[judge] backfill_consent = true` and `replay_limit_usd` raised if needed (default 0.5 may refuse 50 sessions; the refusal prints the estimate):
   `theseus judge replay --onto --limit 50 > /tmp/onto-replay.md`. Expect the per-kind tables at both floors, left-out sessions with reasons, the
   undo rates, and cost. The dollar cost is not measured here: read the daemon's printed estimate.
2. First +onto bench: the commands are in `report.py`'s "What T13 needs" (generate, drive A1o and A1 per run under `bench/plans/threadbench-t13-onto.json`
   with `--config-overlay`, score, report with `--onto-replay`), 33 to 40 dollars per the plan, probably more (see part 2).
3. `theseus ontology list --session <id>` for a threaded, project-attached session shows `thread:<slug>` and `project:<tsk>` (origin transport).

## Follow-ups
- Measured main-model price once a real run exists; the replay's cost per session once run live.
- A session's other (operator/Jev) memberships still reach the replay's state; decide whether the replay should hide them too.
- Part 2/3 docs: the spec's Part III item and `docs/status.md` for the four parts are the maintainer's.
