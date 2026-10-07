# Benchmarks

Every benchmark run now has its own report in [`benchmarks/`](benchmarks/README.md), newest first: the answer, the
question, the setup, the results with their uncertainty and plots, the analysis, the threats to validity, the cost,
and the commands that run it again. How to run a benchmark, and the rule that every run gets its report, are in
[`bench/README.md`](../bench/README.md).

This page held the first full Terminal-Bench 2.0 run's write-up until 2026-10-06. Its report, with the retrospective's
analysis, is [benchmarks/2026-10-04-terminal-bench-first-full-run.md](benchmarks/2026-10-04-terminal-bench-first-full-run.md).
Its headline, unchanged:

| Date | Theseus build | Agent | Model | Tasks × attempts | Mean reward | Cost |
|---|---|---|---|---|---|---|
| 2026-10-04 | 079f1db | Theseus (bench profile) | Claude Sonnet 5.5 | 89 × 2 | 0.719 | $24.72 |
| 2026-10-04 | 079f1db | Theseus + batching paragraph | Claude Sonnet 5.5 | 89 × 2 | 0.736 | $24.23 |
| 2026-10-04 | – | Claude Code 2.1.288 | Claude Sonnet 5.5 | 89 × 2 | 0.815 | $22.65 |

How a result is reported, and what a Harbor run records, are in the index's "How a report is written" and "What a
Harbor run records".

_(Since 2026-10-06, batch 11's bench-fair; Part III Item 234: the three Harbor arms run comparably. Every arm asks for reasoning effort medium (Theseus's bench profile, Claude Code's `--effort`, Pi's `--thinking`; an `--ak` sets another as an ablation) and 128,000 output tokens. Claude Code is pinned at 2.1.290, and every arm's record keeps the version it ran (`version`, `version_asked`) and its effort (`effort`; Pi's `effort_ran`). A timed-out Claude Code or Pi is stopped before the sampler and the verifier. Pi runs offline (`PI_OFFLINE`: no catalog refresh, version check or download), and a Pi trial that failed at the provider counts as an error. Still unequal: the spend cap. Theseus's `THESEUS_BENCH_SPEND_LIMIT` counts reservations and Claude Code's `--max-budget-usd` actual spend, so at $2.00 a Theseus trial is refused past about $0.50 to $0.70 of real spend; the reviewer's recommendation for the B5 rerun is 3.75, with every Theseus trial past $2.00 of real spend named.)_
