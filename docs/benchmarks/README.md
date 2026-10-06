# Benchmarks

Every benchmark Theseus runs ends in a report here: a short research note that gives the answer first, then the
question the run asked, its setup, its results with their uncertainty and plots, what the run teaches, the threats to
its validity, what it cost, and the commands that run it again. Each report keeps its numbers in a data file beside
it, so every table and plot can be checked and redrawn. How a benchmark is set up and run is in
[`bench/README.md`](../../bench/README.md), which also holds the rule that every run gets its report.

## Every run, newest first

RUN_TABLE

## The house palette

Every chart in these reports draws an arm in the same colour, so a reader who learns "Claude Code is orange" in one
report can trust it in the next. The colours are the categorical palette of the dataviz method these reports follow,
in its fixed order, with a light and a dark step for each slot; each chart is shipped in both modes and GitHub shows
the one that matches the reader's theme. `bench/report/charts.py` owns the palette: a report names arms by key, never
by colour.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/palette-dark.svg">
  <img alt="The house palette: each arm's colour in light and dark mode, in slot order, with the neutrals and the status colours." src="img/palette.svg" width="720">
</picture>

| Slot | Arm key | Arm | Light | Dark |
|---|---|---|---|---|
| 1 | `theseus` | Theseus | `#2a78d6` | `#3987e5` |
| 2 | `claude-code` | Claude Code | `#eb6834` | `#d95926` |
| 3 | `theseus-batching` | Theseus + the batching paragraph (an ablation arm) | `#1baf7a` | `#199e70` |
| 4 | `openclaw` | OpenClaw (reserved: its arm is not built yet) | `#eda100` | `#c98500` |
| 5 | `bm25` | BM25, the lexical source | `#e87ba4` | `#d55181` |
| 6 | `vector` | vectors, the dense source | `#008300` | `#008300` |
| 7 | `fused` | fused ranks (BM25, entities and vectors) | `#4a3aa7` | `#9085e9` |
| 8 | `entity` | exact entities | `#e34948` | `#e66767` |

Harness arms hold slots 1 to 4; configurations inside Theseus (memory and retrieval arms) hold 5 to 8. Four neutrals
are not slots: `none` (a no-memory baseline) and `context` (the emphasis form's de-emphasised series: a second
statistic of an arm, or the rest beside the one that matters) in the gray `#898781`; `oracle` (a ceiling) in the
secondary ink; and `other` (an arm outside the registry) in the gray, with its name on the point. Status colours
(good `#0ca30c`, warning `#fab219`, serious `#ec835a`, critical `#d03b3b`) are reserved for what means good or bad,
such as a run over its budget, and always come with a glyph and a key. A new arm takes the next free slot here and
in `charts.ARMS`, and the palette is validated again.

**Validated** with the method's validator (OKLab ΔE × 100, Machado-Oliveira-Fernandes 2009 at severity 1), on the
chart surfaces `#fcfcfb` (light) and `#1a1a19` (dark):

| Set | Mode | Pairs | Worst CVD ΔE (target 8) | Worst normal-vision ΔE (floor 15) | Contrast against the surface |
|---|---|---|---|---|---|
| All eight slots | light | adjacent | 9.1 | 19.6 | 3 below 3:1: aqua 2.74, yellow 2.11, magenta 2.62 |
| All eight slots | dark | adjacent | 8.4 | 19.3 | all at least 3:1 |
| The three harness arms (slots 1 to 3) | light | all | 9.2 | 24.0 | aqua 2.74 |
| The three harness arms (slots 1 to 3) | dark | all | 9.4 | 20.9 | all at least 3:1 |
| Memory slots 5 to 8 | light | adjacent | 17.6 | 33.6 | magenta 2.62 |
| Memory slots 5 to 8 | dark | adjacent | 13.0 | 22.5 | all at least 3:1 |
| `bm25`, `vector`, `fused` | light / dark | all | 17.6 / 13.0 | 33.9 / 19.7 | magenta 2.62 (light) |
| A dumbbell's two steps (`theseus`) | light / dark | ordinal | monotone, ΔL ≥ 0.06 | – | light end 2.12:1 / 2.12:1 (floor 2:1) |

Every check passes. The three light-mode slots under 3:1 take the method's relief rule: no chart is the only way to
read its numbers, since each figure's values are in a table in its report and its marks are labelled or keyed. Text
is never drawn in a series colour: labels use the ink (`#0b0b0b`, 19.2:1) and the secondary ink (`#52514e`, 7.7:1;
dark `#c3c2b7`, 9.7:1). A scatter, whose points can sit beside any other, carries at most three arms: the harness
arms' three, or the three retrieval arms, each set validated over all its pairs.

## How a report is written

**Its name:** `<YYYY-MM-DD>-<suite>[-<slug>].md`, the date the run happened; its figures under `img/<same name>/`; its
data file `<same name>.json` beside it, and a per-trial (or per-task, per-probe, per-run) `<same name>.csv` when there
is one. Suites: `terminal-bench`, `swe-bench`, `harbor` (mixed samples), `gate-bench`, `memory-exam`, `retrieval`,
`recall`, `async`.

**Its sections,** in order, each one there even when it is short:

1. **The answer first:** one paragraph, the headline with its numbers and their intervals.
2. A table at a glance: suite, arms, model, tasks × attempts, date and commit, cost, data.
3. **The question** the run asked, and why it mattered then.
4. **The setup:** each arm's harness and version; the model; the dataset and its task count; the limits (attempts,
   wall clock, spend and turn caps); the machine; the date and the commit.
5. **Results:** tables with uncertainty, and figures that each answer one question named in their caption.
6. **Analysis:** where each arm wins and loses, with a failure taxonomy and its examples (task ids, never
   transcripts); cost, tokens and time against success (a Pareto view where there are arms); what changed since the
   last comparable run.
7. **Threats to validity:** sample size, flaky tasks, spend parity, contamination, the harness's own bugs at the time.
8. **What it cost** to run.
9. **Reproduction:** the exact commands, from this repository, on public datasets.
10. **Data:** what the data file holds.

"Novel" means the insight, not the format: a report says what the run teaches that its tables don't, says plainly
where a run was flawed, and says so when a later look finds that an old conclusion doesn't hold.

**Its statistics** come from `bench/report/stats.py`, the same code for a past run and a future one:

- a pass rate with its Wilson 95% interval, saying whether the unit is the trial or the task;
- two arms compared on the same tasks by an exact McNemar test on the discordant pairs (a task and attempt that one
  arm passed and the other did not), with the counts beside the p;
- a mean cost or time with a seeded bootstrap 95% interval (10,000 resamples), or its range when there are fewer than
  ten values, and the median beside a skewed mean.

**Its figures** are SVG, drawn by `bench/report/charts.py` (the Python standard library, nothing to install) from
declarative specs kept in the data file's `figures`: `python3 bench/report/draft.py plot docs/benchmarks/<report>.json`
draws them again. The forms are chosen by the data's job: an interval per row for estimates (`intervals`), bars for
counts, lines and small multiples for a number over time against its budget, a scatter for a Pareto view, a strip for
a distribution of per-trial values, a matrix for each task's outcomes, and a dumbbell for before and after. One axis
per chart, recessive hairline grids, thin marks, a legend for two or more series and labels only where they carry the
story. Each figure is shipped twice, `<name>.svg` and `<name>-dark.svg`, each painting its own surface, and a report
shows them with a `<picture>` whose dark source GitHub picks for a reader in its dark theme. Every mark carries a
`<title>`, so opening a figure on its own shows each value on hover; in the report, the table beside each figure is
its readable twin. Before a figure ships, it is looked at, rendered, in both modes, and checked against the method's
list of what goes wrong (overlap, clipped labels, a dual axis, a number on every point).

**Its data** is the run's summary, never its raw outputs: numbers, task ids and figure specs. Trial transcripts stay
off this public repository.

**Its words** follow the repository's rules: no person's name (the owner is "the owner"), no private path, account,
key or address, and public datasets only.

## Drafting a report

`bench/report/draft.py` does the mechanical half of every report: it reads what a run left, computes the numbers with
their intervals, writes the data file, the CSV and both modes of every figure, and a skeleton with every section in
order, its tables filled and its figures placed. The run's lane writes the narrative into the skeleton.

```bash
# A Harbor run (Terminal-Bench, SWE-bench, the async families): one --arm per arm, KEY=JOBS.
python3 bench/report/draft.py harbor --suite terminal-bench@2.0 --date 2026-10-04 --slug first-full-run \
    --arm theseus=jobs/theseus-tb2 --arm claude-code=jobs/claude-tb2 --model anthropic/claude-sonnet-5-5

# The gate's speed benches over a span of the bench history ($THESEUS_BENCH_HISTORY).
python3 bench/report/draft.py history --since 2026-10-01 --branch main --date 2026-10-06

# The recall bench, from its scorer's scores.json; the async bench, from its Harbor jobs.
python3 bench/report/draft.py recall --scores <scores>/scores.json --date <day>
python3 bench/report/draft.py async --date <day> jobs/async-theseus jobs/async-claude

# Any report's figures again, after editing a spec.
python3 bench/report/draft.py plot docs/benchmarks/<report>.json
```

An existing report is never overwritten (`--force` rewrites its skeleton). `bench/report/efficiency.py`, the
efficiency report over Harbor jobs, draws its Pareto charts with the same renderer.

## What a Harbor run records

- The job's `result.json` from Harbor, and each trial's reward, time and spend.
- Since 2026-10-05 (the spec's Part III Item 167), each trial's efficiency record, `agent/efficiency.json`, one shape
  for every arm: tokens by class (input, cache read, cache write, output), dollars, model and tool calls, and the
  harness's own CPU and peak RSS apart from the commands it runs, sampled inside the task's container
  (`bench/harbor/sampler.py`). A job from before it, such as the first full run's, reads "not sampled" for CPU and
  RAM. Since 2026-10-06 (Items 199 and 200) the async harness's Theseus record counts a call a stop cut at its
  estimate apart (`cut_calls`, `cut_cost_usd` and `billed_usd`, with `cost_usd` their sum, what the kernel books as
  spent), its tool calls include its tasks' (`tool_calls_from`), and a trial whose ledger read hit its 1,000-row cap
  is marked `truncated`; a published async table shows `billed_usd` beside the dollars, notes that Claude Code's
  record carries no estimate for a request its interrupt cut, and footnotes a truncated trial. A recall progression
  records the overhead it was planned at, and the driver refuses a daemon more than 50 tokens over it
  (`--allow-overhead` runs on); a published run is generated at the measured overhead. `--stale retracted` binds a
  retracting phrase to the nearest value within four words in its clause; strict stays the published rule.
- Theseus's build (`theseus --version`) and the profile's settings (`THESEUS_BENCH_*`), so a run can be repeated.
- Trials that ended with an error, by Harbor's class for it: a timeout (`AgentTimeoutError`), a spend limit
  (`TheseusSpendLimitError`), a cut turn (`TheseusTurnCutError`), and so on (`bench/README.md`). Their rewards still
  count: the task's tests run after every trial.
