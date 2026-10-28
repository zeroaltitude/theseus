# theseus-kernel

The durable kernel (spec §3.2a, §3.15, §3.16; Part II M2): every state transition of executions and actions, written
as WAL frames through the `Store` contract. Synchronous and deterministic. Read by theseus-core, theseus-discord,
theseusd, and theseus-sim.

Key modules: `kernel.rs`, `tx.rs`, `locks.rs`, `job.rs`, `cgroup.rs`, `children.rs`, `outbox.rs`. Read by: core, discord, theseusd, sim.

## What's here

- `kernel.rs`: the transitions. `Kernel::view` and `turn_of` give a turn its view; `observe` takes the push's one
  observer.
- `types.rs`: the durable objects (executions, actions, completions, budgets, wakes).
- `locks.rs`: one writer at a time per execution (theseus-id9).
- `tx.rs`: the kernel transaction (`Kernel::frame`, theseus-0owd): several transitions staged, then one frame.
- `job.rs`: the job wrapper (detached, durable, cancellable), `job::Stopping`, `holder`, and `wrapper_alive`. The
  daemon spawns it with no `pre_exec`, so by posix_spawn, which copies nothing of the daemon; the wrapper makes its
  own session as it starts (theseus-ypqg).
  Since M4 18a a wrapper catches SIGTERM: the daemon's cancel asks it alone (`ask_to_stop`, by `sigqueue`, the grace
  in the signal's value), and it stops its whole tree, writes its verdict to the spool's `stops/`, and exits with no
  completion. `Stopping` tells it from a wrapper from before 18a by `/proc/<pid>/status`'s `SigCgt`
  (`catches_sigterm`) and stops an older one by its process group, as before (`verified_by: group`).
- `job_wait.rs` (Tier 7.1): the wrapper's wait on its command, asleep until something happens: the command's pidfd
  (an L1 job's init's), and a wake pipe its SIGTERM and SIGCHLD handlers write a byte to, polled with the time left
  before the deadline. It looked every 20 ms before. A spooled completion is taken (`Kernel::take_completion_with`):
  the drain and the turn waiting on the job both read it, and the second finds it settled and writes nothing; a
  cancelled job's late completion too, once one taker has written its arrival (theseus-jnnj).
- `tree.rs` (18a): a job's process tree, found through each task's `children` file, and stopped in three phases: SIGTERM
  to every process, the grace (asleep on up to 64 of their pidfds, `WATCHED`, woken by each exit and closed before the
  freeze; theseus-dwoj), the freeze (SIGSTOP, rescanning until nothing new appears and all read stopped), then SIGKILL
  and the reap, until each killed process's pidfd says it exited (up to `KILL_WAIT`, 2 s). Each process is signalled
  through a pidfd checked against its start time. A `children` file can miss a live child, so an empty scan ends a phase
  only when the caller's reap agrees (`tree::Left`: in a wrapper, a subreaper, `waitpid`'s ECHILD means none is left;
  theseus-g11i). The stop wherever a job has no cgroup. Where the host refuses `pidfd_open` (ENOSYS, or an older
  profile's EPERM), each process is signalled by `kill` after the same start-time check, and its end seen by the
  scans (theseus-f7tz).
- `spawn.rs` (theseus-ypqg): an L0 command started by its wrapper with `clone3(CLONE_VM | CLONE_VFORK)`, as
  posix_spawn clones, so nothing is copied; the child sets the operator's umask, which std's `Command` could set only
  by `pre_exec`, a fork. With a cgroup, `CLONE_INTO_CGROUP`: born inside, since a move by `cgroup.procs` waits for an
  RCU grace period (8 to 40 ms measured). Where `clone3` answers ENOSYS (Docker's default seccomp profile, a kernel
  before 5.3), `clone` with the same flags, and a child bound for a cgroup writes `0` to its `cgroup.procs` before its
  exec; the completion's `detail.spawn_fallback` says so (theseus-f7tz). Before, every `proc.run` in a container
  failed to start.
- `cgroup.rs` (theseus-a5nv): an L0 job's cgroup, where the daemon's is delegated: `<daemon's>/job-<id>`, threaded
  (the daemon stays in its unit's cgroup, which `pids` makes a thread root), with `pids.max`; its `cpu.stat` and
  refusals go in the completion. A stop: SIGTERM to each process in it, the grace, then `pids.max` 0 and SIGKILL to
  each until `cgroup.events` says `populated 0` (a threaded cgroup has no `cgroup.kill`), then what is left of the
  tree. The wrapper removes it at its end; the next daemon's `ready` removes one left empty.
- `cancels.rs`: a cancel's steps on one action (`cancel_acknowledged`, then `cancel_verified`, `_unsupported`, or
  `_uncertain`), each settle with its `Verdict` (ACTION schema 3).
  `job_l1.rs` (M4 17b): the wrapper's L1 path, when `WrapperArgs.sandbox` is set: the command below
  `theseus_sandbox`'s init, its scratch summary, and its stop by the init's pid namespace; and `job::self_test`,
  `/bin/true` started the same way, which `theseusd check` runs on demand (theseus-gyin). It never falls back
  to L0. `L1.rw` (theseus-pw1q.21.1) is the one writable bind a view can have, set only by
  the core's self-build launcher; `probe_run::run_logged` runs a long command with its output on a file. `job_egress.rs` (18c): for a job whose `L1.egress` is not empty, the listener in its namespace, the proxy's
  variables, the proxy on the wrapper's threads, stopped once the job has ended (a stop and a deadline included), and
  its `Summary` in `detail.egress`. A job with no list gets none of it. A daemon can `register` a `Local` service for a
  `host:port` (theseus-pw1q.21.5, the sealed scorer's model relay): a job in the daemon's own process (`probe_run`) whose
  list is that one name is served by it on the listener, in place of the proxy; a wrapper process has no registry.
- `mcp_l1.rs` (M7 43a): an MCP server in L1, `theseusd mcp-sandbox`: its `Server` (argv, environment, cwd, more
  read-only binds, the job's `L1` view) rides in `THESEUS_MCP_L1`; it hands its own stdin, stdout, and stderr to
  the init and keeps no end of the pipes, runs `job_egress`'s proxy for a server with a list, and waits on the
  init's pidfd, a signalfd (SIGTERM, SIGINT, SIGHUP: SIGTERM to the init, a 2 s grace, then SIGKILL), and the
  daemon's pidfd (its `kill -9`: SIGKILL to the init at once).
- `children.rs`: the daemon's children: what it spawned, what it adopted, and who reaps each. Job wrappers and
  tenders (the index tender, row 51) are reaped by their pids, a tender's exit reported to its supervisor; an
  `op` is left to tokio; anything else is an orphan. After an exec, `relearn` reads a child whose command line is
  empty (still in its exec, theseus-mi6a) again, up to `EXEC_WAIT` (500 ms) in all, so a tender just started is
  not taken for an orphan (theseus-r4hn); `learn` is the rule, tested by order.
- `outbox.rs`: posts that must reach a channel, as actions of their own record kind, `OUTBOX`.
- `earlier.rs` (theseus-m9iy): an earlier process's in-process calls. Startup's reconcile notes each dispatched
  provider call (`Evidence::in_process`, by its tool) in memory and writes nothing; the driver's first tick after
  serving (or the heartbeat) marks them all `outcome_unknown` in one frame, as `in_process_before_restart`, each
  reservation booked as spent, never held (theseus-f3wr): `mark_unknown_as(.., book: true)`, the action's
  `detail` and row `"cost_basis": "reservation"`. Every other `mark_unknown` holds. A resolution of either goes
  through `earlier::resolve_in`, so a booked call is never counted twice.
- `gone.rs` (theseus-vej5): the jobs whose wrappers went while no daemon ran (a unit stop that killed the cgroup,
  an OOM kill). Startup's scan notes every other dispatched action not overdue and not being cancelled
  (`Earlier::jobs`) and reads nothing more; the harness's first beat after serving, after its drain, runs
  `Kernel::settle_gone_jobs` once: per job the spooled completion first, then `Spool::wrapper_lives` (the pid
  file's or the lingering marker's pid, by `wrapper_alive`, so a reused pid is not alive), then the completion
  again. A finished job's completion is taken, one with a pid file or marker and no live wrapper is marked unknown
  as `wrapper_gone_at_start`, all in one frame; one with neither (an in-process tool, a hand, a job whose daemon
  died before writing its pid) is left to its deadline. The core records `job.wrapper_gone` for each, and the
  probe's cost as the `jobs_at_start` startup phase.
- `overdraw.rs` (theseus-usei): a spend limit that notifies (`KernelConfig::spend_limit_notify`): a provider
  call's reservation past an unpinned limit (or a task's whose parent's is unpinned) is made, never refused
  (`OverBudget`); every other tool's still is, and so is any call under a pinned limit. `open_task` then carves what
  the task asked for, up to the parent's whole limit. At `KernelConfig::spend_hard_multiple` (10) times the limit a
  provider call whose reservation would pass it is refused anyway, `KernelError::HardStop`, nothing written
  (theseus-jse4). Tests: `tests_overdraw.rs`.
- `day_ceiling.rs` (theseus-kp20, softened by theseus-9e02): the daemon's day ceiling, `[kernel] daily_spend_ceiling_usd`
  ($200), one counter for every model call by the local day of `KernelConfig::zone`, in memory (no store read or write).
  A provider call's reservation holds on it in `plan` and is refused with `KernelError::DayCeiling`, nothing written, past
  the day's **bar** (`DayCeiling::bar`): the ceiling under `daily_notify = false` (`[kernel] daily_spend_ceiling_mode =
  "stop"`, the kernel's own default), the runaway line (`daily_runaway_micros`, 0 never) under notify, where the ceiling
  only warns (`take_warning`: the highest unannounced level, 1 at the ceiling and one more a `daily_step_micros` after
  it). Its settle spends the real cost (an unknown one its reservation). A refusal stops the day (theseus-l9on): every
  later hold or fit that day is refused, however small, while the bar is no higher than the one it was refused under
  (`Reached.since` says so; under notify the ceiling is never sticky, only the runaway line). A settle's change is an
  `Effect`, applied after its frame (`day_after`; inside a transaction at its commit, and a failed transaction lets its
  holds go, `tx.rs`); a provider settle also fills the execution's burn ring (`burn_record`, ten minutes,
  `KernelConfig::burn_line_micros`) and each of its ancestors' (`burn_ancestors`, at most 8 steps,
  theseus-o1h3f.5), so a fan-out's helpers reach their parent's line together, and a provider plan whose execution has reached the line is refused with
  `KernelError::BurnRate` (`overdraw.rs::burn_stopped`); an approved budget question that carries `args.call.brake`
  (`reset_budget`) resumes it, spend kept and window empty. Only `PROVIDER_TOOL` counts: an AWS hand and a task's carve
  never, and a task's carry to its parent adds nothing **to the day's total**. The core holds its own calls with `hold_call`/`Hold` (or by
  amount, `settle_part`), books late costs (`book`), and seeds the day at the start (`seed`: spend, the refusal's time,
  the warned level). `budget_question.rs` is the budget question, moved out of kernel.rs whole. Tests:
  `tests_day_ceiling.rs` (stop), `tests_runaway.rs` (notify, the runaway line, the brake).
- `spool.rs` (completions on disk, one sync each: a start finishes a rename a crash cut short, and takes a
  completion its action settled already as a no-op, theseus-yxiv), `redact.rs` (granted secrets withheld from a
  job's output), `stops.rs` (the soft stop), `tasks.rs` (task executions and their carve; `open_task_at` opens one asleep, `Waiting{DueAt}`, for a deferred task: theseus-rcgph), `retry.rs` (theseus-7zgsm, store format 35: `Execution.dismissed` (when, and at which turn, so a conversation that fails again asks again: read only through `Execution::dismissal`) and `retry_of`): `dismiss_execution`, one frame (the record and its `execution.dismissed` row; again writes nothing), and `open_retry`, one frame under the failed task's, its parent's and the new task's locks: the new task (the failed one's `reports_to` and `wake_parent`, the parent's authority, queued), its carve (never more than the failed one had, nor than the parent has left), the failed one dismissed, the core's `extra` records, and the rows; refused, nothing written, for a non-task, a task not in a failure, or a parent that has ended; tests `tests_retry.rs`), `wake_kinds.rs` (theseus-8ux6x: a wake's `Purpose` (`Check` default, `Remind`, `Routine`, `Watch`), `WakeRun` (`Turn` default, `Notice`, `Task` (a routine's run, theseus-pvm5c), `Probe`: a watch, theseus-f2r2b) and `WakeLimits`; store format 30, each default left out of a record's bytes), `watch.rs` and `probe_run.rs` (theseus-f2r2b: `Kernel::probe_result` writes one check's news in one frame, under the execution's lock: the row `watch.checked|met|expired|failing`, the watch's state in META `watch.state.<wake id>` and the post, and for a met watch with `then: turn` puts the wake back as a due turn wake; `open_watches` reads the open ones once; `probe_run::run` runs an argv in L1 from the calling thread with no job record, its output piped and only the head kept), `wake_owner.rs` (the owner's verbs, each one frame under the execution's lock with its row and no turn guard: `create_wake`, `snooze_wake` (a pending wake moves; one that fired is set again from its META record `wake.fired.<id>`), `pause_wake` (held, or with `until` moved past it, a series' occurrences counted as passed over), `resume_wake`; and `fire_notices`, which takes a `Notice` wake the moment it is due, in any state of its execution, in one frame: the wake leaves (a series re-arms), a `wake.fired { notice }` row, that META record and the post of its note), `when.rs` (`tomorrow 9:00`, `fri 9:00`, `17:30`, `2026-10-14 09:00` in the daemon's zone), `wakes.rs` (and since theseus-lherl `move_wake`, one frame with a `wake.moved` row, and `take_wakes_listing`, the take
  and what is still pending from one read), `repeat.rs` (a repeating wake's series:
  its span (`m`, `h`, `d`, `w`, and `mo`, a calendar month), days, and `until`, or a 5-field `cron` expression, by jiff's zoned arithmetic in `KernelConfig::zone`; 37a; store format 34), `gate.rs` (a confirmation's proposal and its
  digest), `clock.rs`, and `umask.rs`.
- `cron.rs` and `routines.rs` (theseus-pvm5c; horizon HZ4): a routine is a conversation's execution holding one pending wake with
  `purpose: Routine` and `run: Task(TaskRun)`. `cron.rs`: `Cron` (five fields, lists, ranges, steps, Vixie's either-day rule, civil
  date and wall time resolved by jiff's compatible rule, so `0 8 * * *` stays 08:00 across a change of offset exactly as
  `Repeat::at` does: `tests_cron.rs` holds the two together over four zones with a fixed seed), and `parse_schedule` /
  `describe` (the words people write: `weekdays 08:00`, `every 30m`, `monthly 09:00`, `the 1st 09:00`, or cron). The `nw`
  term (`terms.kernel.4`) now covers a notice wake *or* a routine's task wake that is not paused, in any open state, so the
  due scan that finds notices finds routines (no new timer). `routines.rs`: `fire_routines` takes every due, unpaused task
  wake of an execution under its lock and, in one frame, puts the series back (numbered past the occurrences passed over),
  then applies the core's `RoutinePlan`: a `Run` opens the task with `open_task`'s records (`tasks::carve_task`, shared with
  `open_task_at`: the carve in the routine's reservations, the task queued; the task depth bound is checked inside
  `carve_task`, so a fire honours it like any other open — theseus-rrqmr, where it did not) and a `wake.fired` row, a `Skip { why }` writes
  `wake.skipped` and nothing else; `reset_month` is a `budget.reset { why: "month" }` row in the same frame, and the
  routine's own `reports` are cleared each fire, since nobody reads them there. `run_routine_now` opens one run from the wake
  without touching the series, paused or not. `reschedule_routine` changes the schedule, brief, per-run limit and month in one
  frame. `Kernel::overdraws` treats a task of a routine like a task of an unpinned parent: a run never waits on a budget
  question (nobody is awake for it), it notifies at its limit and stops at the hard multiple; the month is held where a run is
  carved. Tests: `tests_routines.rs` (a minute series opens three tasks, one frame each; a skip re-arms; paused is never due and
  run now runs once; the month's reset row; a restart across occurrences counts two missed), `tests_cron.rs`.

## Invariants

- **One frame per mutating method, or per transaction**: the records that change, plus the ledger rows that
  describe the change. A crash between two frames leaves a state some earlier call produced, never one no call
  produces.
- **Several transitions in one frame are a transaction, never a new combined transition** (`Kernel::frame`):
  `kernel.frame(&[ids], |k| { k.bind_confirm(..)?; k.wake(..)?; Ok(()) })`. It locks the executions named and
  every execution above each of them first, in id order. The transitions called on `k` stage their records and read what was staged,
  and `k.stage` adds the caller's own (a node, a row). `Ok` commits one frame, observed once; `Err` writes nothing.
  A `frame` inside one joins it, and its failure takes back only its own part. The `_with` family, `admit_input`,
  `plan_and_dispatch`, and `authorize_and_dispatch` are such compositions.
- **The lock.** A transition that reads an execution, or one of its actions, and writes it back holds that
  execution's lock from the read until its frame is indexed. Never call a transition that locks an execution
  this thread holds: it panics ("locked twice on one thread"), and inside a transaction, so does one of an
  execution it did not name. Nor take a lock while the thread holds any other: `lock_all` panics ("a lock taken
  while this thread holds another", theseus-oqxw), so a frame's closure that calls the kernel itself on an
  execution the frame did not name fails at once instead of deadlocking out of id order. Compose in a transaction
  instead, as `mark_unknown` does. Several executions: `Kernel::lock`, in id order, in one call; a task and
  **every execution above it**, up to the conversation: `lock_family` (theseus-deaqt, since what a task settles
  is the spend of each of them; `Kernel::family` reads the chain, whose `parent` is never changed).
  `Kernel::frame` takes that same whole family for each id it is given. Readers that write nothing take no lock.
  **A chain of any length is safe because it is one `lock_all` call with sorted ids: a thread waits only for an
  id greater than every id it holds, and a second call while one is held panics instead of waiting. So: name
  every execution you may write in the one call — never a lock inside a lock.**
  A transition's commit waits for the store's writer on the thread that holds its locks, and a wait for a lock
  another thread holds runs in `theseus_store::blocking`: neither holds a runtime worker (theseus-vni9). A lock
  is its thread's, so `ExecLock` is `!Send` (Review 2's R7), and a build-time check beside it fails the build if
  it ever becomes `Send`.
- **Lock order** is always the session, then the execution, and no kernel transition takes a session's lock.
- **Time is injected** (`Clock`: `RealClock`, `VirtualClock`). The kernel never reads the wall clock.
- **Every child goes through `children::spawn`.** A child spawned another way, and waited for, can be reaped by the
  sweep as an orphan, and its `wait()` fails with `ECHILD`. Never call `waitpid(-1)`. The in-process L1 probe's init
  is one (`job_l1`, `Reap::Command`: the thread that waits for it is its owner, `Kind::Owned`), and the sweep is woken
  by the very exit the wait looks for (theseus-1b2zm).
- **A wrapper's pid can be reused**, so "alive" is `wrapper_alive(pid, job)`, whose command line names the job. A
  process in the middle of its exec has an empty command line: `holder` counts it as still starting (Item 35).
- **A cancel's verdict says how it knows** (M4 18a): `termination_verified` only with a means (`pidns`, `tree`,
  `group`, `task`, and `cgroup` for an L0 job's cgroup, theseus-a5nv), and `verified_by: none` for a call nothing
  can stop. A cancelled job writes no
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
- **A task never waits on input with nothing to wake it** (37b, theseus-7kg). No one gives a task input: it waits on
  input only beside a wake of its own (the core parks it so, `task::parks_on_wake`), and a frame that would leave it
  waiting with none queues it instead (`wakes::task_unparked`, in `cancel_wake` and `end_turn`), so its next turn
  finds nothing new, ends it, and it reports. A `/stop` still leaves a task waiting on input, as it leaves a
  conversation.
- **A question answered while its turn runs is read at the turn's end** (theseus-q5af): `end_turn` queues the
  execution (`Kernel::queued_at_end`, why `answered`) instead of parking it on a question already bound, so an
  answer is never lost to a running turn, and one continuation follows. Tests: `tests_answered.rs`.
- **An attempt that may have run is `OutcomeUnknown`**, never "not sent". Its money is held, unless its process is
  gone (an earlier process's in-process call), when it is booked at its reservation (theseus-f3wr).
- **Read by state, never every record** (theseus-lv2). The store's index keeps `terms.rs`'s terms for each
  execution and action (`s:<state>`, `due`, `nw` (a notice wake that is not paused, in any open state: `notice_executions`), `x:<execution>`, …). A reader on a path that runs often (the start,
  the driver's tick, the reconcile, health, a stop) asks `executions_by` / `actions_by` for its terms; only the
  listings that show every one call `executions()` or `actions()`. A change to what a term means renames
  `terms::PROJECTION`, so every store builds its terms again once. kernel-sim's `check_terms` holds every read by
  state to a full read.

## Tests

- In `src/`: `tests.rs` (whose fixtures run on a `VirtualClock`), `tests_gone.rs` (fake wrappers: `sh` running a
  script named `job-wrapper`, so its command line names the job), `tests_stops.rs`, `tests_tasks.rs`,
  `tests_tx.rs` (the transaction), `tests_wakes.rs`, and `tests_repeat.rs` (a series: the re-arm, missed
  occurrences, a cancel, `until`, the cap, and both changes of offset of a year, in zones from POSIX TZ strings, so no
  tz database is read).
- `tests_wake_owner.rs` (theseus-8ux6x): a reminder fires in one frame with no turn while its conversation's turn runs; a turn takes a check-in and leaves a notice alone; pause and resume keep a series' numbering (`#4` after three held days); `pause --until`; a snooze is one frame, of a pending wake and of one that fired; `Task` and `Probe` refused; the horizon (365 days in, 366 out) and the caps (`max_reminders` over the daemon, `max_wakes` check-ins a conversation).
- `tests_frames.rs` is a golden: every frame a scripted run of the transitions commits, record by record, against
  `tests/golden/kernel_frames.txt`. A refactor leaves it byte-identical; `THESEUS_GOLDEN=write` rewrites it, for a
  change you mean.
- `tests_enosys.rs` (theseus-f7tz): a host that refuses `clone3` or `pidfd_open`, stood in by a seccomp filter on
  the test's own thread (`theseus_sandbox::seccomp::refuse_here`), which the processes it starts inherit; a command's
  cgroup by both clones (where a cgroup v2 directory can be made: as root here); and the spawn's micro-bench
  (ignored: `--run-ignored only --no-capture`).
- `tests/children.rs` makes its process a subreaper, so it is a test binary of its own: a sweep reaps any child of
  the process, other tests' included.
- `tests/tree.rs` (`harness = false`, 18a) re-execs itself as a job's wrapper, and as stand-ins for a wrapper from
  before 18a and a deaf one; each case ends with a `/proc` scan for its own `sleep` marker, which holds the run's
  pid, so a run beside it on the machine is never found (theseus-g11i), and a failure prints the stop's verdict
  and each process found.
- `theseus-sim kernel-sim` drives the kernel under seeded faults and races (`--p-race`; 0 is fully
  deterministic) and checks its invariants. A new transition belongs in its random operations.
- Run this crate's tests as `cargo nextest run --workspace -E 'package(theseus-kernel)'`, never `cargo test -p`,
  which builds a second copy of the dependencies.

## Traps

- A kernel view holds the store open (an `Arc` to it). Don't keep one across a simulated crash's reopen: the reopen
  waits on the store's lock and fails.
- To show a held runtime worker in a daemon test, run the daemon with `TOKIO_WORKER_THREADS=1`.
- A core test with `InlineLauncher` must not kill a job: its "wrapper pid" is the test process.
- A spool marker written with `fs::write` (`pids/<id>`, `lingering/<id>`) is made empty, then filled, so a reader
  between the two finds no pid, and one inside the write a part of it. That is a marker being written, never a dead
  wrapper's: `Spool::lingering` removes only one whose pid is no longer its job's wrapper, and a second old
  (theseus-sdgl, theseus-v18k).
- A wrapper that will linger writes its marker before its report, while its pid file still names it: a reader that
  takes the report removes the pid file and then asks `wrapper_lives` (theseus-v18k). So a marker says nothing of
  the pid file: a wait for the wrapper's own removal of it reads the pid file (`toolrun/steps.rs`).
