# Cloud report: recall in shadow, step 30a (theseus-6fn.1)

Branch `cloud/20261004-memory-recall`, from `main` at d9b0931. Started 02:30 UTC, report 03:35 UTC (2026-10-04).

| Commit | Subject |
|---|---|
| 022d088 | memory: the MemoryScience trait, its baseline, and recall's filters and pack (theseus-6fn.1) |
| 56f8218 | core: recall in shadow on a turn's first loop, memory.search and memory.recalls (theseus-6fn.1) |

No new package (Cargo.lock gains only theseus-core's edge to theseus-memory). **No store format bump**: the step
adds one ledger kind (`recall.shadow`) and no record kind or field. A row of an unknown kind already reads on an
older build (`LedgerKind`'s rule), so `MANIFEST_FORMAT` stays as it was.

## Step 1: the science (022d088)

**Found.** theseus-memory held FSRS-6 and activation, reserved for row 52. Design §2.3's trait was not built yet.

**Changed.**
- `science.rs`: `MemoryScience` with §2.3's verbs (`gate`, `schedule`, `activate`, `decay_sweep`, `rank`), plus
  `id` and `min_score`.
- `Baseline` answers them by §2.3's table: cosine for the gate (0.92 merge, 0.75 supersede for a correction), no
  retention, no activation, age-only sweep hints, and the fused order for rank.
- `ScienceId` names the parameter set by an FNV-1a digest (`baseline@<16 hex>`), and every row carries it.
- `recall.rs`: the pipeline after the index. Each candidate goes to the first filter that takes it, in this order:
  `place`, `in_context`, `untrusted`, `recursion`, `threshold`. Then the science ranks the rest, and a greedy pack
  fills the budget (§2.4's 1,500 tokens, 6 items, 400 tokens an excerpt). A drop for the budget is `budget`, and a
  second chunk of an admitted node is `in_context`.
- `labeled_wrong` is left for 30b, whose `memory.label` produces it. Declaring it now would declare a reason nothing
  can produce.

**Proved.** `cargo nextest run -E 'package(theseus-memory)'`: 45 tests, all passing. They cover each filter's reason,
the pack's limits, the excerpt's cut, the baseline's verbs, and the parameter digest. The place property test,
`recall::tests::the_place_rule_holds_for_every_pack`, runs over generated candidates and places.

## Step 2: the wire-in (56f8218)

**Found.**
- Three facts shaped the hook: a turn's first loop is in `turn_body`, the runner did not hold the index tender, and
  the place of a session is `class_of`'s rule (`outbox.target`, else `outbox.wake_target`).
- `IndexHit.place` is the tender's own guess from META records. I did not use it. The core reads each candidate
  session's place itself.

**Changed.**

*The recall.*
- `crates/theseus-core/src/recall.rs` holds `Memory`: `[memory]`, the science, and who answers the query.
- The tender is asked only while it runs. Otherwise the answer is `unavailable` with its state and why, so recall
  never waits on a dead socket. Tests replace the tender with `Memory::set_ask`.
- The query is the turn's new user-message nodes (input, wakes, reports), their files' names, and the first 500
  characters of the reply before them. It reads `as_of` the first new node's position, with k = 40.
- The manifest is built here, and so is `TurnRunner::place_of`, which reads a place with `class_of`'s rule:
  - private → `Private`;
  - shared → `Shared(target)`;
  - unreadable → `Unknown`, which no place may draw on.

*The turn's hook, `turn/recall_step.rs`.*
- The index is asked in a spawned task as the first loop's model call goes out.
- Its answer is read right after the call returns, and never past `[memory] recall_deadline_ms`.
- In shadow, the request the model gets is the one compiled without recall.
- turn.rs gains the field, a `mod` line, and five lines around `call_model`; it is 3,408 lines, under its 3,500
  ceiling.

*The record, `fact/recall.rs`.*
- The `recall.shadow` row is the `RecallManifest`, scoped `recall:<session>`, so `memory.recalls` reads one
  session's rows with a scope scan. It rides in the turn's next frame through `Store::defer`.
- The row keeps references: node, session, position, rank, scores, and tokens. It never copies another session's
  text, and it keeps the query's length and the first 16 hex digits of its SHA-256, not the query.
- The span is `recall`, of kind `recall`, with the index's stages (bm25, entity, embed, scan, fuse) as children.
- The narrative line goes under `Context`, for example: "Recall (shadow) found 12 candidates in 34 ms (bm25 8) and
  would admit 3 (1,140 tokens) from 2 sessions; dropped 2 for the budget, 1 for its place."

*The protocol and the CLI.*
- `theseus-protocol/src/memory.rs` defines `MemorySearchParams`, `MemoryRecallsParams`, `MemoryRecallsResult`,
  `RecallManifest`, `RecallItem`, `RecallDrop`, and `RecallTimings`. The methods are `memory.search` and
  `memory.recalls`, and the TypeScript is regenerated.
- `rpc/memory.rs` serves them:
  - `memory.search` writes nothing, whatever the mode. With a session it runs as that session's place, the
    session's nodes in context; with none, as the CLI's private place.
  - `memory.recalls` returns the newest rows, each admitted item's text read from its node.
- `theseus memory search <QUERY> [--session S] [-k N]` and `theseus memory recalled <SESSION> [-n N]` render them
  (`render/memory.rs`).

*The config and the bench.*
- `[memory]` lives in `config/memory.rs`. It is off by default, and `mode = "shadow"` is the only other mode it
  accepts: `canary` and `live` fail as unknown variants.
- Its keys are `recall_budget_tokens`, `recall_max_items` (1 to 40), `recall_deadline_ms` (1 to 5,000), and
  `include_external`.
- The template documents the section commented. Its three template tests pass, with
  `the_templates_memory_section` added.
- The bench config (`theseus-sim` `bench_config`) runs recall in shadow. The lifecycle bench times every phase with
  it, and the turn bench counts a plain turn's frames with it.

*The crate guides.* theseus-memory's `reserved_for` is gone (theseus-core reads it now). theseus-memory gains
AGENTS.md and CLAUDE.md, and theseus-core's AGENTS.md gains a Recall entry.

### The place rule as built

- **A turn in a shared place** draws only on sessions whose current target is that same place. That includes a
  task of it, since a task's target is its parent's.
- **A turn in a private place** draws only on sessions whose place is private: the CLI and the web UI, an owner's
  DM, and a channel bound `private = true`.
- **A place that cannot be read** is no place's: the turn draws on none of it.
- The compiler's private-place rules say nothing more about other sessions, so nothing more was needed.

### Proof

**Tests in `crates/theseus-core/src/tests_recall.rs`** (7 tests):
- `a_turn_records_what_recall_would_admit_and_why_it_dropped_the_rest`: a private turn admits a CLI session's note.
  It drops a shared channel's session for `place` and its own earlier input for `in_context`. The raw row holds
  neither "heron" nor "weir", and `memory.recalls` reads the item's text from its node.
- `a_shared_place_recalls_only_its_own_sessions`: the pier channel admits its own task's note only. It drops the CLI,
  an owner's DM, a private channel, and another shared channel, each for `place`.
- `the_place_rule_holds_over_generated_stores`, **the place property test**: 24 generated stores per run, each up to
  11 sessions placed among the CLI, an owner's DM, another person's DM, a private channel, two shared channels, and a
  task of one. The asker is placed at random too. It runs through a whole core and a real turn, and asserts:
  - every hit the hand-written rule forbids is dropped for `place` and never admitted;
  - every hit the rule allows is not dropped for `place`;
  - a session displaced from its place by a later one is read as the rule reads it, as the CLI's.
- `a_stalled_index_never_holds_the_turn`: with the core's own tender not running, the row says `unavailable` and
  why. With a stand-in that never answers and a 50 ms deadline, the turn ends at once, with one loop, and the row
  says `deadline`.
- `memory_off_recalls_nothing`: the index is never asked, and no row is written.
- `shadow_writes_no_frame_and_changes_no_request_byte`: the same two turns on a core with memory off and one in
  shadow. The shadow core would admit the note. Both turns stay at or under 5 frames. The model gets the same
  bytes (system, messages, tools) on both cores, and the assistant nodes' `request_digest`s are equal.
- `memory_search_runs_the_pipeline_and_writes_nothing`: as the CLI's place, and as a shared session's place. No
  frame is written, and an unknown session is an error.

**Other new tests:**
- `config::memory::tests`: off by default; only shadow is accepted; each limit is checked.
- `fact::recall::tests`: the narrative line's words.
- the CLI's `render::memory::tests`.
- `the_bench_config_loads_and_stays_on_the_machine` now asserts that recall is on.

**Planted reverts**, each restored with a fresh mtime and `git status` checked clean of the plant:
1. **The place filter dropped** (the `may_draw_on` check removed from `theseus_memory::recall::filter`): 6 tests
   failed, both property tests among them.
   - `recall::tests::the_place_rule_holds_for_every_pack` (minimal input: `here = Private`)
   - `tests_recall::the_place_rule_holds_over_generated_stores` (minimal input: `asker = Cli`)
   - `recall::tests::each_filter_drops_with_its_reason`
   - `a_turn_records_…`, `a_shared_place_recalls_only_its_own_sessions`, `memory_search_runs_…`
2. **Shadow put its pack in front of the model** (in turn.rs, before `call_model`, the first hit's text appended to
   the compiled request): `shadow_writes_no_frame_and_changes_no_request_byte` failed with "shadow changed the
   request", and the other 6 passed.

**Under load** (AGENTS.md's recipe: four `sh` busy loops at nice 0, the tests at nice 19): the filter was
`(package(theseus-core) and test(/recall|memory/)) or package(theseus-memory)`, 56 tests a round. 5 of 5 rounds
passed 56 of 56, and the loops were killed by their pids.

**The lifecycle bench, for the shape** (`theseus-sim bench lifecycle --runs 10 --check`, debug, with `[memory]` in
shadow, idle 4-core VM): LIFECYCLE OK.

| Phase | p95 | Budget |
|---|---|---|
| Cold start | 13.3 ms | 50 ms |
| From the config copy | 13.9 ms | 50 ms |
| Clean shutdown | 7.0 ms | 100 ms |
| SIGKILL, then restart | 16.1 ms | 150 ms |
| Binary swap | 17.7 ms | 200 ms |

The turn bench: a plain turn's frames are 5 at the p95, against a budget of 5, with recall in shadow.

### A live check run here (not instead of the maintainer's)

- **Setup.** Two scratch daemons (debug build), each on a fresh state dir under `/tmp/r30a` with the real index tender
  (BM25 only, no weights). Each used the bench's config and fake `op`, with Discord off and a Python stand-in model
  that logs every request body. One daemon ran `mode = "shadow"`, the other `mode = "off"`.
- **The question.** In each daemon, session A: "Remember: the grey heron in the photo nests by the old weir at
  Millbrook." Then session B: "Where does the grey heron nest?"
- **What recall found.** `theseus memory recalled B` on the shadow daemon: `ran`, 1 candidate (bm25 1), would admit
  A's message (19 tokens), with its text.
- **The request is unchanged.** The model's last HTTP request body was byte-identical across the two daemons:
  sha256 `f0c5eaafa899e5e2…` on both, with no "weir" and no "Recalled". The `context.compiled` digests were equal too.
- **A stalled tender.** With the tender `SIGSTOP`ped, the turn took 276 ms (a 2 ms model, plus the 250 ms deadline),
  and the row says `deadline`.
- **A killed tender.** After `kill -9`, the next row says `unavailable: the index tender is backoff: it exited
  (signal 9) …`.
- **The narrative** (`narrative.watch`) had all four lines.
- **A trap for the maintainer:** a state dir under a long path makes the tender fail to bind ("path must be shorter
  than SUN_LEN"). Keep the scratch dir short.

## The live check for the maintainer

Run it on a copy of the owner's store, never `bindings.toml`, with a release-thin or debug build of the branch.

```bash
B=target/release-thin                 # or target/debug; theseus-index must sit beside theseusd
W=/tmp/r30a-live && mkdir -p $W/state  # keep it short: the tender's socket path must fit SUN_LEN
cp -a ~/.theseus/store $W/state/store
theseusd example-config > $W/config.toml   # or a copy of the operator's config
# Edit $W/config.toml: narrative = true at the top; [server] state_dir = "$W/state",
# socket = "$W/sock"; [web] enabled = false; [discord] enabled = false; and add:
#   [memory]
#   mode = "shadow"
$B/theseusd --config $W/config.toml --socket $W/sock --state-dir $W/state &
T="$B/theseus --socket $W/sock"
$T index status            # after ~2 s and the backfill: ready, through position N
```

1. **A fact from another session in the same kind of place** (two CLI sessions; both private):
   ```bash
   A=$($T sessions open); $T ask -s $A "Remember: the grey heron in the photo nests by the old weir at Millbrook."
   B2=$($T sessions open); $T ask -s $B2 "Where does the grey heron nest?"
   $T memory recalled $B2
   ```
   Should show `shadow · baseline@… · private · ran`, `would admit N`, with A's message among the items and its
   text under it. Other private sessions that mention herons may be admitted too, and B2's own nodes are dropped
   `in_context`.
2. **The request's digest is unchanged.** The model's reply must not mention the weir unless the model already knew
   it (in shadow it never sees the note).
   ```bash
   $T ledger -k context.compiled -s $B2 -n 1 --json | jq '.rows[].data.digest'
   ```
   For a strict A/B, start a second scratch daemon on a second copy of the store with `mode = "off"`, ask the same
   two questions in fresh sessions, and compare the same digest: equal. Equal digests with the same input mean the
   same request bytes.
3. **A shared place.** If the owner's store has a shared guild channel's session S, run
   `$T memory search "<a word from a private session>" --session S`. Every private session's hit should be
   `dropped … for place`.
4. **A stalled index.** Take the tender's pid from `index status`, run `kill -STOP <pid>`, ask in B2 again, then
   `kill -CONT <pid>`. The reply comes about 250 ms past the model's pace, and `memory recalled $B2 -n 1` says
   `deadline`.
5. **The narrative line.** `$T rpc narrative.watch | jq -r '.lines[].text' | grep Recall` shows "Recall (shadow)
   found …".
6. **Stop.** `$T shutdown`.

## What is left, or uncertain, and choices the owner should hear about

- **The deadline overlaps the model call.** The design puts recall before the call, under a 250 ms deadline. In
  shadow nothing needs it first, so I ask the index as the call goes out and read it after.
  - A healthy index costs the turn nothing.
  - A stalled one costs at most the deadline past the call: the full 250 ms only when the model answers faster than
    the deadline.
  - Canary and live (30b) must move the read before the call. `recall_begin` and `recall_end` are already split for
    that.
- **No format bump**, as said above. If the owner's rule counts a new ledger kind as a stored-shape change, bump it
  at the merge.
- **The row is scoped `recall:<session>`.** That is a new use of the store's scope table for ledger records, through
  the existing `NewRecord::scoped` and `Store::scope_after`. I touched no windowed ledger reads and no `RedbIndex`.
  - `memory.recalls` reads a session's whole scope and keeps the newest N. That is one row per turn, fine at today's
    sizes; if it grows, 30b can read from the tail of the scope.
- **A session displaced from its place by `/new`** loses its outbox target, so the rule reads it as the CLI's.
  - A shared place does not recall its own older sessions.
  - Private places can recall them; private places get everything anyway.
  - This is a recall miss, not a leak. Keeping a place's earlier sessions would need a record of every session a
    place had: a design choice for 30b or the books.
- **`memory.search` with no session runs as a private place.** It is a read for the operator, as `index.query`
  already is, and it shows text from any private session.
  - It is not gated by `judge_act`, nor refused inside a job. A job in a private place could already read the same
    through `index.query`.
  - A job in a shared place has no `proc.run`, so it cannot run either.
- **The threshold is 0** (`Baseline::min_score`): nothing is dropped for `threshold` until shadow's rows calibrate
  it. The filter and its reason are tested with a raised threshold.
- **Not built here:**
  - `context.compiled`'s `recall` summary, which §2.14 mentions. It would change a notification's shape while recall
    is shadow-only.
  - Health's `memory` block.
  - `[memory]`'s other keys (arm, canary_fraction, experiment, session_recall_cap_tokens, and the compaction and
    consolidation keys), each for its step.
  - The `bench recall` row (§2.12).
- **The cockpit (a later step) should show:**
  - per session, each turn's recall: the outcome, the science, the admitted items with each source's rank, fused
    score, and tokens, and the text on demand (`memory.recalls`);
  - the drops grouped by reason (place first);
  - the budget used against its limit;
  - the index's time against the deadline;
  - a search box over `memory.search` with an optional "as session" picker.
- **Docs to write at review:**
  - spec Part III: 30a's item;
  - the version line;
  - `docs/status.md`: the Updated line, the landed step, and row 52;
  - `docs/design/m6-memory.md`:
    - 30a's row and §3.2: "the audience property test" became the place property test;
    - §2.4: `index_down` is `unavailable`; the read overlaps the call in shadow; `labeled_wrong` waits for 30b;
    - §2.14: `memory.search` writes nothing whatever the mode; `[memory]` is off by default (not shadow), with
      shadow the only other mode for now;
  - `docs/design/README.md`'s reserved list: theseus-memory is no longer reserved.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree:
- fmt, shape, clippy, bench build, test build, and the reader rule (9 of 9): ok.
- **Suite: 1,771 tests, 1,739 passed, 32 failed, 10 skipped.** Every failure is a known VM case, not this step's:
  - 19 `theseus-sandbox::contract` tests and `theseus-sandbox::bench spawn_100`;
  - 12 `theseusd::sandbox` tests, among them `the_jobs_bench_l1_row`, `a_probe_script_in_l1_…`, and `l1_argv_…`.
  - Each says "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC" (theseus-pv6i).
- I then ran the phases after the suite myself, all ok:
  - the protocol types, ok once the generated TypeScript was staged (it is committed);
  - the turn bench, 5 frames;
  - `cargo deny --offline check` (the advisory database fetched today);
  - the web app's lint and build; the cockpit's lint, test, and build;
  - the committed web dist, unchanged.
- The timing benches are skipped, as a lane's gate skips them. The lifecycle bench ran separately (above).
- **The TZ.** Without `TZ`, `tests_output::the_cores_output_matches_its_golden` fails on this UTC VM, and on `main`
  too. The golden holds a negative UTC offset (`-#:#`) in a wake's time, and UTC prints `+#:#`.
  - Commit 022d088's gate ran without `TZ` and failed only on that and the 32 sandbox cases. That commit touches
    nothing the core links.
  - The golden passed under `TZ=America/Phoenix`.
  - The golden should mask the offset's sign, or set a TZ itself; that is not this step's file.
