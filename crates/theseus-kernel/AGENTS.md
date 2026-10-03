# theseus-kernel

The durable kernel (spec §3.2a, §3.15, §3.16; Part II M2): every state transition of executions and actions, written
as WAL frames through the `Store` contract. Synchronous and deterministic. Read by theseus-core, theseus-discord,
theseusd, and theseus-sim.

Key modules: `kernel.rs`, `tx.rs`, `locks.rs`, `job.rs`, `children.rs`, `outbox.rs`. Read by: core, discord, theseusd, sim.

## What's here

- `kernel.rs`: the transitions. `Kernel::view` and `turn_of` give a turn its view; `observe` takes the push's one
  observer.
- `types.rs`: the durable objects (executions, actions, completions, budgets, wakes).
- `locks.rs`: one writer at a time per execution (theseus-id9).
- `tx.rs`: the kernel transaction (`Kernel::frame`, theseus-0owd): several transitions staged, then one frame.
- `job.rs`: the job wrapper (detached, durable, cancellable), `job::Stopping`, `holder`, and `wrapper_alive`.
  Since M4 18a a wrapper catches SIGTERM: the daemon's cancel asks it alone (`ask_to_stop`, by `sigqueue`, the grace
  in the signal's value), and it stops its whole tree, writes its verdict to the spool's `stops/`, and exits with no
  completion. `Stopping` tells it from a wrapper from before 18a by `/proc/<pid>/status`'s `SigCgt`
  (`catches_sigterm`) and stops an older one by its process group, as before (`verified_by: group`).
- `tree.rs` (18a): a job's process tree, found through each task's `children` file, and stopped in three phases:
  SIGTERM to every process, the grace, the freeze (SIGSTOP, rescanning until nothing new appears and all read
  stopped), then SIGKILL and the reap. Each process is signalled through a pidfd checked against its start time.
- `cancels.rs`: a cancel's steps on one action (`cancel_acknowledged`, then `cancel_verified`, `_unsupported`, or
  `_uncertain`), each settle with its `Verdict` (ACTION schema 3).
  `job_l1.rs` (M4 17b): the wrapper's L1 path, when `WrapperArgs.sandbox` is set: the command below
  `theseus_sandbox`'s init, its cgroup, its scratch summary, and the probe's run (`job::probe`). It never falls back
  to L0. `job_egress.rs` (18c): for a job whose `L1.egress` is not empty, the listener in its namespace, the proxy's
  variables, the proxy on the wrapper's threads, stopped once the job has ended (a stop and a deadline included), and
  its `Summary` in `detail.egress`. A job with no list gets none of it.
- `children.rs`: the daemon's children: what it spawned, what it adopted, and who reaps each. Job wrappers and
  tenders (the index tender, row 51) are reaped by their pids, a tender's exit reported to its supervisor; an
  `op` is left to tokio; anything else is an orphan.
- `outbox.rs`: posts that must reach a channel, as actions of their own record kind, `OUTBOX`.
- `spool.rs` (completions on disk), `redact.rs` (granted secrets withheld from a job's output), `stops.rs` (the
  soft stop), `tasks.rs` (task executions and their carve), `wakes.rs`, `gate.rs` (a confirmation's proposal and its
  digest), `clock.rs`, and `umask.rs`.

## Invariants

- **One frame per mutating method, or per transaction**: the records that change, plus the ledger rows that
  describe the change. A crash between two frames leaves a state some earlier call produced, never one no call
  produces.
- **Several transitions in one frame are a transaction, never a new combined transition** (`Kernel::frame`):
  `kernel.frame(&[ids], |k| { k.bind_confirm(..)?; k.wake(..)?; Ok(()) })`. It locks the executions named and
  their parents first, in id order. The transitions called on `k` stage their records and read what was staged,
  and `k.stage` adds the caller's own (a node, a row). `Ok` commits one frame, observed once; `Err` writes nothing.
  A `frame` inside one joins it, and its failure takes back only its own part. The `_with` family, `admit_input`,
  `plan_and_dispatch`, and `authorize_and_dispatch` are such compositions.
- **The lock.** A transition that reads an execution, or one of its actions, and writes it back holds that
  execution's lock from the read until its frame is indexed. Never call a transition that locks an execution
  this thread holds: it panics ("locked twice on one thread"), and inside a transaction, so does one of an
  execution it did not name. Compose in a transaction instead, as `mark_unknown` does. Several executions:
  `Kernel::lock`, in id order; a task and its parent: `lock_family`. Readers that write nothing take no lock.
  A transition's commit waits for the store's writer on the thread that holds its locks, and a wait for a lock
  another thread holds runs in `theseus_store::blocking`: neither holds a runtime worker (theseus-vni9). A lock
  is its thread's, so `ExecLock` is `!Send` (Review 2's R7), and a build-time check beside it fails the build if
  it ever becomes `Send`.
- **Lock order** is always the session, then the execution, and no kernel transition takes a session's lock.
- **Time is injected** (`Clock`: `RealClock`, `VirtualClock`). The kernel never reads the wall clock.
- **Every child goes through `children::spawn`.** A child spawned another way, and waited for, can be reaped by the
  sweep as an orphan, and its `wait()` fails with `ECHILD`. Never call `waitpid(-1)`.
- **A wrapper's pid can be reused**, so "alive" is `wrapper_alive(pid, job)`, whose command line names the job. A
  process in the middle of its exec has an empty command line: `holder` counts it as still starting (Item 35).
- **A cancel's verdict says how it knows** (M4 18a): `termination_verified` only with a means (`pidns`, `cgroup`,
  `tree`, `group`, `task`), and `verified_by: none` for a call nothing can stop. A cancelled job writes no
  completion, so a cancel never races the drain into a `failed` settle; its verdict is `stops/<id>`. A deadline
  uses the cancel's stop, and its verdict rides in the completion's `detail.stop`. A SIGTERM that is not a cancel
  (no `SI_QUEUE`) still ends the wrapper by the signal once its tree is stopped (theseus-6uo).
- **A stop is not a cancel.** A cancel is terminal; `stop_execution` halts the work and keeps the conversation
  (Item 9). A cancel settles every action that was never dispatched, in its own frame (Item 17). The results of
  the calls a cancel ended are the core's sweep (`ToolRuntime::answer_after_cancel`, theseus-0o8): it runs under
  `Kernel::frame`, and only once `holds_turn` is false, since a running turn owns its transcript.
- **`stop_call` stops one running call** and leaves its execution as it is (theseus-ht82): the daemon stopping a
  job below the disk's floor. A job the spool says runs is `Spool::running` (its pid file) or `wrapper_lives` (the
  pid file, or the lingering marker of a wrapper whose command has exited and whose child holds the output open).
- **An attempt that may have run is `OutcomeUnknown`**, never "not sent".
- **Read by state, never every record** (theseus-lv2). The store's index keeps `terms.rs`'s terms for each
  execution and action (`s:<state>`, `due`, `x:<execution>`, …). A reader on a path that runs often (the start,
  the driver's tick, the reconcile, health, a stop) asks `executions_by` / `actions_by` for its terms; only the
  listings that show every one call `executions()` or `actions()`. A change to what a term means renames
  `terms::PROJECTION`, so every store builds its terms again once. kernel-sim's `check_terms` holds every read by
  state to a full read.

## Tests

- In `src/`: `tests.rs` (whose fixtures run on a `VirtualClock`), `tests_stops.rs`, `tests_tasks.rs`,
  `tests_tx.rs` (the transaction), and `tests_wakes.rs`.
- `tests_frames.rs` is a golden: every frame a scripted run of the transitions commits, record by record, against
  `tests/golden/kernel_frames.txt`. A refactor leaves it byte-identical; `THESEUS_GOLDEN=write` rewrites it, for a
  change you mean.
- `tests/children.rs` makes its process a subreaper, so it is a test binary of its own: a sweep reaps any child of
  the process, other tests' included.
- `tests/tree.rs` (`harness = false`, 18a) re-execs itself as a job's wrapper, and as stand-ins for a wrapper from
  before 18a and a deaf one; each case ends with a `/proc` scan for its own `sleep` marker.
- `theseus-sim kernel-sim` drives the kernel under seeded faults and races (`--p-race`; 0 is fully
  deterministic) and checks its invariants. A new transition belongs in its random operations.
- Run this crate's tests as `cargo nextest run --workspace -E 'package(theseus-kernel)'`, never `cargo test -p`,
  which builds a second copy of the dependencies.

## Traps

- A kernel view holds the store open (an `Arc` to it). Don't keep one across a simulated crash's reopen: the reopen
  waits on the store's lock and fails.
- To show a held runtime worker in a daemon test, run the daemon with `TOKIO_WORKER_THREADS=1`.
- A core test with `InlineLauncher` must not kill a job: its "wrapper pid" is the test process.
