# cockpit

The cockpit (spec §3.14; Part III Items 23 and 27): the daemon's web UI, a Vite app served by `theseusd` at `/`.
It took the place of the first one, the Observatory, on 2026-10-03 (theseus-vm3n.6), and its old address,
`/cockpit/…`, redirects to the same route. React, Tailwind, ECharts, React Flow, and TanStack. Its build,
`crates/theseusd/cockpit/dist`, is not committed: the gate builds it, and the install builds it before the release
build.

## What's here

- `src/views/`: the seventeen views. The Ship (`Ship.tsx`, the landing view at `/ship`), then `Bridge`, `Fleet`,
  `SessionDeck`, `Actions`, `Boundaries` (the boundaries board), `Ledger`, `Money` (the money river), `Economics`,
  `Speed` (the speed wall), `Benchmarks` (below), `Policy` (`policy.explain`: each place's tools, layer by layer, and the tightenings with their undo; M7 42b), `Judgment` (Jev's judgments, M5 23b, over `judge.list` and `judge.get`, with
  `src/lib/judgment.ts`), `Systems`, `Ontology` (the kinds, the category tree, guidance, and topics; people,
  searchable by handle, a person's page and merge, adding one, and Jev's proposals with select-all accept, in
  `src/components/People.tsx` over `src/lib/people.ts`, theseus-wy7y; it shows the present only), `Context` (the context explorer, theseus-7n3e: what Theseus knows and what a turn sees), and `Books` (the imported episodes by their book, first cut: `books.list`, `books.page`, an episode's messages by `session.history`; its pure parts are `src/lib/books.ts`, theseus-civ0). A session's memberships are in its deck's Context tab (`src/components/Memberships.tsx`), whose pulldown groups topics and people and searches both.
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
  voice, fades, ducks, wave shape and noise; pure, theseus-pl0x), `instruments.tsx` (the brass gauges), `Minimap.tsx`, `useShipData.ts` (the reads and
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
  `test/since.test.ts`. The watch keeps the day's scan (`DayScan`) and the stretch's walk (`StretchWalk`) between
  recomputes (theseus-qilc): the ledger's rows only join its end and the live moment (and a replay's) only moves on,
  so a recompute reads only the rows since its last, and a median is a look; a moment moved back reads afresh. A kept
  read says what a fresh one says: a test holds it on seeded ledgers (`test/busy.ts`), and holds a busy day's recompute
  and replay step under a frame.
- Benchmarks (theseus-raf4): `src/views/Benchmarks.tsx` (`/benchmarks`: the frontier, a Pareto chart of measured
  harnesses on two properties the operator picks, and every run, newest first) and `src/views/BenchRun.tsx`
  (`/benchmarks/<report>`: a run's report whole, every figure with its table). The runs are `docs/benchmarks/` as the
  build embeds it (`src/lib/benchfiles.ts`, Vite's `import.meta.glob`: each report's files a chunk of their own, every
  figure's SVG an asset); no protocol method reads them, and the view reads nothing from the daemon. Its pure half is
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
- The route footer (theseus-q31l): `src/components/RouteFooter.tsx`, under the composer after a turn it sent,
  `routed: quick · haiku` and the owner's one-tap correction (⬆ stronger, ⬇ cheaper, or a profile), each confirmed and
  `route.correct`; the live correction layer is Judgment's Corrections panel (`src/components/Corrections.tsx`, over
  `route.corrections`). Their words are `src/lib/routefooter.ts` (`test/routefooter.test.ts`).
- `src/components/PromptPicker.tsx` (beside the composer; its pure parts are `src/lib/prompts.ts`): runs an MCP server's
  prompt as the next turn, a field per argument, through `turn.submit { prompt }`.
- `src/components/Extensions.tsx` (in Systems; its pure parts are `src/lib/extensions.ts`): the loaded extensions
  (M7 43b) from `extend.list`, each with its manifest, digest, files, tests, who acked it, calls, errors, and Revoke
  (`extension.revoke`, confirmed first), then the proposals not loaded.
- `src/components/SelfChanges.tsx` (in Systems; its pure parts are `src/lib/selfchanges.ts`, theseus-pw1q.4): "What
  Theseus changed about itself", over `self.log`: the kill switch's state and mode, a Halt button (`self.halt`,
  anyone's, confirmed first), and the newest changes with what, why, numbers and undo. The resume is the owner's from
  a private place (`theseus self resume`); the card never sends it. Tests: `test/selfchanges.test.ts`.
- `src/components/TaskGraph.tsx` (in Actions; its pure parts are `src/lib/taskgraph.ts`, M7 39b): the task records as a
  tree (state, owner, claim, version, a waiting change with its card) and, at `?taskgraph=1` (`&task=<id>`), as a React
  Flow graph. It shows the present: under the time machine it says so and its acts are off. `task.changed` reads
  `task.list` again (`bindPush`).
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
- `src/protocol.ts`: the protocol client, imported as `@protocol`. It re-exports the protocol's types,
  `src/protocol.gen/`, which theseus-protocol's test writes from the Rust ones: never edit them by hand.
- The look is in `index.css`'s tokens and the shared components (`.panel`, `.brass-card`, `.brass-button`,
  `.panel-title`): brass, ivory, and night navy, with colour carrying state. Restyle there, not view by view.

## Invariants

- **Read only what is on screen.** A panel reads the ledger only while it is open: both inspectors once polled
  5,000 rows every 3 s with neither open (Item 27).
- **Every control is confirmed first**, and each is a protocol method, judged by the core as any surface's.
- **Every view keeps its state in the address**, so any view deep-links.
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
