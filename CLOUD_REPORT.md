# Cloud report: ops-shell, the cockpit's Operations page (theseus-jid7t.3)

Branch `cloud/20261023-ops-shell`, from `main` at `2d1d8d6f`. Cockpit only: no Rust, no protocol type, nothing in
`src/protocol.gen/`, no `package.json` change, no store format change (format stays 49), no line ceiling raised.
`views/Actions.tsx` and `views/SessionDeck.tsx` are untouched.

**Signing.** The VM has no GPG key (`gpg --list-secret-keys` is empty), so every commit is unsigned
(`-c commit.gpgsign=false`). Each carries the trailer the base's recent commits carry,
`Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>`, copied from `git log -3 --format=%B origin/main` as the task's
row rules say (the preamble's `Claude (cloud session, …)` line disagrees with them; I followed the row's rule).

## Commits

| commit | what |
|---|---|
| `d178885` | the pure half: `src/ops/address.ts`, `board.ts`, `queue.ts`, `facts.ts`, `keys.ts`, `regions.ts`, and their tests |
| `391d758` | the page at `/ops` (rail item, route), the queue, the Board and Threads lenses, the inspector, the parity table and test |
| `8b7bf81` | `/agenda`, `/tasks`, `/threads` redirect; the three rail items go; `g d`/`g t`; old tests' route assertions updated in place |
| `45a3417` | `cockpit/AGENTS.md`'s Operations section |
| `74c362e` | fixes found by the live check: a deep link kept on load, threads matched by slug, an `owner_ask` as a question, the idle inspector |
| `00dd881` | fix: an endless render loop that kept every address change from taking effect (found by the browser key check) |
| `4ca5e8b`, `bdb8ad5` | screenshots under `cloud-report/ops-shell/` (not for main) |

## What I built, item by item

1. **Route, rail, redirects.** `src/main.tsx`: `path: 'ops'` (route `lazy`, as `tasks` was); `tasks`, `agenda`, `threads`
   are `<ToOps />`, which navigates to `redirectOf(pathname, search)` with `replace`. `threads/:slug` is unchanged
   (`views/Threads.tsx` is now only `ThreadView`, over `ops/ThreadPanel.tsx`'s `ThreadBody`). `components/Shell.tsx`:
   `{ to: '/ops', label: 'Ops', icon: ShipWheel }` second after the Ship; Tasks, Threads and Agenda items removed;
   `GO`: `d: '/ops?lens=timeline'`, `t: '/ops?lens=board'`. `lib/navgroups.ts`: Live is `/ship /ops /bridge /fleet
   /actions`, and `/threads` (a thread's page) stays in Live through `ALSO`. The Ship stays the landing view.
   `redirectOf` is pure and tested (`test/ops-address.test.ts`). `views/Tasks.tsx` is deleted: its card is the Board's,
   its `NewTask` moved to `ops/NewTask.tsx` with every field.
2. **The address model**: `src/ops/address.ts` (`parseAddress`, `writeAddress`, `opsHref`, `redirectOf`): `lens`, `sel`,
   `f` (`key:value` chips, URI-encoded, one order), `q`, `w` (the Agenda's own `-52..52`), `join`, `redact` (`1`),
   and `do=answer`. Keys the page does not own (`task`, `table`) are kept. Round-trip test over four full addresses.
3. **Regions** (`src/ops/regions.ts`, `views/Ops.tsx`): queue | canvas | inspector; `[` and `]` fold (kept in this
   browser, `cockpit.ops.queue` / `cockpit.ops.inspector` through `usePref`); `\` reserved (a flash says so). Below
   1280 px the inspector lies over the canvas while something is selected; below 900 px the queue is the page. One
   design choice the owner should hear about: **with nothing selected the inspector is a slim tab** (`idle`), so the
   Board's five columns get the width; a selection opens it. Without this, at 1440 px only two and a half columns showed.
4. **Command bar** (`ops/CommandBar.tsx`): six counters (needs you `b`, working `w`, paused·queued `r`, still today,
   done today `d`, spent today), each a filter by click or key, each with its last hour as an inline SVG sparkline in
   its tone (sampled once a minute by the Bridge's `useHistory`, a timer that reads nothing) and a chart/table toggle
   (`?table=ops.counters`, the six counters at ten-minute steps). The needs-you counter glows while above zero and
   pulses once when it rises (`.ops-pulse-once` in `index.css`, stilled by Calm and reduced motion). Lens switch (also
   `g g/b/t/h`), chips (thread, model, place, kind, state, project, ended within; the `data` chip is a later row's),
   search over title, brief, report line and progress line with "n of m", New task (the moved form).
5. **The needs-you queue** (`ops/queue.ts` `queueOf`, `ops/Queue.tsx`): approvals, budget asks, task changes (the
   confirm, or a record's `proposal` not yet listed), questions with choices (an `owner_ask`'s confirm joined to its
   board view, so it is one card), failures not dismissed (live: the board's failed task/conversation views that still
   need you; under the time machine: executions failed/blocked/budget_exhausted), seals pending, external-text holds
   (from `session.list`'s `external_text`, live, or the world's holds), and the router's held messages
   (`thread.asked` pushes kept while the page is open, `ops/asks.ts`; answered by `thread.answer`). Ordered by
   `since` ascending, ties by key. The cursor is the item's key (`keepCursor`): a new item never moves it; when its item
   is answered, the next takes it. Compact cards (glyph and word, verb and what it does, owner, age, expiry bar); the
   cursor's item shows its answers in place; `⏎` lays out the whole `ConfirmCard`. "Coming up": the board's next three
   hours (`comingUp`). Each answer is one call, `answerOf` (pure, tested): `work.answer`, `task.retry`,
   `execution.dismiss`, `thread.seal`/`thread.seal_dismiss`, `policy.trust`, `thread.answer`.
6. **Board lens** (`ops/board.ts`, `ops/BoardLens.tsx`): `boardRows` extends `taskboard.ts`'s `rowsOf` (unchanged)
   with conversations, jobs, and wakes/routine runs due today; `columnsOf` makes Needs you (longest waiting first),
   Working, Paused·queued, Scheduled (deferred starts today, wakes and routine runs due today), Done today, then older
   fifty at a time ("more (N older)"). A failure not yet dismissed shows under Needs you (the Tasks page put it under
   Done). Cards draw `cardFacts`: every field of the old `TaskCard` plus the model chip (`moved_to`, else the
   session's profile, else its model). Keys `j/k` along a column, `h/l` across (the queue left of the first), `l`
   follows a job on a job row (opens its call in its session).
7. **Threads lens** (`ops/ThreadsLens.tsx`): the Threads page's list moved whole, drawn from `threadFacts`; title
   "N live · N need you on the board"; the loose footer with the Fleet link; the empty state's words plus **New thread**
   (`thread.new`, title and slug by prompt); "Threads then" under the time machine. A click selects the thread; the
   inspector shows `ThreadBody` (the thread page's fields, from `threadPanelFacts`) with the verbs pause, resume, done,
   reopen, seal, unseal, not now (seal_dismiss), edit (title and goal). The lens's panel reads `thread.show` once and on
   `thread.changed`; the thread's own page keeps its 5 s read as it was.
8. **Timeline and Graph lenses**: the Timeline lens renders `views/Agenda.tsx` unchanged (I did not move its body:
   `test/agenda.test.ts` reads that file for its panels, its timer and its idle reads, and stays green); under the time
   machine a line says the agenda is the present. The Graph lens renders `TaskGraphView` with a new `shown` prop (the
   one change to `components/TaskGraph.tsx`); Actions' `?taskgraph=1` is as it was.
9. **The inspector** (`ops/Inspector.tsx`): a row's card whole (model, place, thread, progress, doing, due, waits,
   outcome, report, spend, changed), its `ConfirmCard`s answered in place, and its verbs (pause/resume, stop the turn,
   stop all, cancel, cancel a wake); a queue item's full card; a thread's panel with its verbs; for a mark, a pointer to
   the Agenda's own Selected panel (which stays under the week strip).
10. **The time machine everywhere**: `/ops` is in `FOLDS` (the amber frame); the page shows Actions' banner; the board is
    built from the world's tasks and sessions, the queue from the world's confirms, holds and executions, the Threads
    lens folds `thread.*`. Every act is `useOpsAct` (`ops/act.ts`) or `ActBtn` (`ops/ActBtn.tsx`), both through
    `enabled(verb, timeMachine)` (`ops/keys.ts`); `ConfirmCard` gets `past`, so it is read-only.
11. **The key table** (`ops/keys.ts` `KEYS`): one table for the handler (`handlers: Record<Verb, …>` in `Ops.tsx`, so a
    verb with no handler does not compile, and a test reads it) and the `?` overlay (`ops/KeysOverlay.tsx`, from
    `overlay()`). See the table below.
12. **`test/ops-parity.test.ts`** over `src/ops/parity.ts`: below.
13. **`cockpit/AGENTS.md`**: an Operations section (address, regions, reads, `enabled`, key table, parity table and how to
    keep it true), and the Agenda and Threads entries say where their pages went.
14. **Screenshots**: below.

## The parity table as built

Every id of the inventory is a row (59: A1-A12, T1-T6, H1-H6, X1-X9, B1-B18, R1-R8 for herdr). Each `here`/`stays` row
names the source that renders it, and the test checks that each needle is still in its file.

| ids | home | kind |
|---|---|---|
| A1-A12 (the Agenda: strip, marks, hover line, "more", selected mark and its verbs, "This week · N", routines, watches, projects, wakes, the error line, the read discipline) | the Timeline lens hosting `views/Agenda.tsx` unchanged | here |
| T1 New task, T2 search "n of m", T3 sections with counts, T4 the card (fact fields `pill paused title href place placeTip short progress doing outcome report cost activity waitsOn open pause stop approvals question model`), T5 Done 50 at a time and lazy report lines, T6 `?sel=` scroll and glow | the Board lens, the command bar, the queue | here |
| H1 a thread's header (fields `sealed title href state warmth warmthTip needs needsCounted pausedTip details seal`, and its items), H2 the count line, H3 the loose footer, H4 "Threads then", H5 the empty state | the Threads lens | here |
| H6 `/threads/:slug` (panel fields `title state goal details context today sealPending sealedBy topics links card workingSet turnsOf`) | the page, and the inspector | stays |
| X1 approvals (`ConfirmCard`; compact card fields `glyph kind title owner age left`) | the queue, and Actions | here |
| X3 holds with Trust, X7 the task graph, X9 the banner and acts off | the queue / the Graph lens / the page; X3 and X7 also stay on Actions | here |
| X2 tool calls, X4 the wakes link (Actions' `/agenda` link, which redirects to the Timeline lens), X5 tasks panel, X6 task tree, X8 postures | Actions | stays |
| B2 filters and `/`, B3 tab, B4 `y t n 1-6 a e`, B6 `s s S c c`, B7 `N`, B8 `P`, B9 `l`, B12 `r d o`, B17 flash lines, B18 narrow screens | the key table and the page | here |
| B1 every session as a tree (every session and question is on the Board and in the queue now; the tree is the Graph lens's) | | pending: ops-graph |
| B5 (`enter` is here; `i` type to the session and `⇧tab` its mode), B10 PgUp/PgDn/End, B11 ctrl-t, `/t`, `T`, B13 `L`, B14 `p`, B15 `I` | | pending: ops-act |
| B16 `H` the Horizon | | pending: ops-timeline |
| R1 answer, R2 next, R4 stop, R5 new, R8 deep links `/ops?sel=<id>&do=answer` | the address and the key table | here |
| R3 tui, R6 main | | pending: ops-act |
| R7 `theseus://answer|history|open/<id>`, unchanged | | stays |

No row of 1a-1d is without a home. One honest gap inside "here": the router's held messages (`thread.asked`) have no
read that lists them, so the queue holds only those the push brought while the page was open (said in `ops/asks.ts`);
the spec's table names `thread.move` for them, but a held message is answered by `thread.answer` (the router's own
question); `thread.move` is for a message already placed, and I left it out.

## The key table, and the conflicts

| key | context | verb |
|---|---|---|
| `j` `↓` / `k` `↑` | anywhere | next / previous row or queue item |
| `h` `←` / `l` `→` | anywhere | column left (the queue left of the first) / right |
| `l` | on a job | follow it (its call in its session) |
| `⏎` | on a card | the inspector; a queue item's whole card |
| `o` | on a card | its deck |
| `esc` | anywhere | close the note, then the card, then the selection, then the filters |
| `tab` / `⇧tab` | anywhere | next / previous that needs you |
| `y` `t` `n` | on a question | approve, approve and trust, decline (the note field opens) |
| `1`-`6` | on a question with choices | that choice |
| `a` `e` | on a card offering a grant | the grant (`a` a command's, `e` edits', as `GrantOffer::key`) |
| `r` `d` | on a failure | retry (a task), dismiss |
| `P` | on a task | pause / resume |
| `s` ×2, `S` | on a running conversation | stop the turn; stop and cancel every task under it (asks) |
| `c` ×2 | on a task | cancel |
| `x` | on a wake | cancel it (asks): the TUI's `x` |
| `N` | anywhere | new task, its home the conversation under the cursor |
| `g g` `g b` `g t` `g h` `g m` | anywhere | Graph, Board, Timeline, Threads lens; Home (the Ship) |
| `/` `[` `]` `\` `?` | anywhere | search, fold queue, fold inspector, the dock (reserved), the keys |
| `b` `w` `r` `d` `a` | counters | needs you, working, paused·queued, done today; `a` clears the filters |

Conflicts, resolved the TUI's way (`PRIORITY`): `r` and `d` are retry and dismiss on a failure and filters elsewhere;
`a` is the grant when the card offers one whose key is `a`, else "everything"; `e` is the grant or nothing; `l` follows
on a job row and moves right elsewhere; `t` is approve-and-trust on a question, and `g t` (a sequence) the Timeline
lens. The overlay says each. The page's handler listens in the capture phase ahead of the shell's: a `g` sequence the
page claims is the page's, any other (`g f`) goes on to the shell (checked live). Keys never fire while an input,
text area, select or editable element has focus. `g m` "Home": I read it as the Ship, the landing view; if the owner
meant the home conversation, it is one line in `handlers.home`.

## What the later rows replace or extend

- **ops-graph**: replace the `a.lens === 'graph'` branch in `views/Ops.tsx` (now `<TaskGraphView … shown />`); move
  parity row X7's `at` and B1's home to the new lens; the Board's `selects`/`laneRows` and `keys.ts` gain the
  multi-select and bulk verbs (new `KEYS` rows and `handlers` entries; the page test checks both).
- **ops-timeline**: replace the `a.lens === 'timeline'` branch (now `<Agenda />`); rows A1-A12 move their `at` to the
  new lens's files (the parity test fails until they do, by design); give `Address.redact` its meaning (it is parsed and
  carried; nothing masks yet); B16's Horizon keys; the calendar's events join `comingUp` in `ops/queue.ts`.
- **ops-act**: `ops/Inspector.tsx`'s `RowWhole` gains the live stream, transcript, Record and Standing sections; the Join
  pane reads `Address.join`; the `data` chip is a new `FILTER_KEYS` entry and a `matchesFilters` clause (ops-verbs'
  `SessionInfo.data`); the model and mode menus; the palette's entities; B5, B10, B11, B13-B15, R3, R6 get homes; `\`
  (`fold_dock`) gets the dock.

## Tests and planted reverts

New tests: `test/ops-address.test.ts` (4), `ops-board.test.ts` (10), `ops-queue.test.ts` (12), `ops-keys.test.ts` (7),
`ops-parity.test.ts` (5), `ops-page.test.ts` (6); fixtures in `test/ops-fixtures.ts` (invented: harbour survey, tide
tables, piers, buoys). `npm test`: 568 pass, 0 fail (524 before this row; no test removed: the keel guard agrees).
`test/taskboard.test.ts` and `test/agenda.test.ts` named the old routes, rail items and `g` keys: I rewrote those
assertions in place (same tests, same count) to the redirects, the Ops item and the new keys; `noticeTarget` now opens
`/ops?lens=board&sel=…`, and `explainers.ts`'s `tasks.board` became `ops.board` (plus `ops.queue`, `ops.inspector`).

Each plant was applied, its test run, the file restored and `touch`ed, `git status` clean after the 23 (script:
`/tmp/claude-0/plants/run.py`, output kept for this report):

| plant | file | failing test (file:line) |
|---|---|---|
| the redirect drops the query | address.ts | ops-address:7 "each old page redirects to its lens and keeps ?w= and ?sel=" |
| the cursor kept by index | queue.ts | ops-queue:66 "a new item never moves the cursor" |
| the queue newest first | queue.ts | ops-queue:26 "every kind of item, the longest waiting first" (+2) |
| a dismissed failure still listed | queue.ts | ops-queue:53 "a dismissed failure asks for no one" |
| retry on a conversation | queue.ts | ops-queue:133 "each answer is one protocol call" ("only a task is retried") |
| an owner_ask is an approval | queue.ts | ops-queue:53 "an owner_ask in the confirm list is a question with its choices, once" |
| keys fire while typing | keys.ts | ops-keys:60 "keys never fire while typing" (`j`) |
| an act on under the time machine (retry) | keys.ts | ops-keys:66 "under the time machine every act is off" ("retry in the past") |
| filters before a failure's verbs | keys.ts | ops-keys:24 and the resolution test |
| a narrow screen keeps the canvas | regions.ts | ops-keys:92 "the regions…" |
| an empty inspector keeps its column | regions.ts | ops-keys:92 ("nothing selected: the canvas takes the width") |
| a failure under Done | board.ts | ops-board:11 "the columns…" (+2) |
| the model chip ignores `moved_to` | board.ts | ops-board:36 "the model chip…" (+1) |
| the card loses "waits on" | facts.ts | ops-parity:65 "T4: cardFacts never makes waitsOn" |
| the Board card stops drawing the place | BoardLens.tsx | ops-parity:65 "T4: …does not draw placeTip" |
| the thread list loses its loose footer | ThreadsLens.tsx | ops-parity:35 "H3: … no longer has conversations are in no thread" |
| a key without a handler (`fold_dock`) | Ops.tsx | ops-page:13 "every key in the table has a handler" |
| an act around `enabled` (`useAct` in the inspector) | Inspector.tsx | ops-page:22 "Inspector.tsx acts around enabled" |
| the page polls (`setInterval`) | Ops.tsx | ops-page:35 "Ops.tsx polls" |
| the asks a new array every render | asks.ts | ops-page:60 "a page left alone renders nothing new" |
| a deep link cleared before the queue reads | Ops.tsx | ops-page:53 "a deep link survives the queue's first read…" |
| a notice opens the old Tasks page | taskboard.ts | taskboard:67 "a notice opens its task on the page…" |
| the rail keeps the Agenda | navgroups.ts | navgroups:12 "every view of the rail is in exactly one group…" |

## The live check I ran, and the screenshots

A scratch daemon of this build (`target/debug/theseusd`, config in `/tmp/opsshot/theseus.toml`: `env:` secrets with a
stand-in key, Discord and the judge off, web on 7434, `[tools.defer] enabled = false` so `owner_ask` loads,
`[model.timeouts] first_byte_secs = 7200` so a held answer keeps a task running) on `theseus-sim fake-model --rules`
(invented rules). Data, all invented: threads **Harbour survey** and **Tide tables**; tasks Count the moorings, Count
the buoys, Survey the piers (done), Write up the quay walls (an `fs.write` approval, after `policy.tighten fs.write`),
Tabulate the tides (running), Compare the gauge readings (paused), Paint the lighthouse rail (deferred two hours), Fetch
the harbour chart (failed: the stand-in was stopped mid-call); a question "Which pier should the survey start with?"
(North pier / South pier) in the harbour thread; a reminder "check the tide gauge". Headless Chromium over the DevTools
protocol (a script in `/tmp/claude-0/shot/shot.mjs`; no dependency added), 1440×900, the dark theme, the queue open:

- `cloud-report/ops-shell/board.png`: `/ops?lens=board&sel=q%3Aact_…&redact=1` (the question selected, answered in
  place in the queue, its full card in the inspector)
- `cloud-report/ops-shell/threads.png`: `/ops?lens=threads&sel=harbour-survey&redact=1`
- `cloud-report/ops-shell/timeline.png`: `/ops?lens=timeline&redact=1`
- `cloud-report/ops-shell/graph.png`: `/ops?lens=graph&redact=1`

No real name, address, calendar id, title or account is in them: every title is invented, the paths are the VM's
`/tmp/opsshot/…` and `/home/user/theseus/…`, the AWS account the template's documentation example. `redact=1` is in
each address (it is carried; masking is ops-timeline's).

Also checked in that browser: `/agenda?w=-1&sel=wak_x` → `/ops?lens=timeline&w=-1&sel=wak_x`; `/tasks?sel=ses_t1` →
`/ops?lens=board&sel=ses_t1`; `/threads` → `/ops?lens=threads`; `/threads/harbour-survey` stays; the lens tabs; `g t`,
`g h`, `g b`; `g f` goes on to the Fleet; tab, `j`, `h`, `l` across the queue and the columns; `?` and Esc; Enter on a
queue item. The only console message on `/ops` is react-router's "No `HydrateFallback`" warning, which every lazy
route gives. This check found two real faults the pure tests could not (fixed in `74c362e` and `00dd881`, each with a
guard test): a deep link's queue selection cleared on load, and a render loop (the asks hook's new array every render)
that kept every address change from reaching the page.

**For the maintainer** (with the owner's daemon or a scratch one of the install build):

1. `theseus` → open `http://127.0.0.1:7433/agenda?w=1` → lands on `/ops?lens=timeline&w=1`, the Agenda as before.
2. `/tasks?sel=<a task's ses_ id>` → `/ops?lens=board&sel=…`, the card glowing gold; `/threads` → the Threads lens.
3. With a question waiting: `/ops?sel=<its act_ id>&do=answer` → the queue's card with its answers; `y`/`n`/`1`-`6`
   answer it as the TUI would (the Discord card settles through the same `work.answer`).
4. Drag the ship's log needle back: the amber frame and banner; every button greyed with "acting needs the present".
5. Narrow the window under 900 px: the queue is the page; select an item: the inspector overlays.
6. Leave the page alone a minute with DevTools' network tab open: no request but the ledger's follow and the pushes.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0` in the environment: the VM's disk allowance filled at 28 GB of `target/` with debug info,
and the link died with a bus error; nothing in the repository changed for it): door, keel, keel tests, fmt, shape,
features, clippy, clippy-probe, cockpit (lint, test, build), test build and reader rule pass. **Suite: 8,040 run,
8,006 pass, 34 fail, 53 skipped**: the 34 are exactly the known L1 failures of a root VM (theseus-pv6i):
`theseus-sandbox::bench spawn_100`, 19 `theseus-sandbox::contract` tests, and 14 `theseusd::sandbox` tests. Nothing
else failed (`tests_output::the_cores_output_matches_its_golden` passed under the Phoenix zone; neither flaky test of
theseus-hohs or theseus-1n2y failed). The phases after the suite, run by hand: protocol types ok (no change), lifecycle
and jobs skipped (no bench), `theseus-sim bench turn --check --runs 5 --burst 0` ok, size skipped (no release build),
`cargo deny --offline check`: advisories, bans, licences and sources ok (after `cargo deny fetch`).

Keel guard, `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty output, exit 0.

## A `docs/status.md` sentence

"The cockpit's Operations page (`/ops`, theseus-jid7t.3; ops-shell): the needs-you queue (every question, failure, seal,
hold and held message, the longest waiting first, answered in place), the Board and Threads lenses, the Agenda and the
task graph hosted as the Timeline and Graph lenses until their rows land, an inspector, one key table for the handler
and the `?` overlay, and the time machine across all of it; `/agenda`, `/tasks` and `/threads` redirect to their lens,
and a parity test holds every datum of the old pages to a home."

Docs the maintainer may want to touch: `docs/spec` Part III's item for the row; `docs/design/` if the Operations design
document lives there (the idle-inspector rule above is new).

## Follow-ups and uncertainties

- The router's held messages are seen only while the page is open (no read lists them): a `thread.asks` read, or
  `work.watch` views for them, would let the queue show ones asked before it opened.
- Seal recommendations come from `ThreadGroup.seal_pending`; I could not raise one on the scratch daemon (only
  `Core::thread_recommend_seal` sets it), so the seal card's live look is untested here; `answerOf`'s seal calls are tested.
- The Bridge's "Wakes due" tile and Actions' links still go to `/agenda` and `/tasks`, which redirect (Actions was out of
  bounds; the Bridge I left alone to keep the diff small).
- The Graph lens's panel keeps the task graph's 420 px height inside the lens; ops-graph replaces it anyway.
- `g m` reads Home as the Ship (above).
- `npm run lint` reports no warning in any file this row touched beyond `main.tsx`'s existing "fast refresh" family,
  which its new `ToOps` joins.
