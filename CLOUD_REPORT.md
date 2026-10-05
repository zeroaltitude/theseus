# Cloud report: both async arms measured, and the async bench's missing tests (theseus-z5ty, theseus-6xre)

Branch `cloud/20261005-bench-async-measured`, from `main` at 60b43fb6 (with the task's commit, 31ae3674). Started
20:22 UTC; the report at 22:07 UTC. Every change is Python and Markdown under `bench/async/` and `bench/harbor/`;
nothing the gate builds changed. No new import beyond the standard library and the Harbor the agents already use.

| Commit | Step |
|---|---|
| 23c30398 | 1. z5ty: the scorer reads the record's own fields |
| 7d369168 | 2. 6xre (a), (c), (d), (e): the stand-ins race as a real daemon does, and leave nothing running |
| e98ece38 | 3. z5ty: TheseusAsync sampled, its record the ledger's |
| d29f3dd0 | 4. z5ty: ClaudeCodeAsync on `MeasuredClaudeCode`; 6xre (b): its run tested whole |

**The order differs from the brief's**: 6xre's stand-in fixes went in second, on the unmeasured driver, so that
the measured steps' tests stand on stand-ins that already behave; 6xre (b) (the test of `ClaudeCodeAsync.run`)
went in with the measured Claude Code arm, since it asserts the sampler and the record.

## 1. The scorer (23c30398)

**Found.** `score.py`'s `efficiency()` read top-level `cpu_s` and `peak_rss_mb` (and four other spellings); the
record (`bench-efficiency/1`) has none of them, so every CPU and RAM cell of the async report was empty.

**Changed.** `efficiency(trial, result)` reads `agent/efficiency.json`, else `result.json`'s
`agent_result.metadata["efficiency"]`, only a record whose `schema` is `bench-efficiency/1`, and only when its
`sampler.status` is `ok` or `running` and `harness` is a dict (bench/report's `sampled` rule): `harness_cpu_s`,
`harness_peak_rss_mb` (`harness.peak_rss_kb / 1024`), and `work_cpu_s` (`work.cpu_s`). An unsampled trial gets
None, never zeros. The columns are "Harness CPU s", "Harness peak RSS MB", "Work CPU s". The async README's line
says the same.

**Proved.** `test_score.py`'s fixtures are made by `efficiency.record` over a sampler summary of the real shape
(no cgroup, so the work's CPU is the samples' and the test does not move when bench-efficiency-fixes changes
`machine()`'s cgroup sum): one in `efficiency.json` (4.5 s, 75.0 MB, 31.0 s), one only in `result.json`'s metadata
with its sampler `running`, one whose sampler never ran (None), and a record of the old flat shape and one whose
sampler `failed` (None); the report row is checked to the cell. **Plant**: `harness = e` (the top-level keys
back) fails `test_interrupt_…`, `test_contention_…` and `test_the_report_has_a_row_per_arm_and_family`
(`(None, None, 31.0) != (4.5, 75.0, 31.0)`).

## 2. The stand-ins (7d369168, theseus-6xre a, c, d, e)

**Found.** On HEAD, each run of `TheseusTrial` left one stand-in `theseusd` running (3 of 3 runs, listed by
`ps` with the run's deleted temp dir in its command line; killed by pid). tearDown touched `shutdown` and deleted
the directory, with it the file, often before the daemon's next 50 ms look. Two more came from my own step-1
suite runs, which still had HEAD's test; I killed them by pid.

**Changed** (test_driver.py only):
- (a) The stand-in's `wakes` lists one wake due `STANDIN_WAKE_S` (1 s) after its first look until it is due. The
  trial's test checks the wall clock at settle against the wake's due time, and
  `test_settle_waits_for_a_pending_wake_after_the_job` (job at once, wake 1.5 s on) does it alone.
- (c) `health` exits 1 until the stand-in daemon has written `daemon.env`, which it does after `STANDIN_START_S`;
  `test_the_daemon_is_asked_for_nothing_before_it_answers_health` (0.5 s) checks `sessions open` came after.
- (d) The stand-in daemon also exits once its directory is gone. tearDown touches `shutdown`, waits for the pid in
  `theseusd.pid` (gone or a zombie, 10 s), then asserts nothing whose command line names the test's directory
  runs. The timeout test leaves the stop to tearDown, as it did on HEAD, so tearDown's path is exercised.
- (e) The FIFO test collects its CLI (the stand-in writes its pid), its tee (by its command line) and the FIFO's
  holder (`<fifo>.holder`) and kills each by pid in a `finally`.
- The stand-in's state is read and written under `flock` and replaced with `os.replace`: the CLI and the daemon
  now write it at once (the daemon's start time), and a torn read would have crashed a call.
- The stand-in `claude` reads on a thread, answers each message `STANDIN_ANSWER_S` after reading it, and records
  when it answered each and when its input closed (EOF cuts a message still being answered), for step 4's test.

**Planted reverts** (each restored with `cp` and `touch`, `git status` clean after):
- A3, settle's `if not live and not pending:` as `if not live:`: fails
  `test_settle_waits_for_a_pending_wake_after_the_job` and `test_the_trial_runs_…` (settled 1.3 s and 0.25 s before
  the wake was due).
- The health fix reverted (`health` answers at once), the delay set: fails
  `test_the_daemon_is_asked_for_nothing_before_it_answers_health` (the session opened 0.46 s before the daemon was
  up).
- tearDown's wait and the own-dir check both removed: fails `test_a_trial_that_never_settles_ends_at_its_deadline`
  (`['24121: … /tmp/tmphoss3prt/bin/theseusd']`), and that stand-in was still running after the run: the old leak,
  killed by pid. (Removing them with the sampler's stop still between the touch and the check passed: the stop's
  wait hid the race, so tearDown now stops the sampler first.)

## 3. TheseusAsync measured (e98ece38)

**Found**, and where the code differs from the brief:
- A `provider.call` row's data (theseus-core `fact/turn.rs`, `ProviderCall::row`) carries `model`, `usage` (the
  four Anthropic keys), `cost_usd` (None when the catalog has no price), and more (`loop`, `request_id`, `timing`,
  `node_id`, …). Confirmed.
- **What the rows leave out** that a turn's totals count: a call a `/stop` cut is a `provider.cut` row (estimated
  `input_tokens`, `output_tokens`, `cost_usd`), and a failed call is a `provider.error` row with neither usage nor
  cost (what its settle booked is only in the trace's `action.settle` span). The finish reads only
  `-k provider.call`, so a cancel trial's cut call is not in the record; reading `provider.cut` too would be a
  small follow-up (a second `ledger -k provider.cut` in the finish, summed beside the calls).
- **Tool calls** come from the conversation's history (`theseus-history.json`'s `tool_call` nodes); a task's
  session's tool calls are not read (the finish reads one history), so a trial with tasks under-counts them.
- **`theseus ledger -n 1000 -k provider.call`** returns the newest 1000 matching rows (the RPC caps `n` at 1000), and
  its `total` is not the read's count: in the EndToEnd run it said 82 for 5 rows. A trial past 1000 model calls
  would lose its oldest silently; none comes near.
- **Judge calls** are no `provider.call` rows; the bench profile leaves `[judge]` off (its default), so none are
  made.
- The daemon script's own `theseus health` loop and `theseus sessions open` still run as `theseus`, so they count as
  harness: they are the arm's start, as the headless arm's `--spawn` is.

**Changed:**
- `theseus_bench.daemon_script(bin_dir, state, logs, sampler=None, sample_ms=…)`: links `<state>/async-driver` to
  `theseus` (`POLL`); with a sampler, starts it as `run_script` does (`ARMS["theseus"]`: theseusd's job wrappers
  apart), before the daemon, through `$detach sh -c '<start_script>'` (setsid when present), so it outlives the
  first command and Harbor's cancel of it.
- `driver.Theseus`: `cli` (settle's `executions`, `wakes`, `wait`; the finish's reads, stops and shutdown) runs
  `<state>/async-driver`, whose `comm` is no harness name, so the sampler counts them outside; `ask_cli` (the two
  asks) runs `theseus`. `finish_script(session, sampler=None)` stops the sampler last, after the daemon's clean
  stop; `stop_sampler_script()` for a trial with no session. driver.py now imports bench/harbor's `sampler` and
  `theseus_bench` (both standard library; the directory is on the path wherever driver.py is used).
- `TheseusAsync.run` passes `ta.SAMPLER` and `BENCH_SAMPLE_MS`; its shielded `finally` runs the finish with the
  sampler, or the sampler's stop alone, so Harbor's timeout path stops it too.
- `efficiency.ledger_spend` and `efficiency.theseus_ledger_record` (at efficiency.py's end, nothing of
  bench-efficiency-fixes' changed): the rows by model (`spend_from: "ledger"`, `model_calls` the rows, `cost_usd`
  their sum, None when one has no price), the conversation's tool calls, the sampler's summary, and the trial's
  wall (`async-driver.json`'s `wall_s`) when the sampler has none. `TheseusAsync.populate_context_post_run` writes
  it after the inherited record, replacing `efficiency.json` and `metadata["efficiency"]`, when
  `theseus-calls.json` exists.

**Proved:**
- The stand-in trial now runs with the sampler (50 ms) around it: summary `ok`, the harness seen, `sampler.pid`
  gone; while the driver waits, the sampler's own classifier (`Tracker.classify` on this host's `/proc`) puts
  `theseusd` in harness and `async-driver` outside, never harness. The stand-ins are named by their interpreter
  directly (`#!<python>`), so their `comm` is their own name, as a binary's is.
- `test_efficiency_async.py` (bench/harbor, a file of my own): the spend by model over fixture rows (5 calls,
  $0.0423, the four classes), a row with no price, the record with and without a sampler.
- Under Harbor, `TheseusAsync.populate_context_post_run` over a first turn of 2 calls and $0.0168 and a ledger of 5
  and $0.0423 records `ledger`, 5, $0.0423, and the same in `metadata["efficiency"]` and `context.cost_usd`.
  **Plant**: the record from `ef.theseus_record` (theseus-turn.json) again fails it:
  `('turn', 2, 0.0168) != ('ledger', 5, 0.0423)`.
- **EndToEnd** on this workspace's debug build (`ASYNC_E2E_BIN=$PWD/target/debug`, from bench/async), 37 s: reward
  1, sampler `ok` (368 samples at 100 ms over 36.7 s; harness 0.31 s CPU, peak RSS 95,032 kB, 3 processes; wrappers
  2; work 6; no cgroup `cpu.stat` here, so work CPU is the samples'), `theseusd` harness and `async-driver` outside
  while it ran, the record's `spend_from` `ledger` with 5 calls equal to theseus-calls.json's 5 rows and to the
  history's 5 answers (now an assertion), and health's cgroup phase: `"state": "none", "why": "the daemon runs in
  /sys/fs/cgroup/, not in a unit of its own (\`theseusd install --user\` makes one)"`. Nothing left running.

## 4. ClaudeCodeAsync measured (d29f3dd0), and its run tested (6xre b)

**Changed.** `class ClaudeCodeAsync(cca.MeasuredClaudeCode)`, importing `claude_code_agent`. The sampler starts
through `environment.exec` (MeasuredClaudeCode's), never `exec_as_agent`, so the FIFO rewrite still sees only
Harbor's run command (`claude_stdin` matches only it). `populate_context_post_run` writes
`efficiency.claude_code_async_record` after the measured one.

**The results.** I coded for one reading: **each result's `modelUsage` and `total_cost_usd` are the session's so
far**, so the last result is the trial's bill (`result_reading: "session"`), which is what `claude_code_spend`
already reads. Why: Claude Code keeps its cost and its per-model usage in the process's state, not per turn, and a
result's `usage` is the field that is the turn's. That is my understanding of the CLI, not something I could run
here: the live check 4 below settles it. The other reading is one argument away
(`claude_code_async_record(logs, per_turn=True)` sums every result's `modelUsage` and `total_cost_usd` through
`_summed_results`), and the record counts the results it saw (`result_events`). Tested over a fixture stream of two
results, both readings, and one result (both the same).

**6xre (b).** `ClaudeCodeAsyncRun` (with `ASYNC_HARBOR=1`) runs `ClaudeCodeAsync.run` itself: a fake environment
whose task dir has an interrupt-shaped injection at 1 s, the stand-in `claude` answering 0.5 s after each read,
Harbor's `ClaudeCode.run` patched to issue `HARBOR_RUN` through `exec_as_agent`, and the sampler real
(`claude_code_agent.SAMPLER`/`STATE` patched to this checkout and a temp dir). It asserts both messages seen, both
answered before the input's EOF, the driver's report (`settled`, delivered), the sampler `ok` with the CLI as
harness, the record's `result_events` 2, and nothing left running. **Plant A4** (the closer's wait replaced, so
the input closes right after the injection): `1 != 2 : {'answered': [3002.32], 'eof': 3002.79}`.

## The suites

| Suite | python3 (3.11) | venv 3.12, Harbor 0.23.0, `ASYNC_HARBOR=1` |
|---|---|---|
| `-s bench/async` | 40 run, OK, 6 skipped (35 on HEAD) | 40 run, OK, 1 skipped (EndToEnd) |
| `-s bench/harbor` | 58 run, OK, 7 skipped (50 on HEAD; the new 8 are test_efficiency_async.py) | 58 run, OK |
| `-s bench/report` | 8 run, OK | 8 run, OK |

Skips under python3: EndToEnd (no `ASYNC_E2E_BIN`), and every test that imports Harbor (bench/async: `Agents` ×2,
`ClaudeStdin.test_it_matches_the_command_harbor_builds`, `ClaudeCodeAsyncRun`, test_tasks' Harbor check;
bench/harbor: the 7 Harbor checks). Under the venv only EndToEnd skips, when not asked for.

**Under load** (AGENTS.md's recipe: the suite at `nice -n 19`, four `sh -c 'while :; do :; done'` loops at nice
0, killed by pid), `ASYNC_HARBOR=1 ASYNC_E2E_BIN=$PWD/target/debug` under the venv, `-s bench/async`, 5 times:
5 of 5 OK, 40 tests each with none skipped (EndToEnd and the Harbor tests included), 458 to 554 s a run. After every run, and after every plant, `ps` listed no process whose command line names a run's temp
dir, except the ones named above (HEAD's leak and the planted one), each killed by pid.

## The live checks (the maintainer's, bench/async/README.md's environment)

From the repository's root, with Docker, Harbor 0.23 in `.venv`, `bench/build.sh`'s binaries and
`ANTHROPIC_API_KEY`:

```bash
export THESEUS_BENCH_BIN_DIR=$PWD/bench/bin PYTHONPATH=$PWD/bench/harbor:$PWD/bench/async HARBOR_TELEMETRY=0
```

**1. Theseus, interrupt.**

```bash
.venv/bin/harbor run -p bench/async/tasks -i interrupt -a async_agents:TheseusAsync \
  -m anthropic/claude-sonnet-5-5 -o jobs --job-name am-theseus
python3 - <<'EOF'
import glob, json
for t in sorted(glob.glob("jobs/am-theseus/*/agent")):
    e = json.load(open(f"{t}/efficiency.json")); c = json.load(open(f"{t}/theseus-calls.json"))
    r = json.load(open(f"{t}/../result.json"))
    print(e["spend_from"], e["sampler"]["status"], e["model_calls"], len(c["rows"]),
          e["cost_usd"], r["agent_result"]["cost_usd"], e["harness"], e["wall_from"])
EOF
```

Should show `ledger ok N N X X {cpu_s…, peak_rss_kb…}` with the two counts equal and the two costs equal, and
`sampler.json` beside it; `theseus-health.json`'s cgroup phase `none` in the container.

**2. Claude Code, interrupt.**

```bash
.venv/bin/harbor run -p bench/async/tasks -i interrupt -a async_agents:ClaudeCodeAsync \
  -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 -o jobs --job-name am-claude
python3 -c "import glob, json; [print(json.load(open(f'{t}/async-driver.json'))['ended'], json.load(open(f'{t}/async-driver.json'))['wall_s'], (e := json.load(open(f'{t}/efficiency.json')))['sampler']['status'], e['result_events'], e['harness']) for t in glob.glob('jobs/am-claude/*/agent')]"
```

Should show `settled`, a wall well inside the task's 900 s agent timeout (the run is 150 to 210 s), the sampler
`ok`, `result_events` 1 or 2, and harness numbers.

**3. The scores.** `python3 bench/async/score.py jobs/am-theseus jobs/am-claude --out /tmp/am` and
`cat /tmp/am/report.md`: every Harness CPU s, Harness peak RSS MB and Work CPU s cell filled (no "–").

**4. Claude Code's results, outside Harbor**: two messages that need no tool, the second after the first result.

```bash
mkdir -p /tmp/cc2/config && cd /tmp/cc2 && export CLAUDE_CONFIG_DIR=/tmp/cc2/config
python3 - <<'EOF'
import json, subprocess
p = subprocess.Popen(["claude", "--verbose", "--output-format=stream-json", "--input-format=stream-json",
                      "--print", "--model", "claude-sonnet-5-5"],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def send(text):
    p.stdin.write(json.dumps({"type": "user", "message": {"role": "user", "content": text},
                              "parent_tool_use_id": None}) + "\n")
    p.stdin.flush()
results = []
send("Reply with one word: alpha.")
for line in p.stdout:
    e = json.loads(line)
    if e.get("type") == "result":
        results.append(e)
        if len(results) == 1:
            send("Reply with one word: beta.")
        else:
            break
p.stdin.close(); p.wait()
json.dump(results, open("results.json", "w"), indent=1)
for r in results:
    print(r.get("num_turns"), r.get("total_cost_usd"), json.dumps(r.get("modelUsage")), json.dumps(r.get("usage")))
EOF
cd - && PYTHONPATH=bench/harbor python3 -c "
import efficiency as ef, glob, pathlib
m = ef.session_messages(ef.read_jsonl(map(pathlib.Path, glob.glob('/tmp/cc2/config/projects/*/*.jsonl'))))
print(len(m), {c: sum(ef.tokens(x['usage'])[c] for x in m.values()) for c in ef.CLASSES})"
```

If the second result's `modelUsage` (and `total_cost_usd`) is about the first's plus its own turn's, and its tokens
match the session log's sum over both messages, the reading coded is right. If it is only its own turn's (near the
first's size), set `per_turn=True` in `ClaudeCodeAsync.populate_context_post_run` and the async README's sentence.

## Left, and for the owner

- The live checks above: the results' reading (4) is the one open question of the brief.
- A cut call's estimate (`provider.cut`) is not in the Theseus record; a cancel trial is where it shows. A failed
  call's settle is in no row's data.
- A task's session's tool calls are not counted (one history is read).
- The ledger read's 1000-row cap.
- Docs for the maintainer: bench/README.md's file table could name `harbor/test_efficiency_async.py` under
  `harbor/test_*.py` (it already does by the glob), and docs/benchmarks.md, when the async bench's first measured
  run is published, should say the async report's CPU columns are the harness's and the work's, by the sampler.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before the first commit and before this one:
both runs alike. fmt, shape, features, clippy, cockpit, test build and the reader rule pass. The suite: 2789
tests run, 2756 passed, 33 failed, 21 skipped. The 33 are the known L1 cases on this root VM with no job cgroup
(theseus-pv6i): theseus-sandbox's 19 contract tests and its bench's `spawn_100`, and theseusd's 13 `sandbox` tests.
No other test failed, and none needed a retry. The phases after it, run by hand: protocol types ok (nothing under
cockpit/src/protocol.gen changed), the turn step `theseus-sim bench turn --check --runs 5 --burst 0` ok (5 and 9
frames at the p95, at their budgets), the lifecycle and jobs benches skipped (`THESEUS_GATE_NO_BENCH`), and deny
(`cargo deny --offline --log-level error check`) ok: advisories, bans, licences, sources. `cargo deny fetch`
worked at setup. No Rust, no lockfile and no package-lock changed.

One more HEAD leak, for the record: running HEAD's bench/async suite once more (in a temporary worktree, for the
counts above) left one stand-in `theseusd` again (pid 8355, killed by pid).
