# theseus-exam

M6's memory exam (steps 34a, 34b; design `docs/design/m6-memory.md` §2.9): items whose success needs something from
an earlier session, written into a scratch store, run through one scratch daemon per `[memory] arm`, and scored
deterministically. A tool of its own (its manifest's `tool` marker): the `theseus-exam` binary, run by hand beside
`theseusd`, never by the shipped binaries.

Key modules: `item.rs` (the exam as data, `exam/exam-v2.toml`), `fixture.rs` (the store writer), `check.rs`,
`render.rs` (the oracle's note), `drive.rs` (the cells), `daemon.rs` and `arms.rs` (a daemon per arm), `stats.rs`
and `report.rs`, `replay.rs`, `tender.rs` (the retrieval probe). Read by: the maintainer, through its binary.

## What's here

- `theseus-exam write-store` puts every item's past into a store as the product writes sessions; its manifest maps
  each item's keyed nodes to their ids and positions.
- `theseus-exam run` copies that store once per daemon (`none`, `bm25`, `baseline`), writes each config from a base
  config (`[memory] mode = "live"` and `arm`; Discord, the web UI and the MCP server off; no index for `none`),
  waits until each tender holds the store, keeps that state as the arm's snapshot, and then, for each run, starts
  fresh daemons from the snapshots, runs that run's cells, and stops them. `oracle`'s cells go to the `none` daemon,
  with the gold after the task.
- `theseus-exam report --out F` writes the frozen report: each arm's rate with its interval and n, the paired
  differences, cost per pass, the halves, the decision per feature, and what could not be measured. It names the
  digest of `docs/m6-ablation-plan.md`, which it embeds.
- `theseus-exam replay` (instrument 2) reads a copy of a store's recorded turns (their `recall.shadow` and
  `recall.ran` rows, each query rebuilt to its row's digest), serves another copy with a scratch daemon for its
  tender, recomputes `none`, `bm25` and `baseline` as of each turn through the real pipeline, and scores them against
  the silver labels.
- `theseus-exam probe` asks a running tender the exam's tasks directly, per arm of sources and weights.

## Invariants

- **The arm is the daemon's config, never a turn's field.** `turn.submit` carries no arm. A cell's recall rows must
  name its daemon's arm, and the `none` daemon writes none; either miss makes the cell an error.
- **The oracle's note is the core's render** (`theseus_core::recall::render`) of the gold, cut as the pack cuts an
  item (`theseus_memory::recall::excerpt`), read from the exam's store. Never format it by hand.
- **A daemon never lives across runs**: it indexes the turns it serves, so a second run on it would recall the
  first run's answers.
- **Nothing is left running**: a `Daemon` stops when dropped, and `arms::run` fails when any process still names
  its work directory.
- **The held-out half is run once**, after every choice is made (`--half out`).
- **The replay never trusts the index with `as_of`**: a hit at or after the turn's is dropped and counted as a leak.
- **A report is frozen**: `report --out` refuses a file that exists.

## Tests

- `cargo nextest run -p theseus-exam`. `tests/daemon_reads.rs` serves the written store with this workspace's
  `theseusd`; `tests/arms.rs` runs a small exam end to end through three real daemons on a stand-in model that
  answers from a recall note when its request has one, then replays the baseline daemon's store. Both find `theseusd` beside the test binary (a workspace
  build makes it), or `THESEUS_EXAM_THESEUSD`, and `tests/arms.rs` needs `theseus-index` beside it too.

## Traps

- Without the embedding model's files (`[index] weights_dir`), `baseline` recalls without vectors and equals `bm25`;
  the report says so from the rows' `skipped`.
- The base config is a scratch config: anything that listens or reaches out (`[mcp.servers]`, telemetry) runs once
  per daemon.
