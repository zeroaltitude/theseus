# CLOUD_REPORT: routines (HZ4, theseus-pvm5c)

Branch `cloud/20261013-routines`, built from main at 0baeed6 (store format 33 there, not 27: the base is
later than the brief says). Two code commits and this report:

- `f6b73a3` routines: the whole step (kernel, protocol, core, CLI, tests, AGENTS.md notes).
- `b25d263` routine: a routine is named by its title too (the live check found it).
- the report commit (last).

## What I found

- The reminders row had done the hard part: `Purpose::Routine`, `WakeRun::Task`, `repeat.rs`, the due scan by the `nw`
  term, `open_task`. Missing: a way to fire a task wake with no turn, a schedule beyond `every/days/until`, a month
  budget, and the owner's verbs.
- `open_task_at` needs a `TurnGuard` and a turn. A routine fires with no turn, so I moved the body of its records into
  `tasks::carve_task` (shared, output byte-identical: the kernel frames golden and `tests_tasks` pass untouched).
- A task of a pinned parent is itself hard-limited, so a routine run (pinned month, $0.50 run) **stalled on a budget
  question at its first model call** in my first end-to-end test (the worst-case reservation of one call is over $0.50
  on the example profile). Nobody is awake at 08:00 to answer it, and an unanswered question would also block every
  later run through the overlap rule. See "Design choices".
- `authority_for` (task_graph/tools.rs) cleared `Plan::authority` for any tool that is not a task-graph edit, so a
  plan that names an authority did not ask; it keeps it for `routine.create` now.
- `policy.explain`'s probe plan has no authority; it carries the routine tool's, or explain and the gate disagree.
- A new tool grows every request's tools block by about 600 tokens: `tests_tiering` and `tests_route_keep` set their
  window from it (their own doc comments keep that history). I bisected the new window (41,100 to 41,700; 42,200 to
  42,800 and 36,700 to 37,300) and trimmed the tool's description first, which halved the growth.

## What I changed

**Kernel** (`crates/theseus-kernel`)
- `cron.rs`: `Cron` (5 fields, lists, ranges, steps, Vixie's either-day rule, civil date and wall time resolved by jiff's
  compatible rule), `parse_schedule` (`weekdays 08:00`, `daily 7:30am`, `mon,thu 09:00`, `mon-wed 6pm`, `every 30m`,
  `1mo`, `monthly 09:00`, `the 15th at 18:00`, or cron), `describe`, time formatters. No new crate.
- `repeat.rs`: a `mo` (calendar month) span and `Repeat.cron: Option<String>`; `at`, `next_after`, `between` answer for it.
- `routines.rs`: `fire_routines` (one frame: series re-armed with the occurrences passed over counted, the run opened
  with `open_task`'s records or `wake.skipped { why }`, the month's `budget.reset { why: "month" }`),
  `run_routine_now` (once, series untouched, paused or not), `reschedule_routine`. Plan and decision are the core's.
- `terms.rs`: `nw` now covers a notice **or a routine's task wake**, not paused (`terms.kernel.4`): the due scan that finds
  notices finds routines; no new timer, no new read per tick.
- `overdraw.rs`: a task of a routine notifies at its limit (as a task of an unpinned parent does) and stops at the hard
  multiple; `Execution::is_routine`.
- `wake_kinds.rs`: `Task` accepted (an empty brief refused with words); `PendingWake::task_due`.
- Ledger kinds `wake.skipped` and `routine.changed` (both written, so the reader rule holds).

**Protocol**: `routines.rs` (`RoutineSpec`, `RoutineInfo`, `RunRow`, the eight `routine.*` params, `CatchUp`, `Speak`,
`NOTHING_NEW`), the eight methods, the TS (generated, staged).

**Core**
- `routine.rs`: `Book` (the records in memory; one META prefix scan on the first tick after serving), `prepare`/`create`
  (shared by the verbs and the tool, no `Core` needed), `plan` (catch-up, overlap, month, then the run), `info_of`,
  `run_row`, `lines` (the commitments block), `quiet`.
- `rpc/routines.rs`: `routine.create|list|runs|pause|resume|run_now|update|delete` (the owner's acts through the new
  `Act::Routine`: owner from a private place, refused inside a job by the CLI) and `fire_routines_in`, called at the end of
  `Core::fire_notices` in the harness's 500 ms tick.
- `routine_tool.rs`: the model's `routine.create`: private places only, always one card (`Plan::authority`), reads
  `Create routine "Morning digest": weekdays at 08:00 (next: Fri 08:00, Mon 08:00, Tue 08:00) · up to $0.50 a run, $10 a
  month · posts to your DM · each run asks before it uses a tool, as any task does`.
- `speak: when_notable`: the brief asks for a report ending `NOTHING NEW`; such a run only logs (`TaskOf::target_for`).
- The commitments block lists the routines a conversation made (`🔁 a41d04 weekdays 08:00 Morning digest (next Fri)`),
  from memory only, within its 300-token bound.
- `[routines]` (`default_run_usd` 0.50, `default_month_usd` 10, `max_routines` 50) with its template section and
  validation. **`[kernel] max_routines` is removed** (declared at 8ux6x, "they come with the routines row"); a note that
  named it will stop loading. Say so if the owner wants it kept as an alias.

**CLI**: `theseus routine` (list), `add SCHEDULE BRIEF [--title --budget --month --to --catch-up --speak]`, `runs ID [-n]`,
`pause ID [--until WHEN]`, `resume`, `run`, `update`, `rm`; the writes are in `client::OPERATORS`.

**Store format 34** (33 on this base): `Repeat.cron`. The old layout is a literal in `tests_layouts` (`WAKE_AT_33`), read
with no cron and written back byte for byte. The routine's record is a META key and needed no stored field of its own;
the task's mark is another META key. `store.rs`'s manifest doc lists 34; `tests_layouts`, `store.rs` and
`theseusd/tests/versions.rs` assert 34 now (35 as "newer").

Files I edited in shared areas, small: `rpc/confirms.rs` and `fact/answer.rs` (the `Act::Routine` variant, three lines each),
`rpc/import.rs` (the routing list), `turn/compile_step.rs` and `commitments.rs` (the routine lines), `toolrun.rs` (the tool
and the runtime's `routines` field), `session.rs` (`TaskOf::target_for`), `task.rs` (`brief_text` is `pub(crate)`),
`wake_home.rs` (one call), `task_graph/tools.rs` (the authority), `rpc/explain.rs`, `config.rs`. `turn.rs` and `harness.rs`
are at their ceilings: net 0 lines each.

## How I proved it

All with `CARGO_INCREMENTAL=0` (the first build's incremental cache filled the disk: 16 GB of `target/debug/incremental`
deleted once).

- Kernel `tests_routines.rs` (5): a minute series opens 3 tasks, **one frame each** (`frames_written_here`), series back
  with `occurrence` 2, 3, 4 and the carve in the reservations; a skip writes `wake.skipped` and re-arms; paused is never
  due, the `nw` scan leaves it out, run now opens one task and leaves due, occurrence and pause as they were; the month's
  `budget.reset { why: "month" }` row and the kernel handing the core the available figures with and without a reset; a
  restart (`crash`) across 3 occurrences hands the core `missed: 2, while_down: true`, one catch-up task, the series
  numbered 4.
- Kernel `tests_cron.rs` (6): cron against `Repeat::at` (daily, 60 random times of day per zone, 400 occurrences from a
  January start) and `Repeat::next_after` (weekdays) and the month span, in UTC, New York, Sydney and Lord Howe (a half
  hour change), fixed seed; cron fields, `until`, `between`, the schedule words, and what is refused.
- Core `tests_routines.rs` (8): a minute routine runs **3 times as 3 fresh tasks** (the wake's own snooze brings each
  due in a second), each report posted to the DM target, run 2's brief says `the last run reported: Tide at 06:12.`, run 3's
  request does not hold run 1's header; overlap skips with a `wake.skipped` row and runs again after; paused never fires,
  run now while paused runs once and a second is refused in words; a spent month skips, says so **once**, and the 1st
  resets it (`plan` with the figures the kernel would give); the catch-up's words `(2 missed while the daemon was down)`
  and `catch_up: none|auto|one`; the model's `routine_create` **holds on its card** (nothing exists before the yes; after
  it, `created_in` is the conversation and its next request holds the `🔁` line); `when_notable` logs a `NOTHING NEW`
  report; update and delete.
- CLI unit tests (2) for the list and run lines.
- Workspace suite: 4,495 tests, 4,461 pass. The 34 failures: the 33 known L1 ones, and
  `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (see below).
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, twice, the second on the final tree): door, keel,
  keel tests, fmt, shape, features, clippy, cockpit, test build, reader rule: all pass; suite: the failures above only;
  protocol types: no diff after the suite; `cargo deny check`: ok. The three benches are skipped by the flag.
- `python3 scripts/keel-guard.py`: `keel: ok (... 463 files changed, 0 findings acked ...)`; its only printed line is the
  base's own renamed store test.

**The discord failure is the base's, not mine.** Nothing under `crates/theseus-discord` changed. It fails alone, 3 of 3,
after 21 s. Cherry-picking `b495389` ("the thinking-answer test holds the process message's create whatever its text",
theseus-wpv2g; not in my base) makes it pass in 1.2 s; I took the cherry-pick back out.

**Planted reverts** (restored, `touch`ed, `git status` clean after each):
- *the month not reset* (kernel `open_run` ignores `reset_month`): `a_months_reset_is_a_budget_reset_row_in_the_fires_frame`
  fails with `nothing to carve a task's budget from: $0 of the $1 limit is left ($1 spent)`: the other four kernel tests pass.
- *overlap not skipped* (core `held_back`'s check always false): `a_run_still_going_makes_the_next_occurrence_skip_with_a_row`
  fails (`no the skip in 10 s`) and so does `a_paused_routine_never_fires_and_run_now_runs_once_while_it_is_paused` (a second
  run was not refused); the other six pass.
- *cron in the wrong zone* (the date's time resolved in UTC): `a_daily_cron_is_a_daily_series_across_every_change_of_offset`
  fails at the first change (`New York: 52 7 * * *, occurrence 46: cron 2026-03-09T03:52:00 against the series 2026-03-09T02:52:00`),
  with `a_monthly_cron_is_a_month_span` and `the_words_people_write_become_series`.
- *quiet reports not quiet* and *a spent month not skipped*: not planted (time); the tests exist.

**Under load**: see "Load" below.

## The live check, run here, and the one for the maintainer

I ran it on this VM against a scratch daemon on `theseus-sim fake-model` (state `/tmp/live/state`, `[web]` and `[discord]`
off, `min_repeat_minutes = 1`, every secret `env:LIVE_KEY`). Result:

```
routine 0073cb (Tide check) made: every 1m, next Wed 09:51; up to $0.50 a run and $10.00 a month
  (three runs, 09:51:31 09:52:31 09:53:31; the daemon stopped at 09:54:06, started again at 09:56:52)
09:56:51  wake.fired  {"due_at_ms":...,"late_ms":140701,"missed":2,"while_down":true,...}   <- the catch-up: "2 missed"
#6 complete 2026-10-14 09:59:41 (307 ms) $0.03 Tide at 06:12. High water holds an hour.
#5 complete 2026-10-14 09:57:31 (235 ms) $0.03 ...
#4 complete 2026-10-14 09:56:51 (327 ms) $0.03 ...        <- one catch-up run, not three
#3 .. #1 complete (61, 81, 65 ms) $0.03 each
0073cb every 1m Tide check  next Wed 10:00 · $0.50/run · $0.17 of $10.00 this month · none · 6 runs · last: complete ...
```
and, on the same state: `routine pause` (paused, kept its place), `routine run` (task `b4e8ff`, still paused, run #7),
`routine rm` (deleted; `routine list` empty). (The first `runs` calls in the script said `no routine is named tide`:
the title lookup `b25d263` came from that.)

Maintainer's commands (a scratch daemon, a fresh state dir; build the install first):
```
theseus-sim fake-model --rules rules.json --addr 127.0.0.1:47911 &      # rules.json: [{"when":"Run ","text":"Tide at 06:12."}]
theseusd example-config > full.toml   # then: api_base = "http://127.0.0.1:47911", every [secrets] entry env:LIVE_KEY,
                                      # min_repeat_minutes = 1, [web] and [discord] enabled = false
LIVE_KEY=x theseusd --config config.toml --socket $S --state-dir $D &
theseus --socket $S routine add "every 1m" "Read the tide table and say when the next high tide is." --title "Tide check" --budget 0.50 --month 10 --to none --catch-up one
sleep 215; theseus --socket $S routine runs tide             # 3 runs, 3 tasks, cost, first line of each report
theseus --socket $S shutdown; # wait for the pid; sleep 165; start the daemon again; sleep 70
theseus --socket $S routine runs tide -n 20                   # one catch-up run; `theseus ledger --kind wake.fired` shows missed: 2
```
With a Discord DM bound (`--to dm`, the default), each report posts to the DM as a task's report does: **not run here**
(no Discord on this VM), nor the model's card on a real DM (the test drives `confirm_list`).

## Load

`tests_routines` (core, 8 tests, with the three end-to-end ones that wait on the real clock) at `nice -n 19` beside four
`sh -c 'while :; do :; done'` loops at nice 0 (killed by their pids), twice: **8 of 8 passed both times**, in 12.2 s and
13.1 s (5.7 s unloaded). A first attempt rebuilt the workspace's test binaries at nice 19 under the same load and made no
progress in 20 minutes; I stopped it, built unloaded, and ran the prebuilt binary under load. The kernel tests run on a
virtual clock and need no load.

## Keel findings expected

None: the guard reports 0 findings. Tests I changed that a reviewer should see, none of them removed or weakened:
- `kernel tests_wake_owner::a_task_or_a_probe_is_refused_with_words_and_nothing_is_written`: a `Task` is now accepted
  with a brief, so its case is a Task with an **empty brief** (`needs a brief` / `say what each run does`); the probe case
  is as it was. One assertion each, as before.
- `core config::tests` expects 57 `[policy.tools]` lines (56, plus `routine.create`); `store::tests`, `tests_layouts` and
  `theseusd/tests/versions.rs` expect format 34 where they said 33 (and 35 as the "newer" store).
- `tests_tiering::small_window` 41,100 to 41,700, `tests_route_keep::WINDOW` 42,200 to 42,800 and `BILLED` 36,700 to
  37,300: configuration of the test's model, as the same constants moved for `wake.cancel` and `wake.at` before.
- `tests_explain`'s input for `routine.create` (its schedule must read).

## Design choices the owner should hear about

1. **A run of a routine notifies at its limit; the month is held at the fire.** Pinned-month plus hard per-run limits made
   an unanswered budget question the failure mode. Now (`Kernel::overdraws`) a run goes out past its $0.50 as any task of an
   unpinned parent does, with the usual one notice and the hard stop at 10 times its limit ($5). The month is enforced where
   the run is carved: short of a run's whole budget, the occurrence is skipped (`wake.skipped { why: budget }`) and the owner
   told once a month. Consequence: a month can overrun by one run's overspend. If the owner wants a hard $0.50, the choice is
   a stalled run plus an overlap skip until someone answers: say so and I would do it the other way.
2. **Catch-up** `auto` is by the schedule's shortest gap: an hour or more runs once ("N missed" is the occurrences passed
   over after the one that runs); under an hour it waits for the next. Lateness without downtime (the scan late by a whole
   period) counts as missed too.
3. **The routine's record is a META key** (`routine:<session>`), not a field of the session record: no second layout
   change. `Repeat.cron` is the only stored field, hence the bump.
4. **Owner verbs are `judge_act`-gated** (owner, private place) and in the CLI's `OPERATORS`; the wake verbs are anyone's.
   A flood of routines is bounded by `[routines] max_routines`.
5. The model's tool adds about 600 tokens to every request's tools block.
6. Speaking: the routine's reports go where a task's do (`TaskOf.target` = the owner's DM, `none`, or a place).
   With no DM bound (this VM) a `dm` routine's runs only log.

## What is left or uncertain

- **Not honoured: `TaskRun.tools`** (stored; the card says a run asks before tools). The run's task is gated as any task.
- **No `paused` skip row**: a paused wake is never due, so nothing is skipped; `wake.paused`/`resumed` rows say it.
- **Visibility is rows, not facts**: `wake.fired`, `wake.skipped`, `budget.reset { why: "month" }`, `routine.changed`
  are ledger rows (the kernel's and a direct `LedgerRow`), with a `tracing::info!` per fire. There is no `fact/` type, no
  push notification, no narrative line and no metric for a routine yet.
- **No kernel-sim operation, no cockpit page** for routines.
- `wake.cancel` of a routine's wake (anyone's verb) leaves the record with no schedule: `routine.update` then says no
  wake is pending; `routine rm` still works.
- Runs have no task-graph record (`task.get`); `task.list` and `routine.runs` show them.
- The cron `every` is its shortest gap over the next dozen times (used for the floor and for `auto`).
- One verification I could not make: the `dm` path on a real Discord DM.

## Docs to change (I did not touch them)

- `docs/spec` Part III: the HZ4 item (what is above); its version line; `docs/status.md`: "Updated", the routines row.
- `docs/design/roadmap-v2.md` / the horizon rows: HZ4 done, `routine-trust` next (reads `TaskRun.tools`).
- `docs/technical-overview.md`: one paragraph on routines. `scripts/long-files.txt`: no change needed.
