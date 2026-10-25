# CLOUD_REPORT: p3e-ops-oneset (theseus-odj0m)

Branch `cloud/20261024-p3e-ops-oneset`, from `ac298666`. Cockpit TypeScript only: no Rust, store, wire or protocol change, so no cargo setup was run.
Commits: `dbe3c98b` (the one work set, its tests), `7fb19480` (the Graph fits to the kept cards once per filter change).

## 1. For the maintainer

Store format: 51, untouched. Ceilings: none raised; `scripts/shape.sh` says ok. Sizes now: state.ts 189, board.ts 490, queue.ts 240, graph.ts 439, GraphLens.tsx 256, views/Ops.tsx 539, CommandBar.tsx 163 (the "ceilings" you listed were the base's line counts; board.ts, graph.ts, state.ts and GraphLens.tsx grew because the rows now carry the logic that three files kept apart).

`OWNER_CALLS` (exported from `ops/state.ts`) holds one boolean per question, all `false` (today's answers). `RECOMMENDED` beside it holds all `true`. The flip of a question is the one word `false` -> `true` in the `OWNER_CALLS` literal at `state.ts:31`.

OWNER CALL: Q1. "Needs you counts pieces of work: one per card that waits on you, however many questions it has (A, recommended), or every question separately, as the Needs-you list's length today (B)?" Field `needsPerCard`, flip `needsPerCard: true`. No existing expectation moves (the existing tests never count asks). Only my own default-pinning test moves (ops-board.test.ts:235, needs 0 -> 2). Guard: `exit=0`, no output.

OWNER CALL: Q2. "To-do steps (listed, not started): a state of their own, *to do*, with no counter and no Board column, shown in their plan's n/m and on the Graph (A, recommended), or counted under paused·queued (B)?" Field `todoApart`, flip `todoApart: true`. Existing expectation that moves: ops-state.test.ts:78 `bucketOfRecord('accepted')` and `'proposed'`: 'paused' -> 'todo' (it reads the default parameter). My new tests that pin the default also move (ops-board.test.ts:226, ops-graph-state.test.ts:29, ops-state.test.ts:125). Guard: exit 0, no output (no test file was touched by the flip).

OWNER CALL: Q3. "Working counts agents only: running tasks and turns. A step being done and a running job are drawn working but are not counted again. Yes/no? (Recommended yes.)" Field `agentsOnly`, flip `agentsOnly: true`. Existing expectation that moves: ops-board.test.ts:103 (the counters test) `working: 3` -> `2` (the job `act_j1` stops counting). Mine: ops-board.test.ts:232. Guard: exit 0, no output.

OWNER CALL: Q4. "A counter click hides everything else on the Graph and the Board, keeping only each match's parents as faint context, with a "paused·queued · 4 ×" chip (A, recommended), or only outlines the rest, as today (B)?" Field `hideRest`, flip `hideRest: true`. No existing or new test moves: the Graph lens reads it as a defaulted prop (`calls`), and the layout's hide (`LayoutOpts.keep`) is tested by passing it explicitly. **The Board is not hiding anything under Q4 A yet**: the Board already draws only the kept rows (the filter always removed the rest there); what Q4 A adds on the Board is the faint context, which needs `BoardLens.tsx` (hl0r1's file). The chip is built and the Graph wears it today under both answers (it is not an owner call). Guard: exit 0.

OWNER CALL: Q8. "Rename the counter "still today" to "scheduled today" to match the Board's Scheduled column. Yes/no? (Recommended yes.)" Field `scheduledLabel`, flip `scheduledLabel: true`; `CommandBar` takes `calls` (defaulted) and words the counter and its table row by `counterLabel`. No existing expectation moves; mine: ops-board.test.ts:247 ('still today' -> 'scheduled today'). The `still` key stays in the URL. Guard: exit 0.

OWNER CALL: Q9. "A step left "doing" whose agent has stopped counts as paused·queued ("nobody is working it"), not working. Yes/no? (Recommended yes.)" Field `stalledIsPaused`, flip `stalledIsPaused: true`. No existing expectation moves; mine: ops-state.test.ts:125 and ops-graph.test.ts:361. Guard: exit 0.

OWNER CALL (a conflict between two instructions, not a keel stop): two existing tests pin the **source text** of `views/Ops.tsx`: `ops-notices.test.ts:67` wants `needs: queue.length` and `ops-parity.test.ts:82` wants `<GraphLens views={world ? graphViews : views} records={records}`. The brief asks that `Ops.tsx` stop using `queue.length`, and the Graph takes rows alone, but also that no test be edited. I kept both tests green without editing them: a comment in `Ops.tsx` (above `counts`) says Q1 B's needs is what `needs: queue.length` was; `graphViews` is kept and the page still passes `views`/`records` to `GraphLens`, which declares them as unused optional props. **Suggested join edit:** change those two test lines to pin `allCounters(rows` and `<GraphLens rows={rows}`, then delete the comment, `graphViews`, and the two dead props.

OWNER CALL (a deviation in shape): the brief says `countersOf` computes all six counters. The existing `ops-board.test.ts:103` `deepEqual`s its result against five keys (no `needs`), so `countersOf(rows, now, spent)` still returns those five and the new `allCounters(rows, now, spent, calls = OWNER_CALLS)` returns the six; the page uses `allCounters`. Both read rows only.

## 2. The one set

`boardRows()` (ops/board.ts) returns one row per card. New or changed `OpsRow` fields: `bucket` (a `StateBucket`), `column` (`Column | null`), `counted`, `asks`, `recordId`, `record`, `plan`, `carrier` (row id), `parent` (what the Graph hangs it from), `item` (an ask row's queue item). `RowKind` gains `step`, `ask`, `watch`. Row ids are unchanged: a task-carrying record keeps `ses_…` and adds `recordId: tsk_…` (matched by `record.session` or by the `tsk_X`/`ses_X` tail); a taskless record's row id is its `tsk_…`; an ask row's is its queue key (`a:…`, `q:…`, `s:…`). `selects()` matches both ids.

Rows: tasks (`rowsOf`), conversations, jobs, wakes/routines/watches (every one is now a row; those not due today, and watches, have `column: null`, are not counted, and show only under the Graph's "+ future"), step/plan rows from `records` and from `WorkKind::Step`/`Project` views, past-session rows, and ask rows.

`bucketOfCard(card, now, calls = OWNER_CALLS)` (ops/state.ts, the only place a state maps to a bucket): an ask on the card, or kind `ask`: needs. A record `waiting_human`/`blocked`: needs. A task row: the old `columnOfTask` as buckets (failure not dismissed needs; section done ended; a start later than now scheduled/paused; paused or queued paused; else `bucketOf(view)`), with a suspended record turning ended into paused. A view: `bucketOfWork`. A record without view: proposal waiting needs; `in_progress` working, paused if its carrier is not in flight and Q9; `suspended` paused; `accepted`/`proposed` paused, `todo` under Q2; done/failed/abandoned ended. A wake or routine run due today is `scheduled` (as the Board always kept it), a later one has no column.

Ask mapping (`askKeysOf`, queue.ts): an item names, in order, its question's parent, its proposed record, its approval's task, its session, its view, its execution; the first card any of those names gets `asks += 1`, so a seal (sessionId = its thread head) lands on the head. None named: an ask row (`kind: 'ask'`, thread from the item, Home when none).

`counted` = the row has a column, and not (Q3) a job or a taskless `in_progress` step whose carrier is in flight. Thread heads are not rows (threads are lanes), so nothing to exclude; folds are Graph nodes only.

Counters: `countersOf`/`allCounters`/`needsOf` count `counted` rows with a column, by bucket (done: column done and ended since midnight). Their doc comment carries the design's definitions. `COLUMN_OF: Record<StateBucket, Column | null>` is exported; `columnsOf` skips a null column; the time machine's resting conversations now get `column: null` from the builder, replacing the `r.rest` skip (`rest` stays a flag for the Graph anchors; nothing tests it for a column or a count).

Graph: `layoutRows(rows, lanes, prev, opts)` lays out the rows; `opsLayout(views, records, …)` stays as the shim existing tests call (it builds rows through `boardRows`). Node ids and sticky slots are unchanged. New `LayoutOpts.keep` (Q4 A): nodes outside it are not drawn, their slots and the `bands` memory are kept (nothing moves when the filter clears), and a lane with none shrinks to its head. `contextOf(rows, kept)` is each kept row's ancestors plus the jobs and carried steps of kept rows. `Ops.tsx` passes `shownIds` (from the rows) and `contextIds`; the lens outlines context (hover title "context"), outlines or hides the rest by `calls.hideRest`, wears `FilterChip` ("paused·queued · 4 ×"), and fits to the kept cards once per filter change.

## 3. Per owner call: see section 1 (each line has the flip, the moved expectations and the guard).

Method: `scratchpad/flip.sh` flipped the field in a throwaway commit, ran `npm test`, ran `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`, and `git reset --hard HEAD~1`. Nothing flipped was pushed. The guard printed nothing and exited 0 in every case, because a flip edits no test; it would flag a test someone then edited to fit.

## 4. zo72s

Flip: `rest: 'done'` -> `rest: null` in `COLUMN_OF` (`board.ts`, one line), throwaway, dropped. Moved, all in ops-board.test.ts: `the columns…` (:20, `ses_home` leaves Done), `filters…` (:63 and :64, bucket `done` and `ended` lose `ses_home`), `the counters…` (:103, done 2 -> 1): three tests. **One more moves, outside ops-board.test.ts:** ops-state.test.ts:36 (`the Board, the Graph and the Threads lens name the same open work`) builds its open set with `r.column !== 'done'`, which reads a null column as open and adds `ses_h`. Nothing else moves (5 failing of 746). No other code tests `rest` for a column or a count.

## 5. Tests and plants

New: `test/ops-counts-parity.test.ts` (4 tests: the parity under the defaults and under the recommended answers, every node a row, the Needs-you length). Added beside the existing ones, none rewritten: ops-board.test.ts (+7), ops-state.test.ts (+2), ops-graph.test.ts (+3), ops-graph-state.test.ts (+1). `npm test`: 746 pass, 0 fail (729 before).

Red at the base: copied into a worktree of `ac298666`, all five test files fail to load (`does not provide an export named 'OWNER_CALLS'`, `'COLUMN_OF'`, `'contextOf'`, `'RECOMMENDED'`): whole-file red, not assertion-by-assertion.

Plants (each reverted with `git checkout` and `touch`; `git status` clean after):
- P1 the Graph lays out records beyond the rows (a duplicate node for each carried record): parity `every graph node is a row…` red, and the existing edges and open-work tests red too.
- P2 drop the ask mapping: `an ask maps to its card`, `a seal maps…`, the Needs-you length test red (and the existing `lanes…` test, which reads the question badge).
- P3 route tasks past the one path (the old `columnOfTask` role): 7 red: `the columns`, `a deferred start`, `the time machine's board`, `filters`, `the counters`, `countersOf reads the rows alone`, and `bucketOfCard has one path for a task`.
- P4 `shown()` drops step rows (shownIds from the old rows): both parity tests, `a counter click keeps records too`, and `the filter keeps records` (ops-graph) red.

## 6. Gate

Run in `cockpit/`: `npm ci`, `npm run lint` (0 errors; the existing warnings), `npm test` (746/746), `npm run build` (ok), `bash scripts/shape.sh` (ok). `scripts/gate.sh` was not run: this row changes no Rust and the cargo setup was skipped.

## 7. Keel

`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` printed nothing, exit 0. Without `--quiet`: `keel: ok (ac298666..HEAD, ac29866655; 16 files changed, 0 findings acked, 0.1 s)`. No `Keel:` trailer.

## 8. DM golden

`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` exits 0, no output. Nothing in Discord changed.

## 9. Join notes

Files and the new/changed lines (HEAD): `ops/state.ts` 12-35 (`OWNER_CALLS`, `RECOMMENDED`, `todo`), 70, 75, 110, 131-189 (`bucketOfCard`, `countedCard`, `bucketOfNode` takes `bucket`); `ops/board.ts` 20-310 (rows, `COLUMN_OF`, `boardRows`), 379-490 (`inBucket`, `selects`, `countersOf`, `allCounters`, `needsOf`, `counterLabel`, `chipWords`, `contextOf`); `ops/queue.ts` 131-139 (`askKeysOf`, mapping only); `ops/graph.ts` 97-180 (`layoutRows`, items from rows), 286-330 (`keep`, hole slots), `Graph.hidden`, `GNode.bucket`; `ops/GraphLens.tsx` 20-47, 72-90, 126, 149-160, 225; `views/Ops.tsx` 122-152 (queue before rows, `boardRows` takes `queue`, `contextIds`, `allCounters`), plus the `<GraphLens …>` props (~447); `ops/CommandBar.tsx` (`calls`, `counterLabel`, the tooltip); new `ops/FilterChip.tsx`.

**Outside the named scope, forced by types:** `ops/BoardLens.tsx:93` `LOOK[r.column ?? 'paused']` (the column may be null now) and `ops/palette.ts:38,176` (`targetOf` maps `step` -> `project`, `ask` -> `question`, `watch` -> `watch`; `entitiesOf` skips step and ask rows). hl0r1 and gnhnk start from this tip, so these are their base.

The chip's import line for `BoardLens.tsx`: `import { FilterChip } from './FilterChip'`, rendered as `<FilterChip bucket={…} n={counts[bucket]} onClear={…} calls={…} />` (the page already computes `counts` and `filter('bucket', null)`).

For the four rows: **hl0r1** — a plan row has `plan: true`, `recordId`, `parent` (a ref: resolve through row ids, view ids, record ids), `carrier`, `asks`; a Graph node has `bucket` and `asks`; a node of kind `ask` falls to the `work` card type; the `n/m` of a plan is yours to derive from rows whose `parent` names it. **gnhnk** — `selects()` matches `recordId`; an ask row's id is its queue key and `Ops.tsx` resolves an item key to the queue item first; `row.record` is the record of a task row too. **brcs4** — `bucketOfCard` takes a `kind` and `record` for a row with no view; the Threads lens still reads `groupThreads` and was not touched. **0zraw** — the counter tooltip and the chip say "paused·queued · N"; "still today" is "scheduled today" only under Q8.

## 10. Follow-ups

- The Needs-you header's "N asks" word under Q1 A (in `Ops.tsx`, outside this row's lines).
- `select_all` (⌘A, `Ops.tsx` ~339) picks `vis.rows`, which now includes rows no lens draws (to-do steps, later wakes, ask rows); it should pick the rows a lens shows. The palette's `visible` scope has the same property. Palette verbs for a step or a plan row are not designed (`step` is mapped to `project`, which offers only inspect).
- Esc already clears the filter (`back`, last), but a selection or a picked set is cleared first; the brief's "Esc clears the filter" holds only when nothing is selected.
- The Board has no context cards for a counter click under Q4 A (BoardLens is hl0r1's).
- `OWNER_CALLS` is read as a defaulted parameter, never from a prop of the page, so answering a call is the one-word change; if the owner prefers some answers per browser, that is a different design.
- Not run: a browser check of the Graph (no scratch daemon here); the build and the pure tests pass.

## 11. Data floor

Nothing new is sent to a model or routed anywhere; the change reads the page's existing reads and draws them.
