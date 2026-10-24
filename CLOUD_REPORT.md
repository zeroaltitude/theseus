# Cloud report: ops-timeline (theseus-jid7t.5)

Branch `cloud/20261023-ops-timeline`, from `integ/ops` at `d01f1a21`. Started 2026-10-23 23:35 UTC.

**Store format: no bump.** This row is cockpit only: no Rust, no protocol type, nothing in `src/protocol.gen/`, no new
dependency (`package.json` and both lock files are untouched).

Commits:
- `5ae3935` cockpit: the Operations page's Timeline lens, with the calendar layer, redact mode and drag with Undo
- `8d72532` cockpit: the Timeline lens's screenshots (`cloud-report/ops-timeline/`, drop at the merge)
- `8af0eae` cockpit: the clash sweep's timing test takes the best of five runs (a flake under load, below)
- this report (drop at the merge)

## What I built, item by item

1. **The calendar read**: `cockpit/src/ops/calendarwatch.ts`, `useCalendar(spans)` and the page's
   `useOpsCalendar(timeline, weeks, day, now)`. It asks `calendar.window` for `readSpans` (in `ops/timeline.ts`): in
   the Timeline lens the week shown (or a day either side of a Day zoom), plus today and tomorrow for Coming up, merged
   into one span when they meet; in any other lens only today and tomorrow. Spans are local midnights, so they change
   only when the day or the window does. It reads again on `calendar.changed` (and `events.lost`), debounced 300 ms,
   and every 5 minutes while `document.visibilityState === 'visible'`. That interval is the one timer: it is cleared
   when the tab hides (a return reads once at once) and on unmount. A refusal or error becomes the lens's banner
   ("the calendar could not be read: …, Theseus's own marks still draw") and drops the old events, so a stale week is
   never shown as current. The page test's no-poll check on `Ops.tsx` and the `.tsx` files is unchanged; the timer is
   in a `.ts` file, and `test/ops-calendar.test.ts` holds it to one `setInterval`, gated on visibility.
2. **The layout v2**: `cockpit/src/ops/timeline.ts`, beside `lib/agenda.ts` (whose window, marks and `wallHour` it
   reuses): `eventLayout` (blocks by wall-clock hour, cut at each midnight, greedy side-by-side lanes per overlap
   cluster, an all-day band with Google's exclusive end), `ranBars` (routine runs from `routine.runs`, tasks and jobs
   from the board's views, by outcome, running ones to now), `nightFold` with `yOf`/`hourAt`/`heightOf`, `zoomed`,
   `readSpans`. DST: a 23- and a 25-hour day keep wall-clock rows (09:00 sits at 09:00), tested in America/New_York.
3. **Clashes**: `clashes(marks, events)`: a reminder, check-in, routine run due or deferred start inside a meeting,
   and two meetings that overlap (busy blocks count); all-day events and events the owner declined never clash. Each
   pair counts once; `why` says what it clashes with (the tooltip). Sorted sweeps. `meetingCount` is the window's timed,
   not-declined meetings.
4. **The lens**: `cockpit/src/ops/TimelineLens.tsx` (the Agenda's page, **moved**: `views/Agenda.tsx` is deleted, its
   reads, panels and `Detail`, now `MarkDetail` through `useOpsAct`/`ActBtn`), drawing `cockpit/src/ops/WeekStripV2.tsx`.
   It holds the legend (each calendar's label, material swatch, lock, state in a state tone, "how to fix it" below), the
   Agenda's glyph legend and its `agenda.more` warning in the same words, "This week · N items" (marks and events of the
   window; "· Sat 10/24" added in Day zoom), the chart/table toggle (`?table=ops.timeline`), the hover line in the
   panel's header, the banners, and under the strip the Routines, Watches, Projects and Wakes panels, the same
   components unchanged. `Ops.tsx` mounts it in the `a.lens === 'timeline'` branch, plus the handler rows, two
   contexts, the selection, Coming up's events, and the counters (370 lines, from 345).
   - The **Selected panel**: a mark's facts and verbs under the strip, exactly as the Agenda's (A5). A calendar event's
     panel is the inspector's new `Selection` kind `event` (`cockpit/src/ops/EventDetail.tsx`, one case in
     `Inspector.tsx`); the strip says where it is. I put the event in the inspector, not under the strip, because the
     mockup shows it there with its privacy rule and verbs; say if you want both in one place.
   - **Day zoom**: `?day=0..6` (a key the address does not own, so every link keeps it: no `address.ts` change).
5. **The materials**: the block is the calendar's own `color` (the daemon's palette of hull materials), at 30 %; a
   sensitive calendar's is a 135° hatch of its colour with a lock glyph; a free/busy block is a wider hatch, labelled
   "busy" in italics; a declined event is dimmed with a dashed outline. No animation anywhere in the lens; the now line
   is a still cyan line with a soft glow (Calm has nothing to stop). `hatch()` is in `ops/calendar.ts`.
6. **Redact mode**: `redactEvent(ev, cal, on)` drops a sensitive calendar's title (a bar `▬▬▬▬▬▬`), place, call link
   and attendee count, and keeps key, times, all-day, busy, response, organizer and Google's link (as an href only).
   `eventFacts` makes every string the lens draws for an event from it: the block label, tooltip, header line, the
   Selected facts, Coming up's line, the table row and the search text. `⇧R` toggles `redact` in the address (it was
   already parsed and carried); the page never changes it by itself. A "REDACTED" chip shows in the strip's header.
7. **The states**: `stateOf(cal)` → `{ tone, words, fix, draws }` for all six statuses (ok; busy_only: hatched busy
   blocks; not_found: wait tone, "not shared yet", a dashed lane per day, the sharing steps in the page's words;
   forbidden, auth_lapsed, off: fault tone, its one fix, no events drawn). The daemon's `detail` is said first.
8. **The event verbs**: `eventVerbPayload(ev, cal, verb)`; see the data-routing review. Remind me, Start a prep task
   and Ask Theseus go through `useOpsAct` (off under the time machine); the links are plain anchors with
   `target="_blank" rel="noopener noreferrer"`.
9. **Drag with Undo**: `cockpit/src/ops/drag.ts` `dropOf(mark | {event}, at, past, now)` → `{ method, params, words,
   undo(result) }` or `{ refused }`; `dropAt` and `snap` (5 minutes of the local clock; a skipped spring-forward time
   lands an hour on, as the clock does). A reminder or check-in: `wake.snooze { wake, when: <ISO instant> }`, Undo the
   time before; a deferred start: `task.reschedule { task, start_at_ms }`, Undo its `previous`. `ops/reschedule.ts`
   declares the shape with the comment line `// ops-verbs: use @protocol's TaskRescheduleParams/Result at the join`.
   The strip uses native drag and drop; the cursor label says what the drop does or why it is refused; the toast
   carries Undo for 10 s. `useOpsAct` gained `send` (the same `enabled` gate) because the Undo needs the result; a
   refusal shows in the daemon's words (`ops/refusal.ts` `wordsOf`: the client rejects with `{ code, message }`, not an
   Error).
10. **Coming up**: `comingEvents(cals, now, redact)` joins the calendar's timed events in the next 3 hours (not
    declined), each line redacted as the page is; `Queue.tsx`'s `ComingUp` merges them with the marks by time.
11. **Parity**: below.
12. **The Bridge's Wakes tile** goes to `/ops?lens=timeline` and says "on the Timeline".
13. **`cockpit/AGENTS.md`**: a "Timeline lens" paragraph in the Operations section (the layers, the materials rule, the
    states, redact mode and its data rules, the one timer, drag), and the Agenda's and lenses' lines updated for the
    move. The `agenda.week` explainer now describes the calendar layer.
14. **Screenshots**: below.

## Data-routing review

For an event of a **sensitive** calendar (Work 🔒) and of an **open** one (Personal):

| verb / surface | sensitive | open | pinned by |
|---|---|---|---|
| Remind me (`wake.create`) | `when` = 10 min before, as an instant; `note` = "Coming up: the Work event at 14:00 on Fri 10/23." | note quotes the title and time | `ops-calendar.test.ts` "a sensitive event's verbs…" (scans every payload string) |
| Start a prep task | the New task sheet: title "Prep: the Work event at 14:00", brief names the calendar and time and says "read it with the calendar tool"; `usOnly` | title and time quoted; no place or link | same test; the sheet's rule in "the New task sheet…" |
| Ask Theseus | `session.open { label: "about the Work event" }`, then `turn.submit` with the time and calendar only, and a US-origin profile; refused if none is known | quotes title and time | same test; source check of `EventDetail.tsx` |
| Join the call | opens `call` in a new tab, nothing to the daemon; **absent under redact** (the panel takes it from the redacted event) | opens the call | payload test; anchor count test |
| Open in Google | opens `html_link` in a new tab, nothing to the daemon (kept under redact as an href, never drawn as text) | same | payload test |
| block, tooltip, header line, Selected facts, Coming up, table row, search | under redact: bar, time, calendar + lock only; search matches only the calendar's label | everything | "redact mode: no surface…", "redact mode reaches Coming up and the search", the table test in `ops-timeline.test.ts` |
| storage, logs, address | none: no `localStorage`/`sessionStorage`/IndexedDB/`usePref`/`console.*` in the lens's files; the address holds `sel=ev:<opaque key>` only | same | "nothing is stored, logged or put in the address…" |

The New task sheet (`ops/NewTask.tsx`): a profile select (read once from `profile.list`). Under `usOnly` only a profile
with `origin === 'us'` is enabled, the rest greyed with "this event's calendar is sensitive: US models only"; the live
profile is picked when it is of US origin, else the first that is; with none, nothing is picked and Start refuses
(fails closed). `ProfileInfo.origin` is ops-verbs' field and **not on this base**, so it is read through a cast
(`originOf`); until ops-verbs joins, every profile is of unknown origin and a sensitive event's prep task and Ask
Theseus refuse, saying so. That is fail-closed by design; at the join it starts working with no change. An open event's
sheet offers every profile with "Parent's model" first, as before.

Unsure, for the reviewer:
- Ask Theseus is two calls (`session.open`, then `turn.submit`), the draft deck's own path; the brief's "a web act is
  one existing protocol call" is about task changes, and no single call opens a chat with a first message. Say if it
  should instead open `/session/new` empty.
- Under redact, Open in Google is still offered (its `eid` encodes the event and calendar ids, but nothing is drawn).
  Dropping it under redact is a one-line change in `EventDetail.tsx` if you prefer.
- The open calendar's verbs quote the title in a wake note and a session label, which are stored and listed. The
  design allows it for a calendar that is not sensitive.

## Parity rows moved

`ops/parity.ts` (`LENS` = `src/ops/TimelineLens.tsx`, `STRIP` = `src/ops/WeekStripV2.tsx`):
A1 → Ops' `<TimelineLens weeks={a.w}`, LENS `<WeekStripV2`, `weekWindow(now, weeks)`, STRIP `d.hours !== 24`, the now
line, `>Today</button>`; A2 → LENS `allMarks(`, STRIP `{glyph(m)}`, `data-mark="watch"`; A3 → LENS
`onHover={setHover}`, STRIP's `markLine(m)` hover; A4 → LENS `more={agenda?.more}` and the warning's words; A5 → LENS
`<MarkDetail mark={picked}` and `verbsFor(mark)`; A6 `This week ·`; A7 `<RoutinesTable`; A8 `<WatchList`; A9
`<ProjectList`; A10 `<Wakes`; A11 `the agenda could not be read`; A12 `useFuture(win.from_ms, win.to_ms, held)`.
B16 → `here`: KEYS `mark_next`, `mark_snooze`, `mark_open`, and `timelinekeys.ts`'s `horizonVerb(mark, k)`. Every
other row is unchanged; the parity test is green. Its test "the Timeline lens hosts the Agenda unchanged, and the
Graph lens the task graph" keeps its name (renaming it reads as a removed test to the keel guard) and now asserts the
lens's import and that `views/Agenda.tsx` is gone (moved, not copied); its title is stale and could be renamed at the
join with an ack.

## Keys added (one block of `KEYS`, marked `ops-timeline`)

Contexts `timeline` (the lens, with the canvas focused) and `mark` (a mark of the lens selected), placed in `PRIORITY`
as `… job, mark, wake, card, timeline, page, filter`.
- `d` / `w` (timeline): Day / Week zoom. **Conflict** with the counter filters `d`/`w`: the lens is more specific and
  wins, as `r`/`d` on a failure do; elsewhere they still filter, and a failure's `d` still dismisses in the lens. The
  overlay says so.
- `R` (`⇧R`, timeline): redact. `,` / `.`: the previous / next week; **in Day zoom a day** (across the week's edge), my
  choice, said in the overlay.
- `j` / `k`, `↓` / `↑` (timeline): the next / previous mark or event by time. **Conflict** with the page's `down`/`up`:
  in the lens they step the strip; with a queue item selected the context is off and they move the queue.
- On a mark: `z` snooze 10 m, `p` pause/resume, `r` run now (**conflict** with the paused filter: the mark wins), `x`
  cancel (asks; the wake context's `x` never co-occurs), `⏎` open its session (**conflict** with the card's inspect:
  the mark wins). A key the mark has no verb for says so in the flash.
- Button acts (off under the time machine): `event_remind`, `event_prep`, `event_ask`, `drag`, `undo`.

## Tests and planted reverts

New: `test/ops-calendar.test.ts` (9), `test/ops-timeline.test.ts` (12), `test/ops-drag.test.ts` (5), one in
`test/ops-keys.test.ts`; fixtures `test/ops-calendar-fixtures.ts` (`personal.cal.example.test`,
`work.cal.example.test`, invented titles). `npm test`: 594 pass, 0 fail, in UTC and in America/Phoenix (569 before).
`npm run lint`: no finding in the files touched. `npm run build`: ok.

Each plant made the named test fail with the message shown, then the file was restored and `touch`ed; `npm test` was
green after:

| plant | test that failed | its failing assertion |
|---|---|---|
| `redactEvent` never redacts | redact mode: no surface draws… (+2) | "nothing of the work event under redact" |
| a verb names a sensitive title | a sensitive event's verbs carry only… | "remind carries nothing of the event" |
| search text keeps the raw title | redact mode… and …Coming up and the search | "nothing of the work event under redact" |
| Coming up ignores redact | redact mode reaches Coming up and the search | deep-equal of the hits (non-empty) |
| `off` draws its events | the four states… | "off" (draws) |
| US-only greys nothing | the New task sheet… | "not of US origin, or of none known" |
| no lanes | events lay out by wall-clock hour… | "three at once: three lanes" |
| declined meetings clash | clashes: … | "all-day and declined events never clash" |
| series ticks hold the night open | the night folds 22:00 to 07:00… | "two runs at 03:00: folded, and counted" |
| a span sent with its `days` | the Day zoom… | "ten weeks on: two spans" |
| a drop sends local `toString()` | a reminder or check-in moves its wake… | "Phoenix is UTC−7 all year" |
| Undo ignores `previous` | a deferred start moves by task.reschedule… | "the daemon's word for the time before" |
| an event's drag not refused | everything else is refused… | regex /never writes to Google Calendar/ |
| the counters beat the lens (`PRIORITY`) | the Timeline's keys… | strict equal (`zoom_day` vs `filter_done`) |
| a refusal said as `[object Object]` | a refusal is said in the daemon's words… | strict equal |

Two of these were real bugs the tests caught while I wrote them, fixed before the commit: `readSpans` sent the whole
window object (with its `days`) as `calendar.window`'s params for a week far from today, and an RPC refusal would have
reached the toast and banner as "[object Object]".

**Under load** (AGENTS.md's recipe: four `while :; do :; done` at nice 0, `nice -n 19 npm test`, loops killed by pid):
the full cockpit suite three times at `5ae3935`: runs 1 and 2 failed only my new "the clash sweep is quick… under
20 ms" (one 20 ms window preempted), run 3 passed 594/594. `8af0eae` makes that test the best of five timed sweeps
(the bound unchanged). After it: five loaded runs of the lens's seven test files, 55/55 each, and one loaded full
run, 594/594. No other test failed under load.

## Screenshots

`cloud-report/ops-timeline/` (1440×900, dark theme, headless Chromium from the pre-installed Playwright, nothing added
to `package.json`), against a scratch daemon of this build on a fresh state dir (`/home/user/scratch`, web on
127.0.0.1:7434, a stand-in model key, `op` stand-in, Discord, index and judge off) with five invented reminders
("Water the kelp seedlings" 14:10 today, "Call the boatyard about the mooring", "Stretch break", "Pick up the sailcloth
order", a daily "Morning tide table").
- `off.png`: `/ops?lens=timeline&redact=1`, the daemon's own answer: no `[google]`, so `primary` is "Google is off"
  in the fault tone with its fix; Theseus's marks still draw.
- `ok.png`, `ok-open-selected.png` (`&sel=ev%3Ap1`), `ok-sensitive-selected.png` (`&sel=ev%3Aw1`), `ok-day.png`
  (`&day=0`), `ok-table.png` (`&table=ops.timeline`), `not_found.png` (fix expanded): `calendar.window` is answered by
  a **page-level stand-in** (Playwright's WebSocket route answers that one method with two invented calendars,
  Personal `#c9a24a` open and Work `#5fa08d` sensitive; every other call reaches the daemon). The core's test fake
  (`theseus-google`'s `fake.rs`) is a library, not a standalone server, so I did not use it; the shots show the page
  drawing the protocol's shapes, not the daemon reading Google.
- Every address carries `redact=1`; the Work calendar's titles are bars in every shot, and the open calendar's
  invented titles show. No real name, address, calendar id or title appears.

## The live check for the maintainer

On the owner's machine, with ops-verbs joined and the owner's `[google] calendars` set (a personal calendar, and a work
one with `sensitive = true`):
1. `scripts/build.sh` and a scratch daemon on a copy of the store, its web on 7435; open
   `http://127.0.0.1:7435/ops?lens=timeline&redact=1`. Expect: both calendars in the legend with "✓ read", Work hatched
   with a lock and its titles as bars; ◆ on overlaps; the two counters in the command bar.
2. Press `⇧R`: the address loses `redact=1` and Work's titles show; press it again. Search a Work title under redact:
   no match.
3. Select a Work meeting: the inspector shows the privacy note and the US-only chip. Press **Remind me**, then
   `theseus wake list --socket <scratch>`: the note reads "Coming up: the Work event at HH:MM on …", no title.
4. **Start a prep task**: only US-origin profiles are enabled; start it; `theseus task show <id>`: the brief names only
   the calendar and the time. **Ask Theseus**: the new session's first message likewise; its first `calendar.events`
   read marks it sensitive (health / the session's sensitive mark).
5. Drag a reminder to another day: the toast says "moved to …" with Undo; `theseus wake list` shows the new time;
   Undo puts back the exact old one. Drag a deferred task's ▷: `task.reschedule` moves it; Undo uses `previous`.
   Drag a meeting or a routine tick: the cursor says why not.
6. Leave the page open and idle 15 minutes with the browser's network panel: one `calendar.window` every 5 minutes
   while visible and none while the tab is hidden; nothing else.
7. With Work unshared (or a wrong id): its legend says "not shared yet" in the wait tone with the sharing steps and a
   dashed lane; with `[google]` off: "Google is off" in the fault tone.
8. `crates/theseus-discord/src/golden/dm_flow.txt` is untouched by this branch (`git diff d01f1a21 -- crates/` is empty).

## Keel guard and the gate

`python3 scripts/keel-guard.py --base d01f1a21 --head HEAD --quiet`:

```
```

(empty: no finding.)

The gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, with `THESEUS_KEEL_BASE=d01f1a21`), run on
the tree of `5ae3935` less two legend tweaks made during its suite:
- door, keel (0 findings), keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build, reader rule:
  pass.
- suite: 8077 run, 8043 passed, **34 failed, all the known L1 reds** (theseus-pv6i: the VM runs as root): 20 in
  theseus-sandbox (the 19 `contract` tests and `bench spawn_100`) and 14 in `theseusd::sandbox`. No other failure; the
  two named load-sensitive tests (`a_filtered_page_equals_the_scans_answer`, `python3s_repl_computes_on_the_screen`)
  passed.
- After the suite, by hand: protocol types ok (no change), `theseus-sim bench turn --check --runs 5 --burst 0` ok,
  size skipped (no release build, as the step says), `cargo deny --offline check`: advisories, bans, licenses,
  sources ok.
- After the legend tweaks: `npm run lint`, `npm test` (594/594), `npm run build`, `scripts/shape.sh` and the keel
  guard again, all clean.

**The disk**: this session's disk allowance could not hold a full debug build with debug info (the setup's
`cargo build --workspace --all-targets` failed twice with "No space left on device"). I deleted `target/` and ran the
gate with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` set in its environment, which
changes no code and no gate step. The maintainer's gate on the owner's machine is the full one.

## For `docs/status.md`

"The Operations page's Timeline lens (theseus-jid7t.5): the week strip v2 draws both of the owner's Google calendars
in their own materials beside Theseus's marks, a sensitive calendar hatched with a lock and masked by redact mode
(`?redact=1`, ⇧R) on every surface; clashes, a Day zoom, each calendar's state with its fix, event verbs that send a
sensitive event's time and calendar only, and drag with Undo for reminders and deferred starts. The Agenda page moved
into the lens whole."

## Follow-ups

- At the join with ops-verbs: replace `ops/reschedule.ts` with `@protocol`'s `TaskRescheduleParams/Result`, and drop
  `originOf`'s cast for `ProfileInfo.origin`.
- ops-act rebuilds the New task sheet: keep `profileChoices`' rule and the `usOnly` prop.
- Spec / design docs to update at review: the Timeline lens's paragraph of the Operations design (the `?day=` key,
  the night fold's rule that series ticks are counted rather than holding the night open, `,`/`.` moving a day in Day
  zoom, the event panel in the inspector).
- The fold's open/closed state is per view (not in the address); the Agenda's old `WeekStrip.tsx` is now unused but
  kept (agenda.test reads it); it can go in a later cleanup.
