# Terminal-Bench 2.0, HIGH tier: Codex CLI on GPT-6-Astra against the v1 arms, all 89 tasks (2026-10-25)

**The answer first.** Codex CLI 0.161.0 ran GPT-6-Astra, OpenAI's frontier model, at high effort, one trial on each
of the suite's 89 tasks. It solved **76 (85.4%** [76.6, 91.3]), ahead of every v1 arm on the same tasks:
- Theseus as shipped, 73.0%;
- Claude Code, 75.3%;
- OpenCode, 70.8%;
- Pi, 70.4%.

**Paired by task, all four gaps are now clear of zero**: +12.4 points against Theseus (95% +3.7 to +21.0; 23 tasks
better, 6 worse), +10.1 against Claude Code, +14.6 against OpenCode and +15.0 against Pi. This is the strongest
score any arm has recorded on this suite.

**It costs thirteen times as much.** Codex paid **$1.31 a trial and $1.54 a solved task**, against Theseus's $0.100
and $0.136. The run's model spend was **$116.68**. Most of the gap is the model: GPT-6-Astra is listed at $10 in and
$50 out per million tokens, five times Claude Sonnet 5.5. Codex also made about twice the model calls a trial (15.5
against 8.6), and 18 of its 89 trials passed the $2 cap the rivals carried — Codex has no cap.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/2026-10-25-terminal-bench-codex-high/pass-rates-codex-dark.svg">
  <img alt="Pass rate by arm on all 89 tasks, HIGH tier, with 95% Wilson intervals. Where does Codex CLI on GPT-6-Astra land against the v1 arms?" src="img/2026-10-25-terminal-bench-codex-high/pass-rates-codex.svg" width="720">
</picture>

*Figure 1. Where does Codex CLI on GPT-6-Astra land against the v1 arms? Ahead of all of them, though on one trial a
task its interval is twice as wide as theirs and its lower bound (76.6%) still touches Claude Code's point estimate.
OpenCode (70.8%) is in the table.*

| | |
|---|---|
| Suite | Terminal-Bench 2.0 (`terminal-bench@2.0`, the same 89 task directories as v1) through Harbor 0.23 |
| Arm | **Codex CLI 0.161.0** (`codex_agent:MeasuredCodex`), model `openai/gpt-6-astra`, `reasoning_effort` high; caps of $2 and 200 turns recorded, not enforced |
| Compared with | The v1 HIGH arms' trials of 2026-10-18 (`351159fc`): Theseus as shipped, Claude Code 2.1.290, OpenCode 1.18.35, Pi 1.0.4, three trials a task each, not rerun |
| Trials | 89 tasks × 1, the whole suite |
| Date and commit | 2026-10-25, in two sittings (02:00–03:33 and 09:53–11:38, UTC−7) on the owner's WSL machine; the adapters at `b69236c1` and `0fda4d57`, which are byte-identical under `bench/harbor` |
| Cost | $116.68 at list prices (the trials' own records); $108.84 repricing uncached input at the input price |
| Data | [`2026-10-25-terminal-bench-codex-high.json`](2026-10-25-terminal-bench-codex-high.json), [`.csv`](2026-10-25-terminal-bench-codex-high.csv) (one row per trial) |

## The question

The v1 run compared Theseus with the top three harnesses of the broad pass, each on Claude Sonnet 5.5. Codex CLI was
not among them: it speaks only OpenAI's Responses API, so it cannot run Sonnet. This run adds Codex on the best model
its key can use, at the same HIGH tier. The question is where the leading OpenAI harness, on OpenAI's frontier model,
lands against the v1 arms, and at what cost.

Its default model, GPT-6.1-Sol, is not open to the key's project (a 404), so GPT-6-Astra is the best model this key
can reach. The owner chose one trial a task for this run. Per the rule for rivals, the other harnesses are not rerun:
their v1 trials stand.

## The setup

- **The arm.** Harbor's Codex adapter as the other runs measure it (`bench/harbor/codex_agent.py`): Codex pinned at
  0.161.0, `--model gpt-6-astra`, `model_reasoning_effort=high`. Every trial that installed the agent records both.
  The record comes from Harbor's trajectory of the rollout (`efficiency.codex_record`), priced from the bench's
  list-price table.
- **The key.** `OPENAI_API_KEY` was read at run time into the driver's memory and placed only in each Harbor run's
  environment.
- **The host.** One WSL2 machine (16 vCPU, 23 GB, 16 GB swap), up to 4 trials at once. Each task's image was pulled
  for its trial and removed after it.
- **The order, and the two sittings.** The run was split by a spend line, not by task type. The first sitting took
  the heavy tasks — the 17 with a timeout or a trial over 20 minutes in the v1 HIGH run, then the 14 next-longest by
  v1 mean agent time — and stopped on its $75 line after 31 of them. The second took the remaining 58 in the same
  longest-first order, under a $90 line that never bound: it finished them all for $41.50. Between the two, every
  task in the suite ran exactly once.
- **The stops.** A start was held when finished spend plus an estimate for each running trial would pass the sitting's
  line. The first sitting also held starts after 07:40 and stopped trials at 07:55; the second ran without a clock.

## Results

All 89 tasks, one Codex trial each against the v1 arms' three:

| Arm | Trials passed (95% Wilson) | $ a trial (95%) | $ a solve | Agent s (median) | Agent min (total) | Model calls | Timeout / grader timeout / setup timeout / refusal / error |
|---|---|---|---|---|---|---|---|
| Codex CLI 0.161.0 (GPT-6-Astra, high) | 76/89 = **85.4%** [76.6, 91.3] | $1.311 [1.040, 1.622] | $1.535 | **74** | 249 | 15.5 | 0 / 0 / 0 / 2 / 2 |
| Theseus as shipped (v1) | 195/267 = 73.0% [67.4, 78.0] | **$0.100** [0.082, 0.119] | **$0.136** | **50** | 945 | 8.6 | 6 / 6 / 2 / 0 / 7 |
| Claude Code 2.1.290 | 201/267 = 75.3% [69.8, 80.1] | $0.130 [0.111, 0.152] | $0.173 | 56 | 1017 | 8.2 | 6 / 6 / 0 / 0 / 11 |
| OpenCode 1.18.35 | 189/267 = 70.8% [65.1, 75.9] | $0.119 [0.099, 0.143] | $0.168 | 60 | 1099 | 7.7 | 6 / 6 / 1 / 9 / 17 |
| Pi 1.0.4 | 188/267 = 70.4% [64.7, 75.6] | $0.106 [0.086, 0.129] | $0.150 | 55 | 999 | 7.7 | 7 / 4 / 0 / 0 / 11 |

| Codex − arm, by task | Mean gap (95%, bootstrap over tasks) | Tasks better / worse | Sign test |
|---|---|---|---|
| Theseus as shipped (v1) | **+12.4 points** [+3.7, +21.0] | 23 / 6 | p = 0.002 |
| Claude Code | **+10.1 points** [+1.9, +18.7] | 25 / 6 | p = 0.0009 |
| OpenCode | **+14.6 points** [+6.4, +23.2] | 21 / 3 | p = 0.0003 |
| Pi | **+15.0 points** [+6.7, +23.2] | 25 / 4 | p = 0.0001 |

The gap is Codex's 0 or 1 on a task minus the arm's mean over its three trials there. All four intervals exclude
zero, and all four sign tests reject at the 1% level.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/2026-10-25-terminal-bench-codex-high/pareto-dollars-codex-dark.svg">
  <img alt="Pass rate against dollars per trial, on all 89 tasks. What does Codex's pass rate cost against the v1 arms'?" src="img/2026-10-25-terminal-bench-codex-high/pareto-dollars-codex.svg" width="720">
</picture>

*Figure 2. What does Codex's pass rate cost against the v1 arms'? The four v1 arms sit together between $0.10 and
$0.13 a trial. Codex's 10 to 15 points more cost about $1.20 a trial more.*

## Analysis

- **The heavy subset understated Codex by 8 points.** On the 31 heavy tasks of the first sitting it solved 77.4%; on
  the 58 lighter ones it solved 52, or **89.7%**. Both halves beat the v1 arms' rates on the same tasks, but the
  suite-wide figure (85.4%) is the one to quote. This is exactly the direction the first sitting's report said it
  could not predict.
- **Where Codex wins against Theseus** (23 tasks). The nine Theseus lost twice or three times out of three are the
  ones worth reading: `adaptive-rejection-sampler`, `dna-insert`, `extract-moves-from-video`, `kv-store-grpc`,
  `mteb-leaderboard`, `mteb-retrieve`, `polyglot-c-py`, `polyglot-rust-c` and `torch-pipeline-parallelism` (0 of 3
  each), plus `build-pmars`, `path-tracing`, `pytorch-model-recovery`, `raman-fitting`, `train-fasttext` and
  `video-processing` (1 of 3 each). The other eight were 2 of 3 for Theseus, so they are nearer to noise.
- **Where it loses** (6 tasks against Theseus):
  - `break-filter-js-from-html` and `vulnerable-secret`: OpenAI's safeguard refused the first turn of each. Theseus
    and Claude Code solved both 3 of 3. **These two are the whole of Codex's refusal column**, and they cost it 2.2
    points of pass rate for nothing the harness did.
  - `extract-elf`: 0 against 3 of 3, and Codex gave up after 141 s for $1.26.
  - `sanitize-git-repo`: 0 against 2 of 3, at $2.65.
  - `torch-tensor-parallelism`: 0 against 2 of 3.
  - `install-windows-3.11`: 0 against 1 of 3, its second most expensive trial at $6.18.
- **Two tasks are broken for every arm, not just this one.** `qemu-alpine-ssh` and `qemu-startup` ended in
  `NonZeroAgentExitCodeError` before the model was ever called: Harbor installs Node into each container, and on
  these two Debian bullseye images `apt-get install nodejs npm` now fails with 404s from the Debian security pool
  (the index resolves, the `.deb` files have been pruned upstream). **All 12 v1 trials of these two tasks, across all
  four arms, failed with the same exception on EC2 a week earlier**, so this is a property of the task images and it
  costs every arm the same two tasks. Codex's rate on the 87 tasks that ran an agent is **87.4%**; excluding the two
  refusals as well, **89.4%** of 85.
- **Six tasks nobody solved**, in this run or in v1: `caffe-cifar-10`, `filter-js-from-html`, `make-doom-for-mips`,
  `sam-cell-seg` and the two qemu tasks. **Thirty-six tasks everybody solved.** Codex ended all four of the solvable
  unsolved ones on its own, without a timeout.
- **No timeouts at all.** Codex recorded zero agent timeouts, zero grader timeouts and zero setup timeouts across 89
  trials, against 12 to 14 such endings for each v1 arm. Some of that is the quieter host (see the threats), but the
  first sitting already showed `torch-pipeline-parallelism`'s grader finishing where it had timed out on EC2 for
  every arm on both Oct 13 and Oct 18 — so that one, at least, is not inherent.
- **Faster in the loop, slower to start.** Codex's median trial spent 74 s in the agent, against 50 to 60 s for the
  v1 arms, but its total agent time over the suite was 249 minutes against their 945 to 1099 — because it needs
  about a quarter as many trials. Against that, Harbor installs Node and Codex in every container: a median **269 s
  of setup**, more than three times the median agent time, and not counted as agent time by any arm.
- **The cost is mostly the model.** Of the $116.68:
  - output tokens (at $50 a million) were $32.32;
  - cached input (at $1) was $45.27;
  - uncached input, charged at the cache-write price of $12.50, was $39.06.

  At Sonnet 5.5's prices the same tokens would cost about a fifth as much, roughly $0.26 a trial. That is still twice
  the v1 arms', in line with Codex making twice the calls.

## What changed since Codex last ran

- **Against the first sitting's own report (the 31 heaviest tasks, published earlier today).** The subset is gone and
  the picture changed with it: 85.4% rather than 77.4%, $1.31 a trial rather than $2.43, and — the substantive change
  — the paired gaps against Theseus and Claude Code moved from *crossing zero* (+12.9 [−2.2, +28.0] and +9.7 [−5.4,
  +24.7]) to *clear of zero* (+12.4 [+3.7, +21.0] and +10.1 [+1.9, +18.7]). The point estimates barely moved; it was
  the 58 extra paired tasks that narrowed the intervals.
- **Against Oct 7.** Then Codex ran GPT-5.6-Sol at medium effort on the 14-task night set (28 trials): 82.1% at $0.45
  a trial. Now it runs GPT-6-Astra at high effort on the whole suite at $1.31 a trial. The two runs share no task
  set, so they are not comparable as a pair.
- **The harness** is the same pin, 0.161.0.
- **The comparison.** The v1 arms ran at high effort on Sonnet 5.5 (and Jev's routing for Theseus). This is the first
  time Codex sits beside them on the full suite at the same tier.

## Threats to validity

- **One trial a task.** Each task is a single draw for Codex, against three for the v1 arms. Its interval is about
  ±7 points, roughly twice the v1 arms', and one task moves its rate by 1.1 points. The paired gaps are the sounder
  comparison, and they are the numbers that carry the claim.
- **Model and harness are confounded.** Codex runs an OpenAI model, while the rivals ran Claude Sonnet 5.5. This run
  cannot say how much of the gap is Codex and how much is GPT-6-Astra.
- **Two sittings, nine hours apart.** The 31 heavy tasks ran 02:00–03:33 and the 58 others 09:53–11:38, on the same
  machine with the same adapters and the same images. Because the split follows the spend line and the order is the
  v1 run times, the heavy and light halves are not interchangeable — but together they are the whole suite, so no
  task was selected in or out by a result.
- **A different host and day from the v1 arms.**
  - The v1 arms ran on EC2 m7a.8xlarge hosts at 12 jobs a host on Oct 18. Codex ran on the owner's WSL machine at 4
    trials at once, seven days later. The task images were the same.
  - The host was busy beside the run, from the gateway and agent sessions: at trial starts, load was 7.5 to 23.6 on
    16 vCPU in the first sitting (mean 12.4) and 5.7 to 17.0 in the second (mean 9.4).
  - No Rust build or gate ran during either sitting — not one of the 89 trials started beside a compiling tree, and
    the second sitting was explicitly kept clear of x86 builds. Memory pressure stayed low (PSI full avg10 ≤ 5.9 in
    the first sitting, ≤ 1.8 in the second), swap held between 4.0 and 4.3 GB, and the Windows system drive stayed
    above 70 GB free throughout (89 GB at the second sitting's start, 107 GB at its end, as the task images came
    off).
  - The v1 run's heavier host load is a threat to its own timeouts rather than to this run's: that is what
    `torch-pipeline-parallelism` shows, and it is why Codex's zero timeouts are not straightforwardly a harness win.
- **No cap.** Codex has no spend or turn cap; the $2 and 200-turn caps are recorded only. Claude Code enforced its
  $2 cap in v1. 18 of Codex's 89 trials passed $2 and three passed $5 (`make-doom-for-mips` $6.54,
  `install-windows-3.11` $6.18, `filter-js-from-html` $5.01) — all three of those were losses.
- **Cost counts.**
  - The bench's price table charges GPT-6-Astra's uncached input as a cache write ($12.50 a million) rather than as
    input ($10). That overstates the total by $7.81: it is **$108.84** at the input price, or $1.22 a trial.
  - The four trials that recorded no tokens (two refusals, two agent-install failures) are counted as trials in
    `$ a trial`, which understates it very slightly.
- **The driver.** Both sittings used a local driver (`cx_driver.py`, and `cx_driver2.py` for the 58), not v1's
  `tb_driver.py` on EC2. All three call `harbor run` with the same adapter options. These two point Harbor at the
  task directory (`-p`) rather than the registry name; the task files are the same.

## Cost

Model spend: **$116.68** at list prices — $75.18 for the 31 heavy tasks and $41.50 for the other 58 — or $108.84
repricing uncached input at the input price. $1.31 a trial, $1.54 a solved task. The host was the owner's own
machine: no EC2. Total wall clock across the two sittings was 3 h 18 m, for 12.3 hours of trial time at 4 at once.

## Reproduction

```
# the drivers, their order and their guards (spend line, disk guard, memory pressure): outside the repo
cx_driver.py  --dir run    # the 31 heavy tasks, $75 line, 02:00-03:33
cx_driver2.py --dir run2   # the other 58, $90 line, no clock window
                           # both: harbor run -a codex_agent:MeasuredCodex -m openai/gpt-6-astra
                           #   --ak max_budget_usd=2.0 --ak max_turns=200 --ak reasoning_effort=high -k 1 -n 1
cx_report2.py run,run2 docs/benchmarks/2026-10-18-terminal-bench-v1-high.csv out/cx89   # tables, paired gaps
python3 bench/report/draft.py plot docs/benchmarks/2026-10-25-terminal-bench-codex-high.json
```
