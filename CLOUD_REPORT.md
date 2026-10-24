# CLOUD_REPORT: p2f-ops-ui (theseus-i08xr, theseus-n17am, theseus-uy015)

Branch `cloud/20261024-p2f-ops-ui`, base `3796ea45`. Commits: `20e0b88` (i08xr), `92962f0` (n17am; uy015 changed nothing).
Both were made with `git commit -S`; the commit succeeded, I did not check whether the signature is valid.

## 1. For the maintainer first

- Store format 49: untouched. No Rust, no protocol change, no `protocol.gen`, no dependency, no `package.json` change.
- No file is near a ceiling (largest touched: `views/Ops.tsx`, about 520 lines). `NODE_CAP` stays 400.
- No OWNER CALL needed. One wording point: the brief's F5 test line says "one at 23:30 with nothing else is counted".
  A one-off mark at 23:30 opens through 24:00 (22:30 opens to 23:00 by the same rule), so it is drawn, not counted. The
  counted case I tested is a series tick (a `run` mark) at 23:30: it stays folded and is counted.
- Ask Theseus, after a refused hold: the words go to the inspector's status line and the deck opens at once (as the
  brief says), so the words may be left behind with the inspector. Consider carrying them into the deck as a toast.
- Not done: the live checks (OUT), F2 (watch item) and F6 (Keel) were left alone as told.
- No gate.sh run (cockpit only, per the brief); see section 3.

## 2. Per issue and item

### theseus-i08xr
- **F3 done.** `ops/calendar.ts` `askCalls(sessionId, p, pick)` returns `session.open`, `session.profile` (only with a
  pick), `turn.submit` (still carries `profile`). `profileRefusal(r)` gives the floor's words. `EventDetail.tsx` `ask()`
  walks it through `send('event_ask', …)`; on `refused` it sends no turn, shows the words, still opens the deck, and tries
  no other profile. Test `ask on a sensitive event holds its US profile: session.open, then session.profile, then
  turn.submit` (`test/ops-calendar.test.ts`). Plant: drop the `session.profile` entry in `askCalls` -> that test failed.
  The existing pin `send<{ session_id: string }>('event_ask', 'session.open'` is kept as it was.
- **F4 done.** `readSpans(shown, now, zoom, coming)` returns `[]` outside the Timeline when `coming` is false;
  `comingShows(width, queue)` in `ops/regions.ts` is `regionsFor(...).queue !== 'folded'` (one rule); `Ops.tsx` moved
  `qFold` and `useWidth` above `cal` (one small hunk). `latches(e)` in `ops/refusal.ts` is true only for code -32005. In
  `useCalendar` a ref `latched` (outside the effect, so a span change does not clear it) stops the first read, the timer
  and the visibility re-read; `calendar.changed` clears it and re-arms; `open` going false clears it. The one-timer line
  and `EVERY_MS` are unchanged. Test `Coming up's read: nothing when it is not shown, and a refusal stops the re-reads`.
  Plants: `readSpans` ignoring `coming` -> failed; `latches` always true -> failed (`expected: false, actual: true`).
  The hook's wiring is pinned by source regexes only (no React in this runner); I did not run it in a browser.
- **F5 done.** `nightFold` marks call `keep(h, h)`; `count` uses `h > late`. Test `a mark on the fold's edge opens no
  extra hour` (22:00 and 07:00 edge, 22:30 opens to 23, a `run` at 23:30 counted, `yOf(22)` is the edge line). Plant:
  back to `keep(h, h + 0.01)` -> failed.
- **F2 OUT, F6 OUT**, as told. The clash-sweep timing test did not go red here.

### theseus-n17am
1. **Done.** `stepNode` in `ops/graph.ts`; `GraphLens` hands its layout up through `layoutRef` (set in an effect, no
   state); `Ops.tsx` `move` calls it; left from column 0 goes to the queue; the `memory.lay(` call is kept. Test `the keys
   walk the graph's nodes`. Plant: return `fromId` -> failed.
2. **Done.** One effect in `Ops.tsx`: `useOpsPalette.setState({ scope: picked.size > 0 ? 'selection' : 'one' })`. Pure
   test in `test/ops-selection.test.ts` (picked rows -> `scopeTargets('selection')` -> a `model:<name>` entry with one
   `session.profile` plan per session); the effect line is pinned in `ops-page.test.ts`. Plant: effect removed -> the
   page test failed. (The pure selection test passes at the base too; only the pin is red without the effect.)
3. **Done.** `LayoutOpts.opened`, `Graph.cantOpen`. An opened root and everything below it never folds; if the graph is
   still over the cap, `opsLayout` lays it out again without the openings and sets `cantOpen`. `+N folded` is enabled
   and opens its root; tidy clears `opened`; the toolbar says "could not open: the graph is at its cap; narrow it with
   the filters or the search". Test `an opened fold shows its subtree and the cap still holds`. Plant: ignore `opened`
   -> failed.
4. **Done.** `shownTitle(n, redact)` (and `masked`) in `graph.ts`; used for the card title and tooltip, the job pill and
   its tooltip, future nodes, lane titles; a masked node's progress line and a masked lane's words are hidden too.
   `GraphLens` takes `redact`. Test `redact masks a private or sensitive node's title, not an open one's`. Plant: return
   the title -> failed.
5. **Done.** `Menu`, `offersFor`, `onVerb` removed from `GraphLens`; it takes `onContext(id, x, y)` and only for a node
   with a row; the page's one `VerbMenu` mount is used. Pinned in `ops-page.test.ts`.
6. **Done.** `homes` filters with `alive(r) && !r.session?.retired`. Plant: old line -> page test failed.
7. **Done.** Click and lasso pick only `byId.has(id)`. Plant: unguarded `onPick` -> page test failed.
8. **Done.** `select` reads `document.activeElement?.closest('[data-pick]')` first; `BoardLens` checkbox has
   `data-pick={r.id}`; the pinned checkbox line prefix is intact. Plant: `undefined` in place of the read -> failed.
9. **Done.** `hintChanged` and `DropHint` in `ops/rehome.ts`; `DropLayer` sets the hint through it (the verdict is still
   computed per `dragover`). Test `the drag hint changes only when its words or place change`. Plant: always true ->
   failed. (`DropLayer` still re-renders on a change of words, verdict, target or 6 px of travel; I did not profile it.)

### theseus-uy015: already done at the base, nothing changed
- F3: `Ops.tsx` `follow:` opens the inspector if folded; `ops-page.test.ts` asserts `!page.includes('?call=')`.
- F4: `parity.ts` H1 `fields` has `items`, H6 has `openWork`; `ThreadsLens.tsx` draws `f.items`, `ThreadPanel.tsx` `f.openWork`.
- F5: the only `window.prompt` text in `src/ops/` is in two comments (`sheets.ts`, `Sheet.tsx`); the page test scans code for it.
- F6: `views/Bridge.tsx:131` navigates to `/ops?lens=timeline`, hint says "on the Timeline".
- F8 OUT (owner call): every address write is `replace: true` (`Ops.tsx:80`); not changed.
- For the owner: the inspector is a slim tab while nothing is selected, by design.

## 3. The gate (in `cockpit/`)
- `npm ci`: ok. `npm run lint`: 0 errors (existing warnings only). `npx tsc -b`: clean.
- `npm test`: 671 pass, 0 fail (665 after the i08xr commit).
- `npm run build`: ok. `scripts/shape.sh`: `shape: ok`.
- No Rust changed, so no cargo or `gate.sh` run. No load runs were needed.

## 4. Keel
`python3 scripts/keel-guard.py --base 3796ea45 --head HEAD --quiet` -> empty output, exit 0.

## 5. DM golden
`git diff --exit-code 3796ea45 -- crates/theseus-discord/src/golden/dm_flow.txt` -> empty, exit 0.

## 6. Join notes for `integ/p2-follow`
Shared with p2f-ops-act, all in `views/Ops.tsx` (small hunks, search by name):
- import line `@/ops/graph` (`stepNode`, `Graph`) and `@/ops/regions` (`comingShows`), `alive` in the `@/ops/board` import.
- `useOpsCalendar(...)` call: `qFold` and `useWidth` moved above it; the later "regions" block lost those two lines.
- `move()` starts with a Graph branch; `graphRef` declared before `ctxMenu`.
- handler `select:` (data-pick read); `homes` (`alive(r) && !r.session?.retired`).
- the GraphLens mount: `offersFor`/`onVerb` replaced by `onContext`, `redact`, `layoutRef`.
- palette: one `useEffect` calling `useOpsPalette.setState({ scope })` just before `const publish = ...`. If ops-act
  changes the store's shape or `scope`'s values, adapt that line. `VerbMenu` is used unchanged through the existing `ctxMenu`.
- `cockpit/AGENTS.md`: one added "Follow-ups" bullet in the Operations section, before "The time machine".
- `ops/calendar.ts` gained `askCalls`/`profileRefusal` (above the "New task sheet's profiles" section); `EventDetail.tsx`
  `ask()` changed.

## 7. Follow-ups, and a `docs/status.md` sentence
- A refused hold's words could ride into the deck as a toast. A browser pass over the Graph keys, the opened folds and
  redact is for the maintainer's live check (not run here).
- status.md: "Operations follow-ups (p2f-ops-ui): Ask Theseus keeps its US profile with a sticky `session.profile`; the
  calendar reads only where shown and stops after a refusal; the night fold's edge is exact; the Graph has keys, openable
  folds under the 400 cap, redact masking, the page's verb menu and row-only picks."

## 8. Data-routing review
After my change `EventDetail.tsx` sends (all through `send(...)`, so the time machine gates them):
- `wake.create { when, note }`: note is `Coming up: <name>.`; for a sensitive event `<name>` is "the <calendar label> event at <time> on <day>" only. Unchanged.
- `session.open { kind: 'conversation', label }`: label is `about the <calendar label> event` for a sensitive event, `about <title>` for an open one. Unchanged.
- NEW `session.profile { session_id, profile: pick }`: only when a profile was picked (always for a sensitive event, by the US-only `profileChoices`; for an open event only if a pick exists). `session_id` and a profile name; no event content. If the daemon answers `refused`, no turn follows.
- `turn.submit { session_id, input, profile? }`: input is `eventVerbPayload('ask').input` (calendar label, time, "read it with the calendar tool"); sensitive events carry no title, place, attendees or link. Unchanged.
- Join and Open in Google are links in a new tab; nothing is sent. The prep task opens `NewTask` with `eventVerbPayload('prep')`'s brief (unchanged; US-only for sensitive).
`calendarwatch.ts` sends only `calendar.window { from_ms, to_ms }` per span, and only where shown: Timeline lens, or Coming up on screen. After a REFUSED answer it sends nothing more until the `calendar.changed` push or the connection re-opens. Any other error behaves as before. A sensitive event sends no title, place, attendee or call link anywhere. `profileChoices`' US-only rule, `eventVerbPayload`, `redactEvent` and their tests are untouched.
