# Cloud report: theseus-0j2.2 (L3, `theseus judge prove` generator)

Branch `cloud/20261004-judge-prove`.

## Step: the prove report generator
**Found.** crates/theseus-judge has the learning math (learn.rs) but no exit report. It had no AGENTS.md, so I followed its lib.rs docs and style.

**Changed.** Commit 8ea50b1. Everything is inside crates/theseus-judge:
- `src/prove.rs`: the generator. Its module docs define the input shape (JSONL, one finished task per line: `task, arm, success|null, spend_micros (judge included), judge_micros, turns, nudges, unnecessary_nudges, false_completion, stops[{decision, should_stop}]`).
- `src/bin/theseus-judge.rs`: `theseus-judge prove <records.jsonl> [--json P|-] [--markdown P|-] [--min-tasks N] [--min-labeled N]`. Markdown goes to stdout by default. Exit 0 whenever a report was made; exit 1 on bad input.
- `fixtures/prove/`: `canary_wins.jsonl`, `small.jsonl`, and the Markdown golden (`THESEUS_JUDGE_BLESS=1` rewrites it).
- `src/tests_prove.rs` (13 tests) and `tests/prove_cli.rs` (3 tests).
- No new dependencies. The binary parses its flags by hand, so no feature flag is needed. Cargo.lock is unchanged.

**Design choices the owner should hear about.**
- Minimums are 30 labeled tasks per arm and 30 labeled items per precision, recall, false-completion or nudge rate. The 30 is `learn::Minimum.per_acting_class`; 200 is for promotion holdouts. Both are flags.
- A metric under its minimum has no value and says "labeled tasks: 29 of 30". The verdict is `insufficient` when either arm is short.
- The verdict rests on the per-dollar completion difference (canary minus control).
  - A per-task difference wholly below zero also makes it `canary_worse`, so a cheaper but much less successful canary is never called better.
  - `canary_better` needs the per-dollar interval wholly above zero.
  - An unequal total spend (canary over control outside 0.80 to 1.25) is flagged in the verdict's reasons. Per-dollar rates carry the comparison.
- A record without `success` is rejected. `null` means "outcome unknown": the task is counted and left out of every rate.
- Interval methods: Wilson for proportions, Newcombe's hybrid for their difference, the ratio estimator for per-dollar rates, and normal for means and differences.
- The wire-in must supply `success`, `false_completion` and the stop labels from the ledger exactly as §2.9 defines them. The generator does not recompute them.

**Proof.**
- `cargo test -p theseus-judge`: 102 passed, 0 failed (13 new lib tests). `cargo test -p theseus-judge --test prove_cli`: 3 passed.
- Exact metrics: canary 30 of 40 and control 20 of 40 at $0.50 each. Completion is 0.75 and 0.50, per dollar 1.5 and 1.0, Wilson [0.598, 0.858] and [0.352, 0.648], Newcombe difference 0.25 [0.0379, 0.4333]. These were computed independently in Python.
- Small cohorts: 29 per arm, one arm short, an empty input, and the `small.jsonl` fixture all say `insufficient` with counts and no number.
- Planted revert 1, swapping the arm selection (`r.arm != a`): 10 of 13 tests failed. Restored and touched; `git status` shows only the intended files.
- Planted revert 2, dropping the spend normalization (a constant denominator): `a_canary_that_costs_twice_as_much_is_worse_per_dollar_though_equal_per_task` failed. Restored and touched.
- A test run exposed a bug: a missing `success` field was silently read as unknown. It is now required, with a test.
- Not run under load: the report is a pure function with no timing or concurrency.

**Live check for the maintainer** (no keys needed):
- `cargo run -q -p theseus-judge --bin theseus-judge -- prove crates/theseus-judge/fixtures/prove/small.jsonl` should show `## Verdict: insufficient` and "canary: 5 tasks, 4 labeled, 3 successes, $2.00 spent".
- The same command on `canary_wins.jsonl` should show `canary_better`, with 1.500 against 1.000 completions per USD.
- Add `--json -` for the JSON.
- Later, on the scratch daemon's small canary, the wire-in's records should give `insufficient` with its counts.

**Left or uncertain.**
- The wire-in (`theseus judge prove`, the ledger-to-records mapping) is not built, as the task says.
- Doc changes for the maintainer: m5-judgment.md §3's L3 entry should name `prove.rs`, the input shape, and the `theseus-judge` binary. docs/status.md should list the binary. The crate's `reserved_for` marker still says row 37 and now has a reader in the binary; adjust it at the join if wanted.
- Nothing is installed by scripts/build.sh for this binary; I left that alone.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` failed in the suite phase. The earlier phases passed (fmt, shape, clippy, bench build, test build, reader rule). 66 FAIL lines, all from three causes:
- `theseus-sandbox::contract` (14 cases) and `theseusd::sandbox` (10 cases): the known root-VM and `RLIMIT_NPROC` issue (theseus-pv6i). Not my area.
- `theseus-core tests_output::the_cores_output_matches_its_golden`: the golden has a `-07:00` timezone offset in a `wake.at` preview and this VM is UTC (`+00:00`). Environmental, and in code I did not touch.
- The remaining lines are duplicates of those from nextest's retry and summary output.

The phases after the suite, run by hand:
- `cargo deny --offline check`: advisories, bans, licenses and sources ok.
- Protocol types and web dist: clean. I touched no protocol types or UI, and Cargo.lock is unchanged.
- The web and cockpit lint, test and build steps were not run: I touched neither, but `npm ci` succeeded in both.
- I did not run the lifecycle, jobs and turn benches (`THESEUS_GATE_NO_BENCH=1`).
