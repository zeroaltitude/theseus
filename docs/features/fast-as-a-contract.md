# Fast, as a contract

*One page on speed. Numbers come from the data files beside each report they cite. Updated 2026-10-28.*

## What it is

Theseus is one Rust daemon that is already running when you type. The prompt is ready in tens of milliseconds, the
first words of a reply appear as the model writes them, a restart or an upgrade takes a fraction of a second with
work in flight, and the daemon idles in tens of megabytes. None of that is a happy accident: each number has a
written budget, and the commit gate fails any commit that breaks one.

## How it works

- **Budgets in the gate.** `scripts/gate.sh` runs the lifecycle bench (`crates/theseus-sim/src/lifecycle.rs`) on
  every commit: cold start, start from the vault, clean shutdown, shutdown with work in flight, crash and restart,
  binary swap under load, and more, each against a budget with a margin. A phase over budget is a red gate. The
  budgets are the spec's [§9 Efficiency targets](../spec/05-part1-s4.md#9-efficiency-targets-to-measure-not-assert)
  and [§3.22 Lifecycle: restart is routine](../spec/04-part1-s3.10.md#322-lifecycle-restart-is-routine).
- **A turn and a job have budgets too.** The turn bench holds a plain turn's wall time and the daemon's own
  milliseconds on it; the jobs bench holds a sandboxed command's start under 25 ms; the size step holds each binary
  under its ceiling. Their history is kept across gates, so a slow creep is visible as well as a cliff.
- **Why it is fast.** The store is a write-ahead log with a rebuildable index, so a start reads nothing it does not
  need. A request's first layers are the same bytes in every session, so the provider's cache holds across sessions
  and personas. A job starts without a fork of the daemon. Nothing on the critical path waits on Jev. Job wrappers
  exec `/proc/self/exe`, so running jobs survive a binary swap.

## What you see on each surface

- **Terminal:** `theseus ask` answers from a ready prompt; `theseus tui` opens on a long session in tens of
  milliseconds.
- **Discord:** a typed message reaches its first text within tens of milliseconds of the model's first byte.
- **Cockpit:** the Speed page, and the Benchmarks pages with every published run.

## The proof

**Against Claude Code on the same stand-in model** ([the report](../benchmarks/2026-10-18-head-to-head-speed.md),
2026-10-18, medians):

| What | Theseus | Claude Code |
|---|---|---|
| Start to a ready prompt | 38.6 ms | 497.9 ms |
| Model's first byte to the first text on screen (plain turn) | 0.8 ms | 224.7 ms |
| Resume of a long session | 25.7 to 73.4 ms | 423.1 to 471.5 ms |
| CPU per turn | 10.0 ms | 1,655 ms |
| Idle, the surface open | 70 MB | 360 MB |
| Start to a ready prompt under neighbour IO | 40.5 ms | 626.1 ms |

Where it is level: a three-tool coding task end to end, 2.1 s against 2.2 to 2.5 s, since the model's time
dominates.

**The gate's budgets and the release build** ([the gate bench](../benchmarks/2026-10-18-gate-bench-v1.md),
2026-10-18, the same-window A/B, medians of four runs):

| What | Budget | Release build |
|---|---|---|
| Start to answering | 50 ms | 18.5 ms |
| Clean shutdown, with work in flight | 100 ms | 41.2 ms |
| Crash, restart, and answer again | 150 ms | 23.7 ms |
| Swap in a new binary under load | 200 ms | 54.2 ms |

At 10,000 parked sessions the same build started in 24.5 ms against a 250 ms budget and idled at 0.37% of one CPU.
The idle daemon with an empty store used 49 MB and 0.008% of one CPU over thirty seconds.

## The limits today

- The head-to-head is on a stand-in model, so it measures the harness alone, by design; a real model's time
  dominates a turn and is the same for every harness.
- A tool call's own overhead is a few tens of milliseconds, and a sync on the critical path is most of it. The
  owner's daemon waits for the store's `fdatasync` on every tool result; durability stays, and a budget for syncs
  on the critical path is the open row.

## What is coming

- **1.0.4:** the cost regression's fix (see [Coding](coding.md)) also cuts the wall clock of a long turn, which
  tripled on the hardest tasks between the v1 and 1.0.3 builds.
- **After 1.0.4:** the benchmarks run again on the 1.0.4 build.

Related: [Durable tasks](durable-tasks.md) (what the start and resume budgets protect),
[Context compiled for each turn](compiled-context.md) (why the cache holds across sessions).
