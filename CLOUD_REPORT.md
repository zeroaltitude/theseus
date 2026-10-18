# CLOUD_REPORT: cockpit-tasks (theseus-ypnxt.6)

Branch `cloud/20261017-cockpit-tasks`, on `40aa83cd`. TypeScript in `cockpit/` only; no Rust, store, protocol or dependency change.

## What changed
- `cockpit/src/lib/progressLine.ts` (new, cb083514): `progressLine(view, nowMs)` and `lineUsd` (the Rust `usd`'s words), contract C1.
- `cockpit/src/lib/taskboard.ts` (new): the rules, pure: `rowsOf` (board task views joined to `task.list` records), `split` into needs/working/done, `matches` (search), `workingCount`, `placeName`, `noticeTarget`, `reportLineOf`.
- `cockpit/src/views/Tasks.tsx` (new, 2c24dd63, 4th commit): the `/tasks` page. New task box (brief, title, budget, home picker; `task.start`; scrolls to and highlights the new row; refusal shown in the daemon's words), search, Needs you (ConfirmCard per open question), Working (Pause/Resume, Stop, open · join), Done (report line, `$`, 50 at a time with "more"). Live from the shared `useThreadBoard` work.watch plus push-kept `task.list` and `confirm.list`: no timer.
- Wiring: `main.tsx` route `tasks`; `Shell.tsx` NAV item Tasks and `g t`; `navgroups.ts` (Live group, after Fleet); `explainers.ts` (`tasks.board`, `tasks.new`).
- `Heartbeat.tsx` (the header is there, not in Shell.tsx): `TasksWorking`, "N tasks working", links to `/tasks`; the RUNNING readout and Stop menu are unchanged. Not shown in the time machine.
- `TaskHeader.tsx`: progress line, ⏸ paused pill, Pause/Resume.
- `DesktopNotices.tsx`: a click focuses the window and opens `/tasks?sel=<session>` (other work: its session). `seen` stays 0: the cockpit keeps no seen position (the chat's is a file).
- `Actions.tsx`: "reports to discord dm" with the raw target as tooltip; "Tasks →" link.
- Screenshots: `docs/assets/cockpit-tasks/*.png` (the repo keeps its screenshots in `docs/assets/`, not `cockpit/`).

## Tests and plants
`npm test`: 464 pass (was 456; +8: eight progress cases, edges, sections, record-only placement, search, header count, place name, notice target, wiring read as text, report line).
| Plant | Test that failed |
|---|---|
| count only `running` | "the header counts the tasks working…" |
| paused task to Done | "the sections…" and "search…" |
| drop `paused` from the line | "the eight cases" |
| search ignores the report line | "search runs over title and report line…" |
Each restored with `touch`; `git status` clean after. `tsc -b` clean, `oxlint` no warning in my files.

## Budgets
No Rust or prompt change: the first-request budgets and plain-turn frames are untouched. The turn bench and owner-config fixture were not run (nothing they read changed).

## Overlaps
None in other rows' files except the cockpit files listed in the row (plus `Heartbeat.tsx` and `explainers.ts`, for the header and the panels' "?" text). The CLI words `answer`/`task` are not used in new text.

## Proof nothing was lost
Actions' task rows keep every datum: state, waiting on, asks, title link, cancel, short id, parent link, `$`/limit, turns, target (now by name, raw in tooltip), wake-parent, age, ended reason, pieces, check line; Actions' tree and graph are untouched. On the new page: state, waiting on, asks (the question card), title and open link, Stop (Cancel), short id, `$` (Done rows; live rows show it in the line), age, ended reason (Done line fallback), place, wake-parent only on Actions. Not on the page (stay on Actions): parent link, turns, pieces, check line. The task page keeps its title, state, doing, cost, forecast pill, inspector, Stop.
Live check on a scratch daemon (fake model, fake `proc.run sleep 900`, `owner_ask`): page showed Needs you 1 (question with answer buttons), Working 3 (one waiting, one paused ⏸ with "paused · $0.03 of $2 · …", one running), Done 6 with report lines; header "RUNNING 1 · 3 tasks working" (before: RUNNING 0 for two waiting tasks); New task started `ecf538` and scrolled to it; the task page showed the paused pill, the line and Resume. After looking I added reading report lines from the task's session (Done rows first said only "reported") and dropped the doing line on a task that asks (it repeated the question).

## For the joiner
- Gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passed door, keel, fmt, shape, features, clippy, clippy-probe and cockpit (tsc, lint, build, tests); it failed at "test build" because `cargo-nextest` is not installed in this VM (setup not done). The Rust suite was not run; no Rust changed. Keel guard `--base origin/main` (after `git fetch origin main`): empty, rc 0.
- Point `progressLine.test.ts` at the Rust golden after progress-words joins.
- Report lines for Done rows come from `session.history` (first 50 ended tasks without a live view), so search over report lines covers those; switch to task-history's `task.list` `query`/`ended` params after it joins. The ended-task view the board no longer holds has no `report` otherwise.
- "join" and "open" are one link to the task page (its composer messages the task); no autofocus without editing Composer.
- A forecast made from tiny sim tasks reads "past its usual time" at once: correct per C1, noticeable only on the scratch daemon.
