# theseus-kernel

The durable kernel (spec §3.2a, §3.15, §3.16; Part II M2): every state transition of executions and actions, written
as WAL frames through the `Store` contract. Synchronous and deterministic. Read by theseus-core, theseus-discord,
theseusd, and theseus-sim.

## What's here

- `kernel.rs`: the transitions. `Kernel::view` and `turn_of` give a turn its view; `observe` takes the push's one
  observer.
- `types.rs`: the durable objects (executions, actions, completions, budgets, wakes).
- `locks.rs`: one writer at a time per execution (theseus-id9).
- `job.rs`: the job wrapper (detached, durable, cancellable), `job::Stopping`, `holder`, and `wrapper_alive`.
- `children.rs`: the daemon's children: what it spawned, what it adopted, and who reaps each.
- `outbox.rs`: posts that must reach a channel, as actions of their own record kind, `OUTBOX`.
- `spool.rs` (completions on disk), `redact.rs` (granted secrets withheld from a job's output), `stops.rs` (the
  soft stop), `tasks.rs` (task executions and their carve), `wakes.rs`, `gate.rs` (a confirmation's proposal and its
  digest), `clock.rs`, and `umask.rs`.

## Invariants

- **One frame per mutating method**: the records that change, plus the ledger rows that describe the change. A
  crash between two frames leaves a state some earlier call produced, never one no call produces.
- **The lock.** A transition that reads an execution, or one of its actions, and writes it back holds that
  execution's lock from the read until its frame is indexed. Never call another public transition that locks the
  same execution: it panics ("locked twice on one thread"). Use the locked inner form, as `mark_unknown` uses
  `accept_locked`. Two executions: `lock_two`, which takes them in id order; a task and its parent: `lock_family`.
  Readers that write nothing take no lock.
- **Lock order** is always the session, then the execution, and no kernel transition takes a session's lock.
- **Time is injected** (`Clock`: `RealClock`, `VirtualClock`). The kernel never reads the wall clock.
- **Every child goes through `children::spawn`.** A child spawned another way, and waited for, can be reaped by the
  sweep as an orphan, and its `wait()` fails with `ECHILD`. Never call `waitpid(-1)`.
- **A wrapper's pid can be reused**, so "alive" is `wrapper_alive(pid, job)`, whose command line names the job. A
  process in the middle of its exec has an empty command line: `holder` counts it as still starting (Item 35).
- **A stop is not a cancel.** A cancel is terminal; `stop_execution` halts the work and keeps the conversation
  (Item 9). A cancel settles every action that was never dispatched, in its own frame (Item 17).
- **An attempt that may have run is `OutcomeUnknown`**, never "not sent".

## Tests

- In `src/`: `tests.rs` (whose fixtures run on a `VirtualClock`), `tests_stops.rs`, `tests_tasks.rs`, and
  `tests_wakes.rs`.
- `tests/children.rs` makes its process a subreaper, so it is a test binary of its own: a sweep reaps any child of
  the process, other tests' included.
- `theseus-sim kernel-sim` drives the kernel under seeded faults and races (`--p-race`; 0 is fully
  deterministic) and checks its invariants. A new transition belongs in its random operations.
- Run this crate's tests as `cargo nextest run --workspace -E 'package(theseus-kernel)'`, never `cargo test -p`,
  which builds a second copy of the dependencies.

## Traps

- A kernel view holds the store open (an `Arc` to it). Don't keep one across a simulated crash's reopen: the reopen
  waits on the store's lock and fails.
- To show a held runtime worker in a daemon test, run the daemon with `TOKIO_WORKER_THREADS=1`.
- A core test with `InlineLauncher` must not kill a job: its "wrapper pid" is the test process.
