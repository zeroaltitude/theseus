# Cloud report: the memory exam over the real recall pipeline (theseus-6fn.5, step 34b's wire-in)

Branch `cloud/20261004-memory-arm`, from `main` at 760553f (30b merged). Started 10:04 UTC, report written 11:35 UTC.
Every task step is done, the replay (step 6) included. Three commits, each gated:

| Commit | Step(s) |
|---|---|
| a563077 | 1: `[memory] arm` gains `bm25`; a daemon asks the index for its arm's sources |
| d9356d4 | 2 to 5: the driver over real arms, the report over four arms, `docs/m6-ablation-plan.md`, the `tool` marker and the crate's AGENTS.md |
| a676f82 | 6: the replay, with the silver labels and the leakage test |

Steps 2 to 5 went in one commit. They all change one crate and depend on each other: the report embeds the plan, the
driver's records feed the report, and the marker changes with the crate's command. So they make one green step.

## Step 1: the `bm25` arm (a563077)

**What I found.** 30b's `MemoryArm` had `none` and `baseline`. `Memory::begin` sent the index no `sources`, so
`baseline` got the tender's default: BM25 and entities, plus vectors only in `hybrid` mode. A tender without model
files therefore answered `baseline` silently, with nothing in the row to show vectors were missing. In `live` mode with
arm `none`, `assign` returned `live: false`, so the turn still ran a **shadow** recall: the `none` daemon asked the
index on every turn.

**What I changed.**
- `MemoryArm::Bm25` (`"bm25"`), plus `MemoryArm::sources()`: `none` asks for nothing, `bm25` asks for `["bm25",
  "entity"]`, and `baseline` asks for `["bm25", "entity", "vector"]`. Naming vectors outright for `baseline` means a
  tender without its model now says so in the row's `skipped` (`bm25_only: no weights in …`). In hybrid mode the
  behaviour is unchanged.
- `Memory::begin` takes the sources. A live arm asks for its own. Shadow, a canary's control, and `memory.search` ask
  for `baseline`'s.
- `live` with arm `none` asks the index nothing. It still writes the `memory.arm` row as before. A canary's control
  still runs `baseline` in shadow, as 30b intended.
- The template's `arm` line names `bm25`.

**Store format: no bump.** The rows name the arm through 30b's existing fields: `memory.arm`'s `arm`, the manifest's
`arm`, and the `Recall` node's `arm`. They name the sources through the manifest's existing `sources` (hits per
source) and `skipped` (asked for, did not answer). I added no stored field. A dedicated "asked sources" field would be
more explicit but would cost a `MANIFEST_FORMAT` bump; I chose not to add it, and the owner may decide otherwise.

**How I proved it.**
- `tests_memory_arm` (new, 2 tests):
  - In live mode, `none` makes zero index queries and writes no recall row; its `memory.arm` row says `none`,
    `live: false`.
  - `bm25` asks for `["bm25","entity"]`, and its `recall.ran` row and `Recall` node say `bm25`.
  - `baseline` asks for all three sources.
  - Shadow and a canary's control ask for `baseline`'s sources even when `arm = "bm25"`.
- `config::memory` test extended: `none`, `bm25` and `baseline` parse. `+rerank` is still refused (32c adds it).
- The 30a/30b suites (`tests_recall`, `tests_recall_node`) pass unchanged: 23 of 23.
- Under load (nice 19 beside four busy loops at nice 0): the two new tests passed 10 of 10 runs.
- Planted reverts:
  1. Restoring the old shadow recall for live `none` fails `a_daemon_runs_the_arm_its_config_names` ("none asks the
     index nothing: [IndexQueryParams { … sources: ["bm25","entity","vector"] … }]").
  2. Giving the live arm `baseline`'s sources fails the same test at the `bm25` sources assertion.
  - The file was restored and `touch`ed after each, and `git status` was clean.

## Step 2: the driver over real arms (d9356d4)

**What I changed.** `theseus-exam run` is now the one command (`arms.rs`, `daemon.rs`, `drive.rs`).

1. **Oracle notes.** It reads the oracle's notes from the exam's store before any daemon serves a copy of it.
2. **Prepare.** For each daemon (`none`, `bm25`, `baseline`):
   - copies the store to `<work>/prepared/<arm>/state/store`;
   - writes its config from `--base-config`: `[memory] mode = "live"` and `arm` set; Discord, the web UI and
     `[mcp_server]` off; no index tender for `none`;
   - starts `theseusd --config --state-dir --socket`, with `THESEUS_CONFIG`, `THESEUS_STATE_DIR`, `THESEUS_SOCKET` and
     `THESEUS_SESSION` removed from its environment;
   - waits until it serves and its tender has read through the store's last position (and has embedded it, in
     `hybrid` mode), then stops it.

   All daemons start before any is waited on, so their tenders index in parallel. That stopped state is the arm's
   snapshot (`READY`); a resumed run reuses it.
3. **Each run** starts fresh daemons from the snapshots in `<work>/run-<n>/<arm>`. It runs that run's cells with the
   existing `drive::run`, so the seeded paired order, the spend cap and resume are unchanged, and then stops the
   daemons.
   - **Why fresh daemons per run.** A daemon indexes the turns it serves. One that lived across runs would recall an
     item's earlier answer in its next run.
   - **What remains.** Within a run, a cell may still recall another item's cell from the same run. I report this
     below rather than fix it, because a fix needs a core `exclude_sessions`, which is outside the task's core scope.
4. **Oracle cells** go to the `none` daemon. Each cell reads its session's recall rows (`memory.recalls`):
   - every row must name its daemon's arm, and the `none` daemon must write none; otherwise the cell is an error;
   - the record keeps each row's arm, mode, science digest, outcome, candidates, admitted, gold admitted, tokens,
     sources, skipped sources and `total_ms`.
5. **Nothing left running.** A dropped `Daemon` stops: `shutdown`, then a kill after 20 s, then any process whose
   command line names its directory (its tender). The command fails if any process still names the work directory.
   A half-made directory is moved aside as `<name>.bad-<ms>`, never deleted.

**The oracle note** (`render.rs`) is now the core's own `theseus_core::recall::render::render` of a `Recall` node of
the gold:
- each item is cut as the pack cuts one (`theseus_memory::recall::excerpt`, 400 tokens);
- each has `frozen_range` and `header`, read from the store by id;
- it is sent **after** the task (`task\n\n<note>`), where 30b's note sits;
- `note` gains `--store`.

**One remaining byte difference.** The core's note is its own text block after the message's, while the oracle's is
inside the message's single text block. The model reads the same characters in the same order.

**How I proved it.**
- `tests/arms.rs`, end to end:
  - a two-item exam written into a store;
  - `arms::run` with all four arms, **two runs** on fresh daemons;
  - three real `theseusd` daemons, each `bm25`/`baseline` one with a real `theseus-index` tender;
  - a stand-in Messages API model on 127.0.0.1 that answers with the note when its request holds `[Recalled:`, and
    otherwise says "I have no record of that";
  - `recall_max_items = 1`, so each pack holds its best hit alone.

  Results:
  - all 16 cells have a verdict;
  - `none` passes 0 of 4, while `bm25`, `baseline` and `oracle` pass 4 of 4;
  - every `bm25`/`baseline` row names its arm, mode `live`, outcome `ran`, science `baseline@…`, and gold admitted 1;
  - `bm25` rows never name vectors, and `baseline` rows show `vector` in `skipped`;
  - `none` and `oracle` cells have no recall rows;
  - **the oracle note equals the baseline daemon's rendered `Recall` block byte for byte**, compared against the
    request the model received;
  - no stop had to kill anything, and no process names the work directory afterwards.
- Under load (nice 19, four busy loops): 5 of 5 passes, about 34 s each (11 s unloaded).
- Planted reverts:
  1. Every daemon given `baseline`: fails ("the none daemon recalled (a live row, arm baseline): its config runs
     another arm").
  2. Every daemon given `none`: fails ("baseline fact-1: []").
  3. The note sent before the task: fails `render::the_input_is_the_task_then_the_note` and the e2e equality.
  4. A hand-made header (34a's old format): fails `render::the_note_is_the_cores_render_of_the_gold` and the e2e
     equality.
  - Each file was restored, `touch`ed, and verified with `cmp`/`git status`.
- Unit tests:
  - `daemon::each_arms_config_sets_memory_and_turns_off_what_would_collide`;
  - `daemon::a_tender_is_settled_once_it_holds_the_store`;
  - `arms::oracle_shares_nones_daemon_…`;
  - `arms::a_copy_leaves_out_sockets_and_a_half_made_directory_is_moved_aside`;
  - `drive::arms_parse_by_name_and_oracle_runs_on_nones_daemon`.

## Step 3: the report over four arms (d9356d4)

`theseus-exam report --runs F [--rescore] [--out F]` contains:
- the plan's digest (`docs/m6-ablation-plan.md`, embedded by `include_str!`), the exam's digest, the models, and the
  data window;
- each arm's science digest and the sources that answered, read from its rows;
- each arm's pass rate with its 95% t interval over items and its n (items, then cells), for all items, held in, and
  held out;
- the paired differences clustered by item, `baseline − none`, `bm25 − none`, `oracle − none`, `baseline − bm25` and
  `oracle − baseline`, per half, each saying `gain`, `loss` or `insufficient`;
- cost per pass, latency, recall p95, and gold admitted;
- the decision per feature, for recall (`baseline` against `none`) and vectors (`baseline` against `bm25`), with the
  clause that decided it:
  - clause 1 if a private-family item failed under the arm in a run its comparison passed;
  - otherwise clause 3, "insufficient": no canary data, n = 0 sessions per arm against the plan's 120, with the
    exam's held-in n and its held-in and held-out readings;
- what could not be measured, and why: the canary; vectors, when rows show them skipped; a half or an arm not run;
  the replay;
- each item by arm, and the errors.

`--out` writes the report frozen: a file that exists is refused.

**How I proved it.**
- `report::the_report_reads_as_computed_by_hand`: four held-in items × 3 runs and two held-out items × 1 run, under
  four arms. Every asserted cell was computed by hand **before** the first run, with t(1) 12.706, t(3) 3.1824 and
  t(5) 2.5706, and the code matched on its first run. Examples:
  - `none` all: 17% [0, 60] (n = 6, 14);
  - `baseline − none` held in: +58 [-21, +100], 3/0/1, p 0.250, insufficient;
  - `oracle − none` all: +83 [+40, +100], gain;
  - `baseline − bm25` held in: +17 [-14, +47], insufficient;
  - none's cost per pass: $0.04667;
  - the clause-3 row, with "held-in n = 4 items".
- `a_disclosure_the_feature_caused_is_clause_one`.
- `an_interval_that_holds_zero_or_is_missing_cannot_decide`.
- `a_report_is_frozen`.

## Step 4: `docs/m6-ablation-plan.md` (d9356d4, words on the replay amended in a676f82)

The plan follows §2.9's list:
- the arms and their versions: the daemon's science digest per row, and the exam's digest;
- the primary metrics, and the unit (item for the exam, session for the canary);
- the minimum samples: 36 held-in items × 3 runs per arm, then the held-out half once, and 120 canary sessions per
  arm;
- the decision rule, verbatim from §2.9, with the exam's reading of clause 1;
- the analysis.

It pins no digest of `baseline`, so 31a's re-versioning needs no edit; reports name whatever digest the rows carry.
The current digest is `sha256:fd69644d…b53b`, which the report prints.

## Step 5: the reader rule (d9356d4)

- `reserved_for` is now `tool = "run by hand beside theseusd: …"`. `tests_registry` passes: the crate builds a binary
  and nothing ships it.
- `crates/theseus-exam/AGENTS.md` and `CLAUDE.md` are new. The root AGENTS.md's map gains a line (now 19.4 KB, under
  20 KB). `crates/theseus-store/AGENTS.md` no longer calls the crate reserved.
- lib.rs's 34a note ("the driver becomes `theseus-sim exam`") now says the driver and scoring stay in this crate.
  Reasons: the exam needs no code in the shipped binaries, and theseus-sim stays the gate's tool.

## Step 6: the replay (a676f82)

`theseus-exam replay --base-config F --store <copy> --work DIR [--out F]`:
1. **Read.** Reads a copy of the store: every node, every turn with a `recall.shadow` or `recall.ran` row, and the
   labels. It rebuilds each turn's query with the core's `query_of` and keeps the turn only when the query's digest
   and `as_of` equal the row's; otherwise it counts the turn as left out.
2. **Serve.** Serves another copy with a scratch daemon (tender on, memory off) and waits until it holds the store.
3. **Recompute.** Recomputes `none`, `bm25` and `baseline` per turn:
   - each asks for its sources with `as_of`, and the hits go through the real pipeline (baseline science, default
     pack, own session in context, nodes labeled wrong or stale before the turn left out);
   - **a hit at or after `as_of` is dropped and counted as a leak**;
   - no place rule is applied, because a copy of a store has no outbox. The replay says so.
4. **Labels.**
   - re-supply: an 8-word run only; there is no cosine, because no model runs;
   - reference: issue ids, hashes and paths first seen in another session;
   - re-derivation: the same tool and input as an older call; the label is that call's result;
   - should-have: the operator's `should_have` label on the turn's recall.
5. **Metrics.** Recall, precision and MRR of the pack, per arm and label, and the stale rate. The stale rate counts
   admitted nodes labeled wrong or stale *after* the turn; the record holds no supersession, so "already stale at the
   turn" cannot be read, and the replay says so.

**How I proved it.**
- `replay::the_silver_labels_read_from_the_record`: each label kind; a later node sharing the run is not labeled; 7
  words is no run.
- `identifiers_are_issue_ids_hashes_and_paths`.
- `the_metrics_read_as_computed_by_hand`.
- **The leakage test**, `a_node_written_after_a_turn_never_appears_in_its_recall`: an index that offers every node
  regardless of `as_of`; the later node is never admitted, and 2 leaks are counted.
- `tests/arms.rs` replays its run-1 `baseline` daemon's store: 2 turns rebuilt to their rows' digests, 0 left out,
  `none` admits 0, both arms admit, 0 leaks from the real tender, nothing left running.
- Under load: 3 of 3 passes (about 38 s each).
- Planted revert: with the `as_of` guard off, the leakage test fails (`bm25: Pack { admitted: ["old", "after"], … }`)
  and so does the metrics test. The file was restored and `cmp`'d.

## My own live check of the CLI build (stand-in model, no keys)

1. **Setup.** The full exam-v2 store: 758 sessions, 1,550 keyed nodes, through @5412. `theseus-exam run --half in
   --runs 1 --profile sonnet --workers 4` with all four arms, against `theseus-sim fake-model`. The fake answers a
   fixed text, so no check passes; the run exercises the pipeline, not the answers.
2. **Run.** 144 cells, 0 errors, 0 kills, about 10 s. Each tender held the store through @5433 (`bm25_only`). No
   process named the work directory afterwards.
3. **Report.** `report --out` wrote the report. A second `--out` to the same file was refused ("a report is frozen").
   The report shows:
   - 36 recall rows per arm, science `baseline@46038939f14a4f49`;
   - the vectors line, "recalled without vectors in 36 of its 36 recalls";
   - recall p95 of 10 to 11 ms;
   - clause 3 for both features, held-in n = 36.
4. **A real retrieval number.** At the default pack (6 items), the real pipeline admitted **17 of the 38 held-in gold
   nodes** under `bm25`. `baseline` equals `bm25` here, because there are no vectors.
5. **Replay.** `theseus-exam replay` over a copy of that run's `bm25` store: 36 turns rebuilt, 0 left out, 216
   admitted per arm (the same as the live rows), 0 leaks, nothing left running. There are no silver labels in an
   exam store, and the replay says so.

## The live check for the maintainer (GLM key, model files)

The base config is a scratch config. It needs:
- `[model] live = "glm"`;
- the `zai_api_key` secret, as `op://`, `env:` or `file:`;
- `[index] weights_dir` pointing at the embedding model's files;
- `[index] idle_unload_mins = 600`, so the model does not unload mid-run. A recall asks with `wait_ms = 0`, so a
  model that unloaded answers without vectors, and the rows say so in `skipped`;
- optionally `[memory] recall_deadline_ms = 1000`, which the arms share.

```bash
B=~/scratch/m6-34b && mkdir -p $B          # copy the scratch base config to $B/base.toml first
theseus-exam write-store --store $B/store --manifest $B/m.json
# The four arms over the held-in half, 3 runs each (~430 cells; GLM Flash, about $1-3):
theseus-exam run --base-config $B/base.toml --store $B/store --manifest $B/m.json \
  --work $B/work-in --out $B/runs.jsonl --half in --runs 3 --limit-usd 5 --profile glm
theseus-exam report --runs $B/runs.jsonl --out $B/report-held-in.md
# Then the held-out half, once, into the same records:
theseus-exam run --base-config $B/base.toml --store $B/store --manifest $B/m.json \
  --work $B/work-out --out $B/runs.jsonl --half out --runs 1 --limit-usd 5 --profile glm
theseus-exam report --runs $B/runs.jsonl --out $B/report.md
pgrep -fa "$B/work" || echo "no daemon left"
```

What each step should show:
- `write-store`: "758 sessions, 1550 keyed nodes, last position 5412".
- `run`:
  - stderr shows each tender holding the store in `"hybrid"` mode, with vectors equal to chunks;
  - the last line is a JSON summary with `errors` 0 (or few), `stopped_at_cap` false, `killed` [], and `spent_usd`
    under the cap.
- The report:
  - four arms' rows with n = 36 items and 108 cells each (held in);
  - `bm25` and `baseline` falling between `none` and `oracle` (34a measured `oracle − none` at about +74 points on
    exam-v1), or whatever the run finds instead;
  - `baseline − bm25` now measuring vectors: the "Vectors" line should be absent;
  - the science digest each arm ran;
  - both features at clause 3 (no canary);
  - after the held-out run, held-out columns with n = 36 and 36.
- `pgrep`: nothing left running.
- Optionally, `theseus-exam replay --base-config $B/base.toml --store <a copy of the operator's store> --work
  $B/replay` over the shadow rows (instrument 2). It should show the turns replayed, how many were left out, and the
  labels found.

## What is left, or uncertain, for the owner

- **Within-run cross-item recall.** A cell may recall another item's cell from the same run, on the same daemon. Its
  task and reply share words with neighbours in the same family. Excluding the exam's own cell sessions needs the
  core to pass `exclude_sessions`, or a filter on session label; I left the core alone.
- **Tender status after a restart from a snapshot.** In my run, the `bm25` daemon's tender reported `0 chunks` in its
  vectors block while the `baseline` daemon's reported 1,584; both were `bm25_only`. If a hybrid tender restarted
  from a snapshot reports chunks as 0 for a moment, `settled` could pass before the vector catalog has loaded. If the
  maintainer's run shows `vector` in `skipped` early on, have the driver wait on `vectors.chunks > 0` in hybrid mode.
- **The replay applies no place rule** (it reads no outbox), so its precision counts nodes a shared place would not
  draw on. Re-supply has no cosine. "Already stale" cannot be read from the record.
- **Disclosure (clause 1) in the exam** is read from the private family alone: a private item the arm failed in a run
  where its comparison passed. A failure of the check for another reason would also count; that errs safe.
- **Rows name sources through existing fields** (see step 1). An explicit "asked" field would need a format bump.
- **No `ablation.report` row, and no `<state>/ablation/`.** The cockpit replaced the Observatory, so the report is a
  file the exam writes, as the task said.

## Docs the maintainer should change

- `docs/design/README.md`: the reserved-crates table's `theseus-exam … row 55` row. It is a tool now.
- `docs/design/m6-memory.md`:
  - §3.1's 34b row and §2.9: the replay is `theseus-exam replay`, not `theseus-sim ablate replay`; the report is
    `theseus-exam report --out`, not `theseus ablate report`; there is no `ablation.report` row;
  - §2.9: the oracle note now sits after the task and is the core's render.
- Part III: the step's item, with the 17-of-38 gold-admitted number from the stand-in run and whatever the GLM run
  finds.
- `docs/status.md`.

## The gate

Three gates were run, `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, one before each commit. Each failed in the suite
only on known or environmental cases, and the phases after it passed each time: protocol types unchanged, turn bench
5/9 frames ok, `cargo deny --offline check` all ok.

**The last gate (a676f82):** 2065 tests run, 2031 passed, 1 flaky, 34 failed, 17 skipped. fmt, shape, features,
clippy, cockpit, test build and the reader rule all passed. The 34 failures:
- **33 sandbox tests that cannot run as root** (theseus-pv6i, known): 19 `theseus-sandbox::contract` clauses, 1
  `theseus-sandbox::bench spawn_100`, and 13 `theseusd::sandbox` tests.
- **1, `theseus-core tests_output::the_cores_output_matches_its_golden`: environmental.** The golden records a
  `wake.at` preview with a negative UTC offset (`-#:#`), and this VM runs at UTC (`+#:#`). It passes with
  `TZ=America/Denver`. This change touches no wake or output code. I did not run `main`'s build to confirm it
  fails there too. The golden assumes the
  owner's timezone, which the maintainer may want to file.

The one flaky test, `theseus-sim the_kernel_holds_its_invariants_under_seeded_faults`, is on the flaky list
(theseus-81ig) and passed on retry.

All 61 theseus-exam tests and all new core tests passed in every gate.
