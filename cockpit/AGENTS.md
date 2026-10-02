# cockpit

The cockpit (spec §3.14; Part III Items 23 and 27): a second Vite app beside `web/`, served by `theseusd` at
`/cockpit/`. React, Tailwind, ECharts, React Flow, and TanStack. Its build, `crates/theseusd/cockpit/dist`, is not
committed: the gate builds it, and the install builds it before the release build.

## What's here

- `src/views/`: the seven views (`Bridge`, `Fleet`, `SessionDeck`, `Actions`, `Ledger`, `Economics`, `Systems`).
- `src/components/`: the call and model-call inspectors, the transcript, the flame chart, and the shell.
- `src/lib/`: `rpc.ts` and `hooks.ts` (the connection and its queries), `derive.ts`, `summary.ts`, and `format.ts`.
- It imports the protocol client from `web/src/protocol.ts` through the `@protocol` alias, so the two apps never
  drift apart.

## Invariants

- **Read only what is on screen.** A panel reads the ledger only while it is open: both inspectors once polled
  5,000 rows every 3 s with neither open (Item 27).
- **Every control is confirmed first**, and each is a protocol method, judged by the core as any surface's.
- **Every view keeps its state in the address**, so any view deep-links.

## Building and checking

- `npm ci`, then `npm run lint` (oxlint) and `npm run build` (`tsc -b && vite build`). The gate runs both when
  `cockpit/node_modules` exists.
- A release built without the cockpit's build serves a page at `/cockpit/` that says how to build it.
- After a change, load every view from a scratch daemon of the build and check that each loads clean, with no
  console or page errors.

## Traps

- **The dev server connects straight to a scratch daemon** (`THESEUS_DEV_DAEMON`, default 127.0.0.1:7434, never the
  operator's 7433). Set that daemon's `[web] dev_origin = "http://127.0.0.1:5174"` while you work, and stop the dev
  server when you're done.
- **Never add a `/ws` proxy** (theseus-88im): it relays other pages and users to the daemon from your socket.
- The first load after adding a dependency can fail while Vite re-optimizes ("Outdated Optimize Dep"). Load it again.
