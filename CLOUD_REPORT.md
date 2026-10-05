# Cloud report: the async bench (theseus-7gir.16)

Branch `cloud/20261005-bench-async`, from `1720970c` (main at e6f90af3 and later, plus the task commit). Started
08:36 UTC, report written at about 10:00 UTC. All four steps are built: none was cut, contention and cancel
included. No Rust changed, no dependency was added, and bench/'s Python imports only the standard library, apart
from the Harbor that `async_agents.py` imports (as the adapter does).

Commits:

| Commit | What |
|---|---|
| `ed7d73f9` | Step 1: the six task families, their tools, ledger and checks |
| `edae6ddc` | Step 2: the driver, `TheseusAsync`, `ClaudeCodeAsync`, `theseus_bench.daemon_script` |
| `69d2644a` | Step 3: the scorer |
| `08b9a87f` | Step 1 hardened under load: waits on ledger events instead of sleeps; a SIGTERM to the migration reaches its workers |
| `8db94d0d` | Step 4: `bench/async/README.md` |
| `64562755` | Step 2 fix: Claude Code's input closes once a result follows the last message; a timed-out trial's injection task is cancelled |
| `2cd066ea` | Step 2: the end-to-end check's time scale, so it holds under load |

## What I found: the code against the brief

- **The adapter is headless, as the brief says.** `run_script` runs one `theseus --spawn theseusd --json ask -`,
  and the profile sets `proc_sync_secs = 900`. The async arm runs a real daemon for the whole trial instead
  (`daemon_script`). It sets `proc_sync_secs` to the product's default of 60 (override:
  `THESEUS_ASYNC_PROC_SYNC`), so a long command goes on as a background job and its result comes back as a
  continuation. The end-to-end check below confirms that flow on this workspace's daemon.
- **A second `ask -s` while a turn runs is queued, not refused.** `TurnRunner::run` (`turn.rs`) calls
  `admit_input`. If the execution is `running`, or `TurnHeld`/`AdmissionFull`, it waits in `admit` until the
  running turn lets go. So the injection is a second `ask -s`, answered when the running turn ends. A job moved to
  the background ends its turn at once. In the end-to-end run the second turn started 25 ms after the first ended.
- **Tasks have their own sessions.** The trial's spend is every `provider.call` row, read with
  `theseus --json ledger -n 1000 -k provider.call` and summed by `driver.spend`. `ledger.tail` caps `n` at 1000
  (it scans 50,000 rows when filtered), which is plenty for one trial. A trial past 1000 model calls would be
  under-counted; the scorer does not flag that.
- **Jobs get no cgroup in a container.** `cgroup.rs` delegates only a systemd unit with `Delegate=yes`. On this VM
  health's `cgroup` phase said `none: the daemon runs in /sys/fs/cgroup/, not in a unit of its own`. A Docker
  container has no systemd, so jobs stop by their process group and tree. The live check should confirm this from
  `agent/theseus-health.json`.
- **`session.wait --until settled` counts as settled even while a job or task runs** (`main.rs`'s help says so).
  So the trial's end rule (`driver.Theseus.settle`) reads `theseus executions`. Nothing may be `running` or
  `queued`, have `outstanding` or `queued_results`, or wait on `due_at`, `actions` or `execution`. It also reads
  `theseus wakes`, which must be empty. Each wait is `theseus wait SES --until settled --after <position>`, owned by
  the daemon. A pending wake is slept until it is due. Without `--after` an already-settled wait would answer at
  once.
- **Claude Code.** `claude --help` (Claude Code 2.1.289, from npm on this VM) lists
  `--input-format stream-json` ("realtime streaming input", only with `--print`). Harbor 0.23's `ClaudeCode.run`
  pipes the instruction into `claude --verbose --output-format=stream-json ... --print 2>&1 | tee`.
  `ClaudeCodeAsync.exec_as_agent` rewrites that one command (`driver.claude_stdin`):
  - stdin comes from a FIFO (`fifo_path()`, `/tmp/async-claude-stdin`), held open by a `sleep` holder;
  - the instruction is the first stream-json line, and the injection a second;
  - the input closes once a `result` event follows the last message.

  A test checks the rewrite against the exact command Harbor builds (`ASYNC_HARBOR=1`). I could not check without
  a key whether the CLI takes a message mid-turn or queues it. Both are measurable. A message to a CLI already
  gone is refused, and its cell reads "not measurable".
- **Harbor 0.23 details used:**
  - a task's `[metadata]` takes any table (`TaskConfig.metadata: dict`), so each injection lives in
    `[metadata.async.injection]`;
  - `environment.environment_dir` gives the driver the task directory;
  - the verifier reads `/logs/verifier/reward.json`.

  Harbor's `Task` loads all six tasks.

## Step 1: the tasks (`ed7d73f9`, `08b9a87f`)

**What I changed.** I added `bench/async/tasks/{parallel,wait-tax,interrupt,fanout,cancel,contention}`, each a
Harbor task:
- `task.toml`, with the family, the injection and the timeouts;
- `instruction.md`, which says what to do and never how;
- `environment/Dockerfile`, `python:3.12-slim` with the tools in `/opt/async/bin`;
- `solution/solve.sh`, the oracle;
- `tests/test.sh`, the verifier.

All the tools are in one file, `tools/asyncbench.py`. It is copied into each image and each verifier by
`sync.py`, and a test fails on a stale copy.

**The ledger.** It is JSONL at `/var/lib/async/ledger.jsonl`, outside the working directory. Each record holds:
- seq and kind;
- the tool, the step and the pid;
- the process's start time from /proc, so a reused pid is caught;
- wall and monotonic times, read under the lock;
- the drawn duration, the scale, and a sha256 chain.

Durations are drawn at a step's start and logged; `ASYNC_TIME_SCALE` shortens them. The check fails:
- a broken chain or a seq gap;
- time going backwards;
- an end sooner than its drawn duration;
- a step run at a scale other than the verifier's own, so an agent that sets the scale fails.

The check writes `reward.json` and `problems.json`, and copies the ledger into the verifier's logs.

**How I proved it:** `python3 -m unittest discover -s bench/async` (tests in `test_tasks.py`).
- Every oracle gets reward 1 at scale 0.01. The oracles overlap their steps: all six digests start before any
  ends, and contention runs at most two deposits at once, never two on one account.
- One planted wrong effect per family gets reward 0, each with the problem it names:
  - parallel: a part missing from the aggregate;
  - wait-tax: a guessed token;
  - interrupt: an abandoned long job;
  - fanout: an effect twice;
  - cancel: an orphan left running (parent SIGKILLed, two workers alive);
  - contention: a lost update, and a third concurrent deposit.
- An edited value, a dropped line, a rebuilt chain with an end too soon, and a step run at another scale all fail.
- Harbor's `TaskConfig` and `Task` read every task (`ASYNC_HARBOR=1` under Harbor 0.23's python).

**Planted revert.** I skipped the ledger check (`problems = []` in the wait-tax verifier's copy). All four
`test_tasks.Ledger` tests failed. Restored, touched, `git status` clean.

**Under load.** The suites ran at nice 19 beside four busy loops at nice 0. The first run had four failures: the
cancel oracle and three plants slept instead of waiting for their processes' starts. `08b9a87f` and `64562755` fix
them with `await --count`, and run the plants that need two deposits to overlap at a larger scale. After that,
three runs out of three were clean (35 tests, about 395 s each).

**Design choice.** The chain catches an edit, but not a forgery by an agent that reads the tool and rewrites the
whole file. A secret key would need a place the agent cannot read; in these containers the agent is root.

## Step 2: the driver (`edae6ddc`, `64562755`, `2cd066ea`)

**What I changed.**
- `driver.py` (standard library):
  - `injection_of` and `fire`. The trigger is the ledger's first `start` of the long job plus `delay_s`, else
    `at_s` after the agent started. A container-side `asyncbench.py await` waits for it. The injection is written
    to the ledger with the trigger that fired (`by: event` with its seq and time, or `by: time`), then delivered.
  - `Theseus`: the session, ask, settle and finish commands. The finish stops every session, reads history, calls,
    tasks, executions and health, then runs `theseus shutdown` and waits for the daemon's pid.
  - The Claude Code rewrite, its send and close commands, and its log check.
- `async_agents.py`: `TheseusAsync` and `ClaudeCodeAsync`. Each writes `agent/async-driver.json` with the family,
  the trigger, when the message went, and how the trial ended.
- `theseus_bench.py`: `daemon_script` added beside `run_script`, the only addition there. It starts `theseusd`
  with `setsid` on `<state>/theseus.sock`, waits for health, opens the session, and sends the instruction as the
  first `ask -s`.

**How I proved it** (`test_driver.py`):
- **The injection, against a fake environment:**
  - it fires at least `delay_s` after its event and is delivered after it is recorded;
  - with no event, it fires at `at_s`;
  - a refused delivery is reported, not raised.
- **Claude Code's FIFO:**
  - the rewrite matches Harbor's own command;
  - a stand-in `claude` reads both messages from the FIFO and exits once the input closes;
  - a later message is refused with exit 3.
- **The daemon-mode script under a stand-in `theseus`** whose first turn leaves a job running:
  - the daemon gets the trial's socket and config;
  - both asks arrive;
  - the trial settles only after the job's completion, through a `wait --after`;
  - the finish stops sessions before the shutdown;
  - a trial that never settles ends at its deadline.
- **End to end** (`ASYNC_E2E_BIN=$PWD/target/debug`, the interrupt family): this workspace's `theseusd` on
  `theseus-sim fake-model --rules`, making the oracle's `proc.run` calls with `proc_sync_secs = 2`.
  - The first turn's `train-model` went to the background and the turn ended.
  - The injection fired by event and its queued turn ran at once; `ticket-count` ended while the job still ran.
  - The job's continuation came, then the trial settled with reward 1.
  - The run made 5 provider calls, and health's cgroup phase read `none`.
  - Under load it first failed (the job outran the starved driver at scale 0.05). It passed twice at scale 0.2
    beside the busy loops (`2cd066ea`).

**Planted reverts:**
- *An injection firing before its event* (no sleep after the event): `test_the_injection_waits_for_its_event...`
  failed (`1516.297 not >= 1516.740`).
- *The trial ending while a job runs* (`busy()` ignoring `outstanding` and the waits): the trial test errored with
  `job_done_at` missing because it settled before the job, and the deadline and wake tests failed.

Both restored and touched, `git status` clean, the suite OK.

## Step 3: the scorer (`69d2644a`)

**What I changed.** `score.py JOB_DIR... --out DIR`, standard library only, writes `scores.json` (one row per
trial) and `report.md` (one row per arm and family). Its columns:
- success;
- wall over ideal, using the ledger's durations;
- the wait tax: calls and tokens inside the slow job's window, taken from Theseus's ledger rows or else from ATIF
  agent steps;
- responsiveness: the injection to ticket-count's end, or to the migration's first stop;
- orphans: steps never ended, plus processes alive at the check;
- duplicated effects;
- CPU and RAM, from `agent/efficiency.json` when present, under a few candidate key names;
- cost.

An injection that never reached its agent marks the trial "not measurable".

**How I proved it** (`test_score.py`). Four fixture trials with numbers worked by hand:
- interrupt: wall 250 s over an ideal of 200 = 1.25; wait tax 2 calls and 450 tokens out of 4 calls;
  responsiveness 12 s;
- fanout: ideal 30 s (20×0.5 + 10 + 10); 1 duplicated effect; 1 unfinished step; wait tax from the trajectory;
- contention: ideal max(30/2, 18) = 18;
- cancel: not measurable.

Also checked: the report's rows, and that the job's own `result.json` is skipped.

**Planted revert.** The wait tax counting every call: three tests failed (`{'calls': 4, 'tokens': 1478}` where
`{'calls': 2, 'tokens': 450}` was expected). Restored, touched, suite OK.

**What is uncertain.** I don't know bench-efficiency's `efficiency.json` field names. The scorer tries
`cpu_s`/`cpu_seconds`/`cpu_total_s` and `peak_rss_mb`/`max_rss_mb`/`rss_peak_mb`/`ram_mb`/`*_bytes`; align these
at the merge.

## Step 4: the README (`8db94d0d`, amended in `64562755`)

`bench/async/README.md` covers:
- the families table;
- each arm's `harbor run -p bench/async/tasks ...` command;
- each arm's means and settings;
- room for OpenClaw;
- the scores;
- CooperBench (`cooperbench@1.0`) and BFCL's parallel categories (`bfcl@1.0`), to run later;
- the tests.

There are no scores yet. The live run fills them.

**Doc changes for the maintainer** (I did not edit these):
- bench/README.md's files table should link `async/README.md` ("The async bench: six families where concurrency
  is the point").
- docs/benchmarks.md should get an async section once the live run lands.

## The live check (the maintainer's)

From the root, with `bench/build.sh`'s export:

```bash
export PYTHONPATH=$PWD/bench/harbor:$PWD/bench/async HARBOR_TELEMETRY=0
.venv/bin/harbor run -p bench/async/tasks -a oracle -o jobs --job-name async-oracle
```
This should give reward 1 on all six families. The cancel oracle waits for the ledger's three migrate starts, then
sends a SIGTERM to the group.

```bash
.venv/bin/harbor run -p bench/async/tasks -a async_agents:TheseusAsync -m anthropic/claude-sonnet-5-5 -o jobs --job-name async-theseus -k 2
.venv/bin/harbor run -p bench/async/tasks -a async_agents:ClaudeCodeAsync -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name async-claude -k 2
```
What each should show:
- In each interrupt and cancel trial, `verifier/ledger.jsonl` has an `inject` record whose `trigger` is
  `{"by": "event", ...}`. `agent/async-driver.json` has the same trigger and an `ended` of `settled` or `timeout`.
- For Theseus:
  - `agent/theseus-health.json`'s cgroup phase says `none`;
  - `theseus-calls.json` holds every model call;
  - the daemon is gone after the trial (`theseusd.pid` dead, the socket removed).
- For Claude Code: `agent/claude-code.txt` shows the second user message. Whether a second `result` comes or the
  message joins the running turn answers the mid-run question.

```bash
python3 bench/async/score.py jobs/async-theseus jobs/async-claude --out /tmp/async
```
This should print a row per family and arm. "Not measurable" should appear only where a CLI refused the message.

Also worth running:
```bash
ASYNC_HARBOR=1 .venv/bin/python -m unittest discover -s bench/async
cd bench/async && ASYNC_E2E_BIN=$PWD/../../target/debug python3 -m unittest test_driver.EndToEnd
```

## What is left, or uncertain

- **The stream-json user message shape** is `{"type":"user","message":{"role":"user","content":...},
  "parent_tool_use_id":null}`, the Agent SDK's, without a `session_id`. If the CLI rejects it, the first live trial
  will show it at once in `claude-code.txt`.
- **Claude Code's close rule.** The driver reads the stream's tail every 2 s, since the CLI tells the driver
  nothing. In stream-json mode the input closes after the last answer. If a background command finishes after
  that, it may never be read. The same holds in Harbor's own headless run.
- **Theseus's wall** includes the settle and the finish (about 1–2 s of `stop`, the reads and `shutdown`). A
  trial with a child task that never wakes its parent is re-checked at each wait's timeout (the time left), not
  sooner.
- **The interrupt instruction does not mention a second request is coming.** An arm whose first turn blocks on the
  job (proc_sync 900, or a foreground command) answers it late, and responsiveness shows that.
- **The cancel family's oracle** does not receive the message: it plays its part, as the brief allows.
- **OpenClaw** is not built. The README says where its arm fits.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before the first commit:
- fmt, shape, features, clippy, cockpit, test build and the reader rule passed;
- the suite ran 2562 tests: 2529 passed, 33 failed, 19 skipped. All 33 are the known L1 tests: 21 in
  theseus-sandbox (the contract tests and `spawn_100`) and 12 in `theseusd::sandbox`, because a root daemon's job
  without a job cgroup is refused (theseus-pv6i);
- the phases after the suite, run by hand, passed: protocol types unchanged; the turn bench at 5 and 9 frames
  against budgets of 5 and 9; deny with advisories, bans, licenses and sources ok. Lifecycle and jobs were skipped
  under `NO_BENCH`.

The final gate, on `2cd066ea`, matched it:
- the suite again had 2529 passed and the same 33 L1 tests failed (no others);
- protocol types unchanged; the turn bench at 5 and 9 frames, within budget; deny ok;
- the suites passed: `python3 -m unittest discover -s bench/async` (35 tests, 4 skipped without Harbor or
  binaries) and `bench/harbor` (2 skipped). With Harbor (`ASYNC_HARBOR=1`) only the end-to-end check skips, and it
  passes with `ASYNC_E2E_BIN` set.

No timing test from the brief's list failed in either gate run.
