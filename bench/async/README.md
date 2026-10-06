# The async bench

Six families of local Harbor tasks where concurrency is the point (theseus-7gir.16). Every arm runs the same
tasks through Harbor 0.23 with the same model, limits, attempts, and wall clock, each by its own async means: Theseus
with its background jobs, tasks, wakes, and daemon-owned waits; Claude Code with its background commands and
subagents. An instruction states the task, never how to run it ("in parallel", "in the background"), so every arm
reads one text. A driver sends two families a second message mid-trial, and a scorer reads what each trial left.

| File | What it is |
|---|---|
| `tasks/<family>/` | A Harbor task each: `task.toml`, `instruction.md`, `environment/` (a `python:3.12-slim` image with the family's tools in `/opt/async/bin`), `solution/solve.sh` (the oracle), and `tests/test.sh` (the verifier). |
| `tools/asyncbench.py` | The tools, the ledger, and the checks, standard library only. Each task holds two copies of it, its image's and its verifier's; `sync.py` writes them. |
| `driver.py` | The injection and its trigger, and each arm's commands. Standard library only. |
| `async_agents.py` | The Harbor agents: `async_agents:TheseusAsync` and `async_agents:ClaudeCodeAsync`. |
| `score.py` | The scorer: `report.md` and `scores.json` from job directories. Standard library only. |
| `test_tasks.py`, `test_driver.py`, `test_score.py` | Their tests. |

## The families

| Family | The task | Its ideal wall | What fails it |
|---|---|---|---|
| `parallel` | Six independent slow digests (20 to 45 s each), then an aggregate file. | The longest digest. | A part missing from the aggregate, or a digest that never finished. |
| `wait-tax` | One index build that ends at a random time (60 to 180 s); its token is needed. | The build. | A token not the build's. |
| `interrupt` | A long training run (150 to 210 s), its score to a file; 20 s after it starts, the driver asks for the open ticket count, to another file. The run must finish. | The run. | The run abandoned or stopped, or either file wrong. |
| `fanout` | Six parts to ingest (15 to 30 s each); two of them fail once with a transient error. Each part once, then a summary. | Each part's attempts in turn, the parts side by side. | A part ingested twice, or missing, or a wrong total. |
| `cancel` | A migration that runs 15 minutes with two workers; 15 s after it starts, the driver says to cancel it and leave nothing running. | The injection's time. | The migration's end reached, or any of its processes alive at the check. |
| `contention` | Two teams' six deposits, on accounts a race loses an update on, through a service that takes 2 deposits at once and refuses a third. | max(the work / 2, the busiest account's work). | A lost update, a refused (third) deposit, a payment posted twice, or a wrong balance. |

**The ledger.** Every tool appends JSONL to `/var/lib/async/ledger.jsonl`, outside the working directory: each
step's start (with the duration drawn for it), end, failure, stop, effect, and violation, with its pid, its process's
start time, and wall and monotonic times, and a sha256 chain over the lines. The verifier checks the chain, that
each end follows its start by at least its drawn duration, and that every step ran at the verifier's own time scale,
before the family's outcome; an edited, dropped, or reordered line gives reward 0. It writes `reward.json` and
`problems.json`, and copies the ledger, to its log directory (the trial's `verifier/`). The chain detects an edit,
not a forgery by an agent that rewrites the whole file with the tool's own code.

**The injection** (`interrupt` and `cancel`, in `[metadata.async.injection]`): the message, and its trigger, the
ledger's first `start` of the long job plus a delay, else a time after the agent started. The driver records it in
the ledger, with the trigger that fired, just before it delivers it, and the oracle, which receives none, plays its
part. An arm whose CLI cannot take a message mid-run gets "not measurable" for those two families, never a failure.

## Running

From the repository's root, with bench/README.md's environment (Docker, Harbor 0.23 in `.venv`, `bench/build.sh`'s
binaries, `ANTHROPIC_API_KEY`):

```bash
export THESEUS_BENCH_BIN_DIR=$PWD/bench/bin
export PYTHONPATH=$PWD/bench/harbor:$PWD/bench/async HARBOR_TELEMETRY=0

# The oracle: reward 1 on every family.
.venv/bin/harbor run -p bench/async/tasks -a oracle -o jobs --job-name async-oracle

# Theseus.
.venv/bin/harbor run -p bench/async/tasks -a async_agents:TheseusAsync \
  -m anthropic/claude-sonnet-5-5 -o jobs --job-name async-theseus -k 2

# Claude Code.
.venv/bin/harbor run -p bench/async/tasks -a async_agents:ClaudeCodeAsync \
  -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 \
  -o jobs --job-name async-claude -k 2

# The scores.
python3 bench/async/score.py jobs/async-theseus jobs/async-claude --out /tmp/async
```

`-i <family>` runs one family. A trial takes up to its task's agent timeout (10 to 15 minutes); a family's slow steps
take 1 to 4 minutes when they overlap as they can.

### Theseus (`TheseusAsync`)

The adapter's headless run (`theseus --spawn theseusd --json ask -`) is one turn on a daemon that stops after it,
so a job still running is never read; that is why bench/README.md's profile keeps a command in the turn for up to
900 s. This arm keeps a real daemon for the trial instead (`theseus_bench.daemon_script`) and sets
`[tools] proc_sync_secs` to the product's default, 60 s (`THESEUS_ASYNC_PROC_SYNC`): a command that outlasts it goes
on as a background job, and its late result comes back as a continuation.

- The instruction is the conversation's first `ask -s`, and the injection a second. On main a second `ask -s` while
  a turn runs is queued, never refused: `turn.submit` waits for admission, so the message is answered when the
  running turn ends, which a job moved to the background ends at once.
- The trial ends when no execution runs, is queued, or waits on a job, a task, or a due time, and no wake is pending
  (`driver.Theseus.settle`, each wait `theseus wait --after` the last position, owned by the daemon), or at the
  task's timeout. Then every session is stopped (its turn and its jobs), and the daemon's records are read before it
  stops cleanly: `theseus-history.json` (the conversation), `theseus-tasks.json` and each task session's history
  (`theseus-history-<task id>.json`), `theseus-calls.json` (every `provider.call` row, tasks' sessions included: the
  trial's spend), `theseus-cuts.json` (every `provider.cut` row: a call a stop cut), `theseus-executions.json`, and
  `theseus-health.json`.
- **Measured** as bench/README.md's arms are: the harness sampler (`bench/harbor/sampler.py`) starts before the
  daemon, in a session of its own, and stops after the finish's clean stop (on Harbor's timeout too), with
  theseusd's job wrappers apart. The driver's own calls (settle's polls, the finish's reads and stops) go through
  `<state>/async-driver`, a link to `theseus`, so the sampler counts them outside the harness; the two asks are the
  arm's own client and count as harness. The record (`agent/efficiency.json`, `efficiency.theseus_ledger_record`)
  takes its spend from the ledger (`spend_from: "ledger"`) and the sampler's numbers:
  - **Calls**: `model_calls` is the `provider.call` rows, and `billed_usd` their dollars, by model. A call a `/stop`
    cut is a `provider.cut` row, the kernel's estimate of its input, output, and dollars, which it books as spent:
    it is summed into the tokens and the dollars, and counted apart, as `cut_calls` and `cut_cost_usd` (estimated).
    `cost_usd` is the billed and the estimated together, and so is Harbor's cost. A failed call (a `provider.error`
    row) has no usage or cost, and is in neither.
  - **Tool calls**: the conversation's `tool_call` nodes and each task session's. `tool_calls_from` says which:
    `conversation and tasks`, or `conversation` when a task's history did not read (or the tasks did not), and then
    the count is the conversation's alone.
  - **`truncated`**: each ledger read is the newest 1000 rows (the RPC's cap; the reply's `total` counts every row
    of the ledger, not the read's). A read that returns 1000 marks the record `truncated: true`: the trial's oldest
    calls may be missing, so its spend and calls are a floor, not the trial's. It waits for the CLI to read the
    ledger in pages.
- **Jobs and cgroups.** A container has no systemd, so the daemon's cgroup is not delegated: health's `cgroup` phase
  says `none` ("the daemon runs in /sys/fs/cgroup/, not in a unit of its own", as it did on the build VM), and a job
  stops by its process group and tree, not by a cgroup of its own.

### Claude Code (`ClaudeCodeAsync`)

Harbor's `ClaudeCode` sets `ENABLE_BACKGROUND_TASKS=1` and `FORCE_AUTO_BACKGROUND_TASKS=1` and pipes the instruction
into `claude --print`. This arm runs the same command with stdin from a FIFO (`fifo_path()`,
`/tmp/async-claude-stdin`) in the CLI's stream-json input mode (`--input-format stream-json`, "realtime streaming
input", in `claude --help` of Claude Code 2.1.289): the instruction is the first line, the driver writes the
injection as another, and the input is closed once a `result` event follows the last message (a turn of its own, or the turn it joined mid-run), so the CLI exits. A message
to a CLI already gone is refused, and its cell reads "not measurable". It is bench/harbor's
`MeasuredClaudeCode` underneath: the sampler around the CLI (started through `environment.exec`, so only Harbor's
run command is rewritten) and the efficiency record, which counts the stream's `result` events (`result_events`)
and reads the last one's `modelUsage` and `total_cost_usd` as the session's so far (`result_reading: "session"`;
`efficiency.claude_code_async_record(per_turn=True)` sums them instead, should a live two-message run show them per
turn). The live check says whether the CLI reads a
message mid-turn or queues it for the turn's end; either way the responsiveness column measures it.

### A third arm

OpenClaw (not built yet) fits the same shape: a subclass of its Harbor agent whose `run` starts the agent, runs
`driver.fire` with a `deliver` that hands it a message by its own means, and ends the trial by its own rule.

## The scores

`score.py` writes a row per arm and family (medians over its trials; orphans and duplicated effects are totals):

- **Success**: the verifier's reward, as passed/measured trials.
- **Wall / ideal**: the agent's wall (Harbor's `agent_execution`) over the family's ideal, from the ledger's drawn
  durations.
- **The wait tax**: the model calls, and their tokens, timestamped inside the slow job's window: the model working,
  or polling, while it waits. Theseus's calls come from its daemon's ledger, every other arm's from its ATIF
  trajectory.
- **Responsiveness**: the injection to the right answer's ledger line (ticket-count's end; the migration's first
  stop).
- **Orphans and duplicated effects**: steps that started and never ended, the migration's processes alive at the
  check, and effects done twice.
- **Harness CPU, its peak RSS, and work CPU**: from bench-efficiency's record (`agent/efficiency.json`, else
  `metadata["efficiency"]` in the trial's `result.json`): its `harness.cpu_s`, `harness.peak_rss_kb` in MB, and
  `work.cpu_s`, from trials whose sampler ran (`ok`, or `running` when it never wrote its last summary), as
  bench/report reads them; a trial whose sampler did not run has no numbers, never zeros.
- **Cost**: Harbor's, which for Theseus is the sum of its ledger's model calls and its cut calls' estimates.

Results go in [`docs/benchmarks.md`](../../docs/benchmarks.md).

## Public neighbours

Two public benchmarks measure nearby things, and run through the same Harbor, locally, later: **CooperBench**
(`harbor run -d cooperbench@1.0 ...`), agents cooperating on shared work, and **BFCL**'s parallel function-calling
categories (`harbor run -d bfcl@1.0 ...`). Nothing here is built for them.

## Tests

```bash
python3 -m unittest discover -s bench/async && python3 -m unittest discover -s bench/harbor
```

The standard library runs them without Harbor or Docker: each family's oracle on this host under a scratch
`ASYNC_ROOT` at a time scale of 0.01, with a `TMPDIR` of its own that it must leave empty (an oracle removes what it
makes), and a planted wrong effect per family; the ledger's check against an edited
one; the driver against a fake environment, a stand-in `claude` on the FIFO, and the daemon-mode script under a
stand-in `theseus` (a daemon that answers health only once it is up, a wake pending after its job, the sampler
around it all, and nothing left running after a test); and the scorer over fixture trials worked by hand. Three more run on request:

```bash
ASYNC_HARBOR=1 .venv/bin/python -m unittest discover -s bench/async     # Harbor reads each task; the agents load and run
ASYNC_E2E_BIN=$PWD/target/debug python3 -m unittest test_driver.EndToEnd # (from bench/async) a real daemon
```

The second runs this workspace's `theseusd` on `theseus-sim fake-model --rules`, making the interrupt oracle's
`proc.run` calls: the long job goes to the background, the injection is answered while it runs, and the trial
settles only after the job's continuation (`ASYNC_E2E_KEEP=DIR` keeps its logs).

After changing `tools/asyncbench.py`, run `python3 bench/async/sync.py`; the tests fail on a stale copy.
