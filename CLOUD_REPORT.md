# CLOUD_REPORT: theseus-brcs4, the Threads lens over the one work set

Branch `cloud/20261025-p3g-threads-lens`, base `0fda4d57`. Task commit `bc33c721`.

## 1. For the maintainer
- Store format: untouched. No Rust, no wire type, no `protocol.gen/`, no ceiling raised.
- **OWNER CALL (wiring, left undone):** `ThreadsLens` takes new optional props `rows`, `kept`, `context` and `onSelect`. The page must pass them at `views/Ops.tsx` ~:436 (gnhnk's/not my scope): `rows={rows} kept={shownIds} context={contextIds} onSelect={pick}`. Until then the lens falls back to `boardRows` over the views and records alone and applies no counter filter, so the filter works in `threadWork` (proved by test) but is not yet live on the page.
- **OWNER CALL (parity table):** `ops/parity.ts` row H1 and `test/ops-parity.test.ts` pin the old line `f.items.map((it, i) => <Item key={it.id} v={t.items[i]} />)` inside `ThreadsLens.tsx`. I could not edit them, so the lens keeps that line as a fallback: it draws the old open items only for a thread whose work-set lines are empty. When parity.ts H1 is moved to the new `Lines`/`Line` components, the fallback can go.
- `groupThreads` and `listed` are unchanged: `Ops.tsx` (lanes, queue) and `ops/facts.ts` read `GroupedThread.items`. The lens's grouping is the new `threadWork`, which reads rows only. Fold them later if wanted.

## 2. The shape
`threadWork(rows: readonly OpsRow[], groups: readonly ThreadGroup[], opts?: { kept?, context?, calls? }): ThreadWork[]` in `lib/threadboard.ts`.
Each `ThreadWork` is `{ group | null (Home), lines: WorkLine[], needs, kept }`. A `WorkLine` is `{ row, depth, context, task?, carriedBy?, needs, progress? }`.
- Tree: a card hangs from its record's `parent`, else the row's `parent`; a card takes its own `thread`, else its parent's, else Home. A child whose thread differs from its parent's is a root in its own thread.
- A task carrying a record is one line (`task` is the row itself). A taskless step says `carried by <title>` or `nobody carries it`. Jobs and wakes nest under their host.
- Before/after (fixture): before, a thread listed only its open tasks/questions/wakes flat; after:
```
harbour-survey
  exe_hv (conversation)
    Survey the quays   ▾ 1 of 4 done · 1 working · 1 paused · 1 to do · 1 done
      Measure pier one  ☐ carried by exe_hv
      Chart the shoals  ▾ 0 of 2 done · 1 working · 1 to do
        Sound the channel (to do)   Mark the buoys (working)
      Paint the beacon (paused)
    exe_ak  [needs you]   Lay the cable [task] ▾ 0 of 1 · Pull the line, cargo test (job)
tide-tables ... ; Home: wake, loose ask [needs you]
```

## 3. Progress
A plan's n/m is over the leaf cards of its subtree: rows with a record that are not plans (steps and tasks carrying a step), each counted once through a `Set`. `n` = leaves in state `done`/`complete`. The words list open buckets (need you, working, scheduled, paused, to do), then `done`, then `ended`. It is derived from rows, not from hl0r1's Graph helper, which I did not see; fold the two onto one helper at the join if they differ.

## 4. Q1 and Q2
`threadWork` takes `calls` (default `OWNER_CALLS`). Q1: a line's needs mark is `calls.needsPerCard ? 1 : max(asks,1)`. Q2: `todo` is its own word in the summary under `calls.todoApart`, else folds into paused. Nothing is hard-coded. Row buckets/columns come from `boardRows`, so a to-do step has no column and no counter.

## 5. Filter and parity
Kept cards are lines; their tree ancestors are drawn faint (`context: true`); `contextOf` jobs and carried steps are drawn faint under a kept parent. A thread (or Home) with no kept card is not returned. No `shown`/`contextOf` re-derivation: they are passed in. The clear chip is not drawn. Test: for each bucket (needs, working, paused, still, done) the counted kept lines equal `allCounters` (needs: sum of line marks), and every kept id is listed exactly once. Fixture counters are asserted equal for all five buckets.

## 6. Tests
`npm test`: 792 before, 797 after (5 new, in `test/threadboard-work.test.ts`; no existing assertion changed). Plants (each restored, `touch`ed, `git status` clean):
- P1 emit task as own entry: tests 1 and 5 red.
- P2 double-count: tests 2 and 4 red.
- P3 asks instead of cards: tests 3 and 5 red.
- P4 fold todo into paused: tests 2 and 4 red.
- P5 list thread without kept card: test 5 red.
Base-red is by construction: `threadWork` did not exist at `0fda4d57`.

## 7. Gate
- `npm ci`, `npm test` (797 pass, 0 fail), `npx tsc -b` (clean), `npm run lint` (warnings only, same files as before; none in my files).
- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: **failed in the door/keel phase**, before any build. Cause: main's `gate.sh` judges the tree and its keel guard refuses five historic acks on main (`persona_cmd.rs`, `keel-guard.py`, ...) because their signatures do not verify (`%G? is E`) in this clone. It names no file of mine. I did not run the Rust phases (no Rust change; cargo-nextest and cargo-deny not installed here).

## 8. Keel
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`: no output, exit 0. Keel findings expected: none.

## 9. DM golden
`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no diff.

## 10. Join notes
Touched only: `cockpit/src/lib/threadboard.ts` (appended ~L190-324 plus the import line near the top), `cockpit/src/ops/ThreadsLens.tsx` (imports, props, `Group` body, new `Home`/`Lines`/`Line`), `cockpit/test/threadboard-work.test.ts` (new). I touched none of hl0r1's four files (GraphNode, GraphLens, graph.ts, BoardLens) nor 0zraw's (explainers, CommandBar, TaskGraph), added no help button, and hid no ended or stale row: the lens lists whatever the work set gives and leaves the choice to 99l9z.

## 11. Follow-ups
Wire the three props in `Ops.tsx`; move parity H1; consider a `Line` using hl0r1's badge strip once it lands; the thread header's old items count still comes from `threadFacts`.

## 12. Data floor
Nothing new is sent to a model or routed anywhere; this is a display change over data already on the page.
