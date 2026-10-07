# Benchmarks

Theseus runs public coding benchmarks through [Harbor](https://github.com/laude-institute/harbor), the harness
Terminal-Bench 2.0 ships with: Harbor starts each task's container, installs the agent, runs it on the task's
instruction, and runs the task's tests for a reward. **Every benchmark run ends in a report** in
[`docs/benchmarks/`](../docs/benchmarks/README.md), published there first ("Every run gets its report", below).

| File | What it is |
|---|---|
| `theseus-bench.toml` | The bench profile: the config a task's container runs. No vault (the model's key is `env:ANTHROPIC_API_KEY`), every tool open, workspace roots at `/`, L0, and Discord, the web UI, and the index off. A test loads it (`crates/theseusd/tests/bench_profile.rs`). |
| `build.sh` | Builds the two static (musl) binaries the container runs, `theseus` and `theseusd`, into `bench/bin`. |
| `harbor/theseus_agent.py` | The Harbor agent, `-a theseus_agent:Theseus`. |
| `harbor/theseus_bench.py` | Its parts that need no Harbor: the profile a trial writes, the container's script, and the exit codes. |
| `harbor/theseus_atif.py` | A session's history as an ATIF trajectory, the format Harbor's viewer and usage totals read. |
| `harbor/efficiency.py` | A trial's efficiency record, one shape for every arm: tokens by class and model, dollars, calls, and the harness's CPU and memory apart from its work. |
| `harbor/sampler.py` | The harness sampler: run in the task's container around the agent, it reads `/proc` and sorts each process into harness, work, or neither. |
| `harbor/measure.py` | What the arms share in a container: the effort every arm asks for (`EFFORT`, medium), the stop of a timed-out agent, and the read of the agent's version. |
| `harbor/claude_code_agent.py` | Claude Code, measured: `-a claude_code_agent:MeasuredClaudeCode`, Harbor's own adapter with its version pinned, its effort set, the sampler, the stop at a timeout, and the record added. |
| `harbor/pi_agent.py` | Pi, the minimal coding agent, measured: `-a pi_agent:MeasuredPi`, Harbor's own adapter with its version pinned, its effort set, run offline, the sampler, the stop at a timeout, the trajectory, and the record added. |
| `harbor/pi_atif.py` | A Pi session log as an ATIF trajectory (Harbor's own Pi adapter writes none). |
| `harbor/measured.py` | What every arm built on one of Harbor's own agents adds to it: the pin, the effort, the caps (enforced where the harness has them, else recorded), the sampler, the stop at a timeout, and the record. |
| `harbor/codex_agent.py`, `aider_agent.py`, `opencode_agent.py`, `openhands_agent.py`, `openclaw_agent.py` | Codex CLI, Aider, OpenCode, OpenHands (its SDK) and OpenClaw, measured: `-a codex_agent:MeasuredCodex` and so on, each Harbor's own adapter with `measured.py` (below). |
| `harbor/openhands_measure_run.py` | Runs Harbor's OpenHands SDK runner unchanged and writes each LLM's per-call metrics (`openhands-metrics.json`), which Harbor's trajectory lacks. |
| `harbor/test_*.py` | Their tests. |
| `report/efficiency.py` | The efficiency report over jobs, one arm each: per-arm numbers, Pareto tables, and three SVG charts. |
| `report/draft.py` | The report's drafting tool: a run's outputs (Harbor jobs, the bench history, the recall and async scorers' outputs) in; its data file, CSV, figures and skeleton out ("Every run gets its report"). |
| `report/charts.py` | The house charts: SVG from declarative specs, in the house palette, each in a light and a dark file, every mark with its hover text. The standard library only. |
| `report/stats.py` | The statistics every report uses: Wilson intervals, a seeded bootstrap, the exact McNemar test, quantiles. |

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
| `BENCH_SAMPLE_MS` | `250` | How often the harness sampler reads `/proc`, in milliseconds (every arm). |

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
`AgentTimeoutError`. The other two arms are stopped too (theseus-sgpx): Harbor's Docker environment ends only its
`docker compose exec` client, so `claude` or `pi` would run on in the container the verifier shares, spending and
changing files unrecorded. Their adapters catch the cancel and stop the agent first (`measure.stop_agent`): a
SIGTERM to the processes named `claude` or `pi` and to everything they started, a SIGKILL to what is left after
3 s, then the sampler's stop, then the cancel is raised again. It is plain sh over `/proc`, run as root, since a
task's image may lack `pkill`.

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
| `effort`, `version`, `version_asked` | What the arm asked for and ran, the same keys on every arm (theseus-n6p5, theseus-7gir.23): the reasoning effort asked for (`medium`); the agent's version as the container read it (`version.txt` in `agent/`, written at install from Harbor's `get_version_command`; null when it could not be read); and the version the install was told to take (Claude Code's and Pi's pin, or `--ak version=`; null for Theseus, whose binaries are the checkout's). A pin that did not take is a `version` that differs from `version_asked`. |
| `model_calls`, `tool_calls` | Theseus: its turn's `provider` spans (each retry and fallback one) and tool calls. Claude Code: its session log's messages, each message id once, and their tool uses. Pi: its session log's answers (a failed request it retried is one) and summaries' calls, and their `toolCall` blocks. |
| `wall_s` | The sampler's window. The report takes Harbor's agent execution from `result.json`. |
| `harness`, `work`, `wrappers` | Each `cpu_s`, `peak_rss_kb` (the largest summed RSS of the class in one sample), and `peak_hwm_kb` (the largest single process's peak). `wrappers` is Theseus's job wrappers, kept apart from both. Null when the sampler did not run. |
| `container` | The container's cgroup CPU over the window and its `memory.peak`, where they read. |
| `sampler` | `ok`, `unavailable` (no python3 in the image, or one older than 3.6), `failed`, `running` (it never wrote its last summary), or `missing`; why; its interval, samples, and its own CPU. |

**Where the spend comes from.** Theseus: its turn's result has all four classes; its history's answers split
them by model, and a turn cut short (a timeout) is its history's answers, as `spend` does. Claude Code: the
stream-json `result` event's `modelUsage` (Claude Code's own bill, per model, in all four classes, with calls
the session log does not hold); a timed-out run never prints one, so then the session log, each message once
with its last usage (the log repeats a message's usage on each content block's line; Harbor's converter also
counts it once), and then Harbor's trajectory, whose steps keep the write in `metrics.extra`. Pi: its session
log (`pi/sessions/*.jsonl`), each entry once by its id: every answer's `usage` holds all four classes and
`cost.total`, Pi's own price from its model catalog, and a compaction's, a branch summary's, and a usage entry's
own call are counted beside them (a tool's nested model work adds its tokens and dollars, not a call); the
log is written as each call settles, so a timed-out run keeps what it spent. Without a log, the stream
(`pi.txt`), each `message_end` once; then Harbor's trajectory.

**The sampler** (`harbor/sampler.py`, the standard library, Python 3.6 or later, one file) is uploaded at
install, started at nice 19 before the agent's command, and stopped after it, on Harbor's timeout path too.
Every `BENCH_SAMPLE_MS` it reads `/proc`: a process whose executable name (`/proc/<pid>/comm`) is the arm's
is **harness** (Theseus: `theseus` and `theseusd`; Claude Code: `claude`; Pi: `pi`, the process title its
Node CLI sets); a Theseus job wrapper (`theseusd
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
trajectory, and name are Harbor's, so its results read as Claude Code's. It also pins the release
(`PINNED_VERSION` in `claude_code_agent.py`: Harbor installs the latest unless `version` is set, so each run's Claude
Code would be whatever npm served that day), defaults the effort to medium, and stops `claude` at a timeout.

**Pi, measured the same way** (theseus-jp9p). [Pi](https://github.com/earendil-works/pi) is the "almost
nothing" baseline: a short system prompt and four tools (`read`, `bash`, `edit`, `write`), so on the same
model its score is what a harness's machinery adds to:

```bash
.venv/bin/harbor run -d terminal-bench@2.0 \
  -a pi_agent:MeasuredPi -m anthropic/claude-sonnet-5-5 \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name pi-tb2 -n 4 -k 2
```

It is Harbor's own `Pi` (`-a pi`): its install (Node 22 through nvm, then `npm install -g --ignore-scripts
@earendil-works/pi-coding-agent@<version>`), its command line (`pi --print --mode json --session-dir
/logs/agent/pi/sessions --provider anthropic --model <id> <instruction>`, its stream teed to `agent/pi.txt`),
its key (Harbor's model connection passes `ANTHROPIC_API_KEY`), its `thinking` option, and its name are
Harbor's, so its results read as Pi's. It adds:

- **a pinned version**, `PINNED_VERSION` in `pi_agent.py` (1.0.4), where Harbor's installs `@latest`; `--ak
  version=…` names another. Pi needs Node 22.19 or later, and the install downloads nvm, Node and the package in
  the task's container: it needs the network there and a glibc image (Node's builds do not run on Alpine).
- **an offline run** (theseus-a5we): `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1` and `PI_TELEMETRY=0` in the
  environment of Pi's run, beside the model's key. Without them Pi overlays newer model-catalog data from its
  project's server on top of the catalog its release bundles, so the version pin would not pin its prices (the
  record's dollars are Pi's own `cost.total`), its thinking map or its compat flags. Pi 1.0.4's own docs
  (`docs/environment-variables.md`) say of `PI_OFFLINE`: "Disable automatic network activity, including model
  catalog refreshes"; its code gates only the catalog refresh, the latest-version request, package updates and its
  tool downloads (`fd`, `rg`) on the flag, and reads it nowhere in the Anthropic provider, so model calls go out as
  before. The model's endpoint is the only network Pi's run then uses.
- **the effort**, `--ak thinking=…` defaulting to `medium` (below).
- **the sampler** around Harbor's run, stopped in a `finally`, as Claude Code's is;
- **a trajectory**, `agent/trajectory.json` from Pi's session log (`pi_atif.py`), which Harbor's own Pi
  adapter does not write;
- **Harbor's three counters** from the same log, the cache write inside the input as the other arms count it
  (Harbor's Pi adapter leaves the write out);
- **the record**, with three parts only Pi's has: `end`, the last answer's `stopReason` and error (Pi's print
  mode exits 0 when the provider fails, so this is where a failed run says so; the report counts such a trial as
  an error, below), `limits` (below), and `effort_ran`, the thinking level Pi's session log says ran
  (`thinking_level_change`, and each answer's `providerThinkingLevel`), beside the `effort` asked for.

**Fair limits.** Every arm gets the same model, attempts, and wall clock (the task's agent timeout), and the
same spend and turn caps where its harness has them. Pi has neither, and the arm adds none inside it: it takes
`max_budget_usd` and `max_turns` as Claude Code does, passes neither to Pi, and records them in the trial's
`limits`, with `over_budget` (its dollars passed the cap) and `over_turns` (its `turns` passed it: its answers
less the failed requests it retried, since Pi persists each as an answer with `stopReason: "error"` and Claude
Code's `--max-turns` counts none; `answers` counts every one, and `model_calls` keeps them). Such a trial ran on
past where the others would have stopped; its reward counts, and the report says how many there were.

| | Theseus | Claude Code | Pi |
|---|---|---|---|
| Model | `-m` | `-m` | `-m` (`--provider anthropic --model <id>`) |
| Attempts, wall clock | `-k`, the task's agent timeout | the same | the same |
| Spend cap | `THESEUS_BENCH_SPEND_LIMIT` (2.0), enforced: the turn stops (exit 5) | `--ak max_budget_usd=2.0`, enforced by Claude Code | none in Pi: `--ak max_budget_usd=2.0` is recorded, and a trial past it flagged, not stopped |
| Turn cap | `THESEUS_BENCH_MAX_LOOPS` (200 model calls in the turn), enforced (exit 8) | `--ak max_turns=200`, enforced | none in Pi: `--ak max_turns=200` is recorded and held against its answers, not enforced |
| Thinking | `effort = "medium"` in the bench profile, sent as `output_config.effort` (omitted, Sonnet 5.5 runs at its default, high) | `--effort medium`: `MeasuredClaudeCode` defaults `reasoning_effort` to it, so a host's `CLAUDE_CODE_EFFORT_LEVEL` no longer picks it; `--ak reasoning_effort=…` is an ablation | `--thinking medium`: `MeasuredPi` defaults `thinking` to it; `--ak thinking=off\|low\|…` is an ablation |
| Tools | Theseus's toollets, every one open | Claude Code's | `read`, `bash`, `edit`, `write` |
| Version | the binaries built from the checkout | `PINNED_VERSION` in `claude_code_agent.py` (2.1.290; Harbor's install takes the latest release unless told); `--ak version=…` names another | 1.0.4, pinned, and run offline; every arm's version as the container read it is in its record |
| A provider's failure | an error class per exit code | Harbor's | exit 0; the record's `end` says `error` or `aborted`, and the report counts the trial as an error (`PiProviderError`, `PiAbortedError`) |
| A timeout | SIGTERM: the turn stops, the daemon stops | `claude` and what it started are stopped (SIGTERM, then SIGKILL after 3 s) before the verifier starts | the same for `pi` |

**Five more harnesses on Harbor's own agents** (theseus-qags). Harbor 0.23 ships an installed agent for Codex
CLI, Aider, OpenCode, OpenHands and OpenClaw. Each arm is a thin subclass of Harbor's with `measured.py`'s
`MeasuredArm` first in its bases, so the install, the command line and the name stay Harbor's, and it adds the pin,
`--ak`'s effort at medium in the harness's own words, the caps (`--ak max_budget_usd=2.0 --ak max_turns=200`, taken
by every arm), the sampler, the stop at a timeout, and the record (`efficiency.py`: each harness's own log read for
its calls, tokens and dollars, and `list_cost_usd`, the same tokens at the providers' list prices, `LIST_PRICES`).

| | Codex CLI | Aider | OpenCode | OpenHands | OpenClaw |
|---|---|---|---|---|---|
| Arm | `codex_agent:MeasuredCodex` | `aider_agent:MeasuredAider` | `opencode_agent:MeasuredOpenCode` | `openhands_agent:MeasuredOpenHands` | `openclaw_agent:MeasuredOpenClaw` |
| Version | `@openai/codex` 0.161.0 | `aider-chat` 0.86.2 (installed by uv; Harbor's installer takes the latest) | `opencode-ai` 1.18.35 | `openhands-sdk` and `openhands-tools` 1.53.0, through Harbor's `openhands-sdk` agent: its `openhands` agent runs `openhands.core.main`, which `openhands-ai` 1.x no longer ships | `openclaw` 2026.9.8, a released build installed from npm into the task's container, on Node 24 (its `engines`; Harbor's agent installs and uses 22) |
| Model | an OpenAI model (`OPENAI_API_KEY`): Codex 0.161 speaks only OpenAI's Responses API ("`wire_api = "chat"` is no longer supported"), which Anthropic's API does not serve, so it cannot run Claude Sonnet 5.5; a native-model arm. Its default, GPT-6.1-Sol, priced as Sonnet 5.5 is, where the key's project has it; else GPT-5.6-Sol (`codex_agent.NATIVE_MODEL`), the workhorse tier | `-m`, passed to Aider with its provider | `-m` | `-m` | `-m` |
| Effort | `-c model_reasoning_effort=medium` | `output_config: {effort: medium}` in a model-settings file (Aider's own Sonnet 4.5 settings, with no `temperature`, which Sonnet 5.5 refuses, and Sonnet 5.5 as its weak and editor model): its `--reasoning-effort` goes out as a literal `extra_body` key the API does not take | `--variant medium` (adaptive thinking at effort medium in OpenCode's catalog) | `reasoning_effort` medium (the SDK sends `output_config.effort`, with adaptive thinking) | `--thinking medium` |
| Spend and turn caps | none in Codex: recorded, not enforced | none in Aider: recorded | none in OpenCode: recorded | the turns enforced (`max_iterations`); no spend cap in the SDK: recorded | none in OpenClaw: recorded |
| Dollars, its own | Harbor prices Codex's tokens from LiteLLM's table | Aider's, from a model-metadata file the arm uploads with the list prices (its own table predates Sonnet 5.5) | OpenCode's catalog, pinned: `OPENCODE_DISABLE_MODELS_FETCH=1` | its LiteLLM's (which prices a Sonnet 5.5 cache read at half the list price) | its session's |
| The harness, to the sampler | `codex` (the native binary; its npm launcher, `node`, is outside) | `aider` | `opencode.exe` | `openhands-py`: the arm runs the SDK's runner through a link of that name to its venv's Python, since `python` is also the work's | `openclaw` |
| Its own log, for the record | the rollout, through Harbor's trajectory | `agent/aider.txt` (a token line a call) and `--analytics-log` (the exact counts) | `agent/opencode.txt` (a `step_finish` a call) | `openhands-metrics.json` (`openhands_measure_run.py`): each call's tokens and dollars | Harbor's trajectory of its session |

Aider answers one message (with up to three reflections) by editing files; it does not drive a loop of tool calls.
It suggests shell commands but does not run them under Harbor: it asks an explicit yes for each, which `--yes-always`
does not give, so a task that needs a command run fails in Aider as shipped (its record's `tool_calls` counts the
commands it ran, none). OpenClaw's own CLI timeout is lifted to 14400 s so
the task's agent timeout bounds it as it bounds every arm.

**The report** reads jobs, one arm each:

```bash
python3 bench/report/efficiency.py --arm theseus=jobs/theseus-tb2 --arm claude-code=jobs/claude-tb2 \
  --arm pi=jobs/pi-tb2 --out /tmp/eff
```

It writes `report.md` (per arm: trials, solved, mean reward, dollars, solved per dollar, tokens per solved
task by class, the share of input read from the cache, model and tool calls, harness CPU per tool call,
peak harness RSS, and agent time; then score against dollars, tokens, and harness RAM, one point per arm,
the Pareto front marked), `pareto-dollars.svg`, `pareto-tokens.svg`, `pareto-ram.svg` (each with its
`-dark.svg`, drawn by `report/charts.py`), and `trials.csv`. A
job from before the record reports what it kept: its dollars, the cache write from the arm's own files or
its trajectory, and CPU and RAM "not sampled". When an arm's records carry `limits` (Pi's), the table adds a
row of its trials past the others' caps. A trial Harbor recorded no exception for, whose record's `end` says
`error` or `aborted` (Pi exits 0 when its provider fails), is an error all the same: "Trials with an error", the
`error` column of `trials.csv` (`PiProviderError`, `PiAbortedError`) and the drafting tool's endings count it.

## What it costs

Each trial is capped by `THESEUS_BENCH_SPEND_LIMIT`. With Sonnet 5.5, the easy Terminal-Bench tasks cost $0.02 to
$0.05 a trial. The harder ones take far more turns: a fair guess is $0.30 to $1.00 a task, so **$30 to $90 for the
full 89 tasks, per configuration and attempt**.

## Every run gets its report

Every benchmark run, whatever its size (a full run, a held-out rerun, a sample, a live check that paid for model
calls, an A/B of two builds), ends in a report, and the run is not done until its report is joined:

- **Where:** the run's lane writes `docs/benchmarks/<YYYY-MM-DD>-<suite>[-<slug>].md`, named for the day the run
  happened, with its figures under `docs/benchmarks/img/<same name>/`, its data file `<same name>.json` beside it
  (the numbers its tables use and its figures' specs), and a `<same name>.csv` with one row per trial when it has
  trials. Never raw outputs or transcripts: the trials' own files stay off the public repository.
- **What:** the sections, the statistics, the figures' rules and the house palette are in
  [`docs/benchmarks/README.md`](../docs/benchmarks/README.md), "How a report is written". The answer comes first, with
  its numbers and their intervals; a report says what the run teaches that its tables don't, and says plainly where
  the run was flawed.
- **How:** `report/draft.py` drafts it from the run's outputs: the numbers with their intervals, the data file, the
  CSV, both modes of every figure, and a skeleton with every section in order, its tables filled and its figures
  placed. The lane writes the narrative, looks at every figure rendered in both modes, and adds the run's row at the
  top of the index's table. `python3 bench/report/draft.py plot docs/benchmarks/<report>.json` draws a report's
  figures again from its data file.

```bash
python3 bench/report/draft.py harbor --suite terminal-bench@2.0 --date <day> --slug <slug> \
    --arm theseus=jobs/theseus-tb2 --arm claude-code=jobs/claude-tb2 --model anthropic/claude-sonnet-5-5
python3 bench/report/draft.py history --since <day> --branch main --date <day>     # the gate's speed benches
python3 bench/report/draft.py recall --scores <scores>/scores.json --date <day>    # bench/recall
python3 bench/report/draft.py async --date <day> <jobs...>                         # bench/async
```

`--arm` takes a registry key from `report/charts.py` (`theseus`, `claude-code`, `theseus-batching`, `openclaw`, ...):
the same arm is the same colour in every report. Harbor writes each job under `-o` (`jobs/<job name>/`): its
`result.json`, and each trial's directory with the agent's files above and the verifier's output; `--arm` takes a job
directory, a directory of jobs, or a quoted glob.

## Tests

```bash
python3 -m unittest discover -s bench/harbor                 # the standard library: Harbor's checks skip
.venv/bin/python -m unittest discover -s bench/harbor         # with Harbor: the ATIF checks and the agents' load
python3 -m unittest discover -s bench/report                 # the reports: efficiency, the drafting tool, the charts, the statistics
python3 -m unittest discover -s bench/async                  # the async bench's driver and scorer
ASYNC_HARBOR=1 .venv/bin/python -m unittest discover -s bench/async   # with Harbor: the async arms' runs
```

They check the profile a trial writes, the exit codes against `crates/theseus/src/outcome.rs`, the container's
script against a stand-in `theseus` (a turn that ends, a failure, a stop after a timeout, and the sampler around
each, with and without python3), and the trajectory against Harbor's own ATIF model; Pi's record, limits and
trajectory from a fixture of its session log and stream, and with Harbor its install's pinned version, its
command line (offline, at effort medium), and the sampler around its run and its stop at a timeout; the stop
itself on stand-in processes in this host's `/proc` (`test_measure.py`); the sampler's parsers and
classes on fixture `/proc` trees, and on this host's `/proc` a copy of `sh` under a harness name whose busy child
must land in work; the record from fixture turns, histories, and session logs; the efficiency report over fixture
jobs, against numbers worked by hand; the drafting tool over fixture Harbor jobs, a bench history whose header
changes shape, a recall scorer's output and async trials; and every chart form in both modes, parsed back.
