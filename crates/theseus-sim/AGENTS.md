# theseus-sim

The proving tools, installed beside `theseusd` and `theseus` (a tool, so a reader of its own: Part III Item 32).
The gate runs its lifecycle bench, and its crash test and kernel simulator on small fixed seeds (`tests/sim.rs`).

## What's here

- `src/main.rs`: the subcommands.
  - `crash-test` (the M1 exit test): a `worker` appends records and is SIGKILLed at random; each reopen must hold
    every record the worker reported durable, byte for byte. `--tear` also tears the WAL's tail; `--restarts`
    restarts within an iteration.
  - `kernel-sim` (the M2 exit test): the kernel under a virtual clock with seeded faults (a crash between any two
    frames or inside a startup step, lost, duplicate, and late completions, cancels) and invariants checked at every
    step. `--p-race` races a second thread against turns; 0 is fully deterministic.
  - `bench lifecycle` (`src/lifecycle.rs`): §9's budgets on a real `theseusd`: cold start, the same from a vault
    note's copy, clean shutdown with a job running, the same with a reply's post in flight to the in-process fake
    Discord (`inflight`, its own rig), SIGKILL and restart, a binary swap with the job's wrapper adopted, restore
    (with `theseusd restore`'s own phases), and the push's seed. `--check` fails a p95 over its budget plus the
    phase's margin (`lifecycle::margin_ms`, measured on the build machine).
  - `bench history` (`src/history.rs`): each phase's recent runs and headroom, from the CSV every gate appends.
  - `synth-store` (`src/synth.rs`): a store of parked sessions, for `bench lifecycle --sessions N`.
  - `fake-discord`: a stand-in for Discord's REST API.
- `src/lib.rs` exports `fake_discord` for other crates' tests. `src/fake_model.rs` stands in for the Messages API.

## Invariants

- **The budgets are §9's, and they don't move to make a gate pass.** A margin is the measured noise of a phase's
  p95 on the build machine, and changing one is a decision with its data (theseus-zay1).
- **New startup work lands with its bench row**, so the gate times it.
- **A new kernel transition belongs in kernel-sim's random operations**, with any invariant it must keep.
- The fake Discord never records a header, so no token reaches its log.

## Tests and use

- `tests/sim.rs`: the crash test and the kernel simulation on fixed seeds, in the gate.
- Release numbers of record: `target/release/theseus-sim bench lifecycle --theseusd target/release/theseusd --runs
  10`, with `--sessions 10000` for the synthetic store, or `--store` on a copy of a real store.
- Long runs stay out of the gate: `theseus-sim kernel-sim --seeds 40`, or a crash test with `--restarts 8`, so
  stores cross checkpoints.

## Traps

- With ten runs, a p95 is the slowest run, so one stalled fsync decides it. The gate reruns a miss once; read
  `bench history` before calling a miss a regression.
- A raced kernel-sim run reproduces from its seed only up to its first race.
- A bench of release binaries while a build runs: copy them first, since cargo replaces them mid-run.
