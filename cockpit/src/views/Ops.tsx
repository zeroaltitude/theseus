// Operations (theseus-jid7t.3): one page for every live task, session and thread, each of which can be acted on at
// any time. The command bar over three regions: the needs-you queue, the canvas (a lens: Board, Threads, Timeline,
// Graph), and the inspector. Everything on screen is in the address (`ops/address.ts`), so a link reopens it.
//
// It reads, it does not poll: the board's views from the one `work.watch` the Threads views keep, `task.list`,
// `confirm.list` and `session.list` on their pushes, the ledger's one copy for today's spend, and the router's
// questions as the push brings them. Under the time machine every lens shows the folded past and every act is off
// (`ops/keys.ts`'s `enabled`). Its pure halves are `src/ops/*.ts`, tested by `test/ops-*.test.ts`; the parity table
// (`src/ops/parity.ts`) holds that nothing the Agenda, Tasks and Threads pages showed is lost.
import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { useQueries } from '@tanstack/react-query'
import { History, Inbox, PanelLeftOpen, PanelRightOpen, X } from 'lucide-react'
import type { CatalogList, ConfirmRequest, ProfileList, SessionListResult, TaskListResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { pricing } from '@/lib/money'
import { watchRows } from '@/lib/agenda'
import { useTick } from '@/lib/hooks'
import { useWorld } from '@/lib/world'
import { useHistoryRows } from '@/lib/history'
import { useThreadBoard } from '@/lib/threadwatch'
import { useWidth } from '@/lib/chartview'
import { usePref } from '@/lib/usePref'
import { reportLineOf } from '@/lib/taskboard'
import { groupThreads } from '@/lib/threadboard'
import { cn, stamp } from '@/lib/format'
import { fullKey, parseFull, writeFull } from '@/lib/fullgraph'
import { parseAddress, writeAddress, type Address, type Bucket, type FilterKey, type Lens } from '@/ops/address'
import { alive, allCounters, boardRows, chipChoices, columnsOf, contextOf, DATA_MARKS, DONE_PAGE, laneRows, shown, spendToday, type OpsRow } from '@/ops/board'
import { answerOf, asksWord, comingUp, grantKey, itemFor, keepCursor, queueOf, step, arrived, type Answer, type QueueItem } from '@/ops/queue'
import { G_MS, keyName, pressTwice, resolve, SEQUENCES, typingIn, type Ctx, type Verb } from '@/ops/keys'
import { threadFacts } from '@/ops/facts'
import { stepNode, viewsOf, type Graph, type LaneMeta } from '@/ops/graph'
import { GraphLens } from '@/ops/GraphLens'
import { BulkBar, ResultsToast, useBulk } from '@/ops/BulkBar'
import { useDragDrop } from '@/ops/DropLayer'
import { useSelection } from '@/ops/useSelection'
import type { DropTarget } from '@/ops/rehome'
import type { RunResult } from '@/ops/selection'
import { comingShows, DOCK_KEY, flip, idleSel, INSPECTOR_KEY, OPEN_SEL, parseFold, QUEUE_KEY, regionsFor } from '@/ops/regions'
import { BoardLens } from '@/ops/BoardLens'
import { ThreadsLens } from '@/ops/ThreadsLens'
import { Queue } from '@/ops/Queue'
import { Inspector, type Selection } from '@/ops/Inspector'
import { CommandBar } from '@/ops/CommandBar'
import { KeysOverlay } from '@/ops/KeysOverlay'
import { NewTask } from '@/ops/NewTask'
import { ThreadPicker } from '@/ops/ThreadPicker'
import { Dock, type DockState } from '@/ops/Dock'
import { JoinPane } from '@/ops/JoinPane'
import { ModeMenu, ModelMenu } from '@/ops/ModelMenu'
import { Toast } from '@/ops/Toast'
import { VerbMenu } from '@/ops/VerbMenu'
import { entitiesOf, targetOf } from '@/ops/palette'
import { useOpsPalette } from '@/ops/paletteStore'
import { lastTurnOf, noneCounts, rowFor } from '@/ops/sections'
import { Sheet } from '@/ops/Sheet'
import { SelfChangesCard } from '@/components/SelfChanges'
import { useOpsAct } from '@/ops/act'
import { useRouterAsks } from '@/ops/asks'
import { HelpButton } from '@/components/Help'
import { TimelineLens, type TimelineReport } from '@/ops/TimelineLens'
import { useOpsCalendar } from '@/ops/calendarwatch'
import { comingEvents, findEvent } from '@/ops/calendar'
import { parseDay, useTimelineKeys } from '@/ops/timelinekeys'

/** A queue item's key in the address: its kind's letter and its id (`q:act_…`), so a link reopens the queue's card. */
/** One empty list for every absent read, so a memo over it holds. */
const NONE: never[] = []

const isItemKey = (s: string | null): boolean => !!s && /^[qfsha]:/.test(s)

/** The lenses that can fill the window (`useFullGraph`). */
const FULLABLE: readonly Lens[] = ['graph', 'timeline']

export default function Ops() {
  const nav = useNavigate()
  const [params, setParams] = useSearchParams()
  const a = useMemo(() => parseAddress(params), [params])
  const full = parseFull(params) && FULLABLE.includes(a.lens)
  // Each change is made on the address as it stands now: two keys pressed faster than a render (`g h`, then tab)
  // must not rebuild the second from the query the first replaced.
  const set = useCallback((patch: Partial<Address>) => setParams(() => {
    const now = new URLSearchParams(window.location.search)
    const next = writeAddress({ ...parseAddress(now), ...patch }, now)
    // A lens with no full-window view leaves the filled window; the Graph and the Timeline keep it, so the new one fills too.
    return patch.lens && !FULLABLE.includes(patch.lens) ? writeFull(next, false) : next
  }, { replace: true }), [setParams])
  const tick = useTick(15_000)
  const world = useDeferredValue(useWorld())
  const past = world?.t ?? null
  const now = past ?? tick
  const { board, ready, error } = useThreadBoard()
  const { data: tl } = useRpc<TaskListResult>('task.list', {}, 3000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 1500)
  const { data: sl } = useRpc<SessionListResult>('session.list', undefined, 5000)
  const history = useHistoryRows()
  const asks = useRouterAsks(tick)
  const { act, on } = useOpsAct()
  // The Timeline's calendar (ops-timeline): the window shown in its lens, and today and tomorrow for Coming up.
  const day = a.lens === 'timeline' ? parseDay(params.get('day')) : null
  const [qFold, setQFold] = usePref(QUEUE_KEY, parseFold)
  const [frame, width] = useWidth<HTMLDivElement>()
  const cal = useOpsCalendar(a.lens === 'timeline', a.w, day, tick, comingShows(width || 1440, qFold))
  const [tlr, setTl] = useState<TimelineReport | null>(null)

  // What the page holds: the present's, or the time machine's world (the board's views are the present's only).
  const views = useMemo(() => (world ? [] : Object.values(board.views)), [world, board.views])
  const groups = useMemo(() => Object.values(board.groups), [board.groups])
  const tasks = world?.tasks ?? tl?.tasks ?? NONE
  const sessions = world?.sessions ?? sl?.sessions ?? NONE
  const confirms = world?.confirms ?? cl?.confirms ?? NONE
  const records = tl?.records ?? NONE
  const titleOf = useCallback((sid: string) => { const s = sessions.find((x) => x.session_id === sid); return s?.title || s?.label || sid.slice(-6) }, [sessions])

  // An ended task the board no longer holds has its report in its session: the first fifty, read once each.
  const ended = useMemo(() => boardRows({ views, tasks, sessions, records, now, past: !!world }).filter((r) => r.kind === 'task' && r.column === 'done' && !r.view?.report).slice(0, DONE_PAGE), [views, tasks, sessions, records, now, world])
  const read = useQueries({ queries: ended.map((r) => ({ queryKey: ['task.report', r.id], staleTime: Infinity, queryFn: async () => reportLineOf((await call<{ nodes: { kind: string; text?: string }[] }>('session.history', { session_id: r.id, n: 3 })).nodes) ?? null })) })
  const lines = useMemo(() => Object.fromEntries(ended.flatMap((r, i) => (read[i]?.data ? [[r.id, read[i].data as string]] : []))), [ended, read])

  const queue = useMemo(() => queueOf({
    confirms, views, records: world ? [] : records, groups: world ? [] : groups, asks: world ? [] : asks, executions: world?.executions, past: !!world, titleOf,
  }), [confirms, views, records, groups, asks, world, titleOf])
  // The one work set (ops/board.ts): every card once, with its asks mapped to it. The counters, the Board, the Graph,
  // the selection and the filters all read these rows and no others.
  const rows = useMemo(() => boardRows({ views, tasks, sessions, records, lines, queue, now, past: !!world }), [views, tasks, sessions, records, lines, queue, now, world])
  const graphViews = useMemo(() => (world ? viewsOf(rows) : []), [world, rows]) // ops-parity pins the page's GraphLens props
  const spend = useMemo(() => spendToday(history.rows, now), [history.rows, now])
  const cx = useMemo(() => ({ now, spentToday: spend.sessions, records }), [now, spend.sessions, records])
  const vis = useMemo(() => shown(rows, a.f, a.q, cx), [rows, a.f, a.q, cx])
  const [older, setOlder] = useState(DONE_PAGE)
  const cols = useMemo(() => columnsOf(vis.rows, now), [vis.rows, now])
  // The Graph fades what the filters and the search leave out, so nothing moves.
  const shownIds = useMemo(() => (Object.keys(a.f).length || a.q ? new Set(vis.rows.map((r) => r.id)) : null), [a.f, a.q, vis.rows])
  const contextIds = useMemo(() => (shownIds ? contextOf(rows, shownIds) : null), [rows, shownIds])
  // Every counter reads the rows alone: needs you is the cards that wait on the owner (Q1 A), where the Needs-you list's
  // header counts its asks.
  const counts = useMemo(() => allCounters(rows, now, spend.usd), [rows, now, spend.usd])
  const grouped = useMemo(() => groupThreads(views, groups, records), [views, groups, records])

  // The selection (ops-graph): in memory, shared by the Graph and the Board, the bulk bar and the palette.
  const picked = useSelection((s) => s.ids)
  const { toggle: pickOne, set: pickAll, clear: unpick } = useSelection.getState()
  const [results, setResults] = useState<RunResult[] | null>(null)
  const closeResults = useCallback(() => setResults(null), [])
  // The Graph's lanes, the threads' words (`threadFacts`), and where a drag or a re-home may send a report.
  const dataOf = useCallback((sid: string | undefined) => sessions.find((x) => x.session_id === sid)?.data ?? undefined, [sessions])
  const laneMetas = useMemo((): LaneMeta[] => grouped.threads.map((t) => {
    const f = threadFacts(t, now)
    return { key: t.group.slug, title: t.group.title, needs: t.needsYou, words: [f.warmth, f.details].filter(Boolean).join(' · '), sealed: f.sealed, data: dataOf(t.group.session_id), head: t.head?.session_id ?? t.group.session_id }
  }), [grouped.threads, now, dataOf])
  const homes = useMemo((): DropTarget[] => [
    ...rows.flatMap((r): DropTarget[] => (r.kind === 'conversation' && r.sessionId && alive(r) && !r.session?.retired ? [{ kind: 'conversation', sessionId: r.sessionId, title: r.title, data: r.view?.data ?? r.session?.data ?? undefined }] : [])),
    ...laneMetas.map((l): DropTarget => ({ kind: 'lane', slug: l.key, title: l.title, data: l.data })),
  ], [rows, laneMetas])
  const projects = useMemo(() => records.filter((t) => t.kind === 'project').map((t): DropTarget => ({ kind: 'project', id: t.id, title: t.title })), [records])
  const dropTargetOf = (key: string): DropTarget | null => [...homes, ...projects].find((t) => (t.kind === 'conversation' ? `conversation:${t.sessionId}` : t.kind === 'lane' ? `lane:${t.slug}` : `project:${t.id}`) === key) ?? null
  const drag = useDragDrop(dropTargetOf, [...homes.filter((t) => t.kind === 'lane'), ...projects], setResults)
  const bulk = useBulk({ rows, queue, homes, projects, titleOf, now, onResults: setResults })
  const offers = bulk.offers(picked)

  // The queue's cursor is an item's key: a new item never moves it; when its item is answered, the next takes it.
  const keys = useMemo(() => queue.map((i) => i.key), [queue])
  const before = useRef<string[]>(keys)
  const [fresh, setFresh] = useState<ReadonlySet<string>>(new Set())
  useEffect(() => {
    const was = before.current
    before.current = keys
    // A new set only when something arrived: a state set on every render would starve the router's navigations.
    const came = arrived(was, queue)
    if (came.length) setFresh(new Set(came))
    // Only an item that was here and left moves the cursor: a deep link read before the queue is never cleared.
    if (isItemKey(a.sel) && was.includes(a.sel!) && !keys.includes(a.sel!)) set({ sel: keepCursor(was, queue, a.sel) })
  }, [keys]) // eslint-disable-line react-hooks/exhaustive-deps

  // What is selected, and where.
  const item: QueueItem | undefined = isItemKey(a.sel) ? queue.find((i) => i.key === a.sel) : a.do === 'answer' ? itemFor(queue, a.sel) : undefined
  const row: OpsRow | undefined = item ? undefined : rowFor(rows, a.sel)
  // A thread's head card on the Graph selects its lane, which is a thread, not a row (theseus-hl0r1 F4).
  const laneSel = a.lens === 'graph' && laneMetas.some((l) => l.key === a.sel)
  // The idle tab opened the panel with nothing selected; so does a Graph or Board selection nothing resolves
  // (theseus-gnhnk F5), which would otherwise fall back to the idle tab.
  const handOpen = a.sel === OPEN_SEL || ((a.lens === 'graph' || a.lens === 'board') && !!a.sel && !item && !row && !laneSel)
  const slug = (a.lens === 'threads' || laneSel) && a.sel && !item && !handOpen ? a.sel : null
  const event = a.lens === 'timeline' && a.sel?.startsWith('ev:') ? findEvent(cal.calendars, a.sel.slice(3)) : null
  const mark = a.lens === 'timeline' && !item && !handOpen ? tlr?.marks.find((m) => m.key === a.sel) : undefined
  const selection: Selection = item ? { kind: 'item', item } : slug ? { kind: 'thread', slug } : row ? { kind: 'row', row } : event ? { kind: 'event', ...event, redact: a.redact } : a.lens === 'timeline' && a.sel && !handOpen ? { kind: 'mark', key: a.sel } : { kind: 'none', counts: noneCounts(counts), onFilter: (b) => set({ f: { ...a.f, bucket: b } }) }
  const region: 'queue' | 'canvas' = item ? 'queue' : 'canvas'
  // The item a key answers: the queue's cursor, or the needs-you item of the row on the canvas.
  const focusItem = item ?? (row ? queue.find((i) => (row.view != null && i.view?.id === row.view.id) || (row.sessionId != null && i.sessionId === row.sessionId)) : undefined)
  const focusRow = row ?? (item?.sessionId ? rows.find((r) => r.sessionId === item.sessionId && r.kind !== 'job') : undefined)
  const [open, setOpen] = useState<string | null>(a.do === 'answer' && item ? item.key : null)
  const [note, setNote] = useState<{ key: string; text: string } | null>(null)

  // The regions, their folds kept in this browser, and the narrow-screen rules.
  const [iFold, setIFold] = usePref(INSPECTOR_KEY, parseFold)
  const reg = regionsFor(width || 1440, qFold, iFold, selection.kind !== 'none', handOpen)
  const [keysOpen, setKeysOpen] = useState(false)
  const [newTask, setNewTask] = useState<{ home: string } | null>(null)
  // A menu carries the session it acts on: the Join pane's own, not the focused card's (ops-act J1).
  const [menu, setMenu] = useState<{ kind: 'model' | 'mode'; sid: string } | null>(null)
  const [selfOpen, setSelfOpen] = useState(false)
  const graphRef = useRef<Graph | null>(null) // the Graph lens's last layout, for `j k h l`
  const [ctxMenu, setCtxMenu] = useState<{ id: string; x: number; y: number } | null>(null)
  const [picker, setPicker] = useState(false)
  const [dockFold, setDockFold] = usePref(DOCK_KEY, parseFold)
  const [dock, setDock] = useState<DockState>({ paused: false, follow: false })
  const [dockFocus, setDockFocus] = useState(false)
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 0)
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 0)
  const [flash, setFlash] = useState<string | null>(null)
  useEffect(() => { if (!flash) return; const t = window.setTimeout(() => setFlash(null), 3500); return () => window.clearTimeout(t) }, [flash])
  const searchRef = useRef<HTMLInputElement>(null)
  const armed = useRef<{ verb: Verb; at: number } | null>(null)

  const lanes = laneRows(cols, older)
  const threadList = grouped.threads.filter((t) => (!a.f.thread || t.group.slug === a.f.thread) && (!a.q || a.q.toLowerCase().split(/\s+/).every((w) => `${t.group.title}\n${t.group.slug}`.toLowerCase().includes(w))))
  const shownSlugs = new Set(threadList.map((t) => t.group.slug))
  const pick = (sel: string | null) => set({ sel, do: null })
  const answer = (verb: Verb, x: Answer) => {
    const c = focusItem && answerOf(focusItem, x)
    if (!c) { setFlash('nothing here takes that answer'); return }
    void act(verb, `${focusItem!.key}:${x.verb}`, c.method, c.params, c.ask).then((ok) => { if (ok) setNote(null) })
  }
  /** Move within the canvas: along a column (`j`/`k`) or across (`h`/`l`), the queue left of the first column. */
  const move = (dx: number, dy: number) => {
    if (a.lens === 'graph' && (region !== 'queue' || dx > 0)) {
      const to = stepNode(graphRef.current ?? { nodes: [] }, region === 'queue' ? null : row?.id ?? a.sel ?? null, dx, dy)
      if (to) pick(to); else if (dx < 0) pick(queue[0]?.key ?? null)
      return
    }
    if (region === 'queue' || (a.lens !== 'board' && a.lens !== 'threads')) {
      if (dx > 0 && a.lens === 'board') { const first = lanes.find((l) => l.length); if (first) pick(first[0].id); return }
      if (dy) pick(step(queue, item?.key ?? null, dy > 0 ? 1 : -1))
      return
    }
    if (a.lens === 'threads') {
      const at = threadList.findIndex((t) => t.group.slug === slug)
      if (dx < 0) { pick(queue[0]?.key ?? null); return }
      const next = threadList[Math.max(0, Math.min(threadList.length - 1, at + dy))]
      if (next) pick(next.group.slug)
      return
    }
    const c = Math.max(0, lanes.findIndex((l) => l.some((r) => r.id === row?.id)))
    const i = row ? lanes[c].findIndex((r) => r.id === row.id) : -1
    if (dy) { const lane = lanes[c]; if (lane.length) pick(lane[Math.max(0, Math.min(lane.length - 1, i + dy))].id); return }
    if (dx < 0 && c === 0) { pick(queue[0]?.key ?? null); return }
    for (let k = c + dx; k >= 0 && k < lanes.length; k += dx) if (lanes[k].length) { pick(lanes[k][Math.min(Math.max(0, i), lanes[k].length - 1)].id); return }
  }
  const twice = (verb: Verb, fire: () => void, words: string) => {
    const s = pressTwice(armed.current, verb, Date.now())
    armed.current = s.armed
    if (s.fire) fire(); else setFlash(words)
  }
  const lens = (l: Lens) => set({ lens: l, sel: isItemKey(a.sel) ? a.sel : null })
  const bucket = (b: 'needs' | 'working' | 'paused' | 'done') => set({ f: { ...a.f, bucket: a.f.bucket === b ? undefined : b } })
  const living = focusRow?.view?.state === 'running' || focusRow?.view?.state === 'waiting'
  const exe = focusRow?.kind === 'conversation' ? focusRow.view?.id ?? focusRow.session?.execution_id : undefined
  const tk = useTimelineKeys({ a, day, report: tlr, mark, set, pick, flash: setFlash, open: (sid) => nav(`/session/${sid}`) })
  const menuSession = menu ? sessions.find((x) => x.session_id === menu.sid) : undefined
  // The session the keys' menus and the composer act on: the focused task's or conversation's.
  const focusSession = (focusRow && (focusRow.kind === 'task' || focusRow.kind === 'conversation') ? focusRow.session : undefined) ?? sessions.find((x) => x.session_id === a.join)
  /** PgUp, PgDn and End scroll the open transcript (the Join pane's, else the inspector's). */
  const scrollTranscript = (by: number) => {
    const box = document.querySelector<HTMLElement>('[data-testid="join-pane"] .overflow-auto')
    if (box) box.scrollTop = by === Infinity ? box.scrollHeight : box.scrollTop + by * box.clientHeight
  }

  const handlers: Record<Verb, (key: string) => void> = {
    down: () => move(0, 1),
    up: () => move(0, -1),
    left: () => move(-1, 0),
    right: () => move(1, 0),
    // `l` follows a job in the inspector, where its output streams (Live); the job is already the selection.
    follow: () => { if (iFold === 'folded') setIFold('open') },
    inspect: () => { if (item) setOpen(open === item.key ? null : item.key); else if (iFold === 'folded') setIFold('open') },
    open: () => { const sid = focusRow?.sessionId ?? item?.sessionId; if (sid) nav(`/session/${sid}`) },
    back: () => { if (picked.size) unpick(); else if (note) setNote(null); else if (a.join) set({ join: null }); else if (open) setOpen(null); else if (a.sel) pick(null); else set({ f: {}, q: '' }) },
    next: () => pick(step(queue, item?.key ?? null, 1)),
    prev: () => pick(step(queue, item?.key ?? null, -1)),
    approve: () => answer('approve', { verb: 'approve' }),
    decline: () => { if (focusItem) { pick(focusItem.key); setNote({ key: focusItem.key, text: '' }) } },
    choice: (key) => answer('choice', { verb: 'choice', n: Number(key) }),
    grant: () => answer('grant', { verb: 'grant' }),
    retry: () => answer('retry', { verb: 'retry' }),
    dismiss: () => answer('dismiss', { verb: 'dismiss' }),
    pause: () => { if (focusRow?.kind === 'task') void act('pause', focusRow.id, focusRow.paused ? 'task.resume' : 'task.pause', { task: focusRow.id }) },
    stop: () => twice('stop', () => { if (exe) void act('stop', `${exe}:stop`, 'execution.stop', { execution_id: exe }) }, 's again stops the running turn'),
    stop_all: () => { if (exe) void act('stop_all', `${exe}:all`, 'execution.stop', { execution_id: exe, tasks: true }, 'Stop its turn, and cancel every task under it?') },
    cancel: () => twice('cancel', () => { if (focusRow?.kind === 'task') void act('cancel', `${focusRow.id}:stop`, 'task.cancel', { task: focusRow.id }) }, 'c again cancels the task'),
    not_this: () => twice('not_this', () => { if (focusRow?.kind === 'task') void act('not_this', `${focusRow.id}:not_this`, 'task.not_this', { task: focusRow.id }) }, 'u again says not this: it cancels the task and keeps work like it from starting'),
    cancel_wake: () => { if (focusRow?.kind === 'wake') void act('cancel_wake', `${focusRow.id}:x`, 'wake.cancel', { wake: focusRow.id }, `Cancel the wake “${focusRow.title}”?`) },
    new_task: () => { if (on('new_task')) setNewTask({ home: focusRow?.kind === 'conversation' ? focusRow.sessionId ?? '' : focusRow?.task?.info?.parent_session_id ?? '' }) },
    lens_graph: () => lens('graph'),
    lens_board: () => lens('board'),
    lens_timeline: () => lens('timeline'),
    lens_threads: () => lens('threads'),
    home: () => nav('/ship'),
    search: () => searchRef.current?.focus(),
    fold_queue: () => setQFold(flip(qFold)),
    fold_inspector: () => setIFold(flip(iFold)),
    fold_dock: () => setDockFold(flip(dockFold)),
    model_menu: () => { if (focusSession && on('model_menu')) setMenu({ kind: 'model', sid: focusSession.session_id }) },
    mode_menu: () => { if ((focusSession?.kind === 'conversation' || focusSession?.kind === 'task') && on('mode_menu')) setMenu({ kind: 'mode', sid: focusSession.session_id }) },
    compose: () => { const id = focusSession?.session_id ?? focusRow?.sessionId ?? item?.sessionId ?? slug; if (id) set({ join: id }); else setFlash('select a session, a task or a thread to join') },
    parent: () => { const p = focusRow?.task?.info?.parent_session_id; if (p) pick(p); else setFlash('it reports to no parent') },
    last_turn: () => { const sid = focusRow?.sessionId ?? item?.sessionId; const t = sid ? lastTurnOf(history.rows, sid) : undefined; if (sid && t) nav(`/session/${sid}/turn/${t}`); else setFlash('no turn has ended in it yet') },
    scroll_up: () => scrollTranscript(-0.85),
    scroll_down: () => scrollTranscript(0.85),
    scroll_end: () => scrollTranscript(Infinity),
    self_changes: () => setSelfOpen(true),
    thread_picker: () => setPicker(true),
    palette: () => {}, // the shell's own listener opens ⌘K; this entry is its line in the overlay
    dock_pause: () => setDock((d) => ({ ...d, paused: !d.paused })),
    keys: () => setKeysOpen((v) => !v),
    filter_needs: () => bucket('needs'),
    filter_working: () => bucket('working'),
    filter_paused: () => bucket('paused'),
    filter_done: () => bucket('done'),
    filter_all: () => set({ f: {}, q: '' }),
    zoom_day: () => tk.run('zoom_day'),
    zoom_week: () => tk.run('zoom_week'),
    redact: () => tk.run('redact'),
    week_prev: () => tk.run('week_prev'),
    week_next: () => tk.run('week_next'),
    mark_next: () => tk.run('mark_next'),
    mark_prev: () => tk.run('mark_prev'),
    mark_snooze: () => tk.run('mark_snooze'),
    mark_pause: () => tk.run('mark_pause'),
    mark_run: () => tk.run('mark_run'),
    mark_cancel: () => tk.run('mark_cancel'),
    mark_open: () => tk.run('mark_open'),
    select: () => {
      // The pick box that holds the DOM focus (a click on it does not focus the card), else the focused card.
      const held = document.activeElement?.closest<HTMLElement>('[data-pick]')?.dataset.pick
      const id = held ?? focusRow?.id ?? row?.id
      if (id) pickOne(id)
    },
    select_all: () => pickAll(vis.rows.map((r) => r.id)),
    full_window: () => { if (FULLABLE.includes(a.lens)) setParams((p) => writeFull(p, !full), { replace: full }) },
  } // end of handlers

  // The contexts the focus offers, most specific first (`keys.ts`).
  const ctxs = useMemo(() => {
    const c = new Set<Ctx>()
    if (focusItem || focusRow || slug) c.add('card')
    if (focusItem?.confirm || focusItem?.proposal) c.add('answer')
    if (focusItem?.view?.question?.choices?.length) c.add('choice')
    if (focusItem?.confirm?.grant_offer) c.add('grant')
    if (focusItem?.kind === 'failure') c.add('failure')
    if (focusRow?.kind === 'task' && focusRow.column !== 'done') c.add('task')
    if (focusRow?.kind === 'conversation' && living) c.add('running')
    if (focusRow?.kind === 'job') c.add('job')
    if (focusRow?.kind === 'wake') c.add('wake')
    if (a.lens === 'timeline' && region === 'canvas') c.add('timeline')
    if (mark) c.add('mark')
    if (focusSession) c.add('session')
    if (dockFocus) c.add('dock')
    return c
  }, [focusItem, focusRow, focusSession, slug, living, a.lens, region, mark, dockFocus])

  // One handler for the page, ahead of the shell's: a `g` sequence the page claims is the page's; one it does not
  // (`g f`) goes on to the shell. Keys never fire while typing.
  const modal = !!(newTask || menu || selfOpen || picker)
  const picks = a.lens === 'graph' || a.lens === 'board'
  const live = useRef({ handlers, ctxs, focusItem, keysOpen, modal, picks, full })
  useLayoutEffect(() => { live.current = { handlers, ctxs, focusItem, keysOpen, modal, picks, full } })
  useEffect(() => {
    let g = 0
    const k = (e: KeyboardEvent) => {
      // ⌘A and ctrl+a pick everything shown on the Graph and the Board; any other chord is the browser's.
      if (e.altKey || ((e.metaKey || e.ctrlKey) && (keyName(e) !== 'mod+a' || !live.current.picks))) return
      if (typingIn(e.target as HTMLElement | null)) return
      const { handlers: h, ctxs: c, focusItem: fi, keysOpen: ko, modal: mo } = live.current
      if (mo) return // a sheet or a menu has the keys
      if (ko && e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); h.keys('?'); return }
      // The window is filled: Esc only leaves it (the graph helper's own listener does, after this one), and clears nothing.
      if (live.current.full && fullKey(e, true) === 'leave') return
      let name = keyName(e)
      const afterG = g > 0 && Date.now() - g < G_MS
      if (afterG && SEQUENCES.includes(`g ${name}`)) name = `g ${name}`
      else if (afterG && name === 'f') { g = 0; return } // `g f` is the shell's (the Fleet)
      g = 0
      if (name === 'g') { g = Date.now(); return }
      let hit = resolve(name, c, false)
      // A grant's key is the offer's own: `a` for a command, `e` for edits; the other falls through.
      if (hit?.verb === 'grant' && fi?.confirm && grantKey(fi.confirm) !== name) hit = resolve(name, new Set([...c].filter((x) => x !== 'grant')), false)
      if (!hit) return
      e.preventDefault()
      e.stopImmediatePropagation()
      h[hit.verb](name)
    }
    window.addEventListener('keydown', k, true)
    return () => window.removeEventListener('keydown', k, true)
  }, [])

  const filter = (k: FilterKey, v: string | null) => set({ f: { ...a.f, [k]: v ?? undefined } })
  const choices = useMemo(() => ({ ...chipChoices(rows), project: records.filter((t) => t.kind === 'project').map((t) => t.id), data: [...DATA_MARKS] }), [rows, records])
  const n = a.lens === 'threads' ? threadList.length : a.lens === 'timeline' && tlr ? tlr.n : vis.rows.length
  const of = a.lens === 'threads' ? grouped.threads.length : a.lens === 'timeline' && tlr ? tlr.of : vis.of

  const inspectorCx = { records, rows, onMenu: (m: 'model' | 'mode', sid: string) => setMenu({ kind: m, sid }), onParent: (sid: string) => pick(sid), onJoin: (id: string) => set({ join: id }), joined: a.join }

  // What the palette sees: the focused card, all that are visible, and the picked ones (ops-graph's selection).
  const feed = useMemo(() => ({
    targets: {
      one: focusRow ? [targetOf(focusRow)] : [],
      visible: vis.rows.map(targetOf),
      selection: vis.rows.filter((r) => picked.has(r.id)).map(targetOf), // ops-graph: the selection scope
    },
    entities: entitiesOf(rows, grouped.threads.map((t) => ({ slug: t.group.slug, title: t.group.title })), records, pl?.profiles ?? [], watchRows(views)),
    profiles: pl?.profiles ?? [], prices: pricing(cat),
    go: (e: { go: { sel: string } | { model: string } | { lens: 'timeline' } }) => { if ('sel' in e.go) pick(e.go.sel); else if ('lens' in e.go) lens(e.go.lens); else set({ f: { ...a.f, model: e.go.model } }) },
    newTask: () => live.current.handlers.new_task('N'), selfChanges: () => setSelfOpen(true),
  }), [focusRow, vis.rows, picked, rows, grouped.threads, records, pl, cat, a.f, views]) // eslint-disable-line react-hooks/exhaustive-deps
  // The palette opens over the picked cards while any are picked, and over the focused one when none are.
  useEffect(() => { useOpsPalette.setState({ scope: picked.size > 0 ? 'selection' : 'one' }) }, [picked.size])
  const publish = useOpsPalette((x) => x.publish)
  useEffect(() => { publish(feed as never); return () => publish(null) }, [feed, publish])

  const queuePanel = (
    <Queue items={queue} cursor={item?.key ?? null} onCursor={(k) => pick(k)} open={open} note={note} onNote={setNote} past={past} now={now}
      coming={world ? [] : comingUp(Object.values(board.views), tick)} events={world ? [] : comingEvents(cal.calendars, tick, a.redact)} arrivedKeys={fresh} />
  )
  const joined = a.join ?? null
  const lensView = (
    <div className="h-full min-h-0 min-w-0 flex-1">
      {a.lens === 'board' && <BoardLens cols={cols} rows={rows} older={older} onMore={() => setOlder((x) => x + DONE_PAGE)} focus={row?.id ?? null} onFocus={pick} onContext={(id, x, y) => setCtxMenu({ id, x, y })} confirms={confirms} now={now}
        picked={picked} onPick={pickOne} drag={drag} titleOf={titleOf} />}
      {a.lens === 'threads' && <ThreadsLens views={views} groups={groups} records={records} ready={ready || !!world} error={error} now={now} past={past} focus={slug} onFocus={pick}
        matches={(t) => shownSlugs.has(t.group.slug)} rows={rows} kept={shownIds} context={contextIds} onSelect={pick} />}
      {a.lens === 'timeline' && (
        <div className="h-full overflow-y-auto pr-1">
          {world && <p className="mb-2 text-[11.5px] text-wait">The agenda shows the present: its marks are the daemon’s now, not the log’s moment.</p>}
          <TimelineLens weeks={a.w} day={day} sel={a.sel} redact={a.redact} q={a.q} cal={cal} past={!!world} onSel={pick} onWeek={tk.shift} onToday={tk.today} onZoom={tk.zoom}
            onReport={setTl} onOpen={(sid) => nav(`/session/${sid}`)} />
        </div>
      )}
      {a.lens === 'graph' && <GraphLens views={world ? graphViews : views} records={records} rows={rows} lanes={laneMetas} now={now} focus={row?.id ?? a.sel} picked={picked}
        shownIds={shownIds} contextIds={contextIds} filterKey={JSON.stringify(a.f) + a.q} chip={a.f.bucket ? { bucket: a.f.bucket as Bucket, n: counts[a.f.bucket as Bucket], onClear: () => filter('bucket', null) } : null} onFocus={pick} onPick={pickOne} onLasso={(ids) => pickAll([...picked, ...ids])}
        onOpen={(sid) => nav(`/session/${sid}`)} onContext={(id, x, y) => setCtxMenu({ id, x, y })} redact={a.redact} layoutRef={graphRef} drag={drag} titleOf={titleOf} />}
    </div>
  )
  const canvas = joined ? (
    <div className="flex h-full min-h-0 gap-2.5">
      {lensView}
      <aside aria-label="Join" className="flex w-[460px] max-w-[48%] shrink-0 flex-col overflow-hidden rounded-xl border border-line-strong bg-hull/60">
        <JoinPane key={joined} sessionId={joined.startsWith('ses_') ? joined : undefined} thread={joined.startsWith('ses_') ? undefined : joined} session={sessions.find((x) => x.session_id === joined)} focus onPicker={() => setPicker(true)} onMenu={(m, sid) => setMenu({ kind: m, sid })} />
      </aside>
    </div>
  ) : lensView
  return (
    <div ref={frame} className="flex h-full min-h-[560px] flex-col gap-2.5" data-testid="ops-page">
      {world && (
        <div className="flex items-center gap-2 rounded-lg bg-wait/[0.06] px-3 py-2 text-[12px] text-wait ring-1 ring-wait/30">
          <History size={14} /> As of {stamp(world.t)}, folded from the ledger. Acting needs the present: return to LIVE in the ship&rsquo;s log below.
        </div>
      )}
      <CommandBar counts={counts} f={a.f} onFilter={filter} lens={a.lens} onLens={lens} choices={choices} q={a.q} onQ={(q) => set({ q })} n={n} of={of}
        onNew={() => handlers.new_task('N')} searchRef={searchRef} timeline={tlr ? { meetings: tlr.meetings, clashes: tlr.clashes } : undefined} />
      {newTask && <NewTask home={newTask.home} onClose={() => setNewTask(null)} onStarted={(id) => { setNewTask(null); set({ lens: 'board', sel: id }) }} />}
      <div className="relative flex min-h-0 flex-1 gap-2.5">
        {reg.queue === 'side' && (
          <aside aria-label="Needs you" className="flex w-[300px] shrink-0 flex-col overflow-hidden rounded-xl border border-line bg-hull/60">
            <RegionHead title="Needs you" count={queue.length} glow={queue.length > 0} onFold={() => setQFold('folded')} fold="[" />
            {queuePanel}
          </aside>
        )}
        {reg.queue === 'folded' && <Tab side="left" label={`Needs you · ${asksWord(queue.length)}`} onClick={() => setQFold('open')} glow={queue.length > 0} />}
        {reg.queue === 'page' && <section aria-label="Needs you" className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-hull/60"><RegionHead title="Needs you" count={queue.length} glow={queue.length > 0} />{queuePanel}</section>}
        {reg.canvas && (
          <main aria-label={`${a.lens} lens`} className="flex min-h-0 min-w-0 flex-1 flex-col gap-2">
            {picks && picked.size > 0 && <BulkBar count={picked.size} offers={offers} onVerb={(v) => bulk.start(v, picked)} onClear={unpick} />}
            <div className="min-h-0 flex-1">{canvas}</div>
          </main>
        )}
        {reg.inspector === 'side' && (
          <aside aria-label="Inspector" className="flex w-[360px] shrink-0 flex-col overflow-hidden rounded-xl border border-line bg-hull/60">
            <RegionHead title="Inspector" onFold={() => setIFold('folded')} fold="]" />
            <div className="min-h-0 flex-1 overflow-y-auto"><Inspector sel={selection} confirms={confirms} past={past} now={now} cx={inspectorCx} /></div>
          </aside>
        )}
        {reg.inspector === 'over' && (
          <aside aria-label="Inspector" className="absolute inset-y-0 right-0 z-20 flex w-[min(380px,92%)] flex-col overflow-hidden rounded-xl border border-line-strong bg-hull/95 shadow-2xl backdrop-blur">
            <RegionHead title="Inspector" onFold={() => pick(null)} fold="esc" />
            <div className="min-h-0 flex-1 overflow-y-auto"><Inspector sel={selection} confirms={confirms} past={past} now={now} cx={inspectorCx} /></div>
          </aside>
        )}
        {reg.inspector === 'folded' && <Tab side="right" label="Inspector" onClick={() => setIFold('open')} />}
        {reg.inspector === 'idle' && <Tab side="right" label="Inspector · select a card" onClick={() => pick(idleSel(queue[0]?.key))} />}
      </div>
      {dockFold === 'open' && <Dock state={dock} onState={setDock} onFocus={setDockFocus} selected={focusRow?.sessionId ?? null} sessions={sessions} activeModel={a.f.model} onModel={(m) => filter('model', m)} />}
      {dockFold === 'folded' && <button onClick={() => setDockFold('open')} title="the dock (\\)" className="self-start rounded-lg border border-line bg-hull/60 px-3 py-1 text-[11px] text-ink-faint hover:text-ink">Dock · live streams and models <span className="kbd">\\</span></button>}
      <Toast />
      {ctxMenu && rows.find((r) => r.id === ctxMenu.id) && <VerbMenu target={targetOf(rows.find((r) => r.id === ctxMenu.id)!)} x={ctxMenu.x} y={ctxMenu.y} onMenu={(m) => { const sid = rows.find((r) => r.id === ctxMenu.id)?.sessionId; if (sid) setMenu({ kind: m, sid }) }} onClose={() => setCtxMenu(null)} />}
      {menu?.kind === 'model' && menuSession && <ModelMenu session={menuSession} onClose={() => setMenu(null)} />}
      {menu?.kind === 'mode' && menuSession && <ModeMenu session={menuSession} onClose={() => setMenu(null)} />}
      {selfOpen && (
        <Sheet title="What Theseus changed about itself" verb="inspect" submit="Close" onSubmit={() => setSelfOpen(false)} onClose={() => setSelfOpen(false)} wide><SelfChangesCard now={now} /></Sheet>
      )}
      {picker && <ThreadPicker threads={threadList.length ? threadList.map((t) => t.group) : grouped.threads.map((t) => t.group)} onPick={(slug) => { setPicker(false); set({ lens: 'threads', sel: slug, join: slug }) }} onClose={() => setPicker(false)} />}
      {flash && <div role="status" className="pointer-events-none fixed bottom-24 left-1/2 z-30 -translate-x-1/2 rounded-lg bg-hull/95 px-3 py-1.5 text-[12px] text-ink ring-1 ring-line-strong shadow-xl">{flash}</div>}
      {keysOpen && <KeysOverlay onClose={() => setKeysOpen(false)} />}
      {drag.layer}
      {bulk.sheet}
      {results && <ResultsToast results={results} onClose={closeResults} />}
    </div>
  )
}

function RegionHead({ title, count, glow, onFold, fold }: { title: 'Needs you' | 'Inspector'; count?: number; glow?: boolean; onFold?: () => void; fold?: string }) {
  return (
    <header className="flex items-center gap-2 border-b border-line px-3 py-2">
      {title === 'Needs you' && <Inbox size={13} className={glow ? 'text-wait' : 'text-ink-faint'} />}
      <h2 className="font-display text-[11.5px] font-bold uppercase tracking-[0.08em] text-gold">{title}</h2>
      {count !== undefined && <span className={cn('num rounded-full px-1.5 text-[11px]', glow ? 'bg-wait/15 text-wait' : 'text-ink-faint')}>{title === 'Needs you' ? asksWord(count) : count}</span>}
      <span className="ml-auto">{title === 'Needs you' ? <HelpButton id="ops.queue" title="Needs you" /> : <HelpButton id="ops.inspector" title="Inspector" />}</span>
      {onFold && <button onClick={onFold} title={`fold (${fold})`} aria-label={`fold the ${title.toLowerCase()}`} className="flex items-center gap-1 text-[10.5px] text-ink-faint hover:text-ink"><span className="kbd">{fold}</span><X size={11} /></button>}
    </header>
  )
}

function Tab({ side, label, onClick, glow }: { side: 'left' | 'right'; label: string; onClick: () => void; glow?: boolean }) {
  const Icon = side === 'left' ? PanelLeftOpen : PanelRightOpen
  return (
    <button onClick={onClick} title={`${label} (${side === 'left' ? '[' : ']'})`}
      className={cn('flex w-8 shrink-0 flex-col items-center gap-2 rounded-xl border border-line bg-hull/60 py-3 text-ink-faint hover:text-ink', glow && 'border-wait/50 text-wait shadow-[0_0_14px_-4px_var(--color-wait)]')}>
      <Icon size={14} />
      <span className="font-display text-[10.5px] font-bold uppercase tracking-[0.08em] [writing-mode:vertical-rl]">{label}</span>
    </button>
  )
}
