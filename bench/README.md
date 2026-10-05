# Benchmarks

Theseus runs public coding benchmarks through [Harbor](https://github.com/laude-institute/harbor), the harness
Terminal-Bench 2.0 ships with: Harbor starts each task's container, installs the agent, runs it on the task's
instruction, and runs the task's tests for a reward. Results are published in
[`docs/benchmarks.md`](../docs/benchmarks.md) first.

| File | What it is |
|---|---|
| `theseus-bench.toml` | The bench profile: the config a task's container runs. No vault (the model's key is `env:ANTHROPIC_API_KEY`), every tool open, workspace roots at `/`, L0, and Discord, the web UI, and the index off. A test loads it (`crates/theseusd/tests/bench_profile.rs`). |
| `build.sh` | Builds the two static (musl) binaries the container runs, `theseus` and `theseusd`, into `bench/bin`. |
| `harbor/theseus_agent.py` | The Harbor agent, `-a theseus_agent:Theseus`. |
| `harbor/theseus_bench.py` | Its parts that need no Harbor: the profile a trial writes, the container's script, and the exit codes. |
| `harbor/theseus_atif.py` | A session's history as an ATIF trajectory, the format Harbor's viewer and usage totals read. |
| `harbor/efficiency.py` | A trial's efficiency record, one shape for every arm: tokens by class and model, dollars, calls, and the harness's CPU and memory apart from its work. |
| `harbor/sampler.py` | The harness sampler: run in the task's container around the agent, it reads `/proc` and sorts each process into harness, work, or neither. |
| `harbor/claude_code_agent.py` | Claude Code, measured: `-a claude_code_agent:MeasuredClaudeCode`, Harbor's own adapter with the sampler and the record added. |
| `harbor/test_*.py` | Their tests. |
| `report/efficiency.py` | The efficiency report over jobs, one arm each: per-arm numbers, Pareto tables, and three SVG charts. |

## What you need

- Docker with the compose plugin (`docker compose version` answers).
- Harbor 0.23 in a virtualenv: `python3 -m venv .venv && .venv/bin/pip install harbor==0.23.0`.
- The static binaries: `bench/build.sh`, which needs the musl target (`rustup target add
  x86_64-unknown-linux-musl`) and `musl-gcc` (Debian's `musl-tools`). It prints the line to export:
  `export THESEUS_BENCH_BIN_DIR=$PWD/bench/bin`.
- An Anthropic API key in `ANTHROPIC_API_KEY`, from wherever you keep it. Harbor passes it into each task's
  container as the agent's environment, and the profile names it as `env:ANTHROPIC_API_KEY`, so it is never written
  to a file.

## Running

From the repository's root, with the environment above:

```bash
export PYTHONPATH=$PWD/bench/harbor HARBOR_TELEMETRY=0

# One task (an easy one).
.venv/bin/harbor run -d terminal-bench@2.0 -i fix-git \
  -a theseus_agent:Theseus -m anthropic/claude-sonnet-5-5 -o jobs --job-name theseus-fix-git

# A sample: Terminal-Bench's own ten.
.venv/bin/harbor run -d terminal-bench-sample@2.0 \
  -a theseus_agent:Theseus -m anthropic/claude-sonnet-5-5 -o jobs --job-name theseus-sample -n 4

# The full set: all 89 tasks, two attempts each.
.venv/bin/harbor run -d terminal-bench@2.0 \
  -a theseus_agent:Theseus -m anthropic/claude-sonnet-5-5 -o jobs --job-name theseus-tb2 -n 4 -k 2
```

`-i` takes a task name or a glob, `-l N` caps the number of tasks, `-n` runs that many trials at once, and `-k` is
the attempts per task. The same agent runs any Harbor dataset (`harbor datasets list`), SWE-bench Verified among
them.

The adapter's settings come from the environment of `harbor run`:

| Variable | Default | What it sets |
|---|---|---|
| `THESEUS_BENCH_BIN_DIR` | (required) | Where `theseus` and `theseusd` are, static. |
| `THESEUS_BENCH_SPEND_LIMIT` | `2.0` | The most one trial may spend, in dollars (`[kernel] spend_limit_usd`). |
| `THESEUS_BENCH_MAX_LOOPS` | `200` | The model calls one turn may make. |
| `THESEUS_BENCH_PROC_SYNC` | `900` | How long a command may keep the turn waiting, in seconds. A headless run ends with its turn, so a command left running in the background is never read. |
| `THESEUS_BENCH_SYSTEM_FILE` | (none) | Extra system text, for an A/B arm. |
| `BENCH_SAMPLE_MS` | `250` | How often the harness sampler reads `/proc`, in milliseconds (both arms). |

## How a trial runs, and how it ends

1. **install** uploads the two binaries to `/installed-agent/bin`. Nothing is downloaded in the container, so any
   image runs them, and an install takes under a second.
2. **run** writes the profile, with the trial's model, limits, and working directory, and runs one
   `theseus --spawn theseusd --json ask -` with the instruction on stdin: one turn, every loop until the model ends
   it, then a clean stop of the daemon. Then `theseus --json history` reads the session from the store.
3. Harbor runs the task's tests and records the reward.

`theseus ask`'s exit code says how the turn ended (`theseus ask --help`), and the adapter records a turn that the
model did not end as an error, so Harbor's results say why. The tests still run, and the reward still counts.

| Exit | The turn | Harbor records |
|---|---|---|
| 0 | the model ended it | no error |
| 1 | failed: the provider's or a tool's fault | Harbor's own class for the cause (a rate limit is `ApiRateLimitError`), else `NonZeroAgentExitCodeError` |
| 5 | reached the trial's spend limit | `TheseusSpendLimitError` |
| 6 | waits for an approval no one gives | `TheseusApprovalWaitError` |
| 7 | the model refused | `AgentSafetyRefusalError` |
| 8 | the loop cap, the output limit, or the context window ended it | `TheseusTurnCutError` |
| 9, 130, 143 | stopped by a signal | `TheseusStoppedError` |

**A timeout.** At the task's agent timeout Harbor cancels the run, and the adapter sends `theseus` a SIGTERM, which
stops the turn as `/stop` does: its running commands are stopped, it makes no more model calls, and its daemon stops
cleanly. So a timed-out agent neither spends nor changes the task's files while the tests run. Harbor records
`AgentTimeoutError`.

**What each trial leaves** in its `agent/` directory: `theseus-turn.json` (the turn's result: stop reason, loops,
tool calls, tokens, and dollars), `theseus-history.json` (every message, tool call with its gate decision, and
result), `theseus.log` (the CLI's and the daemon's stderr), `theseus-exit.txt`, and `trajectory.json`, the
history in ATIF. The trial's tokens and dollars come from the turn's result. A turn cut short, by a timeout, prints
none, so its spend is summed from the history: every model call that finished is in the store with its cost.

## Efficiency: what each trial spends

Score alone hides what an arm spends to get it, so every trial of every arm leaves the same record,
`agent/efficiency.json` (and `metadata["efficiency"]` in the trial's `result.json`), written by
`harbor/efficiency.py`:

| Field | What it holds |
|---|---|
| `tokens` | The four classes, `input` (uncached), `cache_read`, `cache_write`, and `output`, over every model. Harbor's own counters fold the write into `n_input_tokens`; this keeps it apart. |
| `by_model` | The same per model, with its `cost_usd` and `calls`: a retry is a call, and a refusal's fallback (Sonnet 5.5's is Sonnet 5) bills a second model. |
| `cost_usd`, `spend_from` | The trial's dollars, and the file they came from. |
| `model_calls`, `tool_calls` | Theseus: its turn's `provider` spans (each retry and fallback one) and tool calls. Claude Code: its session log's messages, each message id once, and their tool uses. |
| `wall_s` | The sampler's window. The report takes Harbor's agent execution from `result.json`. |
| `harness`, `work`, `wrappers` | Each `cpu_s`, `peak_rss_kb` (the largest summed RSS of the class in one sample), and `peak_hwm_kb` (the largest single process's peak). `wrappers` is Theseus's job wrappers, kept apart from both. Null when the sampler did not run. |
| `container` | The container's cgroup CPU over the window and its `memory.peak`, where they read. |
| `sampler` | `ok`, `unavailable` (no python3 in the image, or one older than 3.6), `failed`, `running` (it never wrote its last summary), or `missing`; why; its interval, samples, and its own CPU. |

**Where the spend comes from.** Theseus: its turn's result has all four classes; its history's answers split
them by model, and a turn cut short (a timeout) is its history's answers, as `spend` does. Claude Code: the
stream-json `result` event's `modelUsage` (Claude Code's own bill, per model, in all four classes, with calls
the session log does not hold); a timed-out run never prints one, so then the session log, each message once
with its last usage (the log repeats a message's usage on each content block's line; Harbor's converter also
counts it once), and then Harbor's trajectory, whose steps keep the write in `metrics.extra`.

**The sampler** (`harbor/sampler.py`, the standard library, Python 3.6 or later, one file) is uploaded at
install, started at nice 19 before the agent's command, and stopped after it, on Harbor's timeout path too.
Every `BENCH_SAMPLE_MS` it reads `/proc`: a process whose executable name (`/proc/<pid>/comm`) is the arm's
is **harness** (Theseus: `theseus` and `theseusd`; Claude Code: `claude`); a Theseus job wrapper (`theseusd
job-wrapper …`) is apart; whatever descends from them is **work**; the container's own are left out. CPU is
`utime` and `stime` per process, and a child reaped between two samples is counted from its parent's
`cutime`. Where the container's cgroup v2 `cpu.stat` reads, its CPU over the window is the total, and the
work is the total less the harness's, the wrappers', and the rest the samples saw, so a command shorter than
an interval still counts. Each arm's harness names are data (`ARMS` in `efficiency.py`), so a new arm needs
only its line. The sampler writes `sampler.jsonl` (a line a sample) and `sampler.json` (its summary) beside
the agent's files. At 250 ms it costs about 0.3% of a core with 25 processes in the container, and under 1%
with 90; `sampler.py`'s head says what the method misses.

**Claude Code, measured the same way:**

```bash
.venv/bin/harbor run -d terminal-bench@2.0 \
  -a claude_code_agent:MeasuredClaudeCode -m anthropic/claude-sonnet-5-5 \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name claude-tb2 -n 4 -k 2
```

It is Harbor's own `ClaudeCode` with the sampler and the record added: its install, command line, options,
trajectory, and name are Harbor's, so its results read as Claude Code's.

**The report** reads jobs, one arm each:

```bash
python3 bench/report/efficiency.py --arm theseus=jobs/theseus-tb2 --arm claude-code=jobs/claude-tb2 --out /tmp/eff
```

It writes `report.md` (per arm: trials, solved, mean reward, dollars, solved per dollar, tokens per solved
task by class, the share of input read from the cache, model and tool calls, harness CPU per tool call,
peak harness RSS, and agent time; then score against dollars, tokens, and harness RAM, one point per arm,
the Pareto front marked), `pareto-dollars.svg`, `pareto-tokens.svg`, `pareto-ram.svg`, and `trials.csv`. A
job from before the record reports what it kept: its dollars, the cache write from the arm's own files or
its trajectory, and CPU and RAM "not sampled".

## What it costs

Each trial is capped by `THESEUS_BENCH_SPEND_LIMIT`. With Sonnet 5.5, the easy Terminal-Bench tasks cost $0.02 to
$0.05 a trial. The harder ones take far more turns: a fair guess is $0.30 to $1.00 a task, so **$30 to $90 for the
full 89 tasks, per configuration and attempt**.

## Where results go

Harbor writes each job under `-o` (`jobs/<job name>/`): its `result.json`, and each trial's directory with the
agent's files above and the verifier's output. Published results, with the configuration, the model, the attempts,
and the cost, go in [`docs/benchmarks.md`](../docs/benchmarks.md). The first full run fills it.

## Tests

```bash
python3 -m unittest discover -s bench/harbor                 # the standard library: Harbor's checks skip
.venv/bin/python -m unittest discover -s bench/harbor         # with Harbor: the ATIF checks and the agents' load
python3 -m unittest discover -s bench/report                 # the report, over fixture jobs
```

They check the profile a trial writes, the exit codes against `crates/theseus/src/outcome.rs`, the container's
script against a stand-in `theseus` (a turn that ends, a failure, a stop after a timeout, and the sampler around
each, with and without python3), and the trajectory against Harbor's own ATIF model; the sampler's parsers and
classes on fixture `/proc` trees, and on this host's `/proc` a copy of `sh` under a harness name whose busy child
must land in work; the record from fixture turns, histories, and session logs; and the report over fixture jobs,
against numbers worked by hand.
