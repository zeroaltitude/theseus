# CLOUD_REPORT: pattern-miner (theseus-pw1q.21.3)

Branch `cloud/20261017-pattern-miner`. Base: `40aa83cd` (main as cloned; store format 45, no format change here).
Commits: `d5fa51ee` (the miner, whole), `abc59867` (a test tightened). Report commit follows.

## What I found
- Node bodies hold everything §2.1 names: `ToolCall{tool,input,assistant_node,gate}`, `ToolResult{status,is_error,meta.exit_code,content,duration_ms}`,
  the assistant node's usage and `cost_usd`, and `provider.call` rows (`node_id`, `timing.total_ms`) for model seconds.
  The index dates a NODE/ledger record by its **write time**, not `created_at_ms`; the fold pages on that clock and assigns days by `created_at_ms`.
- Tool names in the repo are `fs.read`/`fs.edit`, not `file.read`/`file.edit`; the shapes use the real names.
- The brief's "5 occurrences in 3 executions" is read as: occurrences = runs (one turn's calls, each counted once per pattern), `min_sessions` = distinct sessions, `min_days` = distinct days (these are the §6.1 keys).

## What I changed
- `rsi/mine/` (new): `shape.rs` (normalization, result classes and features), `slots.rs` (CONSTANT/BOUND/INPUT/AUTHORED across occurrences), `motif.rs` (stretches, closed 3..16-grams once per run, thresholds, bench genericity, arms and hand-back exits), `align.rs` (Needleman-Wunsch to the medoid; core 60%, optional 25%), `cost.rs` (tokens kept as four parts, dollars, model/tool seconds, decisions, saving, §2.6 score), `read.rs` (incremental fold, cursor, day lists, vocabulary, scope key), `bench.rs` (ATIF-v1.7 and `theseus-history.json`), `night.rs` (the `Core` entries), `mod.rs` (the stable public types `Motif`, `Shape`, `SlotKind`, `Occurrence`, `Step`, `Branch`, `Arm`, `Exit`, `Input`, `Support`, `Run`, `Call`, `Vocab`, and `Motif::spec()`, the §2.5 skeleton with no name or description; `Motif::cause_key()` is the stable backlog key).
- `config/self_mint.rs` (new): every `[self.mint]` key with its default, `mine = false`; validation at load (thresholds >= 1, `bench_dirs` absolute or `~/`, known kinds); the template block. One field line plus its Default line in `config/self_improve.rs`; one `pub mod` line in `config.rs`.
- Backlog: `Obs.pattern`, `Meta.pattern` (serde default, no format change), `read::of_motif`, `rank::pattern_score` beside `score` (a fix cause scores exactly as before; a golden pins 1040 for the reference meta), `start.rs` never starts a pattern item (also not through the pipeline's `backlog_start_item`). `backlog_night` calls `mine_obs` after `graduate_obs`. The two other `Obs` constructors in `graduate.rs` got `pattern: None`.
- `theseus self patterns [--top N]` (`SelfCmd::Patterns`, `self_cmd/patterns.rs`, `docs/cli.md` line), RPC `self.patterns` (owner-only through `Act::SelfBacklog`, refused inside a job, in `client::OPERATORS`), protocol `patterns.rs` (+ `pub mod patterns;`, `SELF_PATTERNS` appended on an existing line of lib.rs: 2,729 -> 2,730 lines, ceiling untouched), TS regenerated and added.
- Ledger: `self.mine.ran {window, runs, calls_read, motifs, top:[digest,support,saving]}` (`fact/rsi_mine.rs`, `LedgerKind::SelfMineRan`, `SELF_ROWS`).
- `crates/theseus-core/AGENTS.md`: a paragraph on the miner.

Design choices the owner should hear about:
- Support unit is a **turn** ("run"), not a session; a session that lands on four days counts four.
- Left context 3 and right context 16 calls around a seed bound each window; steps in under 25% of windows are dropped.
- A constant's text is kept in the vocabulary only when it recurs (>= 3 uses in the window), passes the scrubber and the shape check; every other value is a keyed digest (0 when the scrubber or the secret-shape check refuses it). The digest key is in META `self.mine.cursor`.
- A background job's late result is not seen (a call whose result lands after the 10-minute settle reads class `unknown`).
- Motifs can be filed only as flows; the score's `work` is 1. The mint step picks the kind.
- Never-mined tools are a prefix list (`shape::NEVER`), plus `proc.run` of curl/wget/mail/ssh/scp/rsync. MCP tools are never mined.
- The night waits for a quiet machine once (`pressure::quiet_blocking`, 2 s) and runs on the backlog night's blocking-pool thread. Not on the start path. It is **not** SCHED_IDLE: the thread is shared.

## How I proved it
Tests (all in `cargo nextest`, TZ=America/Phoenix): shape goldens (10), slots (6), align (4), cost (4), motif counting (4), the synthetic ledger (11: the owner's land flow across 6 sessions on 4 days finds the arms `ok` (3), `err:1+conflict+rerere-resolved` -> commit -> test -> push (2), `err:1+conflict+rerere-left` -> hand-back (1), the sync stretch, thresholds, per-execution counting, never-mined cut), bench (7), read hygiene (6), config (2), rank (2), through the core (6, `rpc/tests_self_patterns.rs`: pattern cause filed and not started, a planted secret reaches no stored byte, cursor reads nothing twice, off/halted/`mine=false` read and write nothing, `self patterns` writes no frame and works with the mode off, owner-only).
Cost: `a_fortnight_of_a_busy_ledger_is_mined_in_seconds`: 840 runs, 20,248 calls, **1.64 s in a debug build, +30 MB** (high-water 69 MB), 50 motifs.

Planted reverts (restored and touched; `git status` clean after each):
| plant | failed |
|---|---|
| skip scrubber and secret shape | `tests_self_patterns::a_planted_secret_never_reaches_a_stored_shape` |
| count per occurrence, not per run (`last_run` check) | `motif::tests::a_run_with_two_stretches_counts_once`, `a_gram_is_counted_once_per_run_however_often_it_repeats` (the full-flow loop test also holds through the window rule; the final windows are per run) |
| drop a subcommand word | 3 `shape::tests` (`programs_keep_their_subcommand_words...`, `steps_and_command_lines...`, `a_subcommand_word_is_part_of_the_shape`) |
| loop cost not shared | 3 `cost::tests` |
| no closed rule | `motif::tests::only_the_longest_gram_with_the_same_support_is_closed`, `a_sub_pattern_with_more_support...` |
| `never_tool` false | `tests::a_call_that_is_never_mined_cuts_the_stretch`, `shape::tests::what_is_never_mined_is_named` |
| drop `min_days` | `tests::the_thresholds_are_five_occurrences_three_sessions_two_days` |
| bench genericity off | `bench::tests::a_bench_motif_with_a_task_path_constant_is_not_generic` |
| pattern score off | `rank::tests::fix_causes_rank_exactly_as_before_and_a_pattern_ranks_beside_them` |
| patterns startable | `tests_self_patterns::a_seeded_transcript_files_a_pattern_cause_and_starts_nothing` |
| **no stretch-subset dedupe** | **nothing failed**: on this data the dedupe never fires; it is unproven (I added an assertion that the land stretch is one motif) |

Keel guard (`scripts/keel-guard.py --base 40aa83cd --head HEAD --quiet`): empty, rc 0. (The clone's `origin/main` ref was older, then newer than my base; I pointed the local `refs/remotes/origin/main` at `40aa83cd` so the gate's door and keel judged against my base. No remote ref was pushed.)

## Live check for the maintainer
On a **copy** of the store, never the live one:
```
mkdir -p /tmp/mine-copy && cp -a ~/.theseus/store /tmp/mine-copy/state-store   # stop-copy or restore from a backup; place it as <state-dir>/store
printf '[secrets]\nanthropic_api_key = "env:SCRATCH_KEY"\n' > /tmp/mine-copy/theseus.toml
SCRATCH_KEY=x theseusd --config /tmp/mine-copy/theseus.toml --socket /tmp/mine-copy/sock --state-dir /tmp/mine-copy/state &
theseus --socket /tmp/mine-copy/sock self patterns --json --top 10
theseus --socket /tmp/mine-copy/sock shutdown
```
Expect `{"calls":N,"found":K,"rows":[...],"runs":R,"window_days":14}`; each row has shapes, arms, counts, cost and saving, never a value; run twice and confirm the second writes nothing (`theseus ledger head` unchanged). I ran the same against an empty scratch daemon on this VM: `{"calls":0,"found":0,"rows":[],"runs":0,"window_days":14}`, mode off, exit 0. I could not report a real top 10: no real store here.
The first read of a cold store covers 14 days, capped at 20,000 calls (`MAX_CALLS`); the nightly continues from its cursor.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` because this VM's disk allowance ran out twice): door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build all pass. Suite: 6,497 tests, 6,462 passed, **35 failed**: the 33 known L1 failures (`theseus-sandbox::contract` x20, `spawn_100`, `theseusd::sandbox` x12, `the_jobs_bench_l1_row`) and two timing tests under load, `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs` and `theseus::markdown rendering_costs_little_per_hundred_lines`, which pass alone (rerun). Phases after the suite: protocol types clean (`git diff` on `cockpit/src/protocol.gen` empty after `git add`), turn bench `--check --runs 5 --burst 0` 4 frames a loop and first-request budgets ok; lifecycle/jobs benches skipped (`NO_BENCH`); size step skipped (no release build).
Not run: any L1 test (root VM).

## Left or uncertain
- Docs to change at review: `docs/status.md` (the miner, `theseus self patterns`), spec Part III item, `docs/design/` roadmap row R5; `docs/technical-overview.md` if it lists `self` commands.
- Stretch-subset dedupe unproven (above). The `Exit::Handback` state names in the spec (`conflict_files`, `resolved_files`) are placeholders for the mint step.
- Bench thresholds (4 trials, 3 tasks) are constants, not config keys.
- `place` is `cli`/`dm`/`channel`/`task` from the outbox target's text (`dm` substring); a Discord target naming a channel with "dm" in it would be classed `dm`.
