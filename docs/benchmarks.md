# Benchmarks

Theseus's results on public benchmarks are published here first: every run's configuration, model, attempts, score,
and cost. How to run one, and what each trial leaves behind, is in [`bench/README.md`](../bench/README.md).

## Terminal-Bench 2.0

89 tasks in their own containers, through [Harbor](https://github.com/laude-institute/harbor) 0.23, with the bench
profile (`bench/theseus-bench.toml`): no vault, every tool open, L0. _(Since 2026-10-04, after the first run below, four of its losses' fixes are in (theseus-7gir.18 to .21; the spec's Part III, Items 150 and 154): the profile asks for its model's whole output (the catalog's 128,000 for Sonnet 5.5, not 32,000), opens private addresses (`[policy] private_addresses = "open"`), and retries a transient provider failure inside its turn (`[model.retries] transient = 4`), and a request Sonnet 5.5 refuses is made once more on Sonnet 5. The first run's numbers predate all four; the held-out rerun, at spend parity with Claude Code, measures them (theseus-7gir.22).)_

| Date | Theseus build | Agent | Model | Tasks × attempts | Mean reward | Cost |
|---|---|---|---|---|---|---|
| 2026-10-04 | 079f1db | Theseus (bench profile) | Claude Sonnet 5.5 | 89 × 2 | 0.719 | $24.72 |
| 2026-10-04 | 079f1db | Theseus + batching paragraph | Claude Sonnet 5.5 | 89 × 2 | 0.736 | $24.23 |
| 2026-10-04 | – | Claude Code 2.1.288 | Claude Sonnet 5.5 | 89 × 2 | 0.815 | $22.65 |

### The first full run (2026-10-04)

- **Tasks:** all 89 of `terminal-bench@2.0`, 2 attempts per task and configuration: 534 trials.
- **Model:** Claude Sonnet 5.5 (`anthropic/claude-sonnet-5-5`) for every configuration.
- **A. Theseus:** `-a theseus_agent:Theseus` with the bench profile as committed, built at 079f1db (static musl,
  `bench/build.sh`).
- **B. Theseus with one paragraph of extra system text** (`THESEUS_BENCH_SYSTEM_FILE`), the batching arm:

  > You are running non-interactively in a task's container: nobody will answer questions, so work on your own until the task is done and verified. Shell work is cheap here: use proc.run with ["bash", "-lc", "..."] freely, and put related steps (inspect, change, check) into one script per call rather than one command per call, since every call costs a round trip.

- **C. Claude Code 2.1.288**, Harbor's own adapter (`-a claude-code`), with its limits set to Theseus's:
  `max_budget_usd=2.0` and `max_turns=200`.
- **Limits per trial:** $2.00 (`THESEUS_BENCH_SPEND_LIMIT`; Claude Code's `--max-budget-usd`), 200 model calls
  (`THESEUS_BENCH_MAX_LOOPS`; Claude Code's `--max-turns`), and each task's own agent timeout. The agent setup
  timeout was tripled for all three, for Claude Code's install of Node and its CLI.
- **Concurrency:** at most 4 trials at once. Trials Harbor recorded as rate-limited (`ApiRateLimitError`) were run
  once more, and the retry counts: 0.
- **Cost:** $71.60 in all, every trial included.

| | A. Theseus plain | B. Theseus + batching paragraph | C. Claude Code |
|---|---|---|---|
| Solved, attempt 1 | 62/89 | 65/89 | 73/89 |
| Solved, attempt 2 | 66/89 | 66/89 | 72/89 |
| Mean pass rate | 71.9% | 73.6% | 81.5% |
| Solved in all 2 / in either | 59 / 69 | 60 / 71 | 68 / 77 |
| Cost, total / per task | $24.72 / $0.278 | $24.23 / $0.272 | $22.65 / $0.255 |
| Model calls per trial | 8.4 | 8.3 | 8.0 |
| Agent time per trial, mean | 4.1 min | 4.4 min | 4.1 min |
| Input read from the cache | 87.5% | 88.2% | 92.9% |
| Trials ending in: timeout (the task's agent limit) | 6 | 5 | 8 |
| Trials ending in: turn cut (loop/turn cap, output or context limit) | 4 | 2 | 0 |
| Trials ending in: spend limit ($2.00 a trial) | 2 | 1 | 0 |
| Trials ending in: error (exit or harness) | 2 | 2 | 4 |
| Trials ending in: refusal | 6 | 6 | 0 |
| Trials ending in: waits for approval | 1 | 0 | 0 |

A task is solved when its tests give reward 1. Mean reward is the mean over every trial, and equals the mean pass
rate.

<details>
<summary>Per task: reward · dollars · model calls for each attempt (<code>!</code> marks a trial that ended in an
error class, <code>r</code> a rate-limit retry)</summary>

| Task | A1 | A2 | B1 | B2 | C1 | C2 |
|---|---|---|---|---|---|---|
| adaptive-rejection-sampler | 0 · 0.27 · 7 | 0 · 0.31 · 7 | 1 · 0.23 · 7 | 0 · 0.27 · 6 | 0 · 0.15 · 5 | 0 · 0.16 · 5 |
| bn-fit-modify | 1 · 0.03 · 5 | 1 · 0.03 · 5 | 1 · 0.05 · 7 | 1 · 0.04 · 5 | 1 · 0.04 · 5 | 1 · 0.05 · 6 |
| break-filter-js-from-html | 0! · 0.01 · 1 | 0! · 0.01 · 1 | 0! · 0.01 · 1 | 0! · 0.01 · 1 | 1 · 0.35 · 14 | 1 · 0.27 · 17 |
| build-cython-ext | 1 · 0.17 · 15 | 1 · 0.16 · 15 | 1 · 0.18 · 17 | 1 · 0.18 · 17 | 1 · 0.13 · 12 | 1 · 0.13 · 11 |
| build-pmars | 1 · 0.05 · 10 | 1 · 0.04 · 8 | 1 · 0.04 · 8 | 0 · 0.04 · 8 | 1 · 0.05 · 7 | 1 · 0.03 · 6 |
| build-pov-ray | 1 · 0.13 · 15 | 1 · 0.19 · 21 | 1 · 0.17 · 15 | 1 · 0.12 · 13 | 1 · 0.07 · 9 | 1 · 0.11 · 14 |
| caffe-cifar-10 | 0! · 0.10 · 10 | –! · ? · ? | –! · ? · ? | –! · ? · ? | 0! · 0.11 · 11 | –! · 0.05 · 6 |
| cancel-async-tasks | 1 · 0.04 · 4 | 1 · 0.03 · 3 | 1 · 0.03 · 4 | 1 · 0.03 · 3 | 1 · 0.05 · 4 | 1 · 0.03 · 3 |
| chess-best-move | 1 · 0.03 · 3 | 1 · 0.03 · 3 | 0 · 0.02 · 3 | 1 · 0.04 · 8 | 0 · 0.03 · 3 | 1 · 0.03 · 3 |
| circuit-fibsqrt | 1 · 0.18 · 6 | 1 · 0.22 · 9 | 1 · 0.18 · 5 | 1 · 0.14 · 6 | 1 · 0.15 · 9 | 1 · 0.10 · 3 |
| cobol-modernization | 1 · 0.18 · 14 | 1 · 0.12 · 11 | 1 · 0.12 · 7 | 1 · 0.14 · 10 | 1 · 0.04 · 3 | 1 · 0.07 · 5 |
| code-from-image | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 4 |
| compile-compcert | 0! · 0.10 · 11 | 1 · 0.11 · 15 | 1 · 0.12 · 13 | 1 · 0.12 · 12 | 1 · 0.13 · 14 | 0! · 0.20 · 19 |
| configure-git-webserver | 1 · 0.03 · 5 | 0 · 0.04 · 7 | 0 · 0.04 · 5 | 0 · 0.04 · 5 | 1 · 0.04 · 4 | 1 · 0.03 · 4 |
| constraints-scheduling | 1 · 0.04 · 3 | 1 · 0.05 · 3 | 1 · 0.03 · 3 | 1 · 0.03 · 3 | 1 · 0.04 · 3 | 1 · 0.03 · 3 |
| count-dataset-tokens | 1 · 0.05 · 6 | 1 · 0.05 · 5 | 1 · 0.04 · 6 | 1 · 0.04 · 5 | 1 · 0.04 · 6 | 1 · 0.05 · 6 |
| crack-7z-hash | 0! · 0.00 · 1 | 0! · 0.00 · 1 | 0! · 0.02 · 1 | 0! · 0.02 · 1 | 1 · 0.10 · 13 | 1 · 0.13 · 15 |
| custom-memory-heap-crash | 1 · 0.15 · 14 | 1 · 0.13 · 12 | 1 · 0.14 · 12 | 1 · 0.09 · 7 | 0 · 0.09 · 9 | 1 · 0.07 · 8 |
| db-wal-recovery | 1 · 0.04 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.02 · 4 |
| distribution-search | 1 · 0.02 · 2 | 1 · 0.03 · 2 | 1 · 0.04 · 3 | 1 · 0.05 · 3 | 1 · 0.04 · 3 | 1 · 0.02 · 2 |
| dna-assembly | 1 · 0.21 · 12 | 1 · 0.19 · 10 | 1 · 0.18 · 8 | 1 · 0.18 · 11 | 1 · 0.15 · 8 | 1 · 0.12 · 8 |
| dna-insert | 0 · 0.08 · 8 | 0 · 0.13 · 10 | 0 · 0.10 · 7 | 0 · 0.13 · 7 | 0 · 0.08 · 7 | 0 · 0.08 · 7 |
| extract-elf | 1 · 0.04 · 4 | 1 · 0.03 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 3 | 1 · 0.03 · 3 |
| extract-moves-from-video | 0! · 0.18 · 14 | 0! · 0.18 · 16 | 0! · 0.09 · 13 | 0! · 0.39 · 28 | 1 · 1.12 · 35 | 0! · 0.44 · 36 |
| feal-differential-cryptanalysis | 1 · 0.07 · 3 | 1 · 0.07 · 3 | 1 · 0.06 · 3 | 1 · 0.07 · 3 | 1 · 0.08 · 5 | 1 · 0.08 · 4 |
| feal-linear-cryptanalysis | 1 · 0.33 · 10 | 1 · 0.10 · 6 | 1 · 0.11 · 4 | 1 · 0.17 · 6 | 0! · 0.20 · 7 | 1 · 0.12 · 3 |
| filter-js-from-html | 0 · 0.10 · 6 | 0 · 0.10 · 5 | 0 · 0.16 · 5 | 0 · 0.10 · 4 | 0 · 0.04 · 2 | 0 · 0.05 · 2 |
| financial-document-processor | 1 · 0.28 · 15 | 1 · 0.30 · 15 | 1 · 0.34 · 10 | 1 · 0.41 · 11 | 1 · 0.20 · 6 | 1 · 0.24 · 9 |
| fix-code-vulnerability | 1 · 0.03 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 5 | 1 · 0.04 · 5 | 1 · 0.03 · 3 | 1 · 0.02 · 3 |
| fix-git | 1 · 0.04 · 7 | 1 · 0.05 · 8 | 1 · 0.04 · 6 | 1 · 0.04 · 6 | 1 · 0.03 · 6 | 1 · 0.04 · 5 |
| fix-ocaml-gc | 1 · 0.36 · 13 | 1 · 0.26 · 11 | 1 · 0.23 · 10 | 1 · 0.21 · 12 | 1 · 0.16 · 9 | 1 · 0.30 · 21 |
| gcode-to-text | 1 · 0.16 · 14 | 1 · 0.10 · 10 | 1 · 0.09 · 9 | 1 · 0.13 · 13 | 1 · 0.06 · 7 | 1 · 0.16 · 13 |
| git-leak-recovery | 1 · 0.02 · 4 | 1 · 0.02 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.02 · 4 | 1 · 0.03 · 4 |
| git-multibranch | 1 · 0.07 · 6 | 1 · 0.08 · 7 | 1 · 0.04 · 5 | 1 · 0.04 · 5 | 1 · 0.04 · 4 | 1 · 0.04 · 4 |
| gpt2-codegolf | 1 · 0.46 · 15 | 1 · 0.30 · 5 | 1 · 0.30 · 11 | 1 · 0.30 · 7 | 1 · 0.35 · 12 | 1 · 0.34 · 14 |
| headless-terminal | 1 · 0.06 · 4 | 1 · 0.07 · 5 | 1 · 0.05 · 6 | 1 · 0.06 · 7 | 1 · 0.05 · 4 | 1 · 0.04 · 4 |
| hf-model-inference | 0! · ? · 0 | 1 · 0.05 · 5 | 1 · 0.04 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 4 | 1 · 0.03 · 4 |
| install-windows-3.11 | 0 · 0.52 · 45 | 0! · 0.05 · 7 | 0 · 0.80 · 61 | 1 · 0.78 · 54 | 1 · 0.51 · 37 | 0 · 1.12 · 68 |
| kv-store-grpc | 1 · 0.04 · 3 | 1 · 0.04 · 3 | 1 · 0.04 · 3 | 1 · 0.04 · 3 | 1 · 0.03 · 3 | 1 · 0.03 · 3 |
| large-scale-text-editing | 1 · 0.04 · 5 | 1 · 0.04 · 5 | 0 · 0.03 · 6 | 1 · 0.03 · 4 | 1 · 0.02 · 3 | 1 · 0.03 · 3 |
| largest-eigenval | 1 · 0.12 · 11 | 1 · 0.10 · 13 | 1 · 0.19 · 7 | 1 · 0.21 · 12 | 1 · 0.21 · 12 | 1 · 0.14 · 8 |
| llm-inference-batching-scheduler | 1 · 0.12 · 6 | 0! · 0.22 · 7 | 1 · 0.13 · 9 | 1 · 0.23 · 12 | 1 · 0.09 · 8 | 1 · 0.14 · 10 |
| log-summary-date-ranges | 1 · 0.05 · 5 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.04 · 3 | 1 · 0.03 · 4 |
| mailman | 1 · 0.19 · 16 | 1 · 0.16 · 14 | 1 · 0.23 · 16 | 1 · 0.30 · 20 | 1 · 0.26 · 19 | 1 · 0.12 · 10 |
| make-doom-for-mips | 0! · 1.44 · 36 | 0! · 1.46 · 32 | 0! · 1.24 · 37 | 0! · 1.45 · 35 | 0 · 0.71 · 30 | 0 · 0.79 · 32 |
| make-mips-interpreter | 0 · 0.49 · 13 | 1 · 0.49 · 13 | 1 · 0.41 · 14 | 1 · 0.39 · 11 | 1 · 0.22 · 10 | 1 · 0.27 · 12 |
| mcmc-sampling-stan | 1 · 0.10 · 9 | 1 · 0.06 · 10 | 0 · 0.07 · 8 | 0 · 0.10 · 11 | 1 · 0.12 · 9 | 1 · 0.07 · 8 |
| merge-diff-arc-agi-task | 1 · 0.07 · 9 | 1 · 0.06 · 9 | 1 · 0.05 · 7 | 1 · 0.05 · 7 | 1 · 0.04 · 5 | 1 · 0.06 · 6 |
| model-extraction-relu-logits | 1 · 0.12 · 9 | 1 · 0.05 · 3 | 1 · 0.09 · 6 | 1 · 0.05 · 3 | 1 · 0.06 · 5 | 1 · 0.07 · 4 |
| modernize-scientific-stack | 1 · 0.03 · 4 | 1 · 0.03 · 3 | 1 · 0.04 · 3 | 1 · 0.04 · 3 | 1 · 0.03 · 3 | 1 · 0.04 · 3 |
| mteb-leaderboard | 1 · 0.28 · 20 | 0 · 0.36 · 20 | 1 · 0.14 · 15 | 1 · 0.33 · 27 | 0! · 0.19 · 18 | 0 · 0.04 · 4 |
| mteb-retrieve | 0 · 0.03 · 4 | 0 · 0.03 · 4 | 0 · 0.02 · 4 | 0 · 0.03 · 4 | 0 · 0.02 · 3 | 0 · 0.03 · 4 |
| multi-source-data-merger | 1 · 0.05 · 4 | 1 · 0.05 · 4 | 1 · 0.03 · 3 | 1 · 0.03 · 3 | 1 · 0.03 · 3 | 1 · 0.04 · 3 |
| nginx-request-logging | 1 · 0.03 · 4 | 1 · 0.03 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 4 | 1 · 0.02 · 2 | 1 · 0.03 · 2 |
| openssl-selfsigned-cert | 1 · 0.03 · 4 | 1 · 0.04 · 5 | 1 · 0.02 · 3 | 1 · 0.02 · 2 | 1 · 0.03 · 3 | 1 · 0.03 · 2 |
| overfull-hbox | 1 · 0.17 · 12 | 1 · 0.08 · 8 | 1 · 0.12 · 9 | 1 · 0.23 · 18 | 1 · 0.10 · 10 | 1 · 0.15 · 13 |
| password-recovery | 1 · 0.07 · 11 | 1 · 0.05 · 9 | 1 · 0.05 · 6 | 1 · 0.07 · 9 | 1 · 0.06 · 9 | 1 · 0.07 · 8 |
| path-tracing | 1 · 0.29 · 15 | 1 · 0.50 · 24 | 1 · 0.23 · 15 | 1 · 0.26 · 11 | 1 · 0.59 · 33 | 1 · 0.54 · 24 |
| path-tracing-reverse | 1 · 0.40 · 10 | 1 · 0.42 · 8 | 1 · 0.42 · 10 | 1 · 0.38 · 9 | 1 · 0.31 · 6 | 1 · 0.45 · 11 |
| polyglot-c-py | 0 · 0.06 · 4 | 0 · 0.10 · 7 | 0 · 0.04 · 3 | 0 · 0.04 · 3 | 0 · 0.05 · 4 | 0 · 0.03 · 2 |
| polyglot-rust-c | 0 · 0.04 · 2 | 0 · 0.04 · 2 | 0 · 0.04 · 2 | 0 · 0.04 · 2 | 0 · 0.08 · 5 | 0 · 0.06 · 3 |
| portfolio-optimization | 1 · 0.14 · 8 | 1 · 0.15 · 8 | 1 · 0.09 · 3 | 1 · 0.08 · 4 | 1 · 0.07 · 3 | 1 · 0.07 · 4 |
| protein-assembly | 0 · 0.21 · 16 | 1 · 0.25 · 15 | 1 · 0.22 · 14 | 0 · 0.31 · 19 | 1 · 0.15 · 9 | 1 · 0.19 · 12 |
| prove-plus-comm | 1 · 0.03 · 5 | 1 · 0.04 · 6 | 1 · 0.03 · 4 | 1 · 0.03 · 3 | 1 · 0.02 · 3 | 1 · 0.03 · 3 |
| pypi-server | 1 · 0.05 · 5 | 1 · 0.05 · 5 | 1 · 0.04 · 4 | 1 · 0.03 · 3 | 1 · 0.02 · 3 | 1 · 0.03 · 3 |
| pytorch-model-cli | 0 · 0.08 · 10 | 0 · 0.11 · 12 | 1 · 0.05 · 8 | 1 · 0.06 · 9 | 1 · 0.06 · 5 | 1 · 0.05 · 5 |
| pytorch-model-recovery | 1 · 0.04 · 4 | 1 · 0.05 · 4 | 1 · 0.05 · 4 | 1 · 0.05 · 3 | 1 · 0.04 · 4 | 1 · 0.05 · 4 |
| qemu-alpine-ssh | 0 · 0.12 · 14 | 0! · 0.06 · 7 | 0 · 0.08 · 12 | 0 · 0.04 · 7 | –! · ? · ? | –! · ? · ? |
| qemu-startup | 0 · 0.21 · 16 | 0 · 0.09 · 13 | 0 · 0.14 · 21 | 0 · 0.11 · 14 | –! · ? · ? | –! · ? · ? |
| query-optimize | 0 · 0.06 · 4 | 1 · 0.07 · 5 | 0 · 0.05 · 6 | 1 · 0.04 · 4 | 0 · 0.07 · 7 | 1 · 0.06 · 7 |
| raman-fitting | 0 · 0.17 · 16 | 1 · 0.11 · 11 | 0 · 0.11 · 10 | 0 · 0.19 · 16 | 1 · 0.10 · 9 | 1 · 0.07 · 8 |
| regex-chess | 0! · 0.35 · 3 | 0! · 0.35 · 3 | 0! · 0.33 · 2 | 1 · 0.60 · 8 | 1 · 0.55 · 10 | 1 · 0.52 · 7 |
| regex-log | 1 · 0.06 · 8 | 1 · 0.05 · 6 | 1 · 0.05 · 3 | 1 · 0.05 · 3 | 1 · 0.03 · 2 | 1 · 0.03 · 2 |
| reshard-c4-data | 1 · 0.20 · 7 | 1 · 0.19 · 6 | 1 · 0.09 · 3 | 1 · 0.11 · 5 | 1 · 0.06 · 3 | 1 · 0.07 · 3 |
| rstan-to-pystan | 1 · 0.14 · 10 | 1 · 0.12 · 9 | 0! · 0.13 · 10 | 1 · 0.15 · 10 | 1! · 0.20 · 9 | 0! · 0.08 · 6 |
| sam-cell-seg | 0 · 0.19 · 13 | 0 · 0.16 · 9 | 0 · 0.15 · 11 | 0 · 0.12 · 8 | 0 · 0.06 · 3 | 0 · 0.05 · 3 |
| sanitize-git-repo | 1 · 0.16 · 9 | 1 · 0.16 · 10 | 1 · 0.21 · 13 | 1 · 0.15 · 11 | 1 · 0.05 · 5 | 1 · 0.05 · 5 |
| schemelike-metacircular-eval | 0! · 0.49 · 4 | 0! · 0.40 · 3 | 1 · 0.52 · 7 | 0! · 0.36 · 3 | 1 · 0.40 · 5 | 1 · 0.46 · 20 |
| sparql-university | 1 · 0.09 · 8 | 1 · 0.10 · 10 | 1 · 0.09 · 6 | 1 · 0.09 · 6 | 1 · 0.06 · 5 | 1 · 0.08 · 5 |
| sqlite-db-truncate | 1 · 0.03 · 4 | 1 · 0.05 · 5 | 1 · 0.07 · 4 | 1 · 0.06 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 5 |
| sqlite-with-gcov | 1 · 0.05 · 8 | 1 · 0.05 · 8 | 1 · 0.03 · 8 | 1 · 0.03 · 7 | 1 · 0.02 · 3 | 1 · 0.03 · 3 |
| torch-pipeline-parallelism | 1 · 0.06 · 4 | 1 · 0.06 · 5 | 1 · 0.07 · 4 | 1 · 0.07 · 4 | 1 · 0.05 · 4 | 1 · 0.06 · 4 |
| torch-tensor-parallelism | 1 · 0.08 · 7 | 1 · 0.08 · 7 | 1 · 0.05 · 4 | 1 · 0.04 · 4 | 1 · 0.04 · 2 | 1 · 0.05 · 2 |
| train-fasttext | 0 · 0.11 · 11 | 0 · 0.44 · 16 | 0! · 0.19 · 13 | 0 · 0.11 · 11 | 1 · 0.10 · 9 | 0 · 0.07 · 9 |
| tune-mjcf | 1 · 0.09 · 9 | 1 · 0.10 · 9 | 1 · 0.08 · 9 | 1 · 0.14 · 15 | 1 · 0.09 · 9 | 1 · 0.06 · 7 |
| video-processing | 0 · 0.17 · 11 | 1 · 0.11 · 7 | 1 · 0.14 · 10 | 0 · 0.19 · 12 | 1 · 0.09 · 7 | 1 · 0.09 · 8 |
| vulnerable-secret | 0! · 0.00 · 2 | 0! · 0.00 · 2 | 0! · 0.00 · 2 | 0! · 0.00 · 2 | 1 · 0.14 · 9 | 1 · 0.11 · 7 |
| winning-avg-corewars | 1 · 0.09 · 6 | 1 · 0.45 · 20 | 1 · 0.35 · 19 | 1 · 0.29 · 19 | 1 · 0.91 · 36 | 1 · 0.32 · 23 |
| write-compressor | 1 · 0.17 · 8 | 1 · 0.13 · 5 | 1 · 0.13 · 6 | 1 · 0.13 · 5 | 1 · 0.07 · 4 | 1 · 0.08 · 6 |

</details>

## How a result is reported

- The job's `result.json` from Harbor, and each trial's reward, time, and spend.
- Since 2026-10-05 (the spec's Part III Item 167), each trial's efficiency record,
  `agent/efficiency.json`, one shape for every arm: tokens by class (input, cache read, cache write, output), dollars,
  model and tool calls, and the harness's own CPU and peak RSS apart from the commands it runs, sampled inside the
  task's container (`bench/harbor/sampler.py`). `bench/report/efficiency.py` turns a run's jobs into solved per dollar,
  tokens per solved task, the cache-read share, harness CPU per tool call and peak harness RSS, with Pareto charts of
  score against dollars, tokens and RAM. A job from before it, such as the first full run's, reads "not sampled" for
  CPU and RAM.
- Theseus's build (`theseus --version`) and the profile's settings (`THESEUS_BENCH_*`), so a run can be repeated.
- Trials that ended with an error, by Harbor's class for it: a timeout (`AgentTimeoutError`), a spend limit
  (`TheseusSpendLimitError`), a cut turn (`TheseusTurnCutError`), and so on (`bench/README.md`). Their rewards still
  count: the task's tests run after every trial.
