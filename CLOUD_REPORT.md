# CLOUD REPORT: ops-graph (theseus-jid7t.4)

Branch `cloud/20261023-ops-graph`, from the lane `integ/ops` at `fd21a228`. Started 2026-10-23 23:39 UTC; report
written 2026-10-24 01:17 UTC.

**The store's format: no bump.** This row changes only the cockpit: no Rust, no protocol type, nothing in
`src/protocol.gen/`. `MANIFEST_FORMAT` stays 49.

## Commits

| commit | what |
|---|---|
| `2fb3868` | cockpit: the Graph lens's layout, a pure `opsLayout` over lanes per thread |
| `7ecdd65` | cockpit: the selection, the bulk verbs with the owner's decision 4, the fan-out and the drop rules |
| `22112fe` | cockpit: the Graph lens, multi-select, the bulk bar and drag to a project or a lane |
| `c21920d` | cockpit: the Graph lens's first view, its minimap and a pick box that keeps the keys, from the live check |
| `0fc5019` | cloud report: the screenshots (not for main) |
| (last) | cloud report (not for main): this file |

The first two commits are the pure halves, each proved alone in a scratch worktree (`npm test`, `tsc -b`, oxlint)
before it was committed. The third is the UI. The fourth fixes five faults the live check found (below).

## What I built, item by item

1. **`opsLayout`** (`cockpit/src/ops/graph.ts`, tested by `cockpit/test/ops-graph.test.ts`, 9 tests). Pure: it
   imports only the protocol's types and `board.ts`'s `OpsRow` type. `opsLayout(views, records, lanes, prev, opts)`
   returns the nodes (each with its lane, column, row and pixel position), the edges, the lanes with their bands, the
   slots (the next call's `prev`) and how many nodes the cap folded. Beside it: `viewsOf(rows)` (the time machine's
   graph: views made from the tasks and sessions as they stood), `nodeIdOf`, and `keeper()`, which holds the last
   layout for the lens (a new `epoch`, the lens's "tidy", forgets it).
2. **The Graph lens** (`cockpit/src/ops/GraphLens.tsx`, `cockpit/src/ops/GraphNode.tsx`), mounted in
   `Ops.tsx`'s `a.lens === 'graph'` branch in place of `TaskGraphView`. It fills the canvas region. React Flow
   draws the lanes as bands (a node type of their own, so they pan and zoom with the graph; their label in the
   gutter in Cinzel, engraved, with the thread's warmth, last activity, context, today's dollars, a lock when sealed
   and "● N need you"), the nodes and the edges; a minimap, zoom and fit (the Controls). `?sel=` pans to its node
   (a task's record id, `tsk_…`, finds its task, so the task graph's link lands). Node types: `work` (a
   conversation, the anchor of its lane; a task; a step) with the state header (glyph and word), short id, title,
   progress line (`progressLine`), spend bar (spent against limit, in money), the model chip (`modelOf`: the
   profile, or `moved_to`), place, a lock with the word for private or sensitive data, a question badge ("? 1"),
   and, when it needs the owner, an amber glow and its keys printed on it (`y approve · n decline`, `1–N choose`,
   or `r retry · d dismiss`); `job` (a pill beside its session); `fold` ("✓ N done", which opens on a click; "+N
   folded", which says to narrow with the filters); `future` (wakes, routines, watches, projects; a project takes a
   drop). Glows: amber for needs-you, gold for picked or focused, cyan for running. A running edge flows (`ops-flow`
   in `index.css`): under stir's rule, like every endless decoration, and stopped in Calm and under reduced motion.
   A new node fades in once (`ops-fade-in`, also stopped by Calm). The legend at the foot says the four lines and
   the gestures. The first view comes with the first nodes: the whole graph when it fits at a readable zoom (0.62),
   else its top-left at that zoom, where the needs-you lanes are.
3. **The selection** (`cockpit/src/ops/selection.ts`, pure, tested by `cockpit/test/ops-selection.test.ts`):
   `toggle`, `selectAll(visible)`, `clear`, `bulkVerbs(rows, queue)` (each verb with its label, its items and the
   reason when none admits it), `fanOut(items, run, 4)`, `afterRun(selection, results)`. The store is
   `cockpit/src/ops/useSelection.ts` (zustand, which the cockpit already uses). **The seam for ops-act** is below.
4. **The bulk bar** (`cockpit/src/ops/BulkBar.tsx`): `BulkBar` (shown over the canvas on the Graph and the Board
   while something is picked), `useBulk` (the table, the sheets, the run), `Sheet`, `ResultsToast`. The ask-once
   sheet lists every item; the Approve review sheet is decision 4's; the re-home and attach sheets pick the home or
   the project first and show, per item, what will happen or why not (with the budget sentence for each re-home).
5. **Drag** on the Graph and the Board: the pure rules are `cockpit/src/ops/drag.ts` (`dropOf`, `dragOf`,
   `budgetWords`, `dropKey`); the hand is `cockpit/src/ops/DropLayer.tsx` (`useDragDrop`): the cursor's words
   while a drag hovers anything marked `data-drop`, a tray of the threads' lanes and the projects at the foot while
   a drag is on (the Board has no lanes; the Graph has projects only with "+ future"), the re-home sheet, and the
   one call. The Graph drags with React Flow's node drag and finds the target under the pointer
   (`elementsFromPoint`, skipping the dragged node); the Board uses HTML5 drag and drop. After the daemon answers,
   the results toast shows its `note` ("the budget stays carved from the original parent") or its refusal.
6. **Keys**, appended to `KEYS` in one block marked `ops-graph (theseus-jid7t.4)`, each with its overlay line and
   its `handlers` entry: `␣` `select` (ctx `card`), `⌘A`/`ctrl+a` `select_all` (ctx `page`, named `mod+a` by
   `keyName`). `esc`'s `back` clears the picks before anything else (its words changed to say so). The bulk verbs
   are buttons: `bulk_pause`, `bulk_stop`, `bulk_cancel`, `bulk_rehome`, `bulk_approve`, `bulk_decline`,
   `bulk_attach` and `drop`, appended to `BUTTON_ACTS` in one marked block, so `enabled` turns them off under the
   time machine.
7. **Parity** (`cockpit/src/ops/parity.ts`): X7 and B1 moved, H1 gains `items`, H6 gains `openWork` (below).
8. **`cockpit/AGENTS.md`**: two paragraphs in the Operations section (the Graph's layout rules; the selection, the
   bulk rules and decision 4, and drag), and the lenses' line no longer says the Graph hosts `TaskGraphView`.
9. **Screenshots**: `cloud-report/ops-graph/` (below).

## The layout's rules as built, and readability at 400 nodes

- **Lanes**: one per live thread (`board.groups`, with `threadFacts`' words), grouped by `WorkView.thread`; work in
  a thread that is not live, and work in none, is in the **home · no thread** lane. A node with no thread of its
  own takes its parent's lane. On a fresh layout the lanes are ordered needs-you first (the layout counts the
  needs-you nodes in each lane as well as the thread's own count), home last. An empty live lane is still drawn: it
  is a drop target.
- **Columns by depth, rows by start time.** A conversation is column 0; a task, step or job is one column right of
  its parent when the parent is in the same lane, else column 1. Within a lane and column, new nodes are placed in
  start order, so a fresh layout's rows are by start time.
- **Sticky by id.** Given the last layout, a node keeps its slot (lane, column, row) as long as its lane is the
  same; a new node takes the row after the last one used in its column (holes are kept, so nothing after them
  moves); a lane's band keeps its rows while they fit, and a band laid out afresh has one spare row, so a single new
  node never grows it. Lanes keep the last layout's order; a new lane goes below. The one exception is deliberate:
  a lane that comes to need the owner does not jump up while live (that would move every lane under it); its label
  glows amber, and **tidy** (the lens's button) lays everything out afresh, needs-you first, holes closed. A layout
  that placed nothing (the page before its first read) keeps no order.
- **Done-folding**: tasks, steps and jobs that ended more than an hour ago (`ended_at_ms`, else `state_since_ms`)
  fold into one `done:<lane>` node per lane, column 1; a failure that still asks for the owner never folds; a click
  opens the lane's fold (`expanded`), and tidy closes them all.
- **The cap** (`NODE_CAP` = 400): past it, the subtrees fold, deepest root first, oldest first among equals, each
  into one `fold:<root>` node ("+N folded"), until the count is under the cap; a subtree that holds a node that needs
  the owner never folds, and a subtree of one is not worth folding. Folded ends' edges go to their fold, once.
- **Readable at 400**: lanes, sticky positions, done-folding and the cap keep it bounded; the filters and the search
  (the address's `f` and `q`) fade what they leave out to 30% instead of removing it, so nothing moves while you
  narrow; the first view never goes below a readable zoom (the minimap has the rest); folds count what they hide.
- **Edges**: `WorkView.parent` (solid, `#9c907a`; a child that runs makes it flow, cyan); `TaskRecord.deps` (dashed
  with an arrow, from the task to what it waits on, via the record's task); `WorkView.retry_of` (dotted, in the think
  tone). `retry_of` is on `WorkView` in the protocol, not on `TaskRecord`; I read it there.
- **Records**: a record whose `session` is a task's session is that task's node (its deps become the task's
  edges); a record with no such task is a step node (`tsk_…`), under its parent record's node or its origin's
  conversation; projects and milestones are "+ future" nodes in the home lane.

## The bulk verbs, their postures, and decision 4

| verb | admits | call (one per item) | posture |
|---|---|---|---|
| Pause N · resume M | a living task not in Needs you or Scheduled | `task.pause` / `task.resume` | do |
| Stop N | a running or waiting conversation | `execution.stop { execution_id }` | asks once, with the list |
| Cancel N | a living task; a wake | `task.cancel` / `wake.cancel` | asks once |
| Re-home N | a living task | `task.rehome { task, home }`, per `dropOf` | asks once: pick the home, each item's effect or refusal and its budget sentence |
| Approve N | decision 4 (below) | `work.answer` (`answerOf`'s approve) | the review sheet |
| Decline N | an approval, budget ask or task change this connection may answer | `work.answer { option: decline }` | asks once |
| Attach N to project… | a living task; a conversation | `project.attach { session, project }`, per `dropOf` | pick the project |

**Decision 4** is `bulkApprovable` in `selection.ts`: `kind === 'approval'` (so not a budget ask, not a task change,
not a question; `queueOf`'s kinds), `!confirm.floor`, and `confirm.may_answer !== false`. The floor approvals among
the picked rows' items are `oneByOne`: the review sheet lists them as "asks one by one" with the floor's words and
never sends them. The sheet lists each approval with its tool, its own words (`plain.what`, else `reason`), its
owner and its age (and its risk badge), each with a box, all ticked; "Approve N" sends one `work.answer` per ticked
item. Pinned by `cockpit/test/ops-selection.test.ts` "decision 4: bulk Approve offers only tool approvals that are
not of the always-ask kind, that this page may answer" (six approvals: only the plain one is offered, the floor one
is listed apart, the budget, change and other-connection ones are neither) and by `cockpit/test/ops-page.test.ts`'s
new test (Approve always opens the review sheet; the floor one is listed, never sent).

Every call goes through `useOpsAct`'s new `send(verb, method, params)`: it returns `{ ok: false, words: PAST_WORDS }`
under the time machine before calling, never confirms or alerts (the sheet asked once; the toast says each
refusal), and returns the daemon's words on a refusal (the client rejects with the RPC error object `{ code,
message }`, not an `Error`; both are read). `settle()` reads the page again once per run. The fan-out runs four at a
time; the results toast lists ✓ or ✗ with the daemon's words per item; the failed items stay picked (a run from a
node's right-click menu leaves the selection alone).

**Design choices the owner should hear:**
- Bulk Decline offers floor approvals too: decision 4 speaks of Approve, and a No at the floor is safe.
- Re-home asks even for one item (the table says "ask"), and Attach does not ask (the table says "do"); the bulk
  attach still opens a sheet, since it needs the project picked.
- A lane's data mark (for the "weaker home" refusal) is the thread's session's `SessionInfo.data`; when the page
  can't tell, it lets the daemon decide.
- The tray at the foot of the screen during a drag offers each live thread and each project. It is how the Board
  reaches a thread or a project (the Board has no lanes), and how the Graph reaches a project without "+ future".

## The selection seam for ops-act

```ts
// cockpit/src/ops/useSelection.ts (zustand)
export interface SelectionStore {
  ids: ReadonlySet<string>                        // row ids (ses_/act_/wak_) and step ids (tsk_), mixed
  toggle: (id: string) => void
  set: (ids: Iterable<string>) => void            // replace: ⌘A's everything shown, a lasso's catch
  clear: () => void
  keepFailed: (results: readonly RunResult[]) => void
}
export const useSelection: UseBoundStore<StoreApi<SelectionStore>>
// read: useSelection((s) => s.ids); act: useSelection.getState().toggle(id)
```
To act on the selection, the palette can use `useBulk(...).start(verb, ids)` (`ops/BulkBar.tsx`), which runs the
same sheets, fan-out and results; `bulkVerbs(rows, queue)` (`ops/selection.ts`) is the pure table, and `fanOut` the
runner for a verb of its own ("switch model" for a selection: one `session.profile` per item through `send`).

## Parity rows moved, keys added, conflicts

- **X7** (the task graph view): `here`, now at `Ops.tsx`'s `a.lens === 'graph' && <GraphLens`, `GraphLens.tsx`'s
  `memory.lay(`, Actions' `<TaskGraphView` (still), and `components/TaskGraph.tsx`'s "open in Operations" link:
  Actions' `?taskgraph=1` keeps working and its header links to `/ops?lens=graph&sel=<the picked tsk_ id>`. The link
  is in `TaskGraphView`'s header, which only Actions renders, so `views/Actions.tsx` is unchanged. `TaskGraphView`'s
  `shown` prop, which only the old graph branch used, is gone.
- **B1** (every session as a tree, questions waiting): `pending: ops-graph` → `here`, at `Ops.tsx`'s
  `<GraphLens` and `<BoardLens`, `graph.ts`'s `opsLayout`, and `GraphNode.tsx`'s question badge.
- **H1**'s `fields` gain `items`, and the Threads lens draws them from `f.items` (its needle moved to that line);
  **H6**'s gain `openWork`, and the thread panel draws `f.openWork`. One line each.
- The parity test's last test still holds the Timeline's host and now the Graph lens's mount and the link (same
  name, same three assertions).
- **Keys**: `select` (`␣`, card), `select_all` (`mod+a`, page). `⌘A`/`ctrl+a` fire only on the Graph and the Board;
  elsewhere the chord is the browser's. The page's keydown used to drop every chord; it now lets `mod+a` through.
  `typingIn` now treats a checkbox, radio or button input as not typing (a clicked pick box held the focus and
  stopped every key, `esc` included: found by the live check). `SEQUENCES` now takes only keys longer than one
  character with a space, so `␣` is a key, not a sequence. No conflict with an existing key: `' '` and `mod+a` were
  unclaimed.

## Files outside the new modules, and why

`views/Ops.tsx` (391 lines): the graph branch, the selection state, the drop's and the bulk's hands, `select` and
`select_all` handlers, `back`'s first clause, the chord check, the bulk bar over the canvas on the Graph and the
Board, and the three overlays (drag layer, sheet, results toast). `ops/BoardLens.tsx`: a pick box per card,
⇧/⌘-click, a card drags, a conversation card takes a drop. `ops/keys.ts`, `ops/act.ts` (`send`, `settle`;
`PAST_WORDS` moved to `keys.ts` so a pure module can say it, re-exported), `ops/parity.ts`, `ops/ThreadsLens.tsx`
and `ops/ThreadPanel.tsx` (one line each), `components/TaskGraph.tsx` (the link), `index.css` (the flow and the
fade, with stir's and Calm's rules), `cockpit/AGENTS.md`.

## Tests, and each plant with its failing line

New: `test/ops-graph.test.ts` (9), `test/ops-selection.test.ts` (6), one more in `test/ops-keys.test.ts`, one more in
`test/ops-page.test.ts`. `npm test`: 586 pass, 0 fail at the head.

Every plant below was put in, the named test run, and the file restored and `touch`ed; `git status` was clean of it
after each.

| plant | file | failed test | at |
|---|---|---|---|
| a placed node never keeps its slot | graph.ts | positions are sticky by id… | ops-graph.test.ts:50 |
| a band keeps no spare row (`n`, then `max(1, n)`) | graph.ts | positions are sticky by id… | ops-graph.test.ts:50 |
| done-folding at 0 instead of an hour | graph.ts | ended work older than an hour folds… | ops-graph.test.ts:79 |
| the cap folds a needs-you subtree | graph.ts | past the cap, subtrees fold… | ops-graph.test.ts:101 |
| no cap | graph.ts | past the cap, subtrees fold… | ops-graph.test.ts:96 |
| deps drawn as parent edges | graph.ts | edges: parent solid…; deps dashed…; retry_of dotted | ops-graph.test.ts:117 |
| no retry edge | graph.ts | edges: … | ops-graph.test.ts:118 |
| lanes not ranked needs-you first | graph.ts | lanes: one per live thread, needs-you first… | ops-graph.test.ts:22 |
| future nodes always drawn | graph.ts | jobs hang beside their session; "+ future"… | ops-graph.test.ts:130 |
| an empty first layout keeps its order | graph.ts | a first layout that placed nothing… | ops-graph.test.ts:154 |
| decision 4 without the floor check | selection.ts | decision 4: bulk Approve offers only… | ops-selection.test.ts:61 |
| decision 4 for any confirm (budget, change) | selection.ts | decision 4: … | ops-selection.test.ts:61 |
| the fan-out ignores its limit | selection.ts | the fan-out runs at most four at a time… | ops-selection.test.ts:86 |
| one throw stops the run | selection.ts | the fan-out … one refusal never stops the others | ops-selection.test.ts:77 |
| afterRun drops the failed | selection.ts | the fan-out … | ops-selection.test.ts:93 |
| pause admits every task | selection.ts | each bulk verb counts the items that admit it… | ops-selection.test.ts:32 |
| no data-mark refusal | drag.ts | drag: every refusal, with its reason | ops-selection.test.ts:134 |
| ended work drags | drag.ts | drag: every refusal… | ops-selection.test.ts:123 |
| no time-machine refusal | drag.ts | drag: every refusal… | ops-selection.test.ts:121 |
| a conversation re-homes | drag.ts | drag: every refusal… | ops-selection.test.ts:132 |
| `␣` counted as a sequence | keys.ts | every key in the table has a line in the overlay… (and the new keys test) | ops-keys.test.ts:36 |
| ⌘A not named `mod+a` | keys.ts | the selection's keys (ops-graph)… | ops-keys.test.ts:110 |
| a checkbox counts as typing | keys.ts | the selection's keys (ops-graph)… | ops-keys.test.ts:126 |
| H6 drawn from `mine.items` | ThreadPanel.tsx | every field the old component showed… ("H6: … does not draw openWork") | ops-parity.test.ts:72 |
| H1 drawn from `t.items` | ThreadsLens.tsx | each home's source renders… | ops-parity.test.ts:39 |
| X7's link renamed | TaskGraph.tsx | each home's source renders… ("X7: … no longer has open in Operations") | ops-parity.test.ts:39 |
| esc doesn't clear the picks first | Ops.tsx | the Graph lens and the bulk acts (ops-graph)… | ops-page.test.ts:86 |
| `send` without `enabled` | act.ts | the Graph lens and the bulk acts… | ops-page.test.ts:80 |
| failed items not kept | BulkBar.tsx | the Graph lens and the bulk acts… | ops-page.test.ts:91 |
| no `select_all` handler | Ops.tsx | every key in the table has a handler in the page… ("select_all") | ops-page.test.ts:16 |

## The live check I ran, and the screenshots

A scratch daemon of this build (`target/debug/theseusd --config /tmp/scratch/theseus.toml`: a fresh state dir, its
own socket, web on 7434, Discord and the index off, the provider at `theseus-sim fake-model` on 127.0.0.1:9448 with
one rule answering "Noted.", the key a `file:` stand-in). `theseus ask "say hello"` answered "Noted." through it.
The built cockpit is the daemon's own (`/ops`). A stand-in model can't make an hour-old ended task, a waiting
approval or a running retry, so a headless Chromium (the VM's Playwright, nothing added to `package.json`) replaced
the daemon's answers to the four reads the page lays out from (`work.watch`, `task.list`, `confirm.list`,
`session.list`) on the WebSocket with an invented board; every other call, every act included, went to the daemon.
The script is not committed (it lives outside the repository); the maintainer's run below needs no injection.

Screenshots, dark theme, 1440×900, each address with `&redact=1` (which this row does not use: it is ops-timeline's):
- `cloud-report/ops-graph/graph.png`: `/ops?lens=graph&redact=1`. Three lanes (Harbour survey, which needs you, then
  Tide tables, then home); a needs-you node (Survey the north pier, amber, "needs you · y approve · n decline"); five
  flowing running edges; the folded "✓ 4 done" (and "✓ 3 done" below the fold); a job pill (cargo test -p quay); a
  step (Check the north wall); the private locks; the minimap.
- `cloud-report/ops-graph/board-selected.png`: `/ops?lens=board&redact=1`, three cards picked across two columns
  (Working: Draft the berth plan, Tabulate the spring tides; Paused·queued: Sort the chart drawer) and the bulk bar:
  "3 picked · Pause 2 · resume 1 · Stop · Cancel 3 · Re-home 3 · Approve · Decline · Attach 3 to project…".
- `cloud-report/ops-graph/approve-review.png`: the review sheet: one tool approval ticked (fs.write, "Write the pier
  survey to notes/pier.md", its owner, its age, "writes"), one floor approval listed as "proc.run · asks one by
  one", never sent.
- `cloud-report/ops-graph/bulk-results.png`: a bulk pause of two: two calls, two refusals in the daemon's words
  ("no task is named `ses_t3`", "no task is named `ses_t5`": the board is invented), both still picked.
- `cloud-report/ops-graph/rehome-sheet.png`: a task dragged onto a conversation's card: "Its report goes to the
  conversation “Workshop” from now on. The budget stays charged to Tide chat." Sent, the daemon refused it in its
  words ("no task is named `ses_t5`").
- `cloud-report/ops-graph/drag-refused.png`: a private task dragged over an open conversation: the cursor says "a
  private task cannot report into an open conversation"; nothing is sent. A conversation over a conversation said
  "a conversation has no parent: only a task's report moves". The tray of homes and projects shows at the foot.

The page logged no console or page errors through the run. **All data in every shot is invented** (threads, titles,
ids, figures); no real name, address, id or title appears, and every address carries `redact=1`. The header's "disk
low" notice is this VM's real one.

The live check found five faults, fixed in `c21920d`: the empty first layout froze the lane order (home first); the
fit ran before the data; the minimap drew nothing (the measured sizes were dropped); bands kept two spare rows; a
clicked pick box stopped every key.

## The live check for the maintainer

On a scratch daemon of the merged build (never the owner's), with a few tasks in two threads:
1. `theseus --socket <its socket> task start --home <a conversation in thread A> "..."` twice, and one in a
   conversation in thread B; then open `http://127.0.0.1:<port>/ops?lens=graph`. It should show a lane per thread
   (labels with warmth, context, today's $), each conversation at its lane's left, tasks to its right with spend
   bars and model chips; a running task's edge flows (stops with `?calm=1`).
2. Let a task finish, wait an hour (or use a store where one ended over an hour ago): its lane shows "✓ N done";
   a click opens it.
3. Start one more task: no other node moves; the new one fades in.
4. Board: tick two cards, press `esc`: the picks clear first. `⌘A`: every shown card is picked; "Pause N" pauses
   each (one `task.pause` row each in the ledger); pick a task that has ended with a living one and Cancel: the
   ended one is not offered.
5. Make a tool approval and a floor approval wait (a write outside the roots, and a `theseus config` through
   `proc.run`): pick their task, Approve: the sheet lists the first ticked and the floor one as "asks one by one";
   only the first is answered.
6. Drag a task onto another conversation's card: the sheet names the budget sentence; after it, the toast shows the
   daemon's `note`, and the task's card reports into the new home on every surface. Drag a private task onto an open
   conversation: the cursor refuses it.
7. Actions: `/actions?taskgraph=1`, pick a task, follow "open in Operations": the Graph lens pans to it.
8. Under the time machine (scrub the log): the bulk bar says the past's words and every button is off; drags are
   refused with the same words.

## What is left, and uncertain

- The Graph's keyboard movement (`h j k l`) still walks the queue on the Graph lens, as at the base; arrow moves
  between graph nodes are not in this row.
- "+N folded" does not open (opening would break the cap); it says to narrow with the filters. A per-fold opening
  that folds something else instead would be a follow-up.
- A lane that comes to need the owner stays where it is until "tidy" (stickiness over ranking): the owner may prefer
  otherwise.
- The Graph lens under the time machine lays out the tasks and conversations as they stood (`viewsOf`), but the
  records, jobs and steps are the present's (the time machine folds no records or jobs).
- The drag tray is fixed at the foot of the window; on a narrow screen it may cover the queue. The Graph's drag on
  touch screens was not tried.
- `redact=1` is carried and unread here (ops-timeline gives it meaning); the Graph's node titles will want it too.
- Disk on this VM: the per-session allowance filled twice during the first build; I built with
  `CARGO_INCREMENTAL=0` and removed stale build artifacts under `target/` (nothing outside the repository).

## The keel guard

`python3 scripts/keel-guard.py --base fd21a228 --head HEAD --quiet` printed nothing (exit 0).

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the head `0fc5019` (the
working tree clean): door, keel (0 findings), keel tests, fmt, shape, features, clippy, clippy-probe, **cockpit**
(lint, test, build), test build and the reader rule passed; **the suite failed on 35 of 8,119 tests**, then I ran the
phases after it myself.

- **34 are the known L1 failures** (the VM runs as root; theseus-pv6i): 19 `theseus-sandbox::contract`, the bench's
  `spawn_100`, and 14 `theseusd::sandbox`.
- **1 is not on the known list, and not this row's:** `theseus-core
  data_mark::tests::a_sensitive_read_marks_the_session_and_a_task_under_it_sensitive` panics at
  `crates/theseus-core/src/tests_data_mark.rs:197` ("the task's view": `work_snapshot_marked` holds no view for the
  task's session). It fails every time, run alone (three runs, 0.43–0.50 s each), so it is not load. This branch has
  no Rust diff against `fd21a228` (`git diff --stat fd21a228 HEAD -- crates Cargo.lock` is empty), and the test came
  with the base's last commit, `fd21a228` ("join fix: the data mark reads the sensitive level…"): it fails on the base
  on this VM. The maintainer should see whether it passes on the owner's machine; I did not dig further, since it is
  outside this row.
- The phases after the suite (`machine_checks`): protocol types ok (`src/protocol.gen/` unchanged); nothing compiled
  under the lock; the lifecycle, jobs, turn and size benches are skipped by `THESEUS_GATE_NO_BENCH` (a lane's gate).
- No test on the flaky list needed a retry. `cargo deny fetch` succeeded during setup; this version of the gate shows
  no deny phase in its table.
- Builds on this VM used `CARGO_INCREMENTAL=0`, since the incremental caches filled the session's disk allowance.

## A sentence for docs/status.md

The Operations page's Graph lens (theseus-jid7t.4): every session, task, step and job as one graph in lanes per
thread (needs-you first, a home lane for the rest), laid out by a pure `opsLayout` whose positions are sticky by id,
with ended work folded at an hour and a 400-node cap; multi-select on the Graph and the Board (`␣`, `⌘A`, `esc`),
a bulk bar whose acts are client fan-outs of single calls (Approve only for non-floor tool approvals, after a review
sheet: the owner's decision 4), and drag to re-home a task (`task.rehome`) or attach work to a project.

## Follow-ups

- Graph keyboard navigation between nodes (arrows along edges and lanes).
- The palette's "switch model" for a selection (ops-act, through `useSelection`).
- An opening for "+N folded" that keeps the cap.
- The spec's Part III item and `docs/status.md` (the maintainer's): the layout rules and decision 4 as built.
