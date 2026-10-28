// The data-parity inventory (theseus-jid7t.3; the owner's rule: no data lost). Every datum and act of the Agenda,
// Tasks, Threads and Actions pages and of the TUI's board, and its home now. `test/ops-parity.test.ts` holds it true:
// every id below is present once; every row of 1a-1d is `here` (on Operations) or `stays` (the old place keeps it);
// a row of 1e and herdr may wait on a later row, named; each home's source says it renders the thing (`at`); and each
// card's fact function makes every field the old component showed (`facts`). A change that drops a field fails it.
//
// To keep it true: a new field on a card lands in its fact function (`ops/facts.ts`) and in the card, and its name in
// the row's `fields`; a row that a later lens takes over moves its `at` there.

export type Home = 'here' | 'stays' | 'pending: ops-act' | 'pending: ops-graph' | 'pending: ops-timeline'

export type FactFn = 'cardFacts' | 'threadFacts' | 'threadPanelFacts' | 'itemFacts'

export interface ParityRow {
  id: string
  group: '1a' | '1b' | '1c' | '1d' | '1e' | 'herdr'
  datum: string
  home: Home
  /** Where it renders: each file (from `cockpit/`) must hold its needle. */
  at: ReadonlyArray<readonly [string, string]>
  /** A card's fields: the fact function that makes them, every field the old component showed, and the file that
   *  draws them (each as `f.<field>`). */
  facts?: { fn: FactFn; fields: readonly string[]; drawn: string }
}

const OPS = 'src/views/Ops.tsx'
const LENS = 'src/ops/TimelineLens.tsx'
const STRIP = 'src/ops/WeekStripV2.tsx'
const BOARD = 'src/ops/BoardLens.tsx'
const THREADS = 'src/ops/ThreadsLens.tsx'
const PANEL = 'src/ops/ThreadPanel.tsx'
const QUEUE = 'src/ops/Queue.tsx'
const KEYS = 'src/ops/keys.ts'
const ACTIONS = 'src/views/Actions.tsx'
const GRAPH = 'src/ops/GraphLens.tsx'
const timeline: readonly [string, string] = [OPS, "a.lens === 'timeline' && ("]

export const PARITY: readonly ParityRow[] = [
  // 1a. The Agenda: the Timeline lens (ops-timeline, theseus-jid7t.5), its strip v2 and the Agenda's panels unchanged.
  { id: 'A1', group: '1a', datum: 'week strip: 7 local days by wall-clock hour, DST days, today, now line, ‹ Today ›, ?w=', home: 'here', at: [timeline, [OPS, '<TimelineLens weeks={a.w}'], [LENS, '<WeekStripV2'], [LENS, 'weekWindow(now, weeks)'], [STRIP, "d.hours !== 24"], [STRIP, 'title={`now ${hm(now)}`}'], [STRIP, '>Today</button>']] },
  { id: 'A2', group: '1a', datum: 'marks: reminder, check-in, run due, past runs, deferred start, milestone, watch bar', home: 'here', at: [[LENS, 'allMarks('], [STRIP, '{glyph(m)}'], [STRIP, 'data-mark="watch"']] },
  { id: 'A3', group: '1a', datum: 'hover line (markLine)', home: 'here', at: [[LENS, 'onHover={setHover}'], [STRIP, "onHover(markLine(m).split('\\n')[0])"]] },
  { id: 'A4', group: '1a', datum: '"the daemon would say more than the strip holds"', home: 'here', at: [[LENS, 'more={agenda?.more}'], [LENS, 'the daemon would say more than the strip holds: busy series are cut']] },
  { id: 'A5', group: '1a', datum: 'selected mark (?sel=): title, line, Open, Snooze, Move…, Pause/Resume, Cancel, routine and watch verbs', home: 'here', at: [[LENS, '<MarkDetail mark={picked}'], [LENS, 'verbsFor(mark)']] },
  { id: 'A6', group: '1a', datum: '"This week · N items"', home: 'here', at: [[LENS, 'This week ·']] },
  { id: 'A7', group: '1a', datum: 'routines table: every column, the 30-run expansion and tally', home: 'here', at: [[LENS, '<RoutinesTable']] },
  { id: 'A8', group: '1a', datum: 'watches, every field', home: 'here', at: [[LENS, '<WatchList']] },
  { id: 'A9', group: '1a', datum: 'projects, every field', home: 'here', at: [[LENS, '<ProjectList']] },
  { id: 'A10', group: '1a', datum: 'wakes: every field and verb, the tip', home: 'here', at: [[LENS, '<Wakes']] },
  { id: 'A11', group: '1a', datum: '"the agenda could not be read: …"', home: 'here', at: [[LENS, 'the agenda could not be read']] },
  { id: 'A12', group: '1a', datum: 'the read discipline: work.agenda and wake.list only when the week changes or touchesFuture', home: 'here', at: [[LENS, 'useFuture(win.from_ms, win.to_ms, held)']] },
  // 1b. Tasks: the Board lens, the command bar and the queue.
  { id: 'T1', group: '1b', datum: 'New task: brief, title, budget validated, home conversation, Ctrl+Enter, the result line, a refusal in the daemon’s words', home: 'here',
    at: [[OPS, '<NewTask'], ['src/ops/NewTask.tsx', "call<TaskStartResult>('task.start'"], ['src/ops/NewTask.tsx', 'a quarter of what’s left'], ['src/ops/NewTask.tsx', 'Ctrl+Enter'], ['src/ops/NewTask.tsx', 'Home conversation'], ['src/ops/NewTask.tsx', 'e?.message'], ['src/ops/NewTask.tsx', "ring-fault/60"]] },
  { id: 'T2', group: '1b', datum: 'search, "n of m"', home: 'here', at: [['src/ops/CommandBar.tsx', '{n} of {of}'], [OPS, 'shown(rows, a.f, a.q, cx)']] },
  { id: 'T3', group: '1b', datum: 'sections Needs you / Working / Done with counts', home: 'here', at: [[BOARD, 'COLUMN_TITLES[col]'], [BOARD, '{count}']] },
  { id: 'T4', group: '1b', datum: 'the task card, every field; its approvals inline or the question with choices', home: 'here', at: [[BOARD, 'cardFacts(r, now)'], [QUEUE, '<ConfirmCard']],
    facts: { fn: 'cardFacts', drawn: BOARD, fields: ['pill', 'paused', 'title', 'href', 'place', 'placeTip', 'short', 'progress', 'doing', 'outcome', 'report', 'cost', 'activity', 'waitsOn', 'open', 'pause', 'stop', 'approvals', 'question', 'model'] } },
  { id: 'T5', group: '1b', datum: 'Done 50 at a time, "more (N older)", report lines read lazily', home: 'here', at: [['src/ops/board.ts', 'DONE_PAGE = 50'], [BOARD, 'older)</button>'], [OPS, 'reportLineOf(']] },
  { id: 'T6', group: '1b', datum: '?sel= scroll-to and glow', home: 'here', at: [[BOARD, 'scrollIntoView'], [BOARD, "focused && 'ring-gold"]] },
  // 1c. Threads: the Threads lens, its panel in the inspector, and the queue for seals.
  { id: 'H1', group: '1c', datum: 'per thread: lock, title link, state, warm/cold and tooltip, ● N need you and the paused tooltip, details, seal box, open items', home: 'here', at: [[THREADS, 'threadFacts(t, now)'], [THREADS, 'f.items.map((it, i) => <Item key={it.id} v={t.items[i]} />)']],
    facts: { fn: 'threadFacts', drawn: THREADS, fields: ['sealed', 'title', 'href', 'state', 'warmth', 'warmthTip', 'needs', 'needsCounted', 'pausedTip', 'details', 'seal', 'items'] } },
  { id: 'H2', group: '1c', datum: '"N live · N need you on the board"', home: 'here', at: [[THREADS, 'need you on the board']] },
  { id: 'H3', group: '1c', datum: 'loose conversations and the Fleet link', home: 'here', at: [[THREADS, 'conversations are in no thread'], [THREADS, 'to="/fleet"']] },
  { id: 'H4', group: '1c', datum: 'the time machine’s "Threads then" fold and its caveat', home: 'here', at: [[THREADS, 'title="Threads then"'], [THREADS, 'is the present’s and is not folded']] },
  { id: 'H5', group: '1c', datum: 'the empty state’s words', home: 'here', at: [[THREADS, 'No thread is live.'], [THREADS, "'thread.new'"]] },
  { id: 'H6', group: '1c', datum: 'the thread page /threads/:slug', home: 'stays', at: [['src/main.tsx', "path: 'threads/:slug'"], ['src/views/Threads.tsx', '<ThreadBody'], ['src/ops/Inspector.tsx', '<ThreadBody']],
    facts: { fn: 'threadPanelFacts', drawn: PANEL, fields: ['title', 'state', 'goal', 'details', 'context', 'today', 'sealPending', 'sealedBy', 'topics', 'links', 'card', 'workingSet', 'turnsOf', 'openWork'] } },
  // 1d. Actions: the queue takes what waits; the rest stays.
  { id: 'X1', group: '1d', datum: 'approvals waiting (ConfirmCard, every field, read-only under the time machine)', home: 'here', at: [[QUEUE, '<ConfirmCard c={i.confirm} past='], [ACTIONS, '<ConfirmCard']],
    facts: { fn: 'itemFacts', drawn: QUEUE, fields: ['glyph', 'kind', 'title', 'owner', 'age', 'left'] } },
  { id: 'X2', group: '1d', datum: 'tool calls', home: 'stays', at: [[ACTIONS, '<Lifecycle']] },
  { id: 'X4', group: '1d', datum: 'the wakes link: to the Timeline lens', home: 'stays', at: [[ACTIONS, 'to="/agenda"'], ['src/ops/address.ts', "'/agenda': 'timeline'"]] },
  { id: 'X5', group: '1d', datum: 'the tasks panel', home: 'stays', at: [[ACTIONS, 'function Tasks(']] },
  { id: 'X6', group: '1d', datum: 'the task tree', home: 'stays', at: [[ACTIONS, '<TaskTree']] },
  { id: 'X7', group: '1d', datum: 'the task graph view (the Graph lens now; Actions’ ?taskgraph=1 keeps it, with an “open in Operations” link)', home: 'here',
    at: [[OPS, "a.lens === 'graph' && <GraphLens"], [GRAPH, 'memory.lay('], [ACTIONS, '<TaskGraphView'], ['src/components/TaskGraph.tsx', 'open in Operations']] },
  { id: 'X8', group: '1d', datum: 'tool postures', home: 'stays', at: [[ACTIONS, '<Postures']] },
  { id: 'X9', group: '1d', datum: 'the time machine’s banner and disabled acts', home: 'here', at: [[OPS, 'folded from the ledger. Acting needs the present'], ['src/ops/act.ts', 'enabled(verb, past)']] },
  { id: 'X10', group: '1d', datum: 'content labels (theseus-9ymz0): a session’s labels as chips in its deck, each with the node it came from, and Remove (content.unlabel, asks first; the operator’s `theseus content remove`)', home: 'stays',
    at: [['src/views/SessionDeck.tsx', 'chipsOf(s.labels)'], ['src/views/SessionDeck.tsx', 'unlabelCall(s.session_id, c.flag)'], ['src/lib/labels.ts', "method: 'content.unlabel'"]] },
  // 1e. The TUI's board and keys.
  { id: 'B1', group: '1e', datum: 'every session as a tree, questions waiting (the tree in the Graph lens’s lanes, a question a badge on its owner; every session on the Board, every question in the queue)', home: 'here',
    at: [[OPS, '<GraphLens'], ['src/ops/graph.ts', 'export function opsLayout('], ['src/ops/GraphNode.tsx', 'n.asks > 0'], [OPS, '<BoardLens']] },
  { id: 'B2', group: '1e', datum: 'b w r d a filters, /', home: 'here', at: [[KEYS, "verb: 'filter_needs'"], [KEYS, "verb: 'search'"]] },
  { id: 'B3', group: '1e', datum: 'tab / ⇧tab', home: 'here', at: [[KEYS, "verb: 'next'"], [KEYS, "verb: 'prev'"]] },
  { id: 'B4', group: '1e', datum: 'y t n, 1-6, a e', home: 'here', at: [[KEYS, "verb: 'approve'"], [KEYS, "verb: 'choice'"], [KEYS, "verb: 'grant'"]] },
  { id: 'B5', group: '1e', datum: 'enter open, i type to the session, ⇧tab its mode', home: 'here', at: [[KEYS, "verb: 'inspect'"], [KEYS, "verb: 'compose'"], [KEYS, "verb: 'mode_menu'"], ['src/ops/JoinPane.tsx', 'composerStep('], ['src/ops/JoinPane.tsx', "e.key === 'Tab' && e.shiftKey"]] },
  { id: 'B6', group: '1e', datum: 's ×2, S, c ×2', home: 'here', at: [[KEYS, "verb: 'stop'"], [KEYS, "verb: 'stop_all'"], [KEYS, "verb: 'cancel'"]] },
  { id: 'B7', group: '1e', datum: 'N new task', home: 'here', at: [[KEYS, "verb: 'new_task'"]] },
  { id: 'B8', group: '1e', datum: 'P pause or resume', home: 'here', at: [[KEYS, "verb: 'pause'"]] },
  { id: 'B9', group: '1e', datum: 'l follow the job (its output streams in the inspector: work.peek, then action.output)', home: 'here', at: [[KEYS, "verb: 'follow'"], ['src/ops/jobout.ts', "'action.output'"], ['src/ops/jobout.ts', "'action.unfollow'"]] },
  { id: 'B10', group: '1e', datum: 'PgUp / PgDn / End in the transcript', home: 'here', at: [[KEYS, "verb: 'scroll_up'"], [KEYS, "verb: 'scroll_end'"], [OPS, 'scrollTranscript(']] },
  { id: 'B11', group: '1e', datum: 'ctrl-t thread picker, /t, T (here: T and /t in the box; ctrl-t is the browser’s new tab)', home: 'here', at: [[KEYS, "verb: 'thread_picker'"], ['src/ops/ThreadPicker.tsx', 'Join a thread'], ['src/ops/JoinPane.tsx', 'onPicker']] },
  { id: 'B12', group: '1e', datum: 'r d o on a failure', home: 'here', at: [[KEYS, "verb: 'retry'"], [KEYS, "verb: 'dismiss'"], [KEYS, "verb: 'open'"]] },
  { id: 'B13', group: '1e', datum: 'L self changes', home: 'here', at: [[KEYS, "verb: 'self_changes'"], [OPS, '<SelfChangesCard'], ['src/ops/PaletteGroups.tsx', 'Self changes']] },
  { id: 'B14', group: '1e', datum: 'p standing records / parent', home: 'here', at: [[KEYS, "verb: 'parent'"], ['src/ops/InspectorSections.tsx', "'standing.list'"], ['src/ops/Inspector.tsx', '<StandingBlock']] },
  { id: 'B15', group: '1e', datum: 'I the newest turn', home: 'here', at: [[KEYS, "verb: 'last_turn'"], [OPS, '/turn/${t}']] },
  { id: 'B16', group: '1e', datum: 'H the Horizon (j k z p r x enter)', home: 'here', at: [[KEYS, "verb: 'mark_next'"], [KEYS, "verb: 'mark_snooze'"], [KEYS, "verb: 'mark_open'"], ['src/ops/timelinekeys.ts', 'horizonVerb(mark, k)']] },
  { id: 'B17', group: '1e', datum: 'flash lines (toasts)', home: 'here', at: [[OPS, 'setFlash(']] },
  { id: 'B18', group: '1e', datum: 'narrow screens', home: 'here', at: [['src/ops/regions.ts', 'NARROW_PX'], [OPS, 'regionsFor(']] },
  // herdr's commands and links.
  { id: 'R1', group: 'herdr', datum: 'herdr answer: answer the item', home: 'here', at: [['src/ops/address.ts', "'answer'"]] },
  { id: 'R2', group: 'herdr', datum: 'herdr next: the next that needs you', home: 'here', at: [[KEYS, "verb: 'next'"]] },
  { id: 'R3', group: 'herdr', datum: 'herdr tui: open the TUI on the item (the web has no honest way to start a terminal program; the page is where the item opens, and `theseus-tui` stays the terminal’s)', home: 'stays', at: [] },
  { id: 'R4', group: 'herdr', datum: 'herdr stop: stop the turn', home: 'here', at: [[KEYS, "verb: 'stop'"]] },
  { id: 'R5', group: 'herdr', datum: 'herdr new: a new task', home: 'here', at: [[KEYS, "verb: 'new_task'"]] },
  { id: 'R6', group: 'herdr', datum: 'herdr main: the main conversation (g m: home)', home: 'here', at: [[KEYS, "verb: 'home'"], [OPS, "home: () => nav('/ship')"]] },
  { id: 'R7', group: 'herdr', datum: 'the links theseus://answer|history|open/<id>, unchanged', home: 'stays', at: [] },
  { id: 'R8', group: 'herdr', datum: 'deep links /ops?sel=<id> and &do=answer (focuses the item’s answer in the queue)', home: 'here', at: [[OPS, "a.do === 'answer'"], ['src/ops/queue.ts', 'export function itemFor(']] },
]

/** Every id the inventory names, in order: the test checks the table against it. */
export const INVENTORY: readonly string[] = [
  ...Array.from({ length: 12 }, (_, i) => `A${i + 1}`),
  ...Array.from({ length: 6 }, (_, i) => `T${i + 1}`),
  ...Array.from({ length: 6 }, (_, i) => `H${i + 1}`),
  ...Array.from({ length: 10 }, (_, i) => `X${i + 1}`).filter((id) => id !== 'X3'), // X3 (the external-text hold) retired with the hold
  ...Array.from({ length: 18 }, (_, i) => `B${i + 1}`),
  ...Array.from({ length: 8 }, (_, i) => `R${i + 1}`),
]
