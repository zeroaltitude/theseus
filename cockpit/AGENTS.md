# cockpit

The cockpit (spec §3.14; Part III Items 23 and 27): the daemon's web UI, a Vite app served by `theseusd` at `/`.
It took the place of the first one, the Observatory, on 2026-10-03 (theseus-vm3n.6), and its old address,
`/cockpit/…`, redirects to the same route. React, Tailwind, ECharts, React Flow, and TanStack. Its build,
`crates/theseusd/cockpit/dist`, is not committed: the gate builds it, and the install builds it before the release
build.

## What's here

- `compress.ts`: a Vite plugin (Node's own zlib) that writes `<file>.br` (quality 11) beside
  every built file but the fonts; theseusd embeds only those, one copy, and sends it by `Accept-Encoding` (theseus-rwpbg.5).

- `src/views/`: the eighteen views. The Ship (`Ship.tsx`, the landing view at `/ship`), then `Bridge`, `Fleet`,
  `SessionDeck`, `Actions`, `Boundaries` (the boundaries board), `Ledger`, `Money` (the money river), `Economics`,
  `Speed` (the speed wall), `Benchmarks` (below), `Policy` (`policy.explain`: each place's tools, layer by layer, and the tightenings with their undo; M7 42b), `Judgment` (Jev's judgments, M5 23b, over `judge.list` and `judge.get`, with
  `src/lib/judgment.ts`), `Systems`, `Ontology` (the kinds, the categories as closed groups one a kind with their trees inside (`groupsOf`; `<unknown>` for a kind the table lacks, theseus-7mow), guidance, and topics; people,
  searchable by handle, a person's page and merge, adding one, and Jev's proposals with select-all accept, in
  `src/components/People.tsx` over `src/lib/people.ts`, theseus-wy7y; it shows the present only; its proposals panel lists people one row per proposed person, each with one accept and one reject, theseus-fvyx; the growth chart and the list of what the machines minted sit above the tree, `src/components/OntologyGrowth.tsx` over `growthOf` and `mintedOf` in `src/lib/ontology.ts`: 90 days of `ontology.category` and `ontology.kind` rows from the shared ledger history, stacked by origin, a ring on a day whose write was later undone, and each mint with its judgment link and an `ontology.undo` button, theseus-dd3kw; the tree's origin and confidence columns and its origin filter, `?origin=`, from the first `ontology.category` row of each category, `originsOf` and `originFilter`: a match keeps its ancestors as dimmed context and `unknown` is an option, so no category is hidden from every filter, theseus-tq7s8; an add-a-kind form beside add-a-topic, `src/components/OntologyKindAdd.tsx` over `buildKindAdd` and `kindAddAsk` in `src/lib/ontokinds.ts`: it mirrors the daemon's checks of `ontology.kind.add`, and its confirm carries the exact params it sends, theseus-s430x; the Judgment view draws the ontology pack's line under its row, `ontoLine` over `ontoStatsOf` in `src/lib/ontostats.ts`: judgments, acts, confirms and undos counted from the ledger's `judge.call`, `onto.filed` and `ontology.undo` rows, theseus-wzvxp; the learning report's ontology block draws the same line for the report's day with the undo rate per kind: the line's counts are `ontoStatsOf(rows, { day })` from the ledger (theseus-0vhyn), the undo rate is the daemon's, the report's own `pack.undo` and `pack.undo_days` worded by `ontoKindWords` (one definition, `onto_undo::undo_rates`, theseus-ukmbn); the kind groups fold open and shut in `src/lib/groupfold.ts`, the reader's own set kept while a filter forces them all open, theseus-5kd37), `Context` (the context explorer, theseus-7n3e: what Theseus knows and what a turn sees), and `Books` (the imported episodes by their book, first cut: `books.list`, `books.page`, an episode's messages by `session.history`; its pure parts are `src/lib/books.ts`, theseus-civ0). A session's memberships are in its deck's Context tab (`src/components/Memberships.tsx`), whose pulldown groups topics and people and searches both.
- The Agenda (theseus-p17fd; horizon HZ9): its page moved whole into the Operations page's Timeline lens
  (`src/ops/TimelineLens.tsx`, theseus-jid7t.5; `/agenda` redirects there, `g d`; `?w=` the week, `?sel=` the selected mark). The first week strip (`src/components/WeekStrip.tsx`, which the lens's v2 replaced: seven local days by the hour; a dot a reminder, a ring a
  check-in, a tick a routine run, a ✓ ✗ – for a past one by outcome, ▷ a deferred task's start, a flag a milestone, a bar a
  watch's window; a click opens the mark's verbs, each one call), the Routines table (`RoutinesTable.tsx`: schedule, next
  and last run, 30-day record, month spend of limit, a cost sparkline, and the last 30 runs of `routine.runs` in an
  expandable row), the watch and project lists (`FutureLists.tsx`), and the Wakes panel (`Wakes.tsx`), which moved here from
  Actions with every field it showed and its Cancel (Actions keeps a link, the Bridge's "Wakes due" tile opens the page).
  Pure half `src/lib/agenda.ts` (the window by local midnights, so a DST day is 23 or 25 hours and a mark still sits at its
  wall-clock hour; the layout; the verbs; the table's rows; `wakeFacts`; `page`), tested by `test/agenda.test.ts`. It
  reads `work.agenda` and `wake.list` (`src/lib/agendawatch.ts`) once per window and again only when `work.changed`
  touches a wake, routine, watch, project or deferred task (`touchesFuture`), the board's views from the one `work.watch`
  the Threads views keep, and each routine's `routine.runs` when its signature changes: the page polls nothing (a test
  reads its files for a timer). A window the daemon's 200-line cap does not fill is one read; a busier one (a routine
  every five minutes) is read on from where its lines stopped, at most 12 reads. It shows the present only.
- Threads on the work board (theseus-5i8i): the list is the Operations page's Threads lens (`src/ops/ThreadsLens.tsx`;
  `/threads` redirects there): each live thread as a group, its open tasks, questions and wakes under it, needs you
  first, a seal waiting as a red line. `/threads/:slug` (`src/views/Threads.tsx` over `src/ops/ThreadPanel.tsx`): the thread's
  goal, card, working set, `parent`/`split_from`/`merged_into` links, recent turns by `session.history`, and its open
  work). One `work.watch` while a Threads view is mounted (`src/lib/threadwatch.ts`), kept by the position rule in the pure
  `src/lib/threadboard.ts` (the grouping and counting rules of `theseus_protocol::group`, tested by
  `test/threadboard.test.ts`). Under the time machine the list is `foldThreads` over the ledger's `thread.*` rows (state
  and title as of the moment); the work under a thread is the present's and the view says so.
- **Operations** (theseus-jid7t.3): `src/views/Ops.tsx` at `/ops`, the rail's second item after the Ship (which stays
  the landing view). `/agenda`, `/tasks` and `/threads` redirect to its Timeline, Board and Threads lenses with their
  query kept (`redirectOf`); `/threads/:slug` stays a page. Its pure halves are `src/ops/*.ts`, each tested by a
  `test/ops-*.test.ts` over the invented board in `test/ops-fixtures.ts`.
  - **The address** (`ops/address.ts`): `lens` (`graph` · `board` · `timeline` · `threads`, board by default), `sel`
    (a row's `ses_`/`act_`/`wak_` id, a thread's slug, a mark's key, or a queue item's key, `q:act_…`, `f:exe_…`,
    `s:<slug>`, `h:ses_…`, `a:<ask>`), `f` (the chips, `key:value` pairs), `q`, `w`, `join` and `redact` (parsed and
    carried; later rows give them meaning), and `do=answer` (a deep link opens the item's answer). Keys the page does
    not own (`task`, `table`) are kept. A link reopens exactly what was on screen.
  - **The regions**: the command bar (`ops/CommandBar.tsx`: counters that are filters, with their hour's sparkline and
    `?table=ops.counters`; the lens switch; the chips; the search's "n of m"; New task), then the needs-you queue
    (`ops/Queue.tsx` over `ops/queue.ts`: longest waiting first by `attention.since_ms`, a cursor kept by key so a
    new item never moves it, each answer one call, `answerOf`), the canvas, and the inspector (`ops/Inspector.tsx`).
    `[` and `]` fold the queue and the inspector (`ops/regions.ts`, kept in this browser); below 1280 px the inspector
    lies over the canvas, below 900 px the queue is the page.
  - **The lenses**: the Board (`ops/BoardLens.tsx` over `ops/board.ts`: five columns over `taskboard.ts`'s `rowsOf`,
    with conversations, jobs, and what is due today); the Threads lens (`ops/ThreadsLens.tsx`); the Timeline lens (below);
    and the Graph lens (`ops/GraphLens.tsx`, below).
  - **It reads, it does not poll**: the one `work.watch`, `task.list`, `confirm.list` and `session.list` on their
    pushes, the ledger's one copy for today's spend, and the router's `thread.asked` while it is open
    (`ops/asks.ts`; no read lists them). `test/ops-page.test.ts` fails a timer or a refetch interval.
  - **Every act goes through `enabled`** (`ops/keys.ts`): `useOpsAct` (`ops/act.ts`) and `ActBtn` refuse an act under
    the time machine, where every lens shows the folded past with Actions' banner. No `useAct`, `Btn` or bare `call`
    in `src/ops/`, which the page test holds.
  - **The key table** (`ops/keys.ts`'s `KEYS`) drives both the page's one handler (`handlers` in `Ops.tsx`, a
    `Record<Verb, …>`, so a verb with no handler does not compile) and the `?` overlay (`ops/KeysOverlay.tsx`). A key
    means the most specific thing the focus offers (`PRIORITY`): `r`/`d` retry and dismiss on a failure and filter
    elsewhere, `a`/`e` take the card's grant when offered, `l` follows on a job row. The handler listens in the
    capture phase, ahead of the shell's: a `g` sequence the page claims (`g g/b/t/h/m`) is the page's, another (`g f`)
    goes on to the shell. A new key is a row of `KEYS` and a handler; the tests check both.
  - **The parity table** (`ops/parity.ts`, `test/ops-parity.test.ts`): every datum and act of the old Agenda, Tasks,
    Threads and Actions pages and of the TUI's board, with its home (`here`, `stays`, or `pending: ops-act|graph|
    timeline` for 1e and herdr only), the source that renders it, and for each card the fact function
    (`ops/facts.ts`) that must make every field the old component showed and the card that must draw it as
    `f.<field>`. A field added to a card goes into its fact function and its row's `fields`; a row a later lens takes
    over moves its `at` there. Never delete a row to make the test pass.
  - **The Timeline lens** (theseus-jid7t.5): `ops/TimelineLens.tsx` (the Agenda's page, moved: its reads, its
    panels unchanged, the mark's `MarkDetail`), drawing `ops/WeekStripV2.tsx`: three layers a day, the calendar's events
    as background blocks in side-by-side lanes with an all-day band (`ops/timeline.ts` `eventLayout`), the ran bars of
    each run in a 3 px left gutter (`ranBars`), Theseus's marks in the right gutter; the night folds (`nightFold`: a
    meeting or one-off mark holds its hour open, a series' ticks are counted inside); clashes (`clashes`: a mark inside a
    meeting, or two meetings that overlap; all-day and declined events never) are a ◆ and the command bar's two
    Timeline-only counters; `?day=` is the Day zoom (`d`/`w`, which beat the counter filters in the lens), `,`/`.` the
    week, and on a focused mark the TUI's Horizon keys (`ops/timelinekeys.ts`). **The materials rule**: colour is for
    state; a calendar is drawn in its own `color` (the daemon's hull materials, never a signal tone), a sensitive one
    hatched with a lock, a free/busy block hatched; a calendar's state (`stateOf`: ok, busy_only, not_found with the
    sharing steps, forbidden, auth_lapsed, off) is said in its legend in a state tone with its one fix, and a calendar not
    read draws no events (a stale week is never current). **The data rules** (`ops/calendar.ts`, the owner's rule 3):
    every surface that draws an event (block, tooltip, header line, the inspector's `EventDetail`, Coming up, the table,
    the search) takes its strings from `eventFacts`, over `redactEvent`, so `?redact=1` (`⇧R`) masks a sensitive
    calendar's title, place, call and attendees everywhere and the search does not match them; a verb's payload is
    `eventVerbPayload`'s, and for a sensitive calendar names only its label and the time (the task or chat reads it with
    the calendar tool; the New task sheet offers only a profile of US origin, `profileChoices`); Join the call and Open in
    Google only open the link in a new tab. An event lives in memory only: no storage, no log, and the address holds its
    opaque key (`?sel=ev:<key>`). `test/ops-calendar.test.ts` scans every string for invented content. **The one timer**:
    `ops/calendarwatch.ts` reads `calendar.window` over `readSpans` (the window shown, and today and tomorrow for Coming
    up) when they change, on `calendar.changed`, and every five minutes while the document is visible; it is the only
    timer, stopped when hidden and on unmount. **Drag** (`ops/drag.ts` `dropOf`): a reminder or check-in moves by
    `wake.snooze` with the instant as `when`, a deferred start by `task.reschedule` (`ops/reschedule.ts` until ops-verbs'
    types join), each with Undo; the rest is refused with the reason on the cursor. `useOpsAct`'s `send` is the gate for
    an act whose result the page reads.
  - **The acts** (theseus-jid7t.6; ops-act): the inspector's sections (`ops/InspectorSections.tsx` over `ops/sections.ts`: facts, the
    Record, Standing, a job's command and eta), **Live** (`ops/Live.tsx` over `ops/live.ts`: the session's narrative, the model's
    words from the push, and a followed job's output, which `ops/jobout.ts` drives by the pure `ops/follow.ts` machine: `work.peek`,
    then `action.output`, and `action.unfollow` on leaving the selection, switching lens or unmounting, never twice), the **Join
    pane** (`ops/JoinPane.tsx`, the address's `join`: `ses_…` a session, else a thread's slug; the deck's `Transcript` and drafts,
    and the composer is the pure state `ops/composer.ts`: ⏎ steers a running turn, ⌘⏎ holds the message until `turn.ended`, esc
    drops it, ⇧⇥ cycles the mode, `/t ` opens the thread picker, a thread with no session opens one on its first message), the
    **model menu** `m` and **mode menu** `M` (`ops/ModelMenu.tsx` over `ops/menus.ts`: "this turn only" is `turn.submit
    { profile }`, "from now on" is the sticky `session.profile` with a revert chip on the header and the toast's Undo; the
    daemon's `refused` is shown in its words and changes nothing), the **dock** (`ops/Dock.tsx`, `\` folds it, ␣ pauses while it
    has focus: the streams of `narrative.watch` and the models strip: one row a model, `groupByModel` in `ops/models.ts`, its
    effort a sub-line from the turns' `route.decided` rows or the profiles' configured efforts said so, its profiles under it,
    theseus-yxkww), the **palette** (`ops/palette.ts`, `PaletteGroups.tsx`,
    `paletteStore.ts`: the page publishes a feed while mounted and the shell's ⌘K shows the verbs over a scope, one, all visible or
    the selection, ⇥ cycling, and every entity; the selection scope is ops-graph's, passed at the one `// ops-graph: the selection
    scope` line in `Ops.tsx`), the right-click menu (`ops/VerbMenu.tsx`), and sheets for the acts that need words (`ops/Sheet.tsx`:
    New task, the thread verbs, re-home, reschedule; no `window.prompt` is left in `src/ops/`).
  - **The verb table** (`ops/verbs.ts`) is the one list of acts: verb, kinds, method, posture (`do`, `arm`, `ask`, `floor`), and
    whether one more call undoes it. The inspector, the palette and the right-click menu read it, and `test/ops-verbs.test.ts` pins
    each row to the owner's table, each method to the protocol's, and each acting key of `keys.ts` to a verb (`KEY_VERB`). Every
    method is the protocol's (`job.stop` since p2f-ops-act); a job's Stop in the inspector arms on a first press and calls it on a
    second, and the toast says the answer's `words`.
  - **The floor contract** (`ops/floor.ts`, `test/fixtures/profile_floor.json`): the daemon's old refusal table (US origin only)
    as data, one row per mark × origin × known. The daemon's half (`tests_profile_floor.rs`, `data_mark::refusal_of`) is gone
    (theseus-gyjyo.5: labels keep off only `cn` and `unknown`), so the fixture and `test/ops-floor.test.ts` hold `greyOf` only to
    themselves, and both are stale until the cockpit's floor words are rewritten.
  - **What the daemon clears, jobs, watches** (p2f-ops-act; theseus-dyukm, theseus-xzf71): the menu greys a profile by
    `ProfileInfo.cleared`, the daemon's own floor answer (now "not of restricted origin": `cn` and `unknown` are not cleared, `us` and
    `eu` are; it was US origin *and* a catalog row of `us` before theseus-gyjyo.5), never by `origin` alone; a missing `cleared` greys (fail closed), and the New task
    sheet's "US only" reads it too. The shared table has a second array, `vouched_uncatalogued`, for the case the origin gets
    wrong. `job.stop` is a method now (`JobStopParams`/`JobStopResult` in `job_follow.rs`, served beside `work.peek` under its
    place rule). A watch is a palette entity (`watchEntity`: its check, when it gives up, "failing"); choosing it opens the
    Timeline lens, and "Cancel the watch" plans `wake.cancel` and asks first. `profile.list` and `catalog.list` are read once
    and on `profile.changed`/the menu's invalidate, not on a timer; `hold` is stamped with the daemon's clock (`heldAt`);
    the deck's live turn is `liveOf` (`ops/live.ts`), one copy. `M` opens the mode menu on a task too: a task is a session
    with its own mode.
  - **Quiet by construction**: a menu or sheet that is open owns the keys (`modal` in `Ops.tsx`); the dock's ␣ works only while the
    dock has focus (the `dock` context outranks the rest), so ops-graph's ␣ is free elsewhere; every act is `useOpsAct`'s `act` (a
    confirmed call) or `post` (the call with its answer, for a refusal in the daemon's words and an Undo).
  - **The Graph lens** (theseus-jid7t.4; `ops/GraphLens.tsx` and `ops/GraphNode.tsx` over the pure `ops/graph.ts`,
    `test/ops-graph.test.ts`): a lane per live thread (needs-you first, a home lane for the rest), each conversation its
    lane's anchor, columns by depth and rows by start time; a node's id is a row's (`ses_` for a conversation or a
    task), so `?sel=`, the Board and the selection name the same thing. **Positions are sticky by id**: `opsLayout`
    takes the last layout (`keeper()` holds it), every placed node keeps its slot, a new node takes the next free row
    of its column, a node that leaves leaves a hole, and a lane's band keeps a spare row, so a live change moves no
    other node; a new lane goes below, and only "tidy" lays it out afresh. Ended work older than an hour folds into
    one "✓ N done" per lane (it opens on a click); past `NODE_CAP` (400) subtrees fold, deepest and oldest first, into
    "+N folded", never one that holds something that needs the owner. Filters and the search fade nodes, never
    remove them. A running edge flows under stir's rule and stops in Calm (`index.css`'s `ops-flow`).
  - **The selection and the bulk acts** (`ops/selection.ts`, `test/ops-selection.test.ts`): a set of ids in memory
    (`useSelection` in `ops/useSelection.ts`, the seam ops-act's palette reads), shared by the Graph and the Board;
    `␣`, `⌘A`/`ctrl+a` (everything shown), `esc` (clears it before anything else). A bulk act is a client fan-out of
    single calls, four at a time (`fanOut`, each through `useOpsAct`'s `send`, so each has its own ledger row and its
    own refusal), never a batch method; the results toast says each refusal in the daemon's words and the failed
    items stay picked. Stop, Cancel, Re-home and Decline ask once with the list. **Bulk Approve is the owner's
    decision 4**: only tool approvals that are not `floor`, not a budget ask or a task change, and whose `may_answer`
    is not false (`bulkApprovable`), each listed in a review sheet with a box, all ticked; a floor approval is listed
    as "asks one by one" and never sent. Drag (`ops/rehome.ts`'s `dropOf`, `ops/DropLayer.tsx`): a task onto a
    conversation, a lane or the tray's thread re-homes it (`task.rehome`, asks, naming who the budget stays charged
    to); a task or a conversation onto a project attaches it (`project.attach`); every refusal shows on the cursor.
  - **Follow-ups** (p2f-ops-ui; theseus-i08xr, n17am): **the calendar read** reads only where it is shown (`readSpans`'s
    `coming` flag, from `comingShows` in `ops/regions.ts`: outside the Timeline, nothing while the queue is a tab) and a
    `REFUSED` (-32005) `calendar.window` latches (`latches` in `ops/refusal.ts`): the banner stays, the five-minute
    re-reads stop, a span change does not retry, and only the `calendar.changed` push or a reopened connection reads
    again. Ask Theseus holds its picked profile with a sticky `session.profile` between `session.open` and the first
    `turn.submit` (`askCalls`); a refused hold sends no turn. **The Graph's keys** `j k h l` walk its nodes (`stepNode`
    over the lens's last layout, handed up by a ref; left from column 0 goes to the queue); a `+N folded` node opens its
    subtree (`opened` in `LayoutOpts`) while the 400 cap holds, folding other subtrees, or folding back with "could not
    open" (`Graph.cantOpen`); "tidy" forgets the openings. Under `?redact=1` a private or sensitive node or lane is drawn
    by `shownTitle` (its kind and short id, "a <mark> thread"). Only a node with a row is picked; a right-click opens
    the page's one `VerbMenu`; the palette opens over the selection while cards are picked.
  - **Leftovers** (theseus-vhtuv): the calendar hook's answer drops to `EMPTY` when it has no span (`heldRead`, `ops/calendarread.ts`); `layout()` decides an opening in its one cap step (`cantOpen` only when the opening is the cause); a refused Ask hold is said as a toast, and the deck mounts the same `<Toast />`.
- The time machine: `src/components/TimeMachine.tsx` (the ship's log, at every page's foot), `src/lib/history.ts`
  (the whole ledger, read once with `ledger.tail`'s `after` and followed), `src/lib/timemachine.ts` (the fold, its
  checkpoints, and the log's axis), `src/lib/marks.ts` (the marks: a start whose build differs from the one before
  it is an install), and `src/lib/world.ts` (`useWorld()`: the lists as of the moment).
- `src/ship/`: the Ship's parts. `model.ts` (the graph as a fleet, and its layout: pure, no three.js), `engine.ts`
  (three.js, drawn directly), `loop.ts` (when a frame is drawn: every display frame, a paced rate, or none; pure),
  `motion.ts` (the motion table: every motion, the one event that starts it, how long, its pace; pure), `sea.ts` (the
  living sea's height from the work; pure), `shaders.ts`, `post.ts` (the glow, and the swell's composite), `labels.ts`
  (nameplates and tags, HTML over the canvas), `words.ts` (every shape's words: the labels, cards, key, tour and the
  session deck read them; pure), `keyset.ts` and `Key.tsx` (the key), `HoverCard.tsx` and `placement.ts` (cards beside
  their point, off the instruments; pure), `Tour.tsx`, `tourText.ts` and `news.ts` (the tour, and what's new after an
  update; pure), `sound.ts`, `audio.ts` and `useShipSound.ts` (the three cues: which push sounds which, pure; the
  sounds, made with Web Audio; the toggle, and the cues the Shell hears on every page), `surf.ts` (the ambient sea's
  voice, fades, ducks, wave shape and noise; pure, theseus-pl0x), `sonar.ts`, `sonarsound.ts` and `Sonar.tsx` (the sonar ping, theseus-mbulo: every few minutes while the Ship is shown, a film submarine's ping with a soft echo, and a ring growing from the Ship's middle; the ring shows with Sound off and not in Calm, the sound only with Sound on; never with a gull, nothing from a hidden tab; `useSonar` in `useShipSound.ts`, test `test/sonar.test.ts`), `gull.ts` and `gullsound.ts` (the seagull that passes now
  and then while the sea is heard: its gaps, call and scheduler, pure, and its voice; theseus-7mow), `foghorn.ts` and
  `foghornsound.ts` (rarer still, a ship's foghorn far off, kept 30 s clear of a gull and of a ping, both ways; not the
  failure `horn` cue; theseus-7mow), `instruments.tsx` (the brass gauges), `Minimap.tsx`, `useShipData.ts` (the reads and
  pushes it composes), `flares.ts` (a failure flares its ship once, whichever of its push, its execution's change and
  its ledger row tells the page first; pure, theseus-1skt), and `synth.ts` (a seeded 10,000-node fleet for measuring).
- Session states (theseus-emqx): `src/lib/sessionState.ts` (pure: the daemon's rule line for line, the filter's
  address `?st=` with Live the default, its counts, the sessions it shows with the visitors, the palette's order, and
  the time machine's fold of `session.superseded`, `session.retired` and `session.reopened`), `src/ship/states.ts` (the
  engine draws the whole model's view under the filter, every vessel in the slot the whole model gave it, so a switch
  never reshuffles the sea), and `src/components/SessionLife.tsx` (the badge, the "Replaced by"/"Replaces" links,
  Retire and Reopen, confirmed first). A selected session, one the address or the palette flies to, a watch plate's
  and a flaring ship show whatever the filter; the palette lists every session, the retired ones after the others. A
  retired ship is laid up (dimmed), a quiet one at anchor, and a superseded one's plate flies a signal to its
  successor. "New session" opens a draft deck (`/session/new`), whose first message opens the session. Tests:
  `test/sessionState.test.ts`.
- The watch, the Ship's column of six plates (theseus-hnof): `src/ship/Watch.tsx` draws them, `src/ship/watch.ts` works
  out five (working, waiting, slow against each job's usual, spent with a day of 23 to 25 hours, went wrong) and
  `src/ship/since.ts` the sixth, since you last looked, whose stretch this browser keeps (`cockpit.watch.looked`) and
  whose replay runs the time machine. Keys 1 to 6 toggle their overlays. Their tests are `test/watch.test.ts` and
  `test/since.test.ts`. Live, the plate is told by the daemon's `work.digest` (theseus-qkoi3; `src/ship/sincedigest.ts`, pure, `test/sincedigest.test.ts` over a recorded answer, `test/fixtures/digest.json`): its lines, failures first, and its counts of reports, failures and answers; the cells the digest does not tell (sessions, spend) stay the fold's. The time machine and a replay still fold the ledger at their moment (`plateOf`); the read is `PUSHED`, so the push reads it again. The watch keeps the day's scan (`DayScan`) and the stretch's walk (`StretchWalk`) between
  recomputes (theseus-qilc): the ledger's rows only join its end and the live moment (and a replay's) only moves on,
  so a recompute reads only the rows since its last, and a median is a look; a moment moved back reads afresh. A kept
  read says what a fresh one says: a test holds it on seeded ledgers (`test/busy.ts`), and holds a busy day's recompute
  and replay step under a frame.
- Benchmarks (theseus-raf4): `src/views/Benchmarks.tsx` (`/benchmarks`: the frontier, a Pareto chart of measured
  harnesses on two properties the operator picks, and every run, newest first) and `src/views/BenchRun.tsx`
  (`/benchmarks/<report>`: a run's report whole, every figure with its table). The runs are `docs/benchmarks/` as the
  build embeds it (`src/lib/benchfiles.ts`, Vite's `import.meta.glob`: each report's files a chunk of their own, every
  figure's SVG an asset); no protocol method reads them, and the view reads nothing from the daemon but one panel's: "Your work" (`src/components/YourWork.tsx` over `src/lib/yourwork.ts`, `test/yourwork.test.ts`: `route.report`, one row a model-and-effort cell, theseus-pw1q.12). Its pure half is
  `src/lib/bench.ts` (`test/bench.test.ts`, on the real reports): the index, a task set's per-trial table folded into
  points (a harness in one run), the properties and their better directions, the reports' intervals, and the frontier.
  **Only measured data**: a condition the data holds is computed; one only a report's words hold is a note carrying the
  report's exact phrase, and the test holds each phrase to its report. A new report shows after the next install; a
  new harness table (another task set) is a row of `TASK_SETS`.
- The context explorer (theseus-7n3e): `src/views/Context.tsx` (the holdings, the imported episodes over
  `import.sessions` with their facets, topic tree, as-of months and an episode's messages, and the books' state) and
  `src/components/ContextTurn.tsx` (a turn's context over `context.explain`: the request's parts with their tokens,
  said against the turn's compilation, and why each note was recalled; and asking the index, `memory.search`). Its
  pure parts are `src/lib/explorer.ts`, tested by `test/explorer.test.ts`. The Ship's vessel panel and the session
  deck's Context tab link to it (`contextHref`). Personal and partner-confidential text is veiled on screen until
  opened (`?veil=off` lifts it); the daemon gives imported text only to a private place.
- The standing records (theseus-slvl.4): the Context page's `Standing` tab, `src/components/StandingRecords.tsx` over
  `src/lib/standing.ts` (pure: `groupsOf`, `scopeWords`, `lapses`, `revokeQuestion`, `tallyWords`; `test/standing.test.ts`):
  `standing.list`'s live records by kind (orders, preferences, facts), each with its scope (a surface's as `on terminal`),
  since, source and how many compiles carried it, and Revoke (`standing.revoke`, confirmed first, which the core judges
  as the owner's act from this private connection). It reads only while on screen.
- The owner card's proposals (theseus-slvl.6): the Context page's `Card` tab, `src/components/CardProposals.tsx` over `src/lib/card.ts` (pure: `restsOn`, `asksUntil`, `summary`, `declineQuestion`, `answeredLine`; `test/card.test.ts`): `card.list`'s open proposals as questions with Keep, Edit (a prompt for the words) and Not this (confirmed first), each `card.answer`; the header counts what waits and the job's spend. The job is the CLI's (`theseus card propose`). It reads only while on screen.
- A reply's citations (theseus-ejdk): `src/lib/cites.ts` (pure: the handles in a reply, the numbered marks, the card's words; `test/cites.test.ts`) and `src/components/Cites.tsx`
  (`ReplyMarkdown`: the reply as markdown with each cited handle a mark linking to its source's session, with a hover card of
  the source's header, excerpt and the command that opens it). The sources are `session.history`'s `cites`; a handle with no
  source (a shared place's private one, or one the turn was not given) stays text.
- The turn inspector (theseus-x9swu): `src/views/TurnInspect.tsx` at `/session/:id/turn/:turn` (lazy; the link a reply's footer
  carries, `[cockpit] public_url`), over `turn.inspect` and, for a loop's system parts, `context.explain`. The header (prompt,
  outcome, totals, route, prev/next, copy as JSON), `src/components/TurnFlame.tsx` (the turn on one clock in lanes: SVG, wheel
  zoom, drag pan, arrow keys), and an accordion of loops with request, response (thinking in full), calls and a diff against
  the loop before. The pure parts (lanes, row packing, zoom and pan, the person's waits, the loop diff, a call's timing, a
  call's ledger rows, the breakpoints' places) are `src/lib/inspect.ts`, tested by `test/inspect.test.ts`. SessionDeck's turn
  headers and timeline link to it (`Inspect`); `?turn=<turn>` on the session still scrolls to the turn.
- The route footer (theseus-q31l): `src/components/RouteFooter.tsx`, under the composer after a turn it sent,
  `routed: quick · haiku` and the owner's one-tap correction (⬆ stronger, ⬇ cheaper, or a profile), each confirmed and
  `route.correct`; the live correction layer is Judgment's Corrections panel (`src/components/Corrections.tsx`, over
  `route.corrections`). Their words are `src/lib/routefooter.ts` (`test/routefooter.test.ts`).
- Desktop notices (theseus-go4ig): the Shell's bell turns on `src/components/DesktopNotices.tsx`, which holds the page's `work.watch` (only while it is on) and shows the daemon's `work.notice` pushes (the notification policy is the daemon's, `theseus_protocol::notices::policy`, as for the chat and the TUI) and a set reminder's `wake.noticed`, through `src/lib/notices.ts`: a port of `deliver` (the session on screen is no ping) and of the reminder's notice. Never port the policy's table here. Test: `test/notices.test.ts`.
- `src/components/PromptPicker.tsx` (beside the composer; its pure parts are `src/lib/prompts.ts`): runs an MCP server's
  prompt as the next turn, a field per argument, through `turn.submit { prompt }`.
- `src/components/Extensions.tsx` (in Systems; its pure parts are `src/lib/extensions.ts`): the loaded extensions
  (M7 43b) from `extend.list`, each with its manifest, digest, files, tests, who acked it, calls, errors, and Revoke
  (`extension.revoke`, confirmed first), then the proposals not loaded.
- `src/components/SelfChanges.tsx` (in Systems; its pure parts are `src/lib/selfchanges.ts`, theseus-pw1q.4): "What
  Theseus changed about itself", over `self.log`: the kill switch's state and mode, a Halt button (`self.halt`,
  anyone's, confirmed first), and the newest changes with what, why, numbers and undo. The resume is the owner's alone
  (`theseus self resume`, or "resume self" from the owner's account on Discord); the card never sends it. Tests: `test/selfchanges.test.ts`.
- `src/components/ToolFamilies.tsx` (in Actions, under the tool postures; its pure parts are `src/lib/families.ts`,
  `test/families.test.ts`, theseus-7m7w.2): the tool families each kind of place loads or defers, with the reason, the use,
  searches and misses, over `tool.defer`. It shows the present: the time machine hides it.
- `src/components/TaskGraph.tsx` (in Actions; its pure parts are `src/lib/taskgraph.ts`, M7 39b): the task records as a
  tree (state, owner, claim, version, a waiting change with its card) and, at `?taskgraph=1` (`&task=<id>`), as a React
  Flow graph. It shows the present: under the time machine it says so and its acts are off. `task.changed` reads
  `task.list` again (`bindPush`).
- A graph fills the window (theseus-uu3b2): the Fleet's graph and the task graph take one control in their panel header
  (`src/components/FullGraph.tsx`; its address and keys are `src/lib/fullgraph.ts`, `?graph=full`, `f`, Esc). The graph stays
  mounted, its panel becomes a `fixed inset-0` dialog under the cmdk overlay, and it fits again on entering and leaving.
- A task's page (theseus-jm46q): on `/session/<task>` the deck shows `src/components/TaskHeader.tsx` above its own header
  (title, state, doing and forecast from the work board's view of the task, cost, Stop as `task.cancel`, `← back to` the
  parent conversation, and the inspector for the latest turn). The Composer joins a running task (its text is a
  `turn.submit` to the task's session, which the task reads at its next step); for an ended one it sends to the parent
  with the shared redirect line. Pure half `src/lib/taskpage.ts` (`joinable` and `taskRedirectLine` are
  `theseus_protocol::joinable` and `task_redirect_line` in TypeScript; keep their words identical), `test/taskpage.test.ts`.
- `src/components/ConfirmCard.tsx` draws its buttons from the question's own `options` and hides them when it says
  `may_answer: false` (pure parts: `src/lib/answers.ts`, `test/answers.test.ts`); every answer is `work.answer`.
- Content labels (theseus-9ymz0): a session's labels (`SessionInfo.labels`: `injection` freezes its writes, programs and sends; `secret`, `confidential` and `personal` keep it off China-origin models) show as chips in the session deck's header, `⚑ injection · node nod_e1`, the kind, source and confidence in the tooltip, `injection` in the warning tone and the others dimmer, each with a Remove that asks first and calls `content.unlabel` (verb `unlabel`, `theseus content remove`; Remove is the re-trust of the session, the only thing that lifts an `injection` stop). Pure half `src/lib/labels.ts` (`chipsOf`, `toneOf`, `unlabelCall`), tested by `test/labels.test.ts`. `rpc.ts` reads again on `session.labelled` and `session.unlabelled`; the Boundaries view reads the `content.labelled` and `content.removed` rows. Under the time machine the labels are the present's.
- `src/components/Budgets.tsx` (Money's Budgets panel; its pure parts are `src/lib/budgets.ts`): `budget.list`'s tree,
  the burn per hour from the history's `provider.call` rows, recent resets, the judge's day and the AWS hands. The
  budget questions waiting (with `ConfirmCard`) are its `BudgetQuestions`, first on Money, above the river, only while
  one waits (theseus-v6vc); the panel points up to them. `src/components/Tightenings.tsx` is the tightenings' list with their undo, shared
  by Boundaries and Policy. The Ledger's filter, saved filters, export and follow count are `src/lib/ledgerview.ts`;
  the Policy view's summaries are `src/lib/policyview.ts`.
- `src/components/`: the call and model-call inspectors, the transcript, the flame chart, the shell, and `brass.tsx`
  (the plank strip and the coin).
- `src/lib/`: `rpc.ts` and `hooks.ts` (the connection and its queries), `derive.ts`, `summary.ts`, `format.ts`,
  `money.ts` (the catalog's rates and a call's split by token kind), `verdict.ts` (18a's verdicts in words), and
  `calm.ts` (calm mode), `stir.ts` (the page's endless decorations run only while something changes), `scores.ts` (a notified call's `risk N% (shadow)`, M5 24), `fallback.ts` (a refusal's
  fallback in theseus-protocol's words, theseus-7gir.18), `sandboxwords.ts` (L1 in the CLI's words), `drafts.ts` (what was sent and not yet written), and `ontology.ts` (the category tree's
  order, and what a session's next compile would change).
- The chart method (theseus-hnof.4): `src/lib/chart.ts` takes its look from `src/lib/viz.ts`; `src/lib/palette.ts`
  holds the method's palette checks, which `test/palette.test.ts` runs on every palette the charts draw with;
  `src/lib/chartview.ts` keeps which charts show their table in the address (`?table=`) and measures the widths a
  label needs; `src/components/instrumentTables.ts` gives the shared instruments' legends and table views;
  `src/lib/calls.ts` is a billed call as the cockpit reads it (pure; `derive.ts` re-exports it); `src/lib/spans.ts`
  flattens a turn's trace for the flame chart and the deck's timeline; `src/lib/sessionrows.ts` gives the session deck
  its session's rows from the one copy of the ledger (theseus-kuzw).
- The frame (theseus-hnof.5): `src/components/Heartbeat.tsx`, the heartbeat bar across every view (the profile chip
  and its menu, UP, the health lamps with `src/lib/healthwords.ts`'s words, and the moment's "then" under the time
  machine); `src/lib/flow.ts` (rows a second, on the activity strip's bar); `src/lib/activity.ts` (the strip's lines
  folded, numbers aside, every line kept); `src/lib/mode.ts` and `src/lib/daylight.ts` (night, daylight, or the
  system's, night by default (theseus-001m), and each night colour's daylight step); `public/mode.js` (the mode's class
  before the first paint, chosen as `daylight.ts` chooses).
- The owner's 10-11 polish (theseus-onxwh): `src/ship/WatchDock.tsx` (the Ship's watch column as a slide-out, kept
  mounted and `inert` while folded, a tab on the right edge to bring it back) and the Depth gauge's minimize and
  maximize, both kept by `src/lib/prefs.ts` (`cockpit.ship.watch`, `cockpit.ship.depth`); `src/lib/navgroups.ts` (the
  rail's accordion groups; a test holds every rail view to exactly one group, so a new view joins one); `src/lib/cardfit.ts`
  (`CARD_BODY`: a card's body shrinks and scrolls sideways instead of running past its border); and the "?" on every
  panel, `src/components/Help.tsx` over `src/lib/explainers.ts` (Goal, How to read it, Why it matters, keyed by panel id).
- `src/protocol.ts`: the protocol client, imported as `@protocol`. It re-exports the protocol's types,
  `src/protocol.gen/`, which theseus-protocol's test writes from the Rust ones: never edit them by hand.
- The look is in `index.css`'s tokens and the shared components (`.panel`, `.brass-card`, `.brass-button`,
  `.panel-title`): brass, ivory, and night navy, with colour carrying state. Restyle there, not view by view.

## Invariants

- **Read only what is on screen.** A panel reads the ledger only while it is open: both inspectors once polled
  5,000 rows every 3 s with neither open (Item 27).
- **Every control is confirmed first**, and each is a protocol method, judged by the core as any surface's.
- **Every view keeps its state in the address**, so any view deep-links.
- **Every panel says what it shows**: `Panel`'s `help` is required (an explainer id, or `false` only for an error,
  stopped or loading notice), and `test/explainers.test.ts` fails a panel whose id has no words in `explainers.ts`.
- **Every chart has a table view**, the same numbers as rows, its toggle in the address (`?table=`).
- **A colour set outside an ECharts option follows the mode**: `mode.ts` swaps `toneHex` and `TONE_MARK`, and an
  option goes through `daylight()` as `Echart` draws it.
- **Inside the Ship's night island a tone is its CSS token**, never `toneHex`, which the island re-points.
- **The Ship and the ship's log's track stay at night** in either mode.
- **The Ship shows only the daemon's data, and moves only when something happens** (theseus-hnof.2). Every motion is
  a row of the motion table (`src/ship/motion.ts`): one event starts it, and every term of the shaders that moves with
  time names its row in a `motion:` comment, which a test reads (a new animation with no row fails it). A one-off (a
  flare, an oar growing out, a result flashing back) draws every display frame for its seconds; a state that moves
  while it lasts (oars rowing, a gear, a wake, a flow) a steady `STEADY_FPS`, and `QUIET_FPS` once a minute has passed
  with no event (`heard`: a long job, a long model call; theseus-n2hd); the swell alone `IDLE_FPS`, one
  composite pass a frame; the idle roll `ROLL_FPS`. A state that only waits (a question for the operator, a failure's
  pennant) is lit, not moved.
  **The sea is the work** (the owner's C5, `src/ship/sea.ts`): its height is tokens a minute and the turns running.
  **In Live mode it never stops** (the owner, 2026-10-07, theseus-42ic): with nothing happening it rolls slowly at
  `SEA_ROLL`, a composite of the sea alone `ROLL_FPS` (8) times a second, and any work raises it from there; so an
  idle Live Ship draws only the roll, and a hidden tab draws nothing. Calm mode (`?calm=1`, the toggle, or
  `prefers-reduced-motion`) stills it all, the roll included (dead calm, exactly 0: no frame at all), and drops the
  glow and every chart's transitions: a change draws one frame. `?swell=0` stills the sea for one page.
- **The page's decorations stand still while nothing changes** (theseus-jgme): the header's sweep, the live dots'
  pings and the soft pulses run while the daemon says something (a push, a new ledger row, the link changing) and
  `STIR_MS` after (`src/lib/stir.ts`, `html.stirred`), then stand lit. A new endless CSS animation takes the same
  rule in `index.css`, beside Calm's.
- **Sound is off until the operator turns it on**, and off again on every page load (the owner's call, 2026-10-07;
  nothing is kept in the browser): three cues on the daemon's own pushes and ledger rows (`src/ship/sound.ts`), made in
  the browser with Web Audio (`audio.ts`), no recorded or third-party sound. A cue is a row of `CUES`, with its events,
  its source and its pages. They play on every page (theseus-7zph): the Shell mounts `useSoundCues` once, and the oar
  splashes only on the Ship. **An audio context is made or resumed only in the Sound button's click**
  (`ShipAudio.turnOn`): nothing at load and nothing on another click, so the browser never logs its autoplay notice.
  The click's bell and the sea wait for `resume()`'s answer, never a timer; should the browser refuse, the button goes
  back to off. **While sound is on, the sea is heard** (theseus-pl0x, `surf.ts`): soft waves at the sea's height (the
  present's work: health's running turns and tokens a minute from the page's one copy of the ledger, read for nothing
  else), on every page, silent where the sea is still (Calm, so reduced motion, and `?swell=0`), and ducked under
  every cue. It starts only once the context runs (`ShipAudio.surf` remembers a height), fades in and out, and costs
  the page no work while it plays: the waves are a looping control signal on the audio thread, and the page sets a new
  height only when the work, the toggle or Calm changes. `ShipAudio.renderSea` renders it offline to listen to;
  `window.__shipSurf()` (dev and bench builds) says what it plays.
- **What's new is one item an addition** (`src/ship/news.ts`): an update that adds something to the Ship adds a stop
  there with its date, which moves `COCKPIT_VERSION`; a browser that has seen the tour flies to only the new stops,
  once.
- **The past is folded, never invented.** A view that shows the time machine's moment reads `useWorld()` (null while
  live), and its acts are off while it does. A new kind of row that changes what a view shows needs its step in
  `timemachine.ts`; `window.__timeMachine.checkNow()` (dev and bench builds) folds to the present and lists every
  difference from the daemon's own lists.
- **One copy of the ledger.** Read it through `useHistoryRows()`, never a second `ledger.tail` loop: a view that
  shows it at once reads it urgently; the ship's log reads it unhurried, so a page's own first reads come first. A hook
  that acts only on new rows (the Ship's horn and flare, for a session whose pushes the page does not hear) takes them
  with `onNewRows`, so it draws nothing for a read that brings none of its kinds.
- **A scrub redraws in under 100 ms.** The fold keeps an unchanged entry the same object, so rows memoized on it do
  not draw again; a heavy view takes `useDeferredValue(useWorld())`, and a laid-out graph follows the needle only
  once it rests (`useSettled`).

## Building and checking

- `npm ci`, then `npm run lint` (oxlint), `npm test`, and `npm run build` (`tsc -b && vite build`). The gate runs all
  three, before the suite, and runs `npm ci` first when `cockpit/node_modules` is missing (offline from npm's cache
  when it can).
- `npm test` is node's own runner over `test/*.test.ts`, with node stripping the types: no dependency. A module it
  tests is pure, imports nothing but the protocol's types (`import type`, which node erases), and names its own
  imports with their `.ts`; `src/lib/marks.ts` (the ship's log's marks) is the first.
- A release built without the cockpit's build serves a page at `/` that says how to build it.
- After a change, load every view from a scratch daemon of the build and check that each loads clean, with no
  console or page errors.

- **The synthetic fleet never ships.** `synth.ts` loads only in a dev or `--mode bench` build (`?synthetic=1`); check
  that a production dist has no `ses_synth` in it.

## Measuring the Ship

- Headless Chrome on this machine renders WebGL with SwiftShader, a CPU rasteriser (the GPU path through WSL's D3D12
  runs below one frame a second), so its frame rates are a floor. Measure on a quiet CPU (`/proc/pressure/cpu`):
  another agent's build halves them.
- `?bench=1` renders every frame; `?scale=1` pins the internal resolution (otherwise it adapts to the frame time);
  `?swell=0` stills the sea, for numbers comparable with builds before the swell; `window.__ship.stats` has the
  frames, the frames of the swell alone (`swellFrames`), the first frame's time, and the scale.
- Under a CPU rasteriser, half-float targets and full-resolution texture reads are what cost: keep the post's
  targets 8-bit and its composite at four reads a pixel (the sea, the fleet's layer, and the two blooms).
- **An idle Ship stays light**: in Live mode the sea's idle roll draws `ROLL_FPS` composites a second and nothing
  else; Calm draws no frame. (It drew `IDLE_FPS` composites a second, idle included, until theseus-hnof.2: 2.4 to 3.7
  cores of SwiftShader at 1920×1080, theseus-wp2d; then none at all, until the owner asked for the roll, theseus-42ic.)
  Each swell or roll frame is the composite alone; a GPU's cost is unmeasured (theseus-n2hd). A headless page left
  open takes `?swell=0` or `?calm=1`; whatever moves with the swell goes in `SEA_SWELL` (`shaders.ts`), worked out per
  pixel from the camera's ray in the composite, never in the sea's cached pass, whose redraw was about half again a
  swell frame's cost.
- `window.__shipEngine.motions` (dev and bench builds) lists the motions of the last frame, and `seaLevel` the sea's
  height; `window.__shipCues` the sound cues played.
- Headless Chrome never hides a page (a tab behind another, or a minimized window, still runs requestAnimationFrame
  at 60 a second), and it composites Live mode's CSS animations while they run (a few seconds after each change,
  `stir.ts`; until theseus-jgme, all the time). To check a hidden tab, set
  `document.hidden` and fire `visibilitychange`; to price the swell, compare Live against Live with `?swell=0`, not
  against Calm, which also stops those animations.

## Traps

- **The dev server connects straight to a scratch daemon** (`THESEUS_DEV_DAEMON`, default 127.0.0.1:7434, never the
  operator's 7433). Set that daemon's `[web] dev_origin = "http://127.0.0.1:5174"` while you work, and stop the dev
  server when you're done. Another port: `npx vite --port 5175`, and set `dev_origin` to match.
- **A GLSL comment inside a template literal must not contain a backtick**: it ends the string, and Vite's error names
  the shader file, not the cause.
- **Never add a `/ws` proxy** (theseus-88im): it relays other pages and users to the daemon from your socket.
- The first load after adding a dependency can fail while Vite re-optimizes ("Outdated Optimize Dep"). Load it again.
