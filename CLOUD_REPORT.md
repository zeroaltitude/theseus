# CLOUD_REPORT: cockpit-polish (theseus-onxwh)

Branch `cloud/20261011-cockpit-polish`. Commits: a256000 (all six items), then the short nav labels and screenshots.

## What I found and changed (cockpit only)
1. **Room under the banner.** The state chips moved from top 72 px to 98 px; the selected-vessel card from 106 to 138 px. Checked at 1280 and 1920 px.
2. **Slide-out watch.** `src/ship/WatchDock.tsx`: the whole watch column (working now through since you last looked) keeps its place; a minimize control in its header slides it off, and a tab hangs on the right edge to bring it back. State is in localStorage (`cockpit.ship.watch`). It stays mounted while folded (nothing lost, keys 1-6 still work) and is `inert`. No slide under reduced motion or Calm. Choice to flag: I slid the whole column, not only the "since you last looked" plate.
3. **Depth gauge.** Smaller by default; minimize (a chip), maximize (a larger overlay that says what each depth is; Escape or the control closes it). Kept in `cockpit.ship.depth`.
4. **Accordion nav.** `src/lib/navgroups.ts`: Live (Ship, Bridge, Fleet, Actions), Memory (Ontology, Context, Books), Spend (Money, Economics, Judgment), Safety (Bounds, Policy), System (Ledger, Systems), Bench (Bench, Speed). Grouped by the question each answers. The current view's group opens on load and when you move into another group; only the operator's own folds are kept (`cockpit.nav.groups`). A folded group still shows the Actions approval count and a dot for the current view. Shortcuts unchanged. The rail has no collapsed mode.
5. **Catalog inside its box.** `lib/cardfit.ts` `CARD_BODY` (`min-w-0 overflow-x-auto`) is the body of every Systems card and the Systems panels; `Panel` gets `min-w-0`. A table wider than the card scrolls inside it.
6. **"?" explainer.** `Panel` has a required `help` prop (an id or `false`); `ChartPanel` and Systems `Card` pass it through. `components/Help.tsx` (radix Popover: Goal, How to read it, Why it matters; Escape and outside click close it; aria label). Words for ~120 panels in `lib/explainers.ts`. Also on the watch and depth widgets. The `help={false}` panels are only error, stopped and loading notices.

## Proof
- `npm test`: 330 pass (13 new in explainers, navgroups, prefs, cardfit); `npx tsc -b`, `npm run lint` (warnings only, none new but one I removed) and `npm run build` pass.
- Planted reverts, each caught: `entered()` forgetting the group (2 navgroups tests fail); `parseWatch` ignoring storage (prefs test); an explainer deleted (explainers test); `overflow-x-auto` dropped from `CARD_BODY` (cardfit test); a panel without `help` (tsc error plus 3 explainer tests). Files restored and touched.
- Live check on a scratch daemon (port 7434, fresh state) with headless Chrome: watch fold persists across reload; the "?" popover opens and Escape closes it; no console or page errors; at 1024 and 1280 px no Systems element runs past its card unless inside a scroller.
- Screenshots (1280 and 1920 px): `docs/assets/cockpit-1011/` (`ship-1280.png`, `ship-1920.png`, `*-watch-folded`, `ship-1280-depth-max`, `-depth-min`, `-help`, `systems-1024`, `systems-1280`, `systems-1280-help`). The scratch Ship was empty (no sessions), so it shows no vessels.
- Not covered: the catalog's wide-table scroll was checked by script, not by a screenshot of a scrolled table; the explainer text for panels I could not read in detail (Ontology, Context, Books, Judgment) follows the view headers and is worth an owner read.

## Live check for the maintainer
`cd cockpit && npm run build`, start a scratch daemon with `[web]` on 7434, open `/ship` (minimize the watch, reload: it stays folded and the tab reopens it; Depth min/max; Escape), `/systems` at 1024 px (Catalog scrolls inside its card), and click "?" on any panel.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in the keel phase** on findings that are not mine and are in files I did not touch: assert-removed `theseus-tui/src/tests.rs`; allow-added `theseus-index/src/vectors.rs` and `theseus-protocol/src/tests_work_join.rs`; ceilings raised in `scripts/long-files.txt`. `scripts/keel-signers` lists no key at the base here, so no ack can pass. The gate therefore did not reach its cockpit or suite phases; I ran the cockpit phase's commands (npm test, lint, build) by hand as above. No Rust changed. The Rust suite was not run.

**Keel findings expected from my commits:** none.

## Left or uncertain
Docs to update (not mine to edit): cockpit/AGENTS.md should list WatchDock, prefs, navgroups, cardfit, explainers (`help` required on Panel).
