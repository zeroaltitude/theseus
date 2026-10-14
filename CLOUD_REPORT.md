# CLOUD_REPORT: task-start (theseus-7oq0w)

Branch `cloud/20261013-task-start`. Commit 21efea9 (the whole step).

## What I found
`task_create` needs a parent *turn* (`open_task_at` required the parent `Running` and a `TurnGuard`), so a start with no model
turn needed a kernel opening without one. No store change was needed (a new ledger kind, `task.started_by_owner`, and no new
stored field: no `MANIFEST_FORMAT` bump).

## What I changed (21efea9)
- **Shared opening (named for routines / retry-dismiss):** `Kernel::open_task_in` (kernel/tasks.rs); `open_task_at` (model, needs a
  turn) and the new `Kernel::open_task_owned` (owner, parent need not run a turn) both call it. Use `open_task_owned` from routines
  and retry-dismiss.
- `task.start` (`core/rpc/task_start.rs`, protocol `task_start.rs`, `ReportTo`): one frame with the task's execution, session, brief,
  graph record, quotes arrangement, carve, deferred record and a `task.started_by_owner` row (`fact/task_start.rs`).
- Authority: new `Act::Task` via `judge_act`; `TASK_START` in `mcp_server::JUDGED` (a ledgered `via: mcp` refusal); the CLI's
  `client::OPERATORS` (47 now) for a job's shell. Refusals say where to start it instead.
- Quotes: one `Body::Arrangement` (role `context`) after the brief; session holds them via `external::VIA_QUOTE`; a parent that
  holds external text passes its own hold.
- CLI `theseus task start BRIEF [--title --in --budget --profile --at|--in-time --quote-file --report-to]` (`crates/theseus/src/task_start.rs`);
  `task` and `tasks` are separate subcommands, no clap collision. TUI: `N` (`new_task.rs`, `Mode::NewTask`).
- Docs: the four crates' AGENTS.md. Generated TS regenerated and added.

## Design choices for the owner
1. **Seam addition:** an extra optional `start_when` (a `wake.create`-style string, read in the daemon's zone) beside `start_at_ms` /
   `start_after_ms`, because the CLI cannot parse "fri 9:00" itself. Plus `limit_usd`, `short`, `home_session` on the result.
2. **Over-budget is refused, not capped** (the test line asks for refusal with numbers; the seam text says "capped"). Flip in
   `carve_of`. With nothing left the kernel refuses (`NothingToCarve`).
3. `report_to: home` posts to the parent's place and the parent reads the report at its next turn; `wake_parent` is false, so a
   report never costs a model turn. `board`: no post (the parent's `reports` list still gets the entry).
4. `Act::Task` is a new variant (4 match sites) rather than reusing `Act::Session`.
5. Paused clock: the kernel uses the wall clock (`RealClock`), so the deferred tests use a 1.5-2 s real delay, like `tests_tasks_deferred`.

## Proof
- New tests (all pass): core `tests_task_start` (10: carve/brief/first turn/one frame, Home once, named home + depth, deferred, time forms,
  quotes hold, parent hold passes, over-budget refusal numbers + nothing written, shared place + MCP refused + ledgered, report_to),
  CLI `tests/task_start.rs` (4) + unit (3), TUI `tests_new_task` (4), daemon `theseusd/tests/task_start.rs` (3, real CLI + real daemon + stand-in model).
- Planted reverts: budget refusal switched off -> `a_budget_over_what_the_parent_has_left_is_refused_with_the_numbers` FAILED;
  quote hold switched off -> `quotes_hold_the_tasks_session_as_external_text` FAILED. Both files restored and touched.
- `python3 scripts/keel-guard.py`: ok, 0 findings (one informational "same test renamed" line from the base's store test, not mine).
- Gate (`THESEUS_GATE_NO_BENCH=1`): keel, fmt, shape, features, clippy, cockpit, reader rule pass. Suite: 4497 run; failures are
  the known root-VM L1 tests (theseus-sandbox contract, theseusd sandbox) and
  `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, which timed out at 21 s under load
  and passes alone (1.35 s). Protocol types: regenerated TS committed. The lifecycle/jobs/turn benches were skipped (NO_BENCH).
  The gate stops at the suite on the known failures, so I ran the post-suite phases' relevant check (protocol types) by hand; deny
  was not separately run.

## Live check (stand-in model, real daemon and CLI; captured from `theseusd/tests/task_start.rs --no-capture`)
```
$ theseus task start "Count the boats" --budget 0.5
task c68008 started (running), $0.50 carved from session …f4d2c2: no model turn was spent
$ theseus task start "Count the boats later" --in-time 2s
task b48861 started (asleep until 2026-10-14 07:52:31), $25.00 carved from session …2bda60: no model turn was spent
```
The first finished with exactly one model request (its own turn); the second was `waiting`, turns 0, no request until due.
The TUI's `N` is proved on the scripted daemon (`tests_new_task`); I did not drive the real TUI in a terminal here.

Maintainer's live check: `theseus task start "Count the boats" --budget 0.5` on a scratch daemon, then `theseus tasks` (complete) and
`theseus history <home>`; with `--in-time 1m` it shows `waiting` + `▷ starts` until then; press `N` in `theseus tui`, type a brief, Enter.

## Left / uncertain
- Home with no place never surfaces the report until its next turn (CLI-only daemons): `theseus tasks` shows it.
- Disk filled twice on this VM during builds; I cleared `target/debug/incremental` and large test binaries.
