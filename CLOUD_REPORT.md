# CLOUD_REPORT: p3g-card-selection (theseus-gnhnk)

Branch `cloud/20261025-p3g-card-selection`, base `0fda4d57`. Cockpit only.

## 1. For the maintainer

- Store format: untouched. No Rust, protocol, wire type or `protocol.gen/` change. No ceiling raised, no new dependency.
- **Touched outside the three named `views/Ops.tsx` hunks, because the change cannot compile without them** (all one-token
  edits, see section 11): line 29 (the `selects` import, now unused), line 58 (import of `noneCounts`, `rowFor`) and
  line 42 (import of `idleSel`, `OPEN_SEL`) and line 203 (the `regionsFor(...)` call gains its fifth argument).
  `OWNER CALL:` the brief forbids lines outside ~:183-191, ~:197 and ~:491; the new `regionsFor` input cannot reach
  `regionsFor` without line 203, so I made that one edit rather than leave item 3 undone. Revert it and the idle strip
  falls back to the base's behaviour for the none case.
- No STOP. The DM golden is unchanged.

## 2. The real cause

**The filed diagnosis was not right about the selection arms, and I could not reproduce a failure there in pure data.**
Over a fixture with a plan, nested steps, a suspended step, a waiting-human step, a step with a task, a loose step,
a step view, a job, wakes and a loose ask, `rows.find(selects(r, node.id))` finds a row for **every** node the Graph
lays out except its two fold nodes (`done:*`, `+N folded`), which are buttons of their own. A graph node's id is the row's
id (`ops/graph.ts` :158/:172, `id: r.id`), and `selects()` (`ops/board.ts` :381) matches `r.id` first. `isItemKey`
(`views/Ops.tsx` :73, `/^[qfsha]:/`) does not capture `tsk_` ids; `ops/address.ts` keeps any `sel`.
So at the page level a click on a step card does yield `Selection { kind: 'row' }`, and `regionsFor` then gives `side`.

What I **could** establish: the inspector had no body of its own for a step, so a step fell into `RowWhole`, which draws
a task's/conversation's card (no breadcrumb, no plan checklist, no objective-less record, no carrier); and the idle tab
(`views/Ops.tsx` base :491) did `pick(queue[0]?.key ?? null)`, a no-op with Needs you empty, so the panel could not be
opened by hand at all. I could not drive a browser against live data, so **the owner's exact "nothing opened on click" is
not reproduced here**; if it persists after this, the next suspect is the click never reaching `onNodeClick` (React Flow,
`GraphNode.tsx`, hl0r1's file) or live `rows` lacking the record rows. A test now pins the selection path so a regression
there names the node kinds (section 7).

## 3. The fix

A mapping, not a `record` kind. `rowFor(rows, sel)` in `ops/sections.ts` is the page's row lookup (`views/Ops.tsx`
:188), over the whole work set, never a filtered one. `selects()` is unchanged and sufficient; no follow-up needed for it.
`Selection { kind: 'none' }` gains optional `counts` and `onFilter`. "Opened by hand" is carried by the address's `sel`
as a sentinel, `OPEN_SEL = '~'` (`ops/regions.ts`): it names no row, so `Esc` (`back` → `pick(null)`) clears it exactly
like a selection and a link reopens the empty panel, with no new state and no edit to the `back` handler. The timeline and
threads arms ignore it (`handOpen`).

## 4. StepWhole (`ops/Inspector.tsx`, facts in `ops/sections.ts` `stepView`)

Chosen by `bodyOf(row)` (`job` | `step` | `row`). It draws: noun (plan/step), glyph + state word, short id; the breadcrumb
thread › … › parent, each crumb selecting it; for a plan *n of m closed* and the checklist of children (each selects);
`Record` (objective, acceptance, claim, evidence, via `recordFacts`/`RecordBlock`); `Waits on` as links; `History`
(created, "became a plan" from its first sub-step, closed) with the ledger link `/ledger?q=<tsk_id>`; then, when a task
row is attached (`stepView.task`: the row's own, or a task row of the step's session), that task's whole `RowWhole`
(facts, Live, pause, stop, cancel); otherwise "carried by ‹conversation›" with Join. No new RPC.
Differences from the brief: Join is shown with the key the page actually binds (`i`, not `c`: `c c` is cancel); the
"escalation" line has no stored timestamp, so it says when the first sub-step was added.

## 5. The idle strip and regionsFor

`regionsFor(width, queue, inspector, selected, byHand = false)`: with `byHand` and nothing selected the inspector is
`side` at wide width (`over` below 1280 and below 900); a folded inspector still folds. The idle tab calls
`pick(idleSel(queue[0]?.key))`: the first ask, else `~`. Empty state: "Nothing is selected. Click a card, or press j to
take the first.", the counts each a link that sets that bucket filter, and the key hints. Test: widths
{1440, 1280, 1100, 900, 899, 600} × queue fold × inspector fold × selected, each equal to the base rule written out as an
oracle, with `byHand` false (and true while selected) unchanged; plus the four by-hand cases.

## 6. Selection × filter

Proved in `ops-card-selection.test.ts`: a step the `paused` counter hides is still selectable (the lookup is over the
whole set), the filter's shown set is not widened by selecting it, clearing the filter in the address keeps `sel`,
and selecting keeps the filter.

## 7. Tests

`npm test`: 792 before, **802** after (10 new, in `test/ops-card-selection.test.ts`; no existing assertion changed or
removed). Plants (each reverted, file `touch`ed, `git status` clean):
- **P1** (`rowFor` skips step rows): red, 6 of 10 failures. At the base the owner's kinds fail as: `these node kinds
  opened nothing: step exe_hull; step tsk_loose; step tsk_plan; step tsk_shoals; step tsk_sound; step tsk_sub` (every
  step and plan node; conversations, tasks, jobs, wakes and asks still open).
- **P2** (task sections dropped from a step with a task): red, test 7.
- **P3** (idle tab restored to `pick(queue[0]?.key ?? null)`): red, test 8.
- **P4** (`regionsFor` ignores `byHand`): red, test 9.
Shift-click and right-click: GraphLens (not edited) calls `byId.has(n.id)` then `onPick`/`onContext`; a test proves every
laid-out card has a row, so both reach every card.

## 8. Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **did not reach the suite**: it failed in its `door` phase
(exit 1). Main's copy of the gate judges this tree against origin/main as fetched (`2d1d8d6f07`), which is ahead of my
base `0fda4d57`; its keel guard then reports erosions in `persona_cmd.rs` and `keel-guard.py` that are main's own
commits since my base (rejected acks with unverifiable signatures), none in my diff. My diff against my base is clean
(section 9). The Rust suite was not run: the cold workspace build also filled this VM's disk once (I cleared
`target/debug/incremental`), and no Rust file changed. What I ran instead, in `cockpit/`: `npm ci`; `npm test`
802 tests, 802 pass, 0 fail (792 at the base); `npx tsc -b` clean; `npm run lint`: warnings only, all in files or lines
that predate the change (no errors reported). `cargo deny fetch` was not reached.

## 9. Keel

`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`: no output, exit 0. Keel findings expected: none.

## 10. DM golden

`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt`: no diff, exit 0.

## 11. Join notes

Touched: `cockpit/src/ops/Inspector.tsx` (Selection none, StepWhole, NoneWhole, imports), `ops/InspectorSections.tsx` (not touched),
`ops/sections.ts` (appended: `bodyOf`, `stepView`, `noneCounts`, `rowFor`, words), `ops/regions.ts` (:29-35 region;
`OPEN_SEL`, `idleSel`), `test/ops-card-selection.test.ts` (new). `views/Ops.tsx`: lines 29, 42, 58 (imports), 188-192
(selection hunk), 203 (`regionsFor` call), 497 (idle tab). I touched none of hl0r1's four files, none of 0zraw's
(`explainers.ts`, its test, `TaskGraph.tsx`, `CommandBar.tsx`), no help button or explainer entry, `board.ts`,
`state.ts`, `ThreadsLens.tsx`, `threadboard.ts`. Lines outside the three named hunks: 29, 42, 58, 203 (above).

## 12. Follow-ups

- If the owner still sees a click do nothing on a step card, check React Flow's `onNodeClick` on step nodes in a live page.
- `docs`: cockpit/AGENTS.md "The regions" paragraph could mention `OPEN_SEL` and the empty state; not edited.

## 13. Data floor

Nothing new is sent to a model or routed anywhere.
