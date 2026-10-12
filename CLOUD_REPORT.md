# CLOUD_REPORT: deferred (theseus-rcgph)

Branch `cloud/20261011-deferred`. Feature commit `5d19273b`.

## What I found
- The kernel already had `Waiting{DueAt}` and a due scan that queues it (`wakes::due_now`, `fire_due`), so a deferred task needs no new timer and no store change.
- `open_task` was one frame; the deferred open is the same frame, with the execution `Waiting{DueAt}` and no `execution.queued` row.
- The local `origin/main` ref was stale (a9ad950f); `git fetch origin main` first, or keel-guard reports 236 commits of other people's findings.
- The disk allowance filled during the gate (target/ is 28 GB); the first suite run failed 157 tests with "disk below the floor". `rm -rf target/debug/incremental` fixed it. Not a code problem.

## What I changed (commit 5d19273b)
- Kernel `tasks.rs`: `Kernel::open_task_at(.., Opening{reports_to, wake_parent, start_at_ms}, extra)`; `open_task` delegates. The sleeping task is `Waiting{DueAt}`, `resume_pending` false, its carve reserved in the same frame; `execution.opened` carries `starts_at_ms`, and an `execution.waiting` row (why `deferred`) replaces `execution.queued`. `Kernel::process_started_ms()` added.
- Core `task_start.rs` (new): `due()` (wake.at's `parse_after`/`parse_at`, `MIN_AHEAD_MS`/`MAX_AHEAD_MS`, so the reminders row's 365 days moves both; error text derives the day count from the constant), `Deferred` META record `task.deferred.<session>` (written in the open's frame), `starts_at()`, `header()`.
- `task.rs`: `start_at`/`start_after` in the schema and `Input`; refused without a `brief`; description gains one sentence; result text "Scheduled task …"; `TaskInfo.starts_at_ms`; `TaskStarted` fact says "scheduled to start …".
- `turn/deferred_step.rs` (new) + 3 lines in turn.rs: a task's first turn writes the node `Deferred task, set <when> for <when>; started <now>[ (N late[: the daemon was not running then])]` with its META mark, once (retry-safe).
- Surfaces: `TaskInfo.starts_at_ms` (protocol; TS regenerated); board label `▷ starts <time>` for a task with no turn (protocol `push.rs`); core `push::hm` names the weekday for a time 20 h or more ahead; `theseus tasks` (`render/tasks.rs::state_word`, `render/time.rs::fmt_starts`); Discord `/tasks` (`render/deferred.rs`).
- AGENTS.md of core and kernel updated.

## How I proved it
- New tests, all pass: kernel `tests_deferred.rs` (4: asleep with carve held; starts when due and not before; cancel releases carve and no turn; restart keeps it, a restart past the due time starts it once); core `tests_tasks_deferred.rs` (4: whole path with carve, `task.list` start and board label, note once, one report; start fallen due while down says so; cancel before start; refusals for 31d, 0s, past `start_at`, both); `task_start::tests` (bounds in words, late header); protocol `tests_task_info.rs` (wire with and without the field, old peer) and `a_deferred_task_starts_and_a_sleeping_one_sleeps`; CLI and Discord line tests; `fmt_starts`; daemon `tests/tasks.rs::a_deferred_task_survives_a_kill_and_a_restart_and_starts_once` (real theseusd, stand-in model, `kill -9` while asleep, restart, starts once, one report, one note): 6.8 s, pass.
- Planted reverts, each reverted and `touch`ed:
  - opened running (Queued, no wake): 4 of 8 tests failed (`..opens_asleep..`, `..starts_when_it_is_due..`, `..restart_keeps..`, core whole-path) plus the core while-down test timed out.
  - carve not reserved at creation: `..opens_asleep_with_its_carve_held`, `..restart_keeps..`, core whole-path failed.
  - start time lost (wake None): 5 of 8 failed (kernel start/restart/open tests, core whole-path, core while-down).
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1`): door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule pass. Suite: 3916 passed, 33 failed, 46 skipped, all 33 theseus-sandbox contract/bench and theseusd sandbox L1 tests (the known root-VM failures). I ran the later phases by hand: protocol types (the generated TaskInfo.ts is staged, clean), `cargo deny --offline check bans licenses sources` ok (the advisories were not checked; the fetch gave no database). Keel guard: ok, 0 findings.
- `theseus-sim bench lifecycle --runs 10 --check` (debug, this VM, no change to the start path): "LIFECYCLE BUDGET MISSED", the health call p95 7.7 ms against 5 ms; cancel p95 21.1 ms ok, serving p95 18.6 ms, start-to-answer figures as on any slow VM. I did not run a baseline without my change here; the budgets are the owner's machine's.

## Live check for the maintainer
On a scratch daemon (a fresh `--state-dir`, `--socket`, config with `theseus-sim fake-model --rules '[{"when":"Defer","calls":[{"name":"task_create","input":{"brief":"Draft the notes","start_after":"2m","arrangement":{"pieces":[{"quote":"Defer the notes as a task","role":"objective"}]}}}]}]'` as `[model] api_base` and the providers'):
1. `theseus --socket S ask "Defer the notes as a task"`: the result says "Scheduled task … to start <time>".
2. `theseus --socket S tasks`: the line reads `▷ starts <time>`, `0 turns`; `theseus --socket S budgets` shows the carve on the parent.
3. Optionally `kill -9` the daemon and start it again: the same line is still there.
4. After 2 minutes `theseus tasks` shows it complete with 1 turn and its report; `theseus history <task session>` has one `harness:deferred` node, `Deferred task, set … for …; started …`.
5. For the cancel: start another with `start_after 1h`, `theseus cancel <short>`, the carve returns.
On this VM the same path ran as the daemon test above (real `theseusd`, stand-in model), with a 6 s start in place of 2 min; I did not drive it from the CLI.

## Left, uncertain, for the owner
- The brief says the brief is headed by the note. The note is a harness node after the brief and arrangement (the brief and arrangement are written at open; their edges derive from the brief). Same words, different position.
- The watch clause of the guidance sentence is left out ("a condition a command can check is a watch (later)"): no watch tool exists yet. The sentence names `wake.at` for reminders. Tool block size: `tests_route_keep` passes.
- The `commitments`/`reminders` row should add a deferred-tasks line to its block (note left here).
- `push::hm` now writes `Fri 09:00` for any label time 20 h or more ahead (wake labels too); other labels are unchanged.
- No ledger kind added (`execution.waiting`, why `deferred`); no new fact for the start note, because the `execution.queued` row (why `due`) and the node already record it.
- Spec/docs for the maintainer: Part III item for theseus-rcgph; `docs/status.md`; the tools note's task line.
- Not tested: a deferred check task (`check_of` with `start_*`); it should work the same, but nothing exercises it.

## Keel findings expected
None.
