# The async bench

Six families of local Harbor tasks where concurrency is the point (theseus-7gir.16), and twenty more whose
independent slow sub-steps measure parallel tool calls (Layer 2, theseus-2wxa), beside a deterministic stand-in
for the calls of one response (Layer 1). Every arm runs the same
tasks through Harbor 0.23 with the same model, limits, attempts, and wall clock, each by its own async means: Theseus
with its background jobs, tasks, wakes, and daemon-owned waits; Claude Code with its background commands and
subagents. An instruction states the task, never how to run it ("in parallel", "in the background"), so every arm
reads one text. A driver sends two families a second message mid-trial, and a scorer reads what each trial left.

| File | What it is |
|---|---|
| `tasks/<family>/` | A Harbor task each: `task.toml`, `instruction.md`, `environment/` (a `python:3.12-slim` image with the family's tools in `/opt/async/bin`), `solution/solve.sh` (the oracle), and `tests/test.sh` (the verifier). |
| `tools/asyncbench.py` | The tools, the ledger, and the checks, standard library only. Each task holds two copies of it, its image's and its verifier's; `sync.py` writes them. |
| `tools/layer2.py` | Layer 2's tools, the facts they draw, its order rules (`DEPS`), ideal walls, overlap, and checks, on asyncbench's ledger. Copied beside the library into each Layer 2 task. |
| `families.py` | Layer 2's twenty tasks: each one's instruction, oracle, files and shape, which `sync.py` writes into `tasks/<family>/`. Not in any image. |
| `layer1.py` | Layer 1's drivers: the stand-in's rules and each harness's command, its wall measured around the CLI. |
| `driver.py` | The injection and its trigger, and each arm's commands. Standard library only. |
| `async_agents.py` | The Harbor agents: `async_agents:TheseusAsync`, `async_agents:ClaudeCodeAsync` and `async_agents:PiAsync`. |
| `score.py` | The scorer: `report.md` and `scores.json` from job directories. Standard library only. |
| `test_tasks.py`, `test_driver.py`, `test_score.py`, `test_layer2.py`, `test_score_layer2.py`, `test_layer1.py` | Their tests. |

## The families

| Family | The task | Its ideal wall | What fails it |
|---|---|---|---|
| `parallel` | Six independent slow digests (20 to 45 s each), then an aggregate file. | The longest digest. | A part missing from the aggregate, or a digest that never finished. |
| `wait-tax` | One index build that ends at a random time (60 to 180 s); its token is needed. | The build. | A token not the build's. |
| `interrupt` | A long training run (150 to 210 s), its score to a file; 20 s after it starts, the driver asks for the open ticket count, to another file. The run must finish. | The run. | The run abandoned or stopped, or either file wrong. |
| `fanout` | Six parts to ingest (15 to 30 s each); two of them fail once with a transient error. Each part once, then a summary. | Each part's attempts in turn, the parts side by side. | A part ingested twice, or missing, or a wrong total. |
| `cancel` | A migration that runs 15 minutes with two workers; 15 s after it starts, the driver says to cancel it and leave nothing running. | The injection's time. | The migration's end reached, or any of its processes alive at the check. |
| `contention` | Two teams' six deposits, on accounts a race loses an update on, through a service that takes 2 deposits at once and refuses a third. | max(the work / 2, the busiest account's work). | A lost update, a refused (third) deposit, a payment posted twice, or a wrong balance. |

## Layer 2: parallel tool calls (theseus-2wxa)

Twenty chores a developer does, each with independent slow sub-steps, so a harness that runs the independent calls
of one response together finishes near the ideal wall, and one that runs them in turn takes their sum. Every slow
sub-step is a tool whose duration is drawn (sleep-based, 10 to 45 s), so load does not move the wall, and the ledger
gives the ideal wall (the critical path of the drawn durations) and the overlap achieved. The facts a task asks
about (which test fails, which host logged the error, which commit is the first bad one) are drawn on a tool's first
use, so only its run tells them. An instruction states the task, never how to run it; the counts are the chores'
own (3 to 8), not tuned to any harness's cap; and a fifth are controls, whose dependent steps punish batching.

| Shape | Family | The task (its tools) |
|---|---|---|
| Independent programs, then combine | `suites` | Four packages' test suites, then the failing tests named (`run-suite`). |
| | `ci-checks` | Lint, type-check and test a project, then a summary (`lint`, `typecheck`, `unit-tests`). |
| | `build-configs` | Build three configurations, then report their sizes (`build-config`). |
| | `bench-settings` | A benchmark at four batch sizes, then the best written to a config (`bench-batch`). |
| | `host-logs` | Six hosts' logs fetched, then the one with the error named (`fetch-log`). |
| | `repos-behind` | Five repositories' fetch and status, then those behind listed (`repo-status`). |
| | `csv-merge` | Five CSV exports converted, then merged (`convert-export`). |
| | `health-restart` | Four services' health checked, the failing one restarted, then verified (`health`, `restart`). |
| Independent reads and fetches | `doc-questions` | Three questions answered from eight long notes on a slow archive (`archive-get`). |
| | `api-summary` | A package's six modules read from a slow mirror, then its API summarised (`module-source`). |
| | `advisories` | Five advisories queried from a slow local registry (`advisory`). |
| | `config-diff` | Four environments' configurations compared (`env-config`). |
| Independent writes | `scaffold` | Eight config files written from a spec, each validated (`validate-config`). |
| | `rename` | One rename across six files, then the tests (`run-tests`). |
| Dependent controls | `pipeline` | Build, then run, then check the output (`build`, `run-app`, `check-output`). |
| | `service` | Start a service, then query it (`start-service`, `query-service`). |
| | `migrations` | Three ordered migrations (`migrate-db`). |
| | `edit-test` | Edit by codemod, then test, then fix (`codemod`, `test-suite`). |
| Mixed | `link` | Two independent slow builds, then the link (`build-lib`, `link-app`). |
| | `bisect` | Four commits tested in worktrees of their own, then the first bad one named (`make-worktree`, `test-commit`). |

**Order rules.** A step that needs another's result is refused, or runs on a stale input, as the real one would
(the app run before its build runs yesterday's binary; a query of a service still starting is refused). And the
check holds the ledger to the family's rules (`layer2.DEPS`): a dependent step whose start came before its
prerequisite's end is an **order violation, and reward 0**, even when the run then does it right. The controls and
the mixed families have rules; the others have none.

Each family is graded by script from what the task left and the ledger. Its oracle does it at the ideal schedule
(independent steps at once, dependent ones in turn) and earns 1; a planted wrong effect per family earns 0
(`test_layer2.py`). `sync.py` writes every Layer 2 task from `families.py` and `tools/layer2.py`.

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

# Pi.
.venv/bin/harbor run -p bench/async/tasks -a async_agents:PiAsync \
  -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 \
  -o jobs --job-name async-pi -k 2

# OpenCode: bench/harbor's measured arm as it is (no family of Layer 2 sends a second message;
# interrupt and cancel read "not measurable" for it).
.venv/bin/harbor run -p bench/async/tasks -a opencode_agent:MeasuredOpenCode \
  -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 \
  -o jobs --job-name async-opencode -k 2

# The scores. LABEL=DIR names a job's arm, for arms Harbor names alike.
python3 bench/async/score.py jobs/async-theseus jobs/async-claude jobs/async-pi jobs/async-opencode --out /tmp/async
```

### The parallel-calls run (theseus-2wxa)

Layer 2's twenty families, six arms, all on Claude Sonnet 5.5 at effort medium (bench/harbor's `measured.py`
convention), $2 and 200 calls a trial, two attempts (`-k 2`): three Theseus arms, each `TheseusAsync` in the bench's
daemon mode at the product's `proc_sync_secs` (60) and its default caps ($2, 200 loops), on a static build of its own
(`bench/build.sh` at its commit, its `bench/bin` copied to `bench/bin-<arm>`, which `THESEUS_BENCH_BIN_DIR` names):

| Arm | Its build | Its report key |
|---|---|---|
| Theseus before | main before theseus-d1hi joined: one response's calls run in turn | `theseus-before` |
| Theseus with d1hi | the calls of one response grouped by class and run together (theseus-d1hi) | `theseus-d1hi` |
| Theseus after | d1hi and the sentence that tells the model so (theseus-da46) | `theseus-after` |
| Claude Code | `async_agents:ClaudeCodeAsync`, pinned as bench/harbor pins it | `claude-code` |
| Pi | `async_agents:PiAsync` | `pi` |
| OpenCode | `opencode_agent:MeasuredOpenCode`, bench/harbor's plain arm | `opencode` |

```bash
L2="-x parallel -x wait-tax -x interrupt -x fanout -x cancel -x contention"  # Layer 2's twenty alone
for arm in before d1hi after; do
  THESEUS_BENCH_BIN_DIR=$PWD/bench/bin-$arm .venv/bin/harbor run -p bench/async/tasks $L2 \
    -a async_agents:TheseusAsync -m anthropic/claude-sonnet-5-5 -o jobs --job-name pc-theseus-$arm -k 2
done
.venv/bin/harbor run -p bench/async/tasks $L2 -a async_agents:ClaudeCodeAsync -m anthropic/claude-sonnet-5-5 \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name pc-claude -k 2
.venv/bin/harbor run -p bench/async/tasks $L2 -a async_agents:PiAsync -m anthropic/claude-sonnet-5-5 \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name pc-pi -k 2
.venv/bin/harbor run -p bench/async/tasks $L2 -a opencode_agent:MeasuredOpenCode -m anthropic/claude-sonnet-5-5 \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name pc-opencode -k 2
```

Then Layer 1 (below), and the report, from both:

```bash
python3 bench/report/parallel.py --date <the run's day> \
  --arm theseus-before=jobs/pc-theseus-before --arm theseus-d1hi=jobs/pc-theseus-d1hi \
  --arm theseus-after=jobs/pc-theseus-after --arm claude-code=jobs/pc-claude --arm pi=jobs/pc-pi \
  --arm opencode=jobs/pc-opencode \
  --layer1 theseus-before=/tmp/l1/theseus-before.json --layer1 theseus-d1hi=/tmp/l1/theseus-d1hi.json \
  --layer1 theseus-after=/tmp/l1/theseus-after.json --layer1 claude-code=/tmp/l1/claude-code.json \
  --layer1 pi=/tmp/l1/pi.json --layer1 opencode=/tmp/l1/opencode.json
```

It writes `docs/benchmarks/<date>-asyncbench-parallel-calls.md` (kept if one stands; `--force`), its `.json` (every
number, the figures, and the omnibus's rows) and `.csv`, and the figures in light and dark.

### Layer 1: N calls in one response, on a stand-in

`theseus-sim fake-model --rules` answers each harness's prompt with one response of N calls of `sleep 2` in that
harness's tool names (Claude Code's `Bash {"command": "sleep 2"}`, Pi's and OpenCode's `bash`, Theseus's
`proc_run`), N in 1, 2, 4, 8, 16, and the result's call with "Done.". `layer1.py` points each harness at it by its own
base-URL setting and times the CLI from spawn to exit: together, about 2 s; in turn, 2 s times N.

```bash
python3 bench/async/layer1.py rules --out /tmp/l1/rules.json
target/release/theseus-sim fake-model --rules /tmp/l1/rules.json --addr 127.0.0.1:9448 &
for h in claude-code pi opencode; do
  python3 bench/async/layer1.py run --harness $h --base http://127.0.0.1:9448 --out /tmp/l1/$h.json
done
for arm in before d1hi after; do
  python3 bench/async/layer1.py run --harness theseus --bin bench/bin-$arm --base http://127.0.0.1:9448 \
    --out /tmp/l1/theseus-$arm.json
done
```

Claude Code is pointed by `ANTHROPIC_BASE_URL`, its side requests quieted
(`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`); Pi by a provider of its own in `models.json` under
`PI_CODING_AGENT_DIR` (`api: anthropic-messages`); OpenCode by `provider.anthropic.options.baseURL` in its own
`OPENCODE_CONFIG`; Theseus by `[model] api_base`. Whether each can be driven so is the first thing the run proves: a
count whose runs fail, or end sooner than one call's 2 s (its calls never ran), is `ok: false` with its reason, and
the report says "not measurable, why", never a number. Theseus's numbers of record are the turn bench's `batch`
kind; its row here is the same wall around the CLI as the others'.

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

### Pi (`PiAsync`, theseus-jp9p)

Harbor's `Pi` runs `pi --print --mode json` with the instruction as its last argument. This arm runs the same
command in Pi's RPC mode instead (`--mode rpc`, "JSONL commands on stdin, session events on stdout", Pi 1.0.4's
rpc.md), its flags kept and its stdin a FIFO (`fifo_path()`, `/tmp/async-pi-stdin`; `driver.pi_stdin`): the
instruction is the first `prompt` command, and the driver's message a `prompt` with `streamingBehavior: "steer"`.
While Pi runs, a steering prompt is queued (`disposition: queued`) and delivered after the current answer's tool
calls, before its next model call; while Pi is idle it starts a run of its own (`disposition: started`). Each run
ends with `agent_settled` (Pi will do no more on its own), and once one follows the last message the input is
closed, which is RPC mode's orderly shutdown. Pi's stream filter is line-buffered here (`stdbuf -oL grep`), since
the driver reads the stream as it runs. A message to a Pi already gone is refused, and its cell reads "not
measurable". It is bench/harbor's `MeasuredPi` underneath: the pinned version, the sampler around Pi, the
trajectory, and the record, from Pi's session log (every run's answers, the injection's included), with
`settled_runs`, the runs the stream settled. By hand, the real Pi 1.0.4 on a stand-in model took a steering
prompt mid-run as above, exited 0 when its input closed, and refused a message after.

### A fourth arm

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
- **Overlap** (theseus-2wxa): the ledger's sum of slow-step time over the slow phase's wall (the first slow start to
  the last slow end, by its monotonic clock): 1 is one step at a time, N is N at once. A family's slow steps are
  its slow job's (Layer 1's six) or every tool's (Layer 2's).
- **Calls per response, and the share of responses with more than one call**: over the trajectory's agent steps
  that call a tool, one step a model response in every arm's ATIF (Theseus's, Pi's from `pi_atif.py`, and Claude
  Code's and OpenCode's from Harbor's converters, which bundle one message's blocks into one step), else OpenCode's
  own stream. A final answer calls no tool and counts in neither. Pooled over an arm's trials in the table.
- **Order violations**: in a family with order rules, each dependent step started before its prerequisite's end.

Every run of it gets its report in [`docs/benchmarks/`](../../docs/benchmarks/README.md) ([`bench/README.md`](../README.md), "Every run gets its report").

## Public neighbours

Two public benchmarks measure nearby things, and run through the same Harbor, locally, later: **CooperBench**
(`harbor run -d cooperbench@1.0 ...`), agents cooperating on shared work, and **BFCL**'s parallel function-calling
categories (`harbor run -d bfcl@1.0 ...`). Nothing here is built for them.

## Tests

```bash
python3 -m unittest discover -s bench/async && python3 -m unittest discover -s bench/harbor
```

The standard library runs them without Harbor or Docker: each family's oracle (Layer 2's twenty too) on this host under a scratch
`ASYNC_ROOT` at a time scale of 0.01, with a `TMPDIR` of its own that it must leave empty (an oracle removes what it
makes), and a planted wrong effect per family; the ledger's check against an edited
one; the driver against a fake environment, a stand-in `claude` and a stand-in `pi` (RPC mode) on the FIFO, and the
daemon-mode script under a
stand-in `theseus` (a daemon that answers health only once it is up, a wake pending after its job, the sampler
around it all, and nothing left running after a test); and the scorer over fixture trials worked by hand, Layer 2's columns over one trajectory per harness format; and
Layer 1's rules, commands, and judgment. Three more run on request:

```bash
ASYNC_HARBOR=1 .venv/bin/python -m unittest discover -s bench/async     # Harbor reads each task; the agents load and run
ASYNC_E2E_BIN=$PWD/target/debug python3 -m unittest test_driver.EndToEnd test_layer1.EndToEnd  # (from bench/async) a real daemon
```

The second runs this workspace's `theseusd` on `theseus-sim fake-model --rules`, making the interrupt oracle's
`proc.run` calls: the long job goes to the background, the injection is answered while it runs, and the trial
settles only after the job's continuation (`ASYNC_E2E_KEEP=DIR` keeps its logs).

After changing `tools/asyncbench.py`, `tools/layer2.py` or `families.py`, run `python3 bench/async/sync.py`; the
tests fail on a stale copy. Under `ASYNC_HARBOR=1`, Harbor's Claude Code and OpenCode converters are run on fixture
sessions too (`test_score_layer2.HarborConverters`).
