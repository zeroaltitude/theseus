# CLOUD_REPORT: stop-all (theseus-cdcwr)

## Step: `execution.stop { tasks: true }` and its readers
**Found.** Depth is one in the kernel (`TaskDepth`): a task cannot start a task, so "a task with its own task" cannot happen
through the tools today. The walk is written for any depth anyway (`Kernel::task_tree`, depth first, an ended task walked
through); the depth is held by a kernel test that writes a nested chain as records. `execution.stop` has no place rule or
job refusal today, so `tasks: true` gets both (below). The task's pointer to `runtime/halt.rs` for Discord's `/stop` is
stale: `/stop` lives in `runtime.rs`; the new code is in `runtime/halt.rs` (runtime.rs is at its 3,500 ceiling, unchanged).
Base store format is 33, not 27; no store change made.

**Changed** (commit 7b70495).
- Protocol: `ExecutionStopParams.tasks` (bool, absent when false) and `.discord` (origin, for the place rule);
  `ExecutionStopResult.cancelled` (absent when empty); `LedgerKind::ExecutionStoppedAll`. TypeScript regenerated.
- Kernel: `task_tree`. Core: `rpc/stop_all.rs` (`Core::stop_execution_all`): the plain stop, then cancel each live task
  below (up to 4 rounds, for a task opened mid-walk), one `task.cancel`-effect frame each, then the `execution.stopped_all`
  row (fact `ExecutionStoppedAll`). Wakes of the conversation stay; a deferred task is cancelled.
- `tasks: true` is `judge_act(Act::Session{method: execution.stop})`: owner, private place only (shared place refused,
  `approval.refused` ledgered). CLI: `theseus stop <id> --all` (`--tasks`), refused inside a job.
- TUI: `S` then `S` or `y`; anything else keeps it. Discord: `/stop all:true` (boolean, default false), answer adds the count.
- `theseus chat`'s `/stop --tasks`: **`theseus chat` is not on this base**: join note, call `execution.stop` with `tasks: true`.

**Proved.**
- New tests pass: `theseusd::tasks::a_stop_all_cancels_every_task_under_the_conversation_and_keeps_its_wake` (real daemon: two
  tasks each with a `sleep` job, one deferred task, the conversation's wake; a plain stop leaves all three; `tasks` cancels
  all three, both sleeps gone, wake kept, conversation answers again, a second call cancels nothing); kernel
  `a_task_tree_lists_each_task_after_its_own_tasks`; core `rpc::tests_stop_all` (shared place and a non-private channel
  refused even for the owner, plain stop allowed, DM/den/CLI go; 3 rows); CLI `stop_all_is_refused_inside_a_job`; TUI
  `shift_s_needs_a_second_key`; Discord `stop_all_is_an_added_option_that_defaults_to_no`, `the_stops_answer_counts_the_cancelled_tasks`.
- Planted reverts: walk not recursing into a task's own tasks -> kernel `a_task_tree...` failed (order assertion).
  `execution.stop` always taking the stop-all path -> the daemon test failed at the plain-stop assertion.
  Both restored and touched; `git status` clean of them.
- Not done: a manual live run of the CLI on a scratch daemon, and runs under load.

**Live check for the maintainer.** On a scratch daemon with a conversation that has started three tasks (one `sleep 60`):
`theseus stop <id> --all` should print the stop line ending `cancelled 3 task(s): ...`; `theseus status` then shows nothing
working within a second; `pgrep -f 'sleep 60'` finds nothing; `theseus wakes` still lists the conversation's wake.
From a job's shell `theseus stop <id> --all` is refused; `/stop all:true` in a shared channel answers the refusal.

**Open.** Doc updates are mine only in `crates/theseus-core/AGENTS.md` and `crates/theseus-tui/AGENTS.md`; the spec
(Part III item, `docs/status.md`) is yours. A task started by a task would need no change here.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run with `CARGO_PROFILE_*_DEBUG=0` because this VM's disk
allowance filled): door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule passed. Suite: 4,480 tests; the
known root-L1 failures (theseus-sandbox contract 38, bench 2, theseusd sandbox 26) and one more:
`theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (times out at 21 s; fails
the same way on the base without my changes: the base lacks the theseus-wpv2g fix). My first run also failed
`the_slash_commands_list_tasks_and_wakes_and_cancel_one_by_name` (command order); fixed by keeping `stop` in place, passes.
After the suite I ran the rest by hand: protocol-types (generated TS staged), `cargo deny check` ok. Benches skipped (NO_BENCH).
Keel guard: ok, 0 findings. **Keel findings expected:** none.
