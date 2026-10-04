# cockpit

The cockpit (spec §3.14; Part III Items 23 and 27): the daemon's web UI, a Vite app served by `theseusd` at `/`.
It took the place of the first one, the Observatory, on 2026-10-03 (theseus-vm3n.6), and its old address,
`/cockpit/…`, redirects to the same route. React, Tailwind, ECharts, React Flow, and TanStack. Its build,
`crates/theseusd/cockpit/dist`, is not committed: the gate builds it, and the install builds it before the release
build.

## What's here

- `src/views/`: the eleven views. The Ship (`Ship.tsx`, the landing view at `/ship`), then `Bridge`, `Fleet`,
  `SessionDeck`, `Actions`, `Boundaries` (the boundaries board), `Ledger`, `Money` (the money river), `Economics`,
  `Speed` (the speed wall), `Systems`.
- The time machine: `src/components/TimeMachine.tsx` (the ship's log, at every page's foot), `src/lib/history.ts`
  (the whole ledger, read once with `ledger.tail`'s `after` and followed), `src/lib/timemachine.ts` (the fold, its
  checkpoints, and the log's axis), `src/lib/marks.ts` (the marks: a start whose build differs from the one before
  it is an install), and `src/lib/world.ts` (`useWorld()`: the lists as of the moment).
- `src/ship/`: the Ship's parts. `model.ts` (the graph as a fleet, and its layout: pure, no three.js), `engine.ts`
  (three.js, drawn directly), `shaders.ts`, `post.ts` (the glow), `labels.ts` (nameplates and tags, HTML over the
  canvas), `instruments.tsx` (the brass gauges), `Minimap.tsx`, `useShipData.ts` (the reads and pushes it composes),
  and `synth.ts` (a seeded 10,000-node fleet for measuring).
- `src/components/`: the call and model-call inspectors, the transcript, the flame chart, the shell, and `brass.tsx`
  (the plank strip and the coin).
- `src/lib/`: `rpc.ts` and `hooks.ts` (the connection and its queries), `derive.ts`, `summary.ts`, `format.ts`,
  `money.ts` (the catalog's rates and a call's split by token kind), `verdict.ts` (18a's verdicts in words), and
  `calm.ts` (calm mode), `sandboxwords.ts` (L1 in the CLI's words), and `drafts.ts` (what was sent and not yet
  written).
- `src/protocol.ts`: the protocol client, imported as `@protocol`. It re-exports the protocol's types,
  `src/protocol.gen/`, which theseus-protocol's test writes from the Rust ones: never edit them by hand.
- The look is in `index.css`'s tokens and the shared components (`.panel`, `.brass-card`, `.brass-button`,
  `.panel-title`): brass, ivory, and night navy, with colour carrying state. Restyle there, not view by view.

## Invariants

- **Read only what is on screen.** A panel reads the ledger only while it is open: both inspectors once polled
  5,000 rows every 3 s with neither open (Item 27).
- **Every control is confirmed first**, and each is a protocol method, judged by the core as any surface's.
- **Every view keeps its state in the address**, so any view deep-links.
- **The Ship shows only the daemon's data, and moves only when something happens.** Its loop renders while
  something moves (the camera, a flare, a sail, a lantern, a gear, a stream, a running task's current) and stops when
  the daemon is idle. Calm mode (`?calm=1`, the toggle, or `prefers-reduced-motion`) drops the motion and the glow,
  and every chart's transitions.
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
  three when `cockpit/node_modules` exists.
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
  `window.__ship.stats` has the frames, the first frame's time, and the scale.
- Under a CPU rasteriser, half-float targets and full-resolution texture reads are what cost: keep the post's
  targets 8-bit and its composite at three reads a pixel.

## Traps

- **The dev server connects straight to a scratch daemon** (`THESEUS_DEV_DAEMON`, default 127.0.0.1:7434, never the
  operator's 7433). Set that daemon's `[web] dev_origin = "http://127.0.0.1:5174"` while you work, and stop the dev
  server when you're done. Another port: `npx vite --port 5175`, and set `dev_origin` to match.
- **A GLSL comment inside a template literal must not contain a backtick**: it ends the string, and Vite's error names
  the shader file, not the cause.
- **Never add a `/ws` proxy** (theseus-88im): it relays other pages and users to the daemon from your socket.
- The first load after adding a dependency can fail while Vite re-optimizes ("Outdated Optimize Dep"). Load it again.
