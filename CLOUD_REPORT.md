# CLOUD_REPORT: fleet-fullscreen (theseus-uu3b2)

## Step D4: a graph fills the window and comes back
**Found.** Nothing in `src/` used React Flow's instance; all three graphs rely on the `fitView` prop. `Panel` had no way to be a dialog.

**Changed** (dd31bc8b, cockpit/ only):
- `src/lib/fullgraph.ts` (pure): `parseFull`/`writeFull` (`?graph=full`, one name for both graphs, other parameters kept) and `fullKey` (Esc leaves, even from a field; `f` toggles, not in input/textarea/select/contenteditable, not with a modifier).
- `src/components/FullGraph.tsx`: `useFullGraph(name)`: the header button (Maximize2 "Fill the window (f)" / Minimize2 "Back to the panel (Esc)"), Esc/`f` in the capture phase (Esc stops propagation, as Depth does), and a double-rAF `fitView({padding: 0.2})` on every change of state after the first. Entering pushes a history entry, leaving replaces, so Back returns.
- `ui.tsx` `Panel`: one optional `dialog` prop (role="dialog" + aria-label).
- `Fleet.tsx`, `TaskGraph.tsx`: the same graph element stays mounted; only the Panel's class changes to `fixed inset-0 z-30` (under the cmdk overlay z-40/dialog z-50). TaskGraphView: cheap (a dozen lines), done; its 420px box becomes `flex-1`.
- `cockpit/AGENTS.md`: one entry.

**Proof.** In cockpit/: `npm run lint` 0 errors (existing warnings only), `npm test` 470 pass / 0 fail (3 new tests in `test/fullgraph.test.ts`), `npm run build` passes, `bash scripts/shape.sh` ok. `explainers` test green (no help id changed).
Planted reverts (file `src/lib/fullgraph.ts`, restored and touched, `git status` clean after):
- Esc ignored: `if (e.key === 'Escape') return full ? 'leave' : null` -> `return null`: "Esc leaves full screen and an unrelated key does not" failed.
- Other parameters dropped: `new URLSearchParams(params)` -> `new URLSearchParams()`: "the address round-trips" failed.
- Typing guard dropped: the `if (t && (TYPING...` line -> `if (false) return null`: "a key typed in a field does nothing" failed.

**Not done: the real browser check.** Chromium is on this VM (/opt/pw-browsers) but I did not build the daemon (about 9 minutes cold); the component behaviour (fit after the resize, no console errors, Back) is therefore untested in a browser. Maintainer's check:
1. `cargo build -p theseusd`; run a scratch daemon with its own `--config`, `--socket`, `--state-dir`; `cd cockpit && THESEUS_DEV_DAEMON=<its web addr> npm run dev` (port 7434).
2. Open `/fleet` (the Fleet view): press the maximize icon in "Graph · tasks and reports": the panel should cover the page, the graph re-fits, no console error. Esc: it returns and re-fits. Enter again, press browser Back: it returns. Open `/fleet?graph=full` directly: opens large.
3. In Actions, `?taskgraph=1`: same control; `&task=<id>` selection survives both ways. With the search box focused, `f` must type, not toggle.

**Uncertain.** `fixed` relies on no transformed ancestor in react-resizable-panels (I believe there is none). `f` is an addition beyond Esc and the button; drop it by editing `fullKey` if unwanted.

## Gate
Not run in full (scoped to cockpit as the task says). Keel guard: `--base origin/main` prints refusals for sealed/keel commits from other branches already in this branch's base (origin/main here is behind the clone's base), none from my commit; `--base HEAD~1 --head HEAD --quiet` prints nothing, rc 0.
