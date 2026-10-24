// The Operations page's keys (theseus-jid7t.3): one table drives both the handler and the `?` overlay, so they cannot
// drift. Pure: `npm test` runs it (test/ops-keys.test.ts), which holds that every entry has a handler in the page and
// a line in the overlay, that no two entries claim one key in one context, and that nothing fires while typing.
//
// A key means the most specific thing the focus offers, as in the TUI: `r` and `d` retry and dismiss on a failure and
// filter anywhere else; `a` and `e` take the card's grant when it offers one, and `a` shows everything otherwise. A
// sequence (`g` then a letter) is one key here: `g b`.

/** What the focus is on, most specific first: each focus offers its contexts, and `page` is always there. */
export type Ctx = 'answer' | 'choice' | 'grant' | 'failure' | 'task' | 'running' | 'job' | 'wake' | 'card' | 'page' | 'filter'

export type Verb =
  | 'down' | 'up' | 'left' | 'right' | 'inspect' | 'open' | 'back' | 'next' | 'prev'
  | 'approve' | 'approve_trust' | 'decline' | 'choice' | 'grant' | 'retry' | 'dismiss' | 'pause' | 'stop' | 'stop_all'
  | 'cancel' | 'cancel_wake' | 'follow' | 'new_task'
  | 'lens_graph' | 'lens_board' | 'lens_timeline' | 'lens_threads' | 'home' | 'search'
  | 'fold_queue' | 'fold_inspector' | 'fold_dock' | 'keys'
  | 'filter_needs' | 'filter_working' | 'filter_paused' | 'filter_done' | 'filter_all'
  // ops-graph (theseus-jid7t.4): the selection.
  | 'select' | 'select_all'

export interface KeyDef {
  verb: Verb
  /** The keys, as `KeyboardEvent.key` names them, a sequence spaced: `j`, `Enter`, `shift+Tab`, `g b`. */
  keys: readonly string[]
  ctx: Ctx
  /** The overlay's words. */
  words: string
  /** It changes something in the daemon: off under the time machine. */
  act?: boolean
  /** It fires on a second press within `TWICE_MS`, as the TUI's `s s` and `c c`. */
  twice?: boolean
}

export const TWICE_MS = 1500

export const KEYS: readonly KeyDef[] = [
  { verb: 'down', keys: ['j', 'ArrowDown'], ctx: 'page', words: 'next row in the column, next item in the queue' },
  { verb: 'up', keys: ['k', 'ArrowUp'], ctx: 'page', words: 'previous row or item' },
  { verb: 'left', keys: ['h', 'ArrowLeft'], ctx: 'page', words: 'the column to the left (the queue left of the first)' },
  { verb: 'right', keys: ['l', 'ArrowRight'], ctx: 'page', words: 'the column to the right' },
  { verb: 'follow', keys: ['l'], ctx: 'job', words: 'on a job: follow it (its call in its session)' },
  { verb: 'inspect', keys: ['Enter'], ctx: 'card', words: 'inspect: the selection in the inspector; a queue item’s full card' },
  { verb: 'open', keys: ['o'], ctx: 'card', words: 'open its deck (/session/…)' },
  { verb: 'back', keys: ['Escape'], ctx: 'page', words: 'back: clear the picked cards first, then close the card, clear the focus, then the filters' },
  { verb: 'next', keys: ['Tab'], ctx: 'page', words: 'the next thing that needs you' },
  { verb: 'prev', keys: ['shift+Tab'], ctx: 'page', words: 'the previous thing that needs you' },
  { verb: 'approve', keys: ['y'], ctx: 'answer', words: 'approve (reset, accept: the card’s first answer)', act: true },
  { verb: 'approve_trust', keys: ['t'], ctx: 'answer', words: 'approve and trust the session again', act: true },
  { verb: 'decline', keys: ['n'], ctx: 'answer', words: 'decline: the note field opens; Enter sends', act: true },
  { verb: 'choice', keys: ['1', '2', '3', '4', '5', '6'], ctx: 'choice', words: 'answer with that choice', act: true },
  { verb: 'grant', keys: ['a', 'e'], ctx: 'grant', words: 'take the card’s grant (yes, and don’t ask again here)', act: true },
  { verb: 'retry', keys: ['r'], ctx: 'failure', words: 'on a failed task: retry it', act: true },
  { verb: 'dismiss', keys: ['d'], ctx: 'failure', words: 'on a failure: dismiss it', act: true },
  { verb: 'pause', keys: ['P'], ctx: 'task', words: 'pause or resume the task', act: true },
  { verb: 'stop', keys: ['s'], ctx: 'running', words: 'twice: stop the running turn', act: true, twice: true },
  { verb: 'stop_all', keys: ['S'], ctx: 'running', words: 'stop, and cancel every task under it (asks)', act: true },
  { verb: 'cancel', keys: ['c'], ctx: 'task', words: 'twice: cancel the task', act: true, twice: true },
  { verb: 'cancel_wake', keys: ['x'], ctx: 'wake', words: 'on a wake: cancel it (asks), the TUI’s x', act: true },
  { verb: 'new_task', keys: ['N'], ctx: 'page', words: 'new task (its home: the conversation under the cursor)', act: true },
  { verb: 'lens_graph', keys: ['g g'], ctx: 'page', words: 'the Graph lens' },
  { verb: 'lens_board', keys: ['g b'], ctx: 'page', words: 'the Board lens' },
  { verb: 'lens_timeline', keys: ['g t'], ctx: 'page', words: 'the Timeline lens' },
  { verb: 'lens_threads', keys: ['g h'], ctx: 'page', words: 'the Threads lens' },
  { verb: 'home', keys: ['g m'], ctx: 'page', words: 'home: the Ship' },
  { verb: 'search', keys: ['/'], ctx: 'page', words: 'search' },
  { verb: 'fold_queue', keys: ['['], ctx: 'page', words: 'fold or unfold the queue' },
  { verb: 'fold_inspector', keys: [']'], ctx: 'page', words: 'fold or unfold the inspector' },
  { verb: 'fold_dock', keys: ['\\'], ctx: 'page', words: 'the dock (kept for a later row)' },
  { verb: 'keys', keys: ['?'], ctx: 'page', words: 'these keys' },
  { verb: 'filter_needs', keys: ['b'], ctx: 'filter', words: 'only what needs you' },
  { verb: 'filter_working', keys: ['w'], ctx: 'filter', words: 'only what is working' },
  { verb: 'filter_paused', keys: ['r'], ctx: 'filter', words: 'only paused·queued (on a failure, r retries)' },
  { verb: 'filter_done', keys: ['d'], ctx: 'filter', words: 'only done today (on a failure, d dismisses)' },
  { verb: 'filter_all', keys: ['a'], ctx: 'filter', words: 'everything: clear the filters (on a card with a grant, a takes it)' },
  // ops-graph (theseus-jid7t.4): the selection, on the Graph and the Board. The bulk verbs are buttons (BUTTON_ACTS).
  { verb: 'select', keys: [' '], ctx: 'card', words: 'pick the focused card or node for a bulk act, or put it back' },
  { verb: 'select_all', keys: ['mod+a'], ctx: 'page', words: 'pick everything shown, after the filters and the search (ctrl+a too)' },
]

/** The contexts in the order a key looks for its meaning: the focus's own first, then the page, then the filters. */
export const PRIORITY: readonly Ctx[] = ['answer', 'choice', 'grant', 'failure', 'task', 'running', 'job', 'wake', 'card', 'page', 'filter']

/** Words for each context, for the overlay's headings. */
export const CTX_WORDS: Readonly<Record<Ctx, string>> = {
  answer: 'On a question', choice: 'On a question with choices', grant: 'On a card that offers a grant', failure: 'On a failure',
  task: 'On a task', running: 'On something running', job: 'On a job', wake: 'On a wake', card: 'On anything selected',
  page: 'Anywhere', filter: 'Counters (where no card claims the key)',
}

/** The verbs that act: off under the time machine. The table's, and the buttons' that no key presses. */
export const BUTTON_ACTS = [
  'answer', 'trust', 'seal', 'seal_dismiss', 'unseal', 'thread_pause', 'thread_resume', 'thread_done', 'thread_reopen', 'thread_edit', 'thread_new', 'thread_answer', 'mark',
  // ops-graph (theseus-jid7t.4): the bulk bar's verbs, and a drop on a home or a project.
  'bulk_pause', 'bulk_stop', 'bulk_cancel', 'bulk_rehome', 'bulk_approve', 'bulk_decline', 'bulk_attach', 'drop',
] as const
export type ButtonAct = (typeof BUTTON_ACTS)[number]
export const ACTS: ReadonlySet<string> = new Set<string>([...KEYS.filter((k) => k.act).map((k) => k.verb), ...BUTTON_ACTS])

/** What an act under the time machine says instead. */
export const PAST_WORDS = 'the past: acting needs the present (LIVE in the ship’s log below)'

/** May `verb` run now? Every act is off while the time machine shows the past; the rest (moving, looking) stay. */
export function enabled(verb: Verb | ButtonAct, timeMachine: boolean): boolean {
  return !(timeMachine && ACTS.has(verb))
}

/** The key a keydown names: `shift+Tab` for a shifted Tab; `mod+a` for ⌘A or ctrl+a; a printable key as itself (its
 *  case is its shift). */
export function keyName(e: { key: string; shiftKey: boolean; metaKey?: boolean; ctrlKey?: boolean }): string {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a') return 'mod+a'
  return e.key === 'Tab' && e.shiftKey ? 'shift+Tab' : e.key
}

/** What a key does with the focus offering `ctxs`: the entry of the most specific context that claims it, or null.
 *  Nothing fires while an input, a text area or an editable element has focus. */
export function resolve(key: string, ctxs: ReadonlySet<Ctx>, typing: boolean): KeyDef | null {
  if (typing) return null
  for (const c of PRIORITY) {
    if (c !== 'page' && c !== 'filter' && !ctxs.has(c)) continue
    const hit = KEYS.find((k) => k.ctx === c && k.keys.includes(key))
    if (hit) return hit
  }
  return null
}

/** Is the event's target a place where a person types? */
export function typingIn(t: { tagName?: string; isContentEditable?: boolean } | null | undefined): boolean {
  return !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable === true)
}

/** A key that fires on its second press: the first arms it, a second within `TWICE_MS` fires it, anything else
 *  disarms. */
export function pressTwice(armed: { verb: Verb; at: number } | null, verb: Verb, now: number): { fire: boolean; armed: { verb: Verb; at: number } | null } {
  if (armed && armed.verb === verb && now - armed.at <= TWICE_MS) return { fire: true, armed: null }
  return { fire: false, armed: { verb, at: now } }
}

/** The `g` sequences: after `g`, the next key within `G_MS` completes one. */
export const G_MS = 900
export const SEQUENCES: readonly string[] = KEYS.flatMap((k) => k.keys.filter((x) => x.length > 1 && x.includes(' ')))

/** The overlay: a heading per context in `PRIORITY` order, a line per entry. */
export function overlay(): Array<{ ctx: Ctx; heading: string; lines: Array<{ verb: Verb; keys: string; words: string }> }> {
  return PRIORITY.map((ctx) => ({
    ctx,
    heading: CTX_WORDS[ctx],
    lines: KEYS.filter((k) => k.ctx === ctx).map((k) => ({ verb: k.verb, keys: k.keys.map((x) => (k.twice ? `${x} ×2` : label(x))).join(' '), words: k.words })),
  })).filter((g) => g.lines.length > 0)
}

/** A key as the overlay prints it. */
export function label(key: string): string {
  return ({ Enter: '⏎', Escape: 'esc', Tab: 'tab', 'shift+Tab': '⇧tab', ArrowDown: '↓', ArrowUp: '↑', ArrowLeft: '←', ArrowRight: '→', ' ': '␣', 'mod+a': '⌘A' } as Record<string, string>)[key] ?? key
}
