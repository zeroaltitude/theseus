# CLOUD_REPORT: horizon-cockpit (HZ9, theseus-p17fd)

Branch `cloud/20261013-horizon-cockpit`. Cockpit only: no daemon, protocol, config or store change (no store format bump,
no new config key, no new dependency; `package-lock.json` and `Cargo.lock` untouched).

## What I found

`horizon-board` had joined (`WorkKind::Routine/Watch/Project`, `RoutineView`, `WatchView`, `ProjectView`, `work.agenda`,
the regenerated TypeScript). Three facts shaped the build:

- `work.agenda` answers **future lines only**, at most 200 (`AGENDA_MAX`). Past runs come from `routine.runs`
  (`RunRow`), which has no "skipped" state: a `cancelled` run is drawn as skipped (–), `failed` and `blocked` as ✗.
- A routine every 5 minutes (the shortest the daemon allows) fills those 200 lines in under a day, so everything later
  in the week (a reminder, a milestone) vanished from the strip. See "Design choices".
- The projects row has not filled `ProjectView` yet (nothing creates a project), so the live check has **no project**; the
  list, its progress bar and its next-milestone line are covered by a pure test only.

## What I changed

| commit | what |
|---|---|
| `76bc6bd1` | the Agenda page: `views/Agenda.tsx`, `components/{WeekStrip,RoutinesTable,FutureLists,Wakes}.tsx`, `lib/{agenda,agendawatch,agendawords}.ts`, `test/agenda.test.ts`; route `/agenda`, rail item in group Live, key `g d`, five `?` explainers (`agenda.week/routines/watches/projects/wakes`, `actions.wakes` reworded); the Wakes panel left Actions (a link stays), the Bridge's "Wakes due" tile opens the page |
| `b13ca9e2` | the page reads on past the daemon's 200-line cap (at most 12 reads, only when the answer says `more`); the header range is dated; a test over a 450-line stream |
| (next) | `cockpit/AGENTS.md` names the page and its reads |
| (last) | this report and its three screenshots (drop them at the merge) |

What each part does:

1. **Week strip.** Seven local days from midnight, 24 wall-clock hours down each column, today in gold, a red now-line.
   Reminder ●, check-in ○, routine run due │, past run ✓ ✗ – (and ◐ running) coloured by outcome, deferred task start ▷,
   milestone ⚑, watch ▬ as a bar from the time it was set to the time it gives up (cut to each day). Hover names it in the
   panel header and the tooltip; a click selects it (`?sel=`) and shows its verbs: reminders and check-ins Snooze 10m / 1h /
   tomorrow 9:00 and Move… (typed `when`), all `wake.snooze`, Pause/Resume (`wake.pause`/`wake.resume`), Cancel (`wake.cancel`);
   routines Run now / Pause / Resume / Cancel (`routine.run_now|pause|resume|delete`); watches Cancel (`wake.cancel`; nothing
   edits a watch); deferred tasks Cancel (`task.cancel`); milestones and past runs none (a past run's **Open** opens its
   session). Each verb is one call, confirmed first where it destroys. The week is in the address (`?w=`).
2. **Routines table.** Per routine: schedule, next run (or "held"), last run (outcome, duration, cost, its line, age), the
   30-day record (✓ ✗ skipped), month spend of limit with a bar, a sparkline of the last 30 runs' cost, Run now / Pause or
   Resume / Cancel; the arrow opens the last 30 runs (run number linking to its session, age, outcome, duration, cost,
   report or block reason, and a tally).
3. **Watches and projects**, two compact lists: a watch's check, condition, last value and age, check count, give-up time and
   failing reason; a project's goal, step progress bar, milestones, next milestone with date, overdue count and review.
4. **Wakes moved.** `components/Wakes.tsx` shows every field the old panel did (note, short id, session title or task, state
   pill, local due time, "in 4m"/"due 2m ago", the tooltip's wake id / set time / reply target) and Cancel, plus the series
   and Pause/Resume. Actions keeps a "Wakes" panel that is one link to `/agenda` with the count and next due time.
   `wakeFacts` is the one place the fields are worked out; a test holds them.

**Reads** (`lib/agendawatch.ts`): `work.agenda` + `wake.list` once per window change and again only when `work.changed`
carries a wake, routine, watch, project or a view with a start time (`touchesFuture`), debounced 300 ms; the board's views
come from the one `work.watch` the Threads views already keep; `routine.runs` once per routine and again only when its view's
signature (state, last run, counts, month spend) changes. No `useRpc`, no `refetchInterval`, no interval. The one timer is a
60 s clock tick that moves the now-line and "in 4m" (a state change, no call).

## How I proved it

- `npm test` in `cockpit/`: **427 pass, 0 fail** (it was 426 before the paging test; 11 new tests in `test/agenda.test.ts`,
  which the earlier 415 do not include). `npm run lint`: exit 0 (the warnings are the tree's; none in the new files).
  `npx tsc -b` and `npm run build`: clean.
- Tests (all in `test/agenda.test.ts`; node's runner has no DOM, so "component tests" are tests of the pure layout and
  of the source files, as the cockpit's other tests are): the window (7 days, today marked, hours per day); a **DST week**
  in `America/New_York` (spring-forward week = 167 h with a 23 h day, fall-back week with a 25 h day, and a 09:00 reminder
  at y = 9/24 on every day of both); an item of each kind at its day and hour with its glyph, tone and lanes; the watch bar
  across days and the watch's own `expires` line drawn once; past runs by outcome; each kind's verbs, methods and params
  (including Snooze presets, Move prompting, Pause vs Resume, Cancel asking); the table (45 runs → the newest 30, 30
  sparkline points left-to-right, flat line mid-height, rows soonest first, signature changes only on a new run); watch and
  project rows; the moved panel's fields; the page on the rail, in a group, behind a route, with explainers; the idle test;
  200 items laid out in **median 0.22 ms, worst 6.7 ms** (limit 16 ms); and paging (450 lines in 3 reads, no line twice,
  in order; an endless stream stops at the limit and says it is cut).
- **Planted reverts**, each restored and `touch`ed, `git status` clean after:
  1. a 5 s `setInterval` poll added to `agendawatch.ts` → `the page idles: no timer of its own that asks…` **failed**;
  2. the wake's "its reply goes to <target>" dropped from `wakeFacts` → `the Wakes panel moved here loses no field…`
     **failed**;
  3. the paging dedupe removed → `a busy series that fills the daemon's cap…` **failed**.
  All three pass again restored.
- Also run by hand, as the gate's later phases: `cargo fmt --all -- --check` ok; `scripts/shape.sh` ok; the cockpit phase
  (lint, test, build, built) ok; `cargo test -p theseusd --test web_ui` 3 passed (it reads the cockpit's build at `/`).

### Live check on this VM (done; captured)

A scratch daemon (fresh state dir `/tmp/hz/state`, socket `/tmp/hz/sock`, web 7435, `theseus-sim fake-model` as the model,
judge and memory off) with: three reminders (`in 2h`, `tomorrow 9:00` daily, `fri 14:00`), a routine `every 5m` ("Inbox
sweep", run now four times: 4 runs, $0.029 each), a routine `weekdays 08:00`, a file watch (`--until exists`, every 1m), and
a deferred task (`fri 9:00`). Headless Chromium (Playwright, `/opt/pw-browsers`) on `/agenda`:

- Screenshots: `CLOUD_REPORT-agenda.png` (the first run, capped at 200: the Fri-onward week empty, which led to the paging
  fix), `CLOUD_REPORT-agenda-runs.png` (the Routines table with the last-runs row open) and
  `CLOUD_REPORT-agenda-detail.png` (after the fix: 1,870 items over the whole week, a reminder selected with its verbs; ▷ the
  deferred task, the purple bar the watch). No console or page errors.
- **Idle, 30 s, after load: 0 requests from the page** (`work.agenda`, `wake.list`, `routine.runs`, `work.watch`); the shell's
  own reads (`health` every 2 s, `ledger.tail`, `profile.list`) ran as on every page: `{"ledger.tail":39,"health":15,"profile.list":3}`.
- **A verb**: clicking Snooze 10m sent exactly one `wake.snooze {wake, when:"10m"}`; the strip moved the reminder from
  15:36 to 13:50 through the push (`work.changed` → one debounced `work.agenda` read, plus its pages). The `confirm.list`/
  `session.list`/`execution.list` reads that follow are `useAct`'s existing `invalidateQueries`, as for every verb in the cockpit.

The maintainer's check, on the install build: start a scratch daemon on a copy of a store, `theseus remind 2h "x"`,
`theseus routine add "every 5m" "y"`, `theseus routine run <id>` twice; open `http://127.0.0.1:<port>/agenda`; expect the dot
at its hour, ✓ ticks for the two runs at the now-line, the routine's row with $0.0x, a sparkline of two points, and, after
`theseus snooze <id> 1h`, the dot moving without a reload.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **fails in its first phases here and cannot pass on this VM**:
the door judged this tree with main's copy, and the keel phase failed on the branch's *base*, not on my diff:

```
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/perf/counted.rs — …` on …: its signature does not verify (%G? is E)
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — …` on 1c2beb2747: its signature does not verify (%G? is E)
keel: 1 erosions of a test, a budget or a ceiling (a branch: merge-base(HEAD, origin/main)..HEAD, 9fa84735c0):
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
```

The two acks are on commits that came with the branch; this VM has no keyring to verify the owner's signature, and my own
commits touch no Rust, test, budget or ceiling. So I ran the later phases myself (above): fmt, shape, cockpit (lint, test, build),
the web_ui suite test. **Not run:** `clippy`, the full nextest suite, the benches and `deny` (no Rust changed; the
VM's disk allowance filled once during a full `nextest` build, so I ran only the one test binary that reads the cockpit
build). The maintainer's gate on the owner's machine runs them.

**Keel findings expected:** none from my commits. (`python3 scripts/keel-guard.py` prints the finding above, from the base.)

Known-on-this-VM failures: none hit (I did not run the L1 or timing tests).

## Design choices and what is left

- **Paging past the cap** (the one departure from "one `work.agenda` read per window change"). Without it a 5-minute routine
  hides the rest of the week, which the live check showed at once. The page reads on from the last line's time only when the
  answer says `more`, at most 12 reads (a week of a 5-minute routine is ~10); it is still one read for any window the cap
  does not fill. If the owner prefers one read, a per-routine "run due" tick (one per hour with a count) or a cap raised in
  the daemon would be the alternatives; I did not touch the daemon.
- A 5-minute routine draws a dense barcode of ticks (12 an hour, in lanes). It is faithful; collapsing them into one tick
  per hour with a count is an easy follow-up if it reads badly.
- The page shows the present only (no time-machine fold): it says nothing about the past but the routines' last 30 runs.
- "skipped" in the strip is a cancelled run: `RunRow.state` has no skipped value (the 30-day *count* has one, so the table's
  record shows it).
- Pause/Resume also appear on the wakes panel (new, beside Cancel), hidden for watch and routine wakes, which the daemon
  refuses to pause as wakes.
- Docs to change at review: spec Part III (a new item for HZ9), `docs/status.md`'s cockpit row, the `?`-explainer count;
  `docs/design/roadmap-v2.md`'s HZ9 row. `cockpit/AGENTS.md` is already updated.
- Disk: the VM's allowance filled mid-run; I deleted `target/debug/incremental` and `/tmp/theseus-gate-door-*` (both rebuildable,
  inside the repo or /tmp).
