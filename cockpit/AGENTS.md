# cockpit

The cockpit (spec §3.14; Part III Items 23 and 27): the daemon's web UI, a Vite app served by `theseusd` at `/`.
It took the place of the first one, the Observatory, on 2026-10-03 (theseus-vm3n.6), and its old address,
`/cockpit/…`, redirects to the same route. React, Tailwind, ECharts, React Flow, and TanStack. Its build,
`crates/theseusd/cockpit/dist`, is not committed: the gate builds it, and the install builds it before the release
build.

## What's here

- `src/views/`: the fourteen views. The Ship (`Ship.tsx`, the landing view at `/ship`), then `Bridge`, `Fleet`,
  `SessionDeck`, `Actions`, `Boundaries` (the boundaries board), `Ledger`, `Money` (the money river), `Economics`,
  `Speed` (the speed wall), `Policy` (`policy.explain`: each place's tools, layer by layer, and the tightenings with their undo; M7 42b), `Judgment` (Jev's judgments, M5 23b, over `judge.list` and `judge.get`, with
  `src/lib/judgment.ts`), `Systems`, `Ontology` (the kinds, the category tree, guidance, and topics; it shows
  the present only). A session's memberships are in its deck's Context tab (`src/components/Memberships.tsx`).
- The time machine: `src/components/TimeMachine.tsx` (the ship's log, at every page's foot), `src/lib/history.ts`
  (the whole ledger, read once with `ledger.tail`'s `after` and followed), `src/lib/timemachine.ts` (the fold, its
  checkpoints, and the log's axis), `src/lib/marks.ts` (the marks: a start whose build differs from the one before
  it is an install), and `src/lib/world.ts` (`useWorld()`: the lists as of the moment).
- `src/ship/`: the Ship's parts. `model.ts` (the graph as a fleet, and its layout: pure, no three.js), `engine.ts`
  (three.js, drawn directly), `loop.ts` (when a frame is drawn: every display frame, the swell's idle rate, or none;
  pure), `shaders.ts`, `post.ts` (the glow, and the swell's composite), `labels.ts` (nameplates and tags, HTML over the
  canvas), `instruments.tsx` (the brass gauges), `Minimap.tsx`, `useShipData.ts` (the reads and pushes it composes),
  and `synth.ts` (a seeded 10,000-node fleet for measuring).
- `src/components/PromptPicker.tsx` (beside the composer; its pure parts are `src/lib/prompts.ts`): runs an MCP server's
  prompt as the next turn, a field per argument, through `turn.submit { prompt }`.
- `src/components/Extensions.tsx` (in Systems; its pure parts are `src/lib/extensions.ts`): the loaded extensions
  (M7 43b) from `extend.list`, each with its manifest, digest, files, tests, who acked it, calls, errors, and Revoke
  (`extension.revoke`, confirmed first), then the proposals not loaded.
- `src/components/Budgets.tsx` (Money's Budgets panel; its pure parts are `src/lib/budgets.ts`): `budget.list`'s tree,
  the burn per hour from the history's `provider.call` rows, recent resets, the questions waiting (with `ConfirmCard`),
  the judge's day and the AWS hands. `src/components/Tightenings.tsx` is the tightenings' list with their undo, shared
  by Boundaries and Policy. The Ledger's filter, saved filters, export and follow count are `src/lib/ledgerview.ts`;
  the Policy view's summaries are `src/lib/policyview.ts`.
- `src/components/`: the call and model-call inspectors, the transcript, the flame chart, the shell, and `brass.tsx`
  (the plank strip and the coin).
- `src/lib/`: `rpc.ts` and `hooks.ts` (the connection and its queries), `derive.ts`, `summary.ts`, `format.ts`,
  `money.ts` (the catalog's rates and a call's split by token kind), `verdict.ts` (18a's verdicts in words), and
  `calm.ts` (calm mode), `scores.ts` (a notified call's `risk N% (shadow)`, M5 24), `sandboxwords.ts` (L1 in
  the CLI's words), `drafts.ts` (what was sent and not yet written), and `ontology.ts` (the category tree's
  order, and what a session's next compile would change).
- `src/protocol.ts`: the protocol client, imported as `@protocol`. It re-exports the protocol's types,
  `src/protocol.gen/`, which theseus-protocol's test writes from the Rust ones: never edit them by hand.
- The look is in `index.css`'s tokens and the shared components (`.panel`, `.brass-card`, `.brass-button`,
  `.panel-title`): brass, ivory, and night navy, with colour carrying state. Restyle there, not view by view.

## Invariants

- **Read only what is on screen.** A panel reads the ledger only while it is open: both inspectors once polled
  5,000 rows every 3 s with neither open (Item 27).
- **Every control is confirmed first**, and each is a protocol method, judged by the core as any surface's.
- **Every view keeps its state in the address**, so any view deep-links.
- **The Ship shows only the daemon's data. Its sea's swell is ambient; everything else moves only when something
  happens.** In Live mode the waves roll and the glints on the water twinkle, always, idle included (theseus-wp2d).
  Everything else moves only while something is happening (the camera, a flare, a sail, a lantern, a gear, a stream, a
  running task's current), and then the loop draws every display frame. Otherwise it draws the swell alone at
  `IDLE_FPS` (`src/ship/loop.ts`), one composite pass a frame, and a hidden tab draws nothing. Calm mode (`?calm=1`,
  the toggle, or `prefers-reduced-motion`) stills the sea and drops the motion and the glow, and every chart's
  transitions: an idle Ship in Calm draws one frame and stops. `?swell=0` stills the sea for one page.
- **The past is folded, never invented.** A view that shows the time machine's moment reads `useWorld()` (null while
  live), and its acts are off while it does. A new kind of row that changes what a view shows needs its step in
  `timemachine.ts`; `window.__timeMachine.checkNow()` (dev and bench builds) folds to the present and lists every
  difference from the daemon's own lists.
- **One copy of the ledger.** Read it through `useHistoryRows()`, never a second `ledger.tail` loop: a view that
  shows it at once reads it urgently; the ship's log reads it unhurried, so a page's own first reads come first.
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
- **The swell is the Ship's idle cost**: in Live mode it draws `IDLE_FPS` composites a second, idle included. Under
  SwiftShader at 1920×1080 that is 2.4 to 3.7 cores, by the resolution scale (theseus-wp2d); a GPU's is unmeasured
  (theseus-n2hd). While something moves, a rolling swell adds about a fifth to each frame's CPU there; Calm and
  `?swell=0` cost what they did before it. So:
  - a headless page left open (a watcher, a long screenshot run) takes `?swell=0` or `?calm=1`;
  - whatever moves with the swell goes in `SEA_SWELL` (`shaders.ts`), worked out per pixel from the camera's ray in
    the composite, never in the sea's cached pass, whose redraw was about half again a swell frame's cost.
- Headless Chrome never hides a page (a tab behind another, or a minimized window, still runs requestAnimationFrame
  at 60 a second), and it composites Live mode's CSS animations all the time. To check a hidden tab, set
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
