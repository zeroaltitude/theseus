# Cloud report: bench-efficiency (theseus-7gir.12)

Branch `cloud/20261005-bench-efficiency`, from `072734df` (the task commit on `80ef1dea`). Started 08:36 UTC; this
report written by 09:50 UTC. All of it is Python and Markdown under `bench/`; no Rust, no dependency, no store
format change.

## Commits

| Commit | Step |
|---|---|
| `c41e43bf` | 2. The sampler, `bench/harbor/sampler.py` (one file, stdlib, Python ≥ 3.6), and `test_sampler.py` |
| `fdeeda6c` | 1. The record, `bench/harbor/efficiency.py`, and `test_efficiency.py` |
| `ad6048a6` | 3. Theseus's adapter: sampler in `run_script`/`stop_script`, uploaded at install, record in `populate_context_post_run` |
| `44cf178e` | 4. `bench/harbor/claude_code_agent.py`: `MeasuredClaudeCode`, and `test_claude_code_agent.py` (needs Harbor) |
| `952e8a9f` | 5. `bench/report/efficiency.py` and `test_report.py` |
| `b568b2a0` | 6. `bench/README.md` |
| `3791637f` | Fix found by a live run: the job wrapper's comm is `exe`; name it by `argv[0]` |
| `fd2fe6d6` | Report: dollars under a dime to four places |
| `590a54fc` | Tests made robust under load (timing only) |

I committed the sampler before the record, the reverse of the brief's numbering, because the record reads the
sampler's summary. Each commit was checked green on its own in a scratch worktree: both suites on the standard
library, and the harbor suite in a Harbor 0.23.0 venv.

## What I found (and how it differs from the plan)

- **Harbor 0.23.0 needs Python ≥ 3.12.** This VM's `python3` is 3.11, so the standard-library suites run there,
  and Harbor runs in a `python3.12` venv (`/tmp/hvenv`). `theseus_agent.py` already uses `typing.override` (3.12).
- **Harbor's converter already counts each Claude Code message once.** `_convert_events_to_trajectory` keeps the
  last usage per `message.id` (`last_usage_by_msg_id`) and reports it on the message's first step. The record does
  the same when it reads the session log itself, and a test shows both give the same calls and cache writes.
- **Where Claude Code's spend comes from: the stream-json `result` event first.** Its `modelUsage` is Claude
  Code's own bill, per model, in all four classes, and it includes calls the session log doesn't hold (a background
  model's). A timed-out run never prints the event, so then the record reads the session log (each message id once,
  with its last usage, dollars from Harbor's trajectory estimate), and then Harbor's `trajectory.json` (the write is
  in `metrics.extra.cache_creation_input_tokens`). Model and tool calls always come from the session log (unique
  message ids and `tool_use` ids), or the trajectory. **Uncertain:** I wrote the `modelUsage` keys (`inputTokens`,
  `cacheReadInputTokens`, `cacheCreationInputTokens`, `outputTokens`, `costUSD`) from memory of Claude Code's SDK
  result message; Harbor's own code reads only `total_cost_usd` from the event. Live check 2 confirms or refutes
  this. If the keys differ, the trial falls back to the session log, and its `spend_from` says so.
- **Theseus: tokens by model and model calls.** The turn's `usage` is the total. Each history answer carries its
  `model`, `usage` and `cost_usd`, so the record splits by model. Whatever the answers don't account for (a cut
  call, a failed try settled at the kernel's estimate) goes to the turn's `model`. Model calls are the turn trace's
  `provider` spans (each retry and the fallback count one), else the answers, else `loops`. A refusal fallback's two
  models are billed apart (tested).
- **The job wrapper's comm is `exe`, not `theseusd`.** theseusd starts it through `/proc/self/exe`. I found this
  by running the real debug binaries under the sampler (below), where the wrapper first showed up as work. A process
  whose comm is `exe` is now named by its `argv[0]`'s basename (`3791637f`), and wrappers (`job-wrapper`,
  `job-sandbox` as `argv[1]`) stay apart. The same should hold for anything else theseusd re-executes.
- **CPU method: `/proc/<pid>/stat` ticks, not procfs.rs's `schedstat`.** The reaped-children accounting needs
  `cutime`/`cstime`, which are ticks. Summing `schedstat` per thread also loses exited threads. Resolution is 10 ms
  per process. When a tree process's `cutime` grows by more than the last-seen time of the processes that vanished
  under it (and theirs), the remainder goes to work. It goes to the harness only when everything that vanished was
  harness (the CLI reaping its daemon).
- **cgroup v2.** The sampler finds the cgroup from `/proc/self/cgroup`'s `0::` line, under `/sys/fs/cgroup` or a
  hybrid's `unified/`. It refuses a cgroup with no `cgroup.type`: that is the machine's own root, which is what this
  VM shows. Work CPU = cgroup total − harness − wrappers − sampled outside processes − sampler (`cpu_from:
  "cgroup"`); without a cgroup it is the samples (`cpu_from: "samples"`). `memory.peak` counts page cache and the
  container's whole life (install included), so I record it as `container.memory_peak_kb` and don't use it for
  peaks. This VM has no usable container cgroup, so the cgroup path is tested on fixtures only.
- **No python3 in the image: no shell fallback.** The start script writes `{"status": "unavailable", "reason":
  "no python3 on PATH"}` (or "older than 3.6") and the trial goes on. A shell loop over `/proc` would fork per read
  and cost more than what it measures.
- **Interval, justified by the sampler's own CPU** (measured in a PID namespace with 23 processes): 100 ms 0.77% of
  a core, **250 ms 0.31%**, 500 ms 0.16%. On this host, with 81 processes, 250 ms costs 0.7%. Reads go through
  `os.open`/`os.read` (open()'s text layer cost about a third more), and memory is read only for tree processes.
- **What the method misses** (also in `sampler.py`'s head): a harness child that lives less than one interval is
  counted as work; a reaped child's last partial interval goes to its reaper's rule; an orphan the container's init
  reaps loses its last interval; zombies count once reaped; a summed-RSS peak between two samples is missed (each
  process's own `VmHWM` is kept).
- **The sampler's `wall_s`** is its own window. The report takes wall time from Harbor's `agent_execution` in
  `result.json`, as the brief says.
- **Wrappers are a third class**, `wrappers`, next to `harness` and `work`. "Harness CPU per tool call" uses
  harness only.

## How it was proved

Suites (standard library, `python3` 3.11): `python3 -m unittest discover -s bench/harbor` ran 50 tests, OK (7
skipped need Harbor). `python3 -m unittest discover -s bench/report` ran 8, OK. With Harbor 0.23.0
(`/tmp/hvenv/bin/python`): 50 harbor tests, OK, 0 skipped. That includes the ATIF checks, the Theseus adapter's
load, and `MeasuredClaudeCode`: it is `ClaudeCode` by `name()` and options, the sampler starts before Harbor's run
and stops after it and after a `CancelledError`, install uploads the sampler after Harbor's own install, and the
record follows Harbor's own `populate_context_post_run`.

Offline proofs the brief asked for:
- parsers on fixture `/proc` text (stat with parentheses in the name, status, cmdline, `cpu.stat`, cgroup paths);
- classes on a fixture tree (init, a shell, `theseus`, `theseusd`, its wrapper (comm `exe`), `bash`, `cc1`), a
  reaped child no sample saw, a CLI reaping its daemon, a harness and its work ending together, an orphan, a reused
  pid;
- on this host's `/proc`: a copy of `sh` named `harnessx` runs a busy child. Work CPU equals the kernel's own
  count (`RUSAGE_CHILDREN`) within 0.05 s, harness < 0.05 s, sampler < 1% of a core. A child shorter than the
  interval (4 s) is counted from its reaper's `cutime`;
- the stop script stops the sampler (the run's end, the timeout path, and a sampler the run left);
- a PATH without python3 runs the trial and records `unavailable`;
- the record from fixture turns and histories (plain, cut, a fallback's two models with a retry), Claude Code's
  stream, session log (a message repeated on three lines counts once) and trajectory;
- the report over three fixture jobs (two sampled arms, one old job), against numbers worked by hand in the test's
  comments: the front, the SVGs (parsed with `xml.etree`, one point per arm, the front filled), and the old job
  (Harbor's dollars, writes from Theseus's files and from a trajectory, "not sampled").

Planted reverts (each restored, `touch`ed, and `git status` clean):
- **Work CPU counted as the harness's** (`self.cpu["harness" if c == "work" else c]`): 4 failures. These were
  `test_each_process_lands_in_its_class`, `test_a_harness_and_its_work_that_end_together…`,
  `test_an_orphan_stays_work…`, and on this host `test_a_busy_child_is_work…`.
- **A reaped child's time dropped** (`unseen = 0`): 6 failures, among them `test_a_reaped_child_no_sample_saw_is_work`,
  `test_a_cli_reaping_its_daemon…`, and `test_a_child_shorter_than_an_interval_still_counts`.
- **Cache writes folded into input** (in `efficiency.tokens`): 5 failures, among them
  `test_a_plain_turn_keeps_all_four_classes`, `test_a_fallback_and_a_retry_bill_by_model`, and
  `test_the_result_event_is_the_bill_by_model`.
- **A non-dominated arm dropped from the front** (score alone decides dominance): `test_the_front_is_every_arm_no_other_dominates`
  and `test_three_charts_that_parse_one_point_per_arm` fail.
- **The `exe` naming fix** (comm alone): `test_a_process_started_through_proc_self_exe…` and
  `test_each_process_lands_in_its_class` fail.

Under load (each suite at `nice -n 19`, four `while :` loops at nice 0, stopped by their pids): the first run had 3
failures, all in test timing (see `590a54fc`'s message). After that commit the same load run passes, and the
report suite passes under load too. The extra time for the sampler's stop at the run's end (at most 5 s) is real
behaviour: on a starved machine it lengthens the timeout path's end, and the adapter's 20 s first wait absorbs it.

A local end-to-end run (no Docker): the real debug `theseus` and `theseusd`, run by `tb.run_script` with the
bench profile pointed at `theseus-sim fake-model` (one `proc.run` of a busy bash loop). The run exited 0. The
sampler reported `ok` (20 samples): harness 0.05 s CPU and 76 MB peak RSS (2 processes), wrapper 22 MB, work 4.5 s.
Its own cost was 0.8% of a core with about 80 processes on this host. The record read `spend_from: turn`, 2 model
calls, 1 tool call. A second run with a long job, stopped by `stop_script` after 4 s: exit 9, the sampler `ok`, no
job left, and the record intact. The report over these two trials as a job wrote all its files.

## The live check (the maintainer's)

From the root, with `bench/build.sh`'s export, `export PYTHONPATH=$PWD/bench/harbor HARBOR_TELEMETRY=0`:

1. `harbor run -d terminal-bench-sample@2.0 -a theseus_agent:Theseus -m anthropic/claude-sonnet-5-5 -o jobs --job-name eff-theseus -n 2`
   Each `jobs/eff-theseus/*/agent/efficiency.json` should have: `sampler.status` `ok`, `harness.cpu_s` > 0 and
   `harness.peak_rss_kb` > 0, `wrappers.processes` ≥ 1 on a task that ran a command, and `work.cpu_s` > `harness.cpu_s`
   on a build-heavy task. Also check `work.cpu_from` (expect `cgroup` on a cgroup v2 Docker host) and that `tokens`
   has all four classes. `python3 -c 'import json,glob;[print(f, json.load(open(f))["sampler"]) for f in glob.glob("jobs/eff-theseus/*/agent/efficiency.json")]'`
2. `harbor run -d terminal-bench-sample@2.0 -a claude_code_agent:MeasuredClaudeCode -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name eff-claude -n 2`
   You should see the same fields, harness processes named `claude`, and `spend_from: "result_event"`. If it reads
   `session_log` on trials that finished, my `modelUsage` keys are wrong: look at the `result` line in
   `agent/claude-code.txt`. The trial's agent name in `result.json` should be `claude-code`.
3. `python3 bench/report/efficiency.py --arm theseus=jobs/eff-theseus --arm claude-code=jobs/eff-claude --out /tmp/eff`
   This should write `report.md`, `pareto-{dollars,tokens,ram}.svg` (open them in a browser), and `trials.csv`.
4. The report over the first full run's jobs (old jobs, no records), e.g. `--arm theseus=jobs/<A> --arm
   theseus-batching=jobs/<B> --arm claude-code=jobs/<C>`. Solved and dollars should match docs/benchmarks.md
   (A 128/178 solved and $24.72; B 131/178 and $24.23; C 145/178 and $22.65). CPU and RAM should read "not sampled",
   and cache writes should come from each trial's own files. **Uncertain:** the published Claude Code row counts
   rate-limit retries; if those trials sit in another job directory, its count differs.

## Left, uncertain, or for the owner

- The `modelUsage` keys (above), unverified without a live run.
- `MeasuredClaudeCode` starts the sampler with `environment.exec` as the default (agent) user, as Harbor runs the
  CLI. With `hidepid` on `/proc`, other users' processes vanish from the samples (the sampler would still run).
- `memory.peak` covers the container's whole life, so it isn't per agent run. On kernels ≥ 6.12, writing to it
  resets it per fd; I left that out.
- The adapter edits touch module imports (`efficiency`, `sampler`) and a `SAMPLER` constant beside `CONFIG` in
  `theseus_agent.py`, beyond the functions; a merge with bench-async may meet them there.
- Docs: docs/benchmarks.md could gain the efficiency rows (solved per dollar, tokens per solved task, cache-hit
  share, harness CPU per tool call, peak harness RSS) under the first run's table once a sampled run exists. That's
  the maintainer's to publish.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before the first commit: fmt, shape, features,
clippy, cockpit, and the reader rule passed. The suite ran 2,562 tests: 2,528 passed, 34 failed. 33 of the
failures are the known L1 failures (theseus-pv6i: theseus-sandbox's contract tests and `spawn_100`, theseusd's
sandbox tests). The 34th was theseus-core `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`, which is not
on the known list: a pty test whose `^C` arrived before `sleep` started while my Python suites ran beside it. It
passed alone. The phases after the suite, run by hand, all passed: protocol types clean, `theseus-sim bench turn
--check --runs 5 --burst 0` ok (5/9 frames), and `cargo deny --offline check` ok (advisories, bans, licences,
sources). The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`. The final gate's result is
below.

**Final gate** (before the report commit, at `590a54fc`): fmt, shape, features, clippy, cockpit, and the reader
rule passed. The suite ran 2,562 tests: 2,529 passed, and the 33 that failed are exactly the known L1 failures
(theseus-pv6i), with no other failures. The phases after the suite, run by hand, passed: protocol types clean,
the turn bench ok (5/9 frames), and `cargo deny --offline check` ok. **Green, by the brief's rule.**
