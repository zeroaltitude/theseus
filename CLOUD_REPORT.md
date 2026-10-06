# Cloud report: Pi as the benchmarks' fourth arm (theseus-jp9p)

Branch `cloud/20261006-bench-pi`, from `main` at 2bf9e7d3 (the task commit aa792531 on top). Started 18:53 UTC,
report written 19:57 UTC. Everything is under `bench/`; nothing outside it changed.

| Commit | Step |
|---|---|
| 956a2f0a | 1–3, 5: Pi learned; `pi_agent:MeasuredPi`, its record, trajectory, fair limits, tests, README |
| 6fbed317 | 4: the efficiency report takes a Pi arm |
| 47c1efcd | 4: the recall bench drives Pi |
| bac6d794 | 4: the async bench runs Pi (RPC mode, steering prompt) |

## 1. Pi, learned (the report's facts, with versions)

From the npm registry, the package's own docs (`docs/*.md` in the tarball), its bundle, and runs of the real CLI on
this VM against a local stand-in Anthropic endpoint (no key was used or needed).

- **Package.** `@earendil-works/pi-coding-agent`, repository github.com/earendil-works/pi (`packages/coding-agent`).
  Latest **1.0.4**, published 2026-10-05 21:51 UTC (1.0.0 on 2026-10-01); a `legacy-node20` tag at 0.74.2. The old
  name `@mariozechner/pi-coding-agent` (last 0.73.1) is deprecated ("use @earendil-works/pi-coding-agent"). A Node
  CLI: `bin: {pi: dist/bundle/cli.js}`, `engines: node >=22.19.0`. Installed with `npm install -g
  @earendil-works/pi-coding-agent@1.0.4`. It sets `process.title = "pi"`, so `/proc/<pid>/comm` is `pi` (checked on
  this VM), which is the sampler's harness name.
- **One task, non-interactively.** `pi --print --mode json [flags] "<prompt>"` (or the prompt on stdin): JSONL on
  stdout, a `session` header, then events (`agent_start`, `turn_start`, `message_start`/`message_update`/
  `message_end`, `tool_execution_*`, `turn_end`, `agent_end`, `agent_settled`), then exit. `--mode rpc` reads JSONL
  commands on stdin (`prompt`, with `streamingBehavior: "steer"|"followUp"` while it runs; `steer`, `follow_up`,
  `compact`, `abort`, …) until stdin closes. Print mode sends `/compact` to the model as plain text (checked).
- **Model and key.** `--provider anthropic --model claude-sonnet-5-5` (or `provider/id`, `:thinking` suffix);
  `--thinking off|minimal|low|medium|high|xhigh|max`, default **medium** (to Sonnet 5.5 it sent adaptive thinking
  and `max_tokens: 128000`, seen at the stand-in). The key is `ANTHROPIC_API_KEY` (also `ANTHROPIC_OAUTH_TOKEN`,
  `ANTHROPIC_AUTH_TOKEN`, or `--api-key`); with none it exits 1 ("No API key found for anthropic"). A base URL goes in
  `$PI_CODING_AGENT_DIR/models.json` (`providers.anthropic.baseUrl`). Its catalog prices Sonnet 5.5 at $2 / $10 /
  $0.20 read / $2.50 write per million, window 1,000,000, output 128,000.
- **Limits.** None on turns, none on spend, none on wall time (only a codemode script's `timeout_ms` and an optional
  per-call bash timeout). It retries a failed request itself (`retry.*` settings) and compacts itself when the
  context passes the model's window less `reserveTokens` (16,384), keeping the newest `keepRecentTokens` (20,000).
- **Default tools and prompt.** `read`, `bash`, `edit`, `write`; a system prompt of five sections (preamble, tools,
  rules, docs, cwd), about 2,300 tokens with the tools, and no date.
- **Where it writes its session.** `~/.pi/agent/sessions/--<cwd>--/<ts>_<id>.jsonl`, or `--session-dir`; JSONL
  version 3, entries in a tree by `id`/`parentId`. Each assistant message carries `model`, `stopReason`
  (`stop|length|toolUse|error|aborted`), `errorMessage`, and `usage` `{input, output, cacheRead, cacheWrite,
  totalTokens, cost: {…, total}}` (Pi's own price). `compaction`, `branch_summary` and `usage` entries carry their
  own call's usage; a tool result may carry nested `usage`. A failed request is persisted as an answer with
  `stopReason: "error"` and zero usage. The file is written as each entry settles.
- **Exit code.** In `--print --mode json`, **0 even when the provider fails** (seen with a 401): the failure is only
  in the last answer's `stopReason`/`errorMessage`.
- **Harbor 0.23 ships a Pi agent** (`harbor/agents/installed/pi.py`, `-a pi`): Node 22 through nvm, `npm install -g
  --ignore-scripts <package>@<version or latest>`, the run `pi --print --mode json --session-dir
  /logs/agent/pi/sessions --provider <p> --model <id> <flags> '<instruction>' 2>&1 </dev/null | grep -v
  message_update | stdbuf -oL tee /logs/agent/pi.txt`, a `thinking` option, no ATIF trajectory, and counters from
  the stream with the cache write left out of `n_input_tokens`.

## 2. `bench/harbor/pi_agent.py`: `-a pi_agent:MeasuredPi` (956a2f0a)

**Found:** Harbor's `Pi` exists, so the arm subclasses it as `claude_code_agent.py` subclasses `ClaudeCode`; Harbor's
install, command line, key, `thinking` and `name()` ("pi") stay Harbor's.

**Changed:**
- `pi_agent.py`: `PINNED_VERSION = "1.0.4"` (a `version` kwarg wins); `MeasuredPiOptions` adds `max_budget_usd` and
  `max_turns` as plain fields, never flags; `capabilities` adds `atif`; `install` uploads the sampler after Harbor's;
  `run` starts the sampler before Harbor's run and stops it in a `finally`; `populate_context_post_run` writes the
  ATIF trajectory, the record, and Harbor's three counters from the record (the cache write inside the input, as the
  other arms' are).
- `pi_atif.py`: Pi's session log as ATIF v1.7 (user steps; an agent step per answer with text, thinking as
  `reasoning_content`, tool calls, metrics, and its results as observation; a compaction/summary/usage entry as an
  agent step of its own; a tool's nested usage as an agent step marked `nested`; a result with no call a system
  step).
- `efficiency.py` (additive only): `ARMS["pi"] = {"names": ("pi",)}`; `pi_calls`, `pi_spend` (session log, each
  entry once by id; else the stream's `message_end`s and `compaction_end`s; else the trajectory), `pi_end` (last
  answer's stopReason and error), `pi_limits`, `pi_wall`, `pi_record`.
- `README.md`: the arm, its command, what it adds, where its spend comes from, and the limits table.

**Proved:**
- `python3 -m unittest discover -s bench/harbor`: 89 tests OK (15 skipped, Harbor's); under Harbor's venv
  (`/tmp/hvenv`, harbor 0.23.0, Python 3.12): 89 OK. `test_pi_agent.py` is 20 of them (12 without Harbor).
- The real Pi 1.0.4 run on a stand-in endpoint (one bash call, then an answer): `pi_record` read 2 calls, 1 tool
  call, $0.00579, all four classes, from its session log and identically from its stream; the trajectory validated
  against Harbor's `Trajectory` model and round-tripped; `compute_model_usage` matched. The sampler around it saw
  `pi` as the harness (0.3 s CPU, 108 MB RSS) and its busy shell as work (1.07 s).
- The Theseus and Claude Code records are byte-equal to the old module's (`theseus_record`, `claude_code_record`,
  `claude_code_async_record`, `theseus_ledger_record` over the test fixtures, old `efficiency.py` from `HEAD` vs new).
- Planted reverts (each restored from a backup and `touch`ed, `git status` clean after):

  | # | Plant | Failed |
  |---|---|---|
  | 1 | session-log entries not deduplicated by id | the session-log, record, limits and arm-record tests |
  | 2 | a tool's nested usage counted as a call | session-log, stream |
  | 3 | no stream fallback | stream |
  | 4 | install unpinned (`version=version`) | install pinned, the arm's record (version) |
  | 5 | `max_turns` annotated `Cli("--max-turns")` | options test, command-line test |
  | 6 | sampler stop not in a `finally` | Harbor's timeout still stops the sampler |
  | 7 | sampler started after Harbor's run | sampler runs around Harbor's run |
  | 8 | Harbor's counters kept (write left out) | the arm's record-and-counters test |
  | 9 | no trajectory written | the arm's record test, the no-logs test |
  | 10 | results not tied to their calls | trajectory steps and totals |
  | 11 | cache write left out of `prompt_tokens` | trajectory steps, totals, Harbor's usage |
  | 12 | summaries not steps | trajectory steps, totals, Harbor's usage |
  | 13 | `over_budget` always false | limits, the arm's record |
  | 14 | `end` from the first answer | provider-error end, record |
  | 15 | `ARMS["pi"]` named `node` | arm name, sampler around the run |

## 3. Fair limits

Pi has neither a spend cap nor a turn cap, and none was added inside it. The arm takes `--ak max_budget_usd=2.0
--ak max_turns=200` as Claude Code's does, passes neither to Pi, and the record's `limits` holds them with
`enforced: false`, `over_budget` (dollars past the cap) and `over_turns` (answers past it, as Claude Code's
`--max-turns` counts turns). The table is in `bench/README.md` ("Fair limits"): model, attempts and wall clock the
same for all; spend and turn caps enforced for Theseus (exit 5, 8) and Claude Code, recorded and flagged for Pi;
thinking each harness's default (Pi's is `medium`, `--ak thinking=` sets it); tools; version (Pi pinned at 1.0.4);
a provider's failure (Pi exits 0, its record's `end` says `error`); a timeout (Harbor cancels the run, as for
Claude Code).

## 4. The report and the suites

- **Report (6fbed317).** `--arm pi=<job>` works like any arm: its name, its Pareto rows and chart points, and the
  charts' colours (blue on the front, grey off it; the report colours by the front, not by arm, so Pi needs none of
  its own). An old Pi trial with no record is rebuilt from its session log or stream (`record_from: "pi files"`).
  When an arm's records carry `limits`, the table adds "Trials past the others' caps (not enforced)" and a note; a
  report without such an arm is byte-identical (report.md, trials.csv, the three SVGs over the test's fixture jobs,
  old vs new). `python3 -m unittest discover -s bench/report`: 11 OK. Plants: Pi files not read → the old-Pi-trial
  test fails; the caps row never shown, or always shown → the caps test fails.
- **Recall (47c1efcd).** `drive.py --arm pi`, under an hour's work as the Claude Code driver's shape fit. See the
  recall README. Compaction is Pi's own threshold set at the progression's window (`reserveTokens` = model window −
  progression window, and `keepRecentTokens` = min(20,000, window/4)). **Found on the real CLI:** with Pi's default
  `keepRecentTokens` (20,000) above the whole context, Pi 1.0.4 silently skips the compaction, so a window at or
  under ~20k never compacts without that second setting. Tests: a stand-in `pi` drives the smoke (session ids,
  settings, compaction from the log, parent variables stripped, per-turn spend) and the failure rule.
  `python3 -m unittest discover -s bench/recall`: 71 OK, none skipped (the Theseus driver's end-to-end tests ran too, on this VM's
  `target/debug`). Plants r1–r8 (reserve wrong, compactions not read, a new session per turn, parent env kept,
  provider error not a failure, only the last answer's spend, summaries' spend left out, keep at Pi's default): each
  failed a PiDriver test. By hand, the real Pi 1.0.4 on `standin.py`: the smoke ran 30 turns, 9 of 9 facts delivered,
  nothing left running; with `--context-window 10000` it compacted at turn 11 (10,323 tokens before) and the driver
  recorded it with the summary call's cost in that turn.
- **Async (bac6d794).** `async_agents:PiAsync`, Pi in RPC mode on a FIFO, the injection a steering `prompt`, the
  input closed after `agent_settled` follows the last message. **Found:** Harbor's Pi command block-buffers its
  `grep -v` filter, so the stream is unreadable live; the rewrite runs it under `stdbuf -oL`. Tests: the rewrite
  (including a quote in the instruction, and with `ASYNC_HARBOR` against the command Harbor's Pi builds), a stand-in
  `pi --mode rpc` on the FIFO, and `PiAsync.run` end to end with the sampler. `python3 -m unittest discover -s
  bench/async`: 44 OK (8 skipped); with `ASYNC_HARBOR=1` under Harbor: 44 OK (1 skipped). Plants a1–a7 (no steer,
  instruction left on the command line, `result` grepped for `agent_settled`, no rewrite, closed before the
  injection's answer, no `settled_runs`, the filter buffered) each failed a Pi test (a3 and a7 by the test's 60 s
  deadline). By hand with the real Pi: the steer came mid-tool-call, Pi answered `disposition: queued`, delivered it
  before its next model call, settled once, exited 0 when the input closed, and refused a later message (exit 3).
- **Under load** (four busy loops at nice 0, the tests at nice 19): harbor `test_pi_agent` + `test_sampler` 40 OK;
  async Pi and Claude Code FIFO and run tests 8 OK; recall Pi and Claude Code drivers 4 OK; report 11 OK.

## 5. Tests (`bench/harbor/test_pi_agent.py`)

Each behaviour the task named, with the plant that fails it (table above): the command line, model and limits
(plant 5); the pinned install (4); the record from a fixture of Pi's session log, invented content in the shape the
real 1.0.4 writes (1, 2, 3, 13, 14); the trajectory's shape (10, 11, 12); the sampler started before and stopped in
a `finally` (6, 7); the counters (8) and the trajectory written (9); the arm's process name (15).

## Live check (the maintainer's)

No Docker daemon and no Anthropic key on this VM, so no Terminal-Bench task ran here. From the repository root, with
Harbor 0.23 in `.venv`, `ANTHROPIC_API_KEY` set, and `THESEUS_BENCH_BIN_DIR` from `bench/build.sh`:

```bash
export PYTHONPATH=$PWD/bench/harbor HARBOR_TELEMETRY=0
M=anthropic/claude-sonnet-5-5
# Theseus (its caps: THESEUS_BENCH_SPEND_LIMIT=2.0, THESEUS_BENCH_MAX_LOOPS=200, the defaults)
.venv/bin/harbor run -d terminal-bench@2.0 -i fix-git -a theseus_agent:Theseus -m $M -o jobs --job-name live-theseus
# Claude Code
.venv/bin/harbor run -d terminal-bench@2.0 -i fix-git -a claude_code_agent:MeasuredClaudeCode -m $M \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name live-claude
# Pi
.venv/bin/harbor run -d terminal-bench@2.0 -i fix-git -a pi_agent:MeasuredPi -m $M \
  --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name live-pi
# OpenClaw: Harbor's own adapter (bench/ has no measured OpenClaw arm yet)
.venv/bin/harbor run -d terminal-bench@2.0 -i fix-git -a openclaw -m $M -o jobs --job-name live-openclaw
# The report over the four
python3 bench/report/efficiency.py --arm theseus=jobs/live-theseus --arm claude-code=jobs/live-claude \
  --arm pi=jobs/live-pi --arm openclaw=jobs/live-openclaw --out /tmp/live-eff
```

What each should show (fix-git is an easy task: reward 1 expected, a few cents a trial):
- **Pi:** `jobs/live-pi/*/agent/` holds `pi.txt`, `pi/sessions/*.jsonl`, `trajectory.json`, `efficiency.json`,
  `sampler.json`. `agent/setup` shows `npm install -g --ignore-scripts @earendil-works/pi-coding-agent@1.0.4` and
  `pi --version` → `1.0.4`. In `efficiency.json`: `arm: "pi"`, `spend_from: "session_log"`, all four token classes
  with `cache_write` > 0, `cost_usd` a few cents and equal to `result.json`'s `agent_result.cost_usd`,
  `model_calls` = the session log's assistant messages, `harness.processes` ≥ 1 with `harness.peak_rss_kb` near
  100 MB, `sampler.status: "ok"`, `end.stop_reason: "stop"`, `limits: {enforced: false, max_budget_usd: 2.0,
  max_turns: 200, over_budget: false, over_turns: false}`. `result.json`'s `n_input_tokens` = input + cache read +
  cache write of the record.
- **Theseus and Claude Code:** as before (reward 1, `efficiency.json` with `arm` `theseus` / `claude-code`).
- **OpenClaw:** reward and Harbor's dollars; its record is rebuilt from its trajectory or Harbor's counters, CPU and
  RAM "not sampled".
- **The report:** four columns; the per-arm table gains the "past the others' caps" row (`0/1` for pi, `–` for the
  rest); each Pareto table lists the arms with their scores. If the setup times out installing Node, give Pi the same
  tripled agent setup timeout the first run gave Claude Code.

Optional, the other suites (keys needed): `python3 bench/recall/drive.py --arm pi --model $M --progression
/tmp/rc-smoke --out /tmp/rc-pi` (in a throwaway container: Pi's tools are not confined) should exit 0 with every
turn exit 0; `harbor run -p bench/async/tasks -a async_agents:PiAsync -m $M --ak max_budget_usd=2.0 --ak
max_turns=200 -o jobs --job-name async-pi` (PYTHONPATH with bench/harbor and bench/async) should leave
`async-driver.json` with `ended: "settled"` and the injection `delivered.sent: true`.

## Left, uncertain, and design choices for the owner

- **No enforced caps for Pi.** By the brief, none was added; a Pi trial past $2 or 200 answers keeps its reward and
  is counted in the report. If the owner wants hard parity, an extension could enforce it, but then it is no longer
  Pi as shipped.
- **Thinking.** Each harness runs at its own default; Pi's is `medium`. Whether Claude Code's and Theseus's defaults
  match that is not established here; `--ak thinking=` can align Pi.
- **A timeout.** As for Claude Code, Harbor cancels the run and the arm sends Pi no signal; whether Pi keeps working
  in the container while the verifier runs depends on Harbor's environment. The Theseus arm stops its turn. Worth a
  look in the live check (`agent/pi.txt` after the timeout).
- **The install needs the network in the task's container** (nvm, Node, npm) and a glibc image, as Harbor's own Pi
  and Claude Code installs do.
- **`--ignore-scripts`** is Harbor's choice; Pi 1.0.4 ran from such an install here.
- **Recall plans at Theseus's overhead.** The smoke never fills Pi's context to its 45k window (Pi's prompt is ~2.3k
  tokens vs the 13.7k planned), so Pi does not compact on it. A Pi-planned progression (`generate.py --overhead
  2300`) would place its marks for Pi; which the comparison should use is the owner's call.
- **`keepRecentTokens`** at a quarter of a small window is my choice (Pi's default would make small windows never
  compact); it is recorded in `run.json`'s `pi_compact`.
- **Async steering vs follow-up.** The injection is a `steer` (taken before Pi's next model call); `followUp` would
  wait for the run's end. Steer is the more responsive of Pi's two; the README says which.
- **Model calls.** A failed request Pi retried counts as a call (it is persisted as an answer), as the record's
  "a retry is a call" says; a tool's nested model work counts its tokens and dollars but no call.

**Doc text for `docs/benchmarks.md`** (the maintainer's to place), for its Terminal-Bench run description, once a
Pi run lands:

> - **D. Pi 1.0.4** (`@earendil-works/pi-coding-agent`), the minimal coding agent, through Harbor's own adapter
>   with the measured arm's additions (`-a pi_agent:MeasuredPi`: the version pinned, the harness sampler, an ATIF
>   trajectory and the efficiency record from Pi's session log). Pi has no spend or turn cap: `max_budget_usd=2.0`
>   and `max_turns=200` are recorded, not enforced, and its trials past either are counted in the report. Thinking
>   at Pi's default, `medium`.

and a results-table row: `| <date> | – | Pi 1.0.4 | Claude Sonnet 5.5 | 89 × 2 | <mean reward> | <cost> |`.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run before the first commit (on aa792531) and again
before this report (on bac6d794): both times fmt, shape, features, clippy, cockpit and the test build passed, the
reader rule passed, and the suite ran 2,980 tests: 2,947 passed, 24 skipped, **33 failed, all the known L1 ones**
(theseus-pv6i: the VM runs as root with no job cgroup): 20 `theseus-sandbox::contract` clauses, `theseus-sandbox::bench
spawn_100`, and 12 `theseusd::sandbox` tests. No other test failed, none was retried, and the output golden passed
under the TZ. After the suite, run by hand: `cockpit/src/protocol.gen` unchanged (protocol types pass), and no crate
compiled under the lock. The benches are skipped by `THESEUS_GATE_NO_BENCH`. Counted green per the brief. The
branch changes only `bench/`, which nothing the gate builds reads except `bench/theseus-bench.toml` (untouched).

bench's own suites on the final tree: harbor 89 OK (15 skipped) under python3 3.11 and 89 OK under Harbor's venv
(Python 3.12, harbor 0.23.0); report 11 OK; recall 71 OK; async 44 OK (8 skipped), and 44 OK (1 skipped) with
`ASYNC_HARBOR=1` under Harbor's venv.
