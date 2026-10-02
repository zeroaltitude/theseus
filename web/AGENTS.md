# web (the Observatory)

The daemon's first web UI (spec §3.14): React and Vite, served by `theseusd` at `/`, from its committed build in
`crates/theseusd/web/dist`. It shares its protocol client with the cockpit.

## What's here

- `src/protocol.ts`: the protocol client, and a re-export of the generated types. A shared file: a lane changes it
  only at its join.
- `src/protocol.gen/`: the wire types, generated from theseus-protocol's Rust (`ts.rs`). Never edit it: change the
  Rust type, run theseus-protocol's tests, and `git add` what they write.
- `src/App.tsx` (a shared file, like `protocol.ts`), `src/Observatory.tsx`, `src/Sessions.tsx`,
  `src/Transcript.tsx`, `src/TraceView.tsx`, `src/Narrative.tsx`, and `src/DiskSpool.tsx`.

## Invariants

- **Wire types come only from `protocol.gen`**: no hand-written copy of a shape, and no `any` added (Item 30).
- **The session list follows the push** (`executions.watch` in `App.tsx`), not a poll (Item 33). The Observatory's
  tables still refresh on a timer (theseus-7ovb lists what still polls); new views use the push.
- **The build is committed.** After a change, `npm run build` rewrites `crates/theseusd/web/dist`; `git add` it
  before the gate, whose check compares the tree with the index. A change to types alone leaves the build unchanged.

## Building and checking

- `npm ci`, then `npm run lint` (oxlint) and `npm run build` (`tsc -b && vite build`). The gate runs both when
  `web/node_modules` exists.
- After a change, load the page from a scratch daemon of the build and check that every tab loads clean, with no
  console errors.

## Traps

- **The dev server's default daemon is the operator's** (`THESEUS_DEV_DAEMON` defaults to 127.0.0.1:7433). Always
  set it to your scratch daemon, and set that daemon's `[web] dev_origin = "http://127.0.0.1:5173"`. Unset it, and
  stop the dev server, when you're done.
- **Never add a `/ws` proxy** to the dev server. A proxy relays other pages and other users' processes to the daemon
  from your own socket, where its owner check can't see them (theseus-88im).
