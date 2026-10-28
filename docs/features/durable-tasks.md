# Durable tasks you can watch, answer and steer from anywhere

*One page on the task system. Numbers come from the data files beside each report they cite. Updated 2026-10-28.*

## What it is

A task is a piece of work with a place of its own. You start one by asking for something long, by `theseus task
start "write the changelog"`, or by a tool call the model makes. From then on it is a child session with its own
budget, its own card on every surface, and its own report, which lands where you started it. It survives a restart,
an upgrade and a `kill -9`. A task with nothing to do is a record on disk, not a process, so the daemon holds
thousands at once.

## How it works

- **A durable state machine, not a process.** A task is a `TASK` record in the store and an execution in the kernel
  (`crates/theseus-kernel/src/tasks.rs`, `crates/theseus-core/src/task.rs`). The tool call that opens it, the
  execution and the session share one id, so a call run again after a crash finds its task instead of opening
  another. Its budget is carved from its parent's. Every action is written before it is attempted and its result
  when it settles, so a restart resumes what was in flight and runs nothing twice. The spec's state machine is
  [§3.5 Tasks](../spec/03-part1-s3.md#35-tasks-fluid).
- **Questions on every surface.** An open question shows at once on every open screen and on the DM's board; with
  no screen open, one needs-you card goes to the owner's DM after a short wait (`crates/theseus-core/src/needs_you.rs`).
  One answer, from any surface, settles it everywhere.
- **The heartbeat never calls a model.** A timer in the daemon (`crates/theseus-core/src/rpc/driver.rs`) drains
  the spool, queues due wakes, and reconciles cards whose question closed while nobody watched. It is bookkeeping
  from the store's truth: it calls no model itself, and a wake it queues runs as its own turn under its task's
  budget.
- **A task's home is decided when it is created.** Where its report, its wakes and its routine's runs speak is read
  from its record when it reports, so a task started from the DM or a voice call reports to the task home, and
  `theseus task rehome` moves it while it runs (`crates/theseus-core/src/task_home.rs`).
- **Later, and again.** A task with a start time opens asleep and the due scan starts it, even if the time passed
  while the daemon was down (`crates/theseus-core/src/task_start.rs`). A routine holds one pending wake, and each
  firing opens a fresh task under it, so a run's context never grows (`crates/theseus-kernel/src/routines.rs`).

## What you see on each surface

- **Discord:** a forum post per task with a live card, a private task-home channel with one pinned board, and
  reactions to pause, resume, stop or retry.
- **Terminal:** `theseus tasks`, `theseus task show ID` for one task whole, `theseus tui` with the task that needs
  you a key away, and `theseus tasks --ended` to find one later.
- **Cockpit:** the Tasks page and the Operations page, with its needs-you queue and its Board, Graph and Timeline
  lenses.
- **Voice:** in a private call an open question is read to you at the next pause, and a spoken "approve" or "stop
  it" counts exactly as pressing the button would.

## The proof

- **Kill and resume** ([the report](../benchmarks/2026-10-18-async-kill-family.md), 2026-10-18): killed with
  SIGKILL mid-task and restarted, Theseus resumed 10 of 12 trials, with 0 effects done twice and 1 trial that left
  a process running. Claude Code repeated an effect in 4 of 12 trials and lost work in 2. OpenCode resumed 1 of 12.
- **Thousands held** ([the gate bench](../benchmarks/2026-10-18-gate-bench-v1.md), 2026-10-18): with 10,000 parked
  sessions the idle daemon used 0.37% of one CPU, 4.2 wake-ups a second and 117 MB; a status call over 1,000 sessions
  answered in 2.1 ms at the median; its cold start was 24.5 ms against a 250 ms budget.
- **The whole path live** (2026-10-17, a scratch daemon with a stand-in Discord): a task's post, channel and board,
  one approval from Discord and one from the CLI, each cleared on every surface, and a message typed into a running
  task that it took at its next step.
- **Tests:** `crates/theseus-kernel/src/tests_tasks.rs` and its siblings for the kernel; the `tests_task_*` files
  in `crates/theseus-core/src/` for the core.

## The limits today

- A task cannot start tasks of its own (depth one in the kernel); fan-out runs through the parent.
- The kill benchmark is twelve trials; its intervals are wide.

## What is coming

- **1.0.4: a scheduling tool.** A built-in tool the model reaches for when it judges that something should recur or
  be watched; from the call on, the schedule, the runs, the carried state, the stop and the report are the daemon's
  deterministic machinery, inspectable like any task.
- **1.0.4: tasks on the agenda.** Running, scheduled and recurring tasks as blocks on the cockpit's Timeline beside
  your calendar.

Related: [Fast, as a contract](fast-as-a-contract.md) (the budgets a start and a resume are held to),
[It trusts its environment](trusted-environment.md) (why a self-started task notifies rather than asks).
