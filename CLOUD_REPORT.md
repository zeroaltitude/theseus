# CLOUD_REPORT: p3g-graph-forms (theseus-hl0r1)

Branch `cloud/20261025-p3g-graph-forms`, from `0fda4d57`. Cockpit only; no Rust file changed.

## 1. For the maintainer
- Store format: untouched. No wire type, no `protocol.gen/` regeneration, no ceiling or cap raised (`NODE_CAP` is 400).
- No STOP. No OWNER CALL needed; `OpsRow` gained no field (all derived in `ops/forms.ts`).
- **The full gate did NOT pass here, and not because of this branch.** `scripts/gate.sh` hands the tree to main's newer copy (the "door"), which runs the keel guard against current `origin/main`. It reports 5 erosions (`persona_cmd.rs` tests removed, `keel-guard.py` and `test_keel_guard.py` changed) and refuses two acks whose signatures do not verify. I touched none of those files. The phases after the keel never ran, so I ran the cockpit checks myself (section 8). Rust fmt, clippy and the Rust suite were not run on this branch: nothing in Rust changed.
- The VM's disk filled once during the session (a cold `cargo build --workspace` is about 19 GB). I deleted `target/debug/incremental` and the gate logs (all rebuildable).

## 2. Each form (file, then before and after)
All forms come from `ops/forms.ts` `formsOf(rows, now)`, a pure function over the one work set. `graph.ts` puts each result on its node as `GNode.form`.
- **thread head** `GraphNode.tsx` `Work`: a conversation that is its lane's thread (`GNode.head`). It shows "thread", the lane's words and seal, the plan *n/m*, and the live line. Clicking it selects the thread slug (`GraphLens` `onNodeClick`; `?sel=` is a slug, and the lens finds the head by lane key). The lane band glows amber when `lane.needs` (`Lane`).
- **anchor**: the navy conversation card, as before.
- **plan** `Plan`: folder tab with ▾/▸, "PLAN" and short id; title; n/m bar (done in ok, open segments by bucket); folded summary line and "● N needs you". The chevron calls the lens's fold.
- **step** `Work` with `form.look`: checkbox glyph in its state colour, "STEP · id". To do is `ink-dim`; only `ended` is faded.
- **task (attached)** `BadgeStrip`: glyph, "TASK", model chip, spend bar, `progressLine`. A claim reads "worked by ‹title›". A task row that carries a record is the plan or step card, with the badge. No node of its own.
- **job**: the pill, as before. **ask** `Ask`: small amber card with its keys. **project** `Future`: ◈, dates, n/m bar. Wake, routine and watch: as before.
- Before, the fixture's plan, steps and task were all identical work cards. After, rendering in text:
```
[thread · exe_home  plan 2/7 · 2 needs you · 1 working · 2 to do · 2 done]
  ▾PLAN Survey the quay  ====2/5   [◐ TASK ◇sonnet  on sonnet]
      ◐ STEP Measure pier one    ☐ STEP Measure pier two    ☑ STEP Order tape (faded)
      ▸PLAN Write it up  1/2  "1 needs you · 1 done"  ● 1 needs you   → ● Draft the chart (drawn anyway)
  ■ STEP Wait for the harbourmaster     ☐ STEP Backlog one          (job pill beside the plan)
```
(I drew this in a throwaway Vite page and looked at the screenshot. It showed a plan with a badge taller than a row and its progress line twice. Both were fixed in 0b82665f. The scratch page is deleted.)
- **Board** `BoardLens.tsx`: plan cards with chevron, PlanBar, summary and their direct children's glyphs; steps with glyph and "STEP · id". Columns are untouched.

## 3. progressOf
`progressOf(rows, planId): Progress { n, m, counts (by bucket), failed, summary, needs }` in `forms.ts`. Children are resolved by the same keys as the graph (id, view id, record id, execution id). Only leaf units count (a step, or a task that carries a record, with no unit below it), so a nested plan is its children's, once. A visited set stops cycles. Edge cases: abandoned work is left out of *m*; failed counts in *m* but not *n*; a row with no children gives 0/0.

## 4. Escalation in place
A row is a plan if `row.plan` or it has a unit child. The row, its id, its `startedMs`, its record and its slot are unchanged. Layout slots are sticky by id, and the form is derived each layout. The test checks the same id, the same x and y, `fresh` false, the same record and created time. `?sel=` names the id, so selection survives.

## 5. Folding and the cap
`graph.ts`, in the block before the done-fold: a plan is open if `LayoutOpts.folds` says so, else while its bucket is not `ended`. A folded plan's descendants are deleted from `shown` (mapped into the plan for edges), except cards that need the owner. They are never in `shown`, so a folded plan is one node toward `NODE_CAP`. The owner's folds are `GraphLens`'s `folds` state (like `opened`), not the URL. Tidy leaves them alone.

## 6. The filter through the forms
`keep` opens a folded plan that holds a kept card; a lane with no kept card shrinks to its head (`rows` 0, height 18); kept nodes keep their slots. Held by the test "the counter filter through the forms".

## 7. Tests and plants
`npm test`: 792 before, 802 after (10 new in `test/ops-forms.test.ts`). Each plant was red, then restored with `touch`, and `git status` was clean after:
- P1 (to do mapped to faded): test 1 failed.
- P2 (nested plan counted too): test 2 failed.
- P3 (a task node emitted beside the step): test 3 failed. I did not run the parity test against this plant.
- P4 (morphed card given a new id): tests 5 to 8 failed.
- P5 (folded plan's children kept in `shown`): tests 6 to 8 failed.

No existing `ops-graph.test.ts` expectation moved.

## 8. Gate
- `npm ci`, `npm test`: 802 tests, 802 pass, 0 fail. `npx tsc -b`: clean. `npm run build`: ok.
- `npm run lint`: exit 0, warnings only. GraphNode's `only-export-components` count rose from 6 to 10 (one per new component in a file that also exports non-components). `Ops.tsx` warnings are as before.
- `scripts/gate.sh`: failed in the door/keel as in section 1.

## 9. Keel
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` exited 0 with no output. Keel findings expected: none.

## 10. DM golden
`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` exited 0 (no diff).

## 11. Join notes
Neither `lib/explainers.ts` nor `ops/CommandBar.tsx` was touched; no help button was added.
- `ops/forms.ts`: new.
- `ops/graph.ts`: import ~:18; `GNode` fields ~:62-68; `LayoutOpts.folds` ~:104; plan block ~:222-238; node push (form, open, head) ~:340.
- `ops/GraphNode.tsx`: imports; `NodeData` (`onPlan`, `laneMeta`); `BadgeStrip`; `Work` rewritten; `Plan`; `Ask`; `Spend`; project in `Future`; lane glow; `typeOf` now takes the node.
- `ops/GraphLens.tsx`: `folds` state and its three layout calls; head focus and click; a toolbar comment; legend.
- `ops/BoardLens.tsx`: `rows` prop; plan and step cards; `PlanBar`.
- `views/Ops.tsx`: **one prop**, `rows={rows}` on `<BoardLens` (~:434, not :440).
- `test/ops-forms.test.ts`: new.
- **99l9z's checkbox group** goes in the Graph toolbar in `GraphLens.tsx`, between the "+ future" button and the "tidy" button, at the `{/* theseus-99l9z … */}` comment. Read `row.bucket` or `GNode.bucket` for the classes.

## 12. Follow-ups
- The Board's plan fold only lists a plan's children inside its card; the children also stay in their own columns.
- The head's "plan n/m" is progress over the whole thread's steps; the daemon has no separate plan-mode count.
- ROW_H (140) barely fits a plan with a badge.

## 13. Data floor
Nothing new is sent to a model or routed anywhere.
