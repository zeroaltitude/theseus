# Coding at the level of the field, on its own model

*One page on coding. Every number is recomputed from the per-trial CSV beside the report it cites. Updated 2026-10-28.*

## What it is

Theseus edits, builds, tests and fixes the way a good coding agent does: language servers as tools, each edit's new
compiler errors in its result, sandboxed commands, and long jobs in the background. The honest summary, on
Terminal-Bench 2.0: level with the harnesses that run on its own model, behind the harnesses on GPT-6 Astra on pass
rate at a fraction of their cost, and carrying a cost regression of its own that 1.0.4 fixes.

## How it works

- **Tools built for code:** edits that return the file's new diagnostics from the language server
  (`crates/theseus-lsp`), commands in a sandbox with no network, no credentials and an empty home
  (`crates/theseus-sandbox`), and jobs that outlive a turn ([Durable tasks](durable-tasks.md)).
- **The model is routed** by Jev per turn ([Jev in the core](jev-in-the-core.md)), so a trial may run on Haiku 5.5,
  Sonnet 5.5, or both.
- **Improve, never max:** no task-specific prompt, no special case for a harness, no tuning on held-out tasks.

## The proof

**Terminal-Bench 2.0, high tier, all 89 tasks.** Theseus ran three trials a task; the harnesses measured in late
October ran one. Pass rates and the mean cost of a trial, from the CSVs:

| Harness, model | Run | Solved | Cost a trial |
|---|---|---|---|
| Codex, GPT-6 Astra | [2026-10-25](../benchmarks/2026-10-25-terminal-bench-codex-high.md) | 76 of 89, 85.4% | $1.31 |
| OpenClaw, GPT-6 Astra | [2026-10-27](../benchmarks/2026-10-27-terminal-bench-openclaw-astra-high-k1.md) | 73 of 89, 82.0% | $1.96 |
| **Theseus 1.0.3, routed Claude models** | [2026-10-26](../benchmarks/2026-10-26-terminal-bench-theseus-1.0.3-high-k3.md) | **209 of 267, 78.3%** | **$0.34** |
| Claude Code, Sonnet 5.5 | [2026-10-18](../benchmarks/2026-10-18-terminal-bench-v1-high.md) | 201 of 267, 75.3% | $0.13 |
| OpenClaw, Sonnet 5.5 | [2026-10-27](../benchmarks/2026-10-27-terminal-bench-openclaw-sonnet-high-k1.md) | 66 of 89, 74.2% | $0.33 |
| Theseus v1, routed Claude models | [2026-10-18](../benchmarks/2026-10-18-terminal-bench-v1-high.md) | 195 of 267, 73.0% | $0.10 |
| OpenHands, Sonnet 5.5 | [2026-10-27](../benchmarks/2026-10-27-terminal-bench-openhands-high-k1.md) | 64 of 89, 71.9% | $0.10 |
| OpenCode, Sonnet 5.5 | [2026-10-18](../benchmarks/2026-10-18-terminal-bench-v1-high.md) | 189 of 267, 70.8% | $0.12 |
| Pi, Sonnet 5.5 | [2026-10-18](../benchmarks/2026-10-18-terminal-bench-v1-high.md) | 188 of 267, 70.4% | $0.11 |
| Aider, Sonnet 5.5 | [2026-10-26](../benchmarks/2026-10-26-terminal-bench-aider-high-k1.md) | 11 of 89, 12.4% | $0.02 |

The 95% intervals of the four harnesses on Claude models overlap, so "level" is the claim, not "ahead". Theseus
1.0.3's 78.3% [72.9, 82.8] against its own v1 build's 73.0% [67.4, 78.0] does not establish that 1.0.3 solves more.

**The 30 hardest tasks at top effort:** Codex on GPT-6 Astra solved 26 of 30, 86.7%, at $1.71 a trial
([2026-10-27](../benchmarks/2026-10-27-terminal-bench-codex-astra-max-k1.md)); Theseus 1.0.3 solved 59 of 90,
65.6%, at $0.61 ([2026-10-27](../benchmarks/2026-10-27-terminal-bench-theseus-1.0.3-max-k3.md)); Claude Code
solved 68 of 90, 75.6% ([2026-10-18](../benchmarks/2026-10-18-terminal-bench-v1-max.md)).

**The cost regression, named.** Like for like on one price table, a 1.0.3 trial costs 2.22 times the v1 build's at
the median and 2.79 times at the mean on the high tier, and 2.3 to 2.6 times on the hard 30, where the median trial
also takes 3.8 times the wall clock. The cause is the refusal fallback: on `stop_reason == "refusal"` the turn
switches to a fallback model and keeps the rest of the turn there. On the hard 30 it touched 35 of 90 trials and
71% of the cell's spend came after the switch; on the high tier it touched 69 of 267 trials. Fallback trials passed
at about the same rate as the rest, so this is a cost fault, not a correctness one. The fix is in 1.0.4.

**Bug Hunt** ([2026-10-26](../benchmarks/2026-10-26-bughunt-rs-theseus-k3.md)): on a Rust repository with 13
planted bugs, three trials fixed 3.3 bugs a trial and 10 of 39 bug-trials, with 31 of 78 hidden tests passing.

## The limits today

- Behind the GPT-6 Astra harnesses by 4 to 7 points on the high tier and 21 points on the hard 30.
- The cost regression above, until 1.0.4.
- Stopping early with budget left is the most common loss on Terminal-Bench; the rule since 2026-10-25 is that a
  stop is judged on lack of progress, never on elapsed time.
- The late-October harness runs are one trial a task; their intervals are wider than Theseus's.

## What is coming

- **1.0.4:** the refusal fallback fixed at its cause; then every bench rerun on the 1.0.4 build, with the rivals
  rerun only where a model or a harness changed.
- **After 1.0.4:** Theseus on GPT-6 Astra, and the harnesses on their best models, in one table.

Related: [Jev in the core](jev-in-the-core.md), [Fast, as a contract](fast-as-a-contract.md),
[every report](../benchmarks/README.md).
