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
import type { ConfirmRequest, SessionListResult, TaskListResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useWorld } from '@/lib/world'
import { useHistoryRows } from '@/lib/history'
import { useThreadBoard } from '@/lib/threadwatch'
import { useWidth } from '@/lib/chartview'
import { usePref } from '@/lib/usePref'
import { reportLineOf } from '@/lib/taskboard'
import { groupThreads } from '@/lib/threadboard'
import { cn, stamp } from '@/lib/format'
import { TaskGraphView } from '@/components/TaskGraph'
import Agenda from './Agenda'
import { parseAddress, writeAddress, type Address, type FilterKey, type Lens } from '@/ops/address'
import { boardRows, chipChoices, columnsOf, countersOf, DONE_PAGE, laneRows, selects, shown, spendToday, type OpsRow } from '@/ops/board'
import { answerOf, comingUp, grantKey, itemFor, keepCursor, queueOf, step, arrived, type Answer, type QueueItem } from '@/ops/queue'
import { G_MS, keyName, pressTwice, resolve, SEQUENCES, typingIn, type Ctx, type Verb } from '@/ops/keys'
import { flip, INSPECTOR_KEY, parseFold, QUEUE_KEY, regionsFor } from '@/ops/regions'
import { BoardLens } from '@/ops/BoardLens'
import { ThreadsLens } from '@/ops/ThreadsLens'
import { Queue } from '@/ops/Queue'
import { Inspector, type Selection } from '@/ops/Inspector'
import { CommandBar } from '@/ops/CommandBar'
import { KeysOverlay } from '@/ops/KeysOverlay'
import { NewTask } from '@/ops/NewTask'
import { useOpsAct } from '@/ops/act'
import { useRouterAsks } from '@/ops/asks'
import { HelpButton } from '@/components/Help'

/** A queue item's key in the address: its kind's letter and its id (`q:act_…`), so a link reopens the queue's card. */
/** One empty list for every absent read, so a memo over it holds. */
const NONE: never[] = []

const isItemKey = (s: string | null): boolean => !!s && /^[qfsha]:/.test(s)

export default function Ops() {
  const nav = useNavigate()
  const [params, setParams] = useSearchParams()
  const a = useMemo(() => parseAddress(params), [params])
  const set = useCallback((patch: Partial<Address>) => setParams((p) => writeAddress({ ...parseAddress(p), ...patch }, p), { replace: true }), [setParams])
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

  // What the page holds: the present's, or the time machine's world (the board's views are the present's only).
  const views = useMemo(() => (world ? [] : Object.values(board.views)), [world, board.views])
  const groups = useMemo(() => Object.values(board.groups), [board.groups])
  const tasks = world?.tasks ?? tl?.tasks ?? NONE
  const sessions = world?.sessions ?? sl?.sessions ?? NONE
  const confirms = world?.confirms ?? cl?.confirms ?? NONE
  const records = tl?.records ?? NONE
  const titleOf = useCallback((sid: string) => { const s = sessions.find((x) => x.session_id === sid); return s?.title || s?.label || sid.slice(-6) }, [sessions])

  // An ended task the board no longer holds has its report in its session: the first fifty, read once each.
  const ended = useMemo(() => boardRows({ views, tasks, sessions, now }).filter((r) => r.kind === 'task' && r.column === 'done' && !r.view?.report).slice(0, DONE_PAGE), [views, tasks, sessions, now])
  const read = useQueries({ queries: ended.map((r) => ({ queryKey: ['task.report', r.id], staleTime: Infinity, queryFn: async () => reportLineOf((await call<{ nodes: { kind: string; text?: string }[] }>('session.history', { session_id: r.id, n: 3 })).nodes) ?? null })) })
  const lines = useMemo(() => Object.fromEntries(ended.flatMap((r, i) => (read[i]?.data ? [[r.id, read[i].data as string]] : []))), [ended, read])

  const rows = useMemo(() => boardRows({ views, tasks, sessions, lines, now }), [views, tasks, sessions, lines, now])
  const spend = useMemo(() => spendToday(history.rows, now), [history.rows, now])
  const cx = useMemo(() => ({ now, spentToday: spend.sessions, records }), [now, spend.sessions, records])
  const vis = useMemo(() => shown(rows, a.f, a.q, cx), [rows, a.f, a.q, cx])
  const [older, setOlder] = useState(DONE_PAGE)
  const cols = useMemo(() => columnsOf(vis.rows, now), [vis.rows, now])
  const holds = useMemo(() => (world
    ? world.holds.map((h) => ({ session_id: h.session_id, title: h.title || titleOf(h.session_id), external_text: h.held }))
    : sessions.flatMap((s) => (s.external_text ? [{ session_id: s.session_id, title: s.title || s.label || s.session_id.slice(-6), external_text: s.external_text }] : []))), [world, sessions, titleOf])
  const queue = useMemo(() => queueOf({
    confirms, views, records: world ? [] : records, groups: world ? [] : groups, holds, asks: world ? [] : asks, executions: world?.executions, titleOf,
  }), [confirms, views, records, groups, holds, asks, world, titleOf])
  const counts = useMemo(() => ({ needs: queue.length, ...countersOf(rows, now, spend.usd) }), [queue.length, rows, now, spend.usd])
  const grouped = useMemo(() => groupThreads(views, groups), [views, groups])

  // The queue's cursor is an item's key: a new item never moves it; when its item is answered, the next takes it.
  const keys = useMemo(() => queue.map((i) => i.key), [queue])
  const before = useRef<string[]>(keys)
  const [fresh, setFresh] = useState<ReadonlySet<string>>(new Set())
  useEffect(() => {
    const was = before.current
    before.current = keys
    if (was !== keys) setFresh(new Set(arrived(was, queue)))
    if (isItemKey(a.sel) && !keys.includes(a.sel!)) set({ sel: keepCursor(was, queue, a.sel) })
  }, [keys]) // eslint-disable-line react-hooks/exhaustive-deps

  // What is selected, and where.
  const item: QueueItem | undefined = isItemKey(a.sel) ? queue.find((i) => i.key === a.sel) : a.do === 'answer' ? itemFor(queue, a.sel) : undefined
  const row: OpsRow | undefined = item ? undefined : rows.find((r) => selects(r, a.sel))
  const slug = a.lens === 'threads' && a.sel && !item ? a.sel : null
  const selection: Selection = item ? { kind: 'item', item } : slug ? { kind: 'thread', slug } : row ? { kind: 'row', row } : a.lens === 'timeline' && a.sel ? { kind: 'mark', key: a.sel } : { kind: 'none' }
  const region: 'queue' | 'canvas' = item ? 'queue' : 'canvas'
  // The item a key answers: the queue's cursor, or the needs-you item of the row on the canvas.
  const focusItem = item ?? (row ? queue.find((i) => i.view?.id === row.view?.id || (i.sessionId === row.sessionId && i.kind !== 'hold')) : undefined)
  const focusRow = row ?? (item ? rows.find((r) => r.sessionId === item.sessionId && r.kind !== 'job') : undefined)
  const [open, setOpen] = useState<string | null>(a.do === 'answer' && item ? item.key : null)
  const [note, setNote] = useState<{ key: string; text: string } | null>(null)

  // The regions, their folds kept in this browser, and the narrow-screen rules.
  const [qFold, setQFold] = usePref(QUEUE_KEY, parseFold)
  const [iFold, setIFold] = usePref(INSPECTOR_KEY, parseFold)
  const [frame, width] = useWidth<HTMLDivElement>()
  const reg = regionsFor(width || 1440, qFold, iFold, selection.kind !== 'none')
  const [keysOpen, setKeysOpen] = useState(false)
  const [newTask, setNewTask] = useState<{ home: string } | null>(null)
  const [flash, setFlash] = useState<string | null>(null)
  useEffect(() => { if (!flash) return; const t = window.setTimeout(() => setFlash(null), 3500); return () => window.clearTimeout(t) }, [flash])
  const searchRef = useRef<HTMLInputElement>(null)
  const armed = useRef<{ verb: Verb; at: number } | null>(null)

  const lanes = laneRows(cols, older)
  const threadList = grouped.threads.filter((t) => (!a.f.thread || t.group.slug === a.f.thread) && (!a.q || a.q.toLowerCase().split(/\s+/).every((w) => `${t.group.title}\n${t.group.slug}`.toLowerCase().includes(w))))
  const pick = (sel: string | null) => set({ sel, do: null })
  const answer = (verb: Verb, x: Answer) => {
    const c = focusItem && answerOf(focusItem, x)
    if (!c) { setFlash('nothing here takes that answer'); return }
    void act(verb, `${focusItem!.key}:${x.verb}`, c.method, c.params, c.ask).then((ok) => { if (ok) setNote(null) })
  }
  /** Move within the canvas: along a column (`j`/`k`) or across (`h`/`l`), the queue left of the first column. */
  const move = (dx: number, dy: number) => {
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

  const handlers: Record<Verb, (key: string) => void> = {
    down: () => move(0, 1),
    up: () => move(0, -1),
    left: () => move(-1, 0),
    right: () => move(1, 0),
    follow: () => { if (focusRow?.sessionId) nav(`/session/${focusRow.sessionId}?call=${focusRow.id}`) },
    inspect: () => { if (item) setOpen(open === item.key ? null : item.key); else if (iFold === 'folded') setIFold('open') },
    open: () => { const sid = focusRow?.sessionId ?? item?.sessionId; if (sid) nav(`/session/${sid}`) },
    back: () => { if (note) setNote(null); else if (open) setOpen(null); else if (a.sel) pick(null); else set({ f: {}, q: '' }) },
    next: () => pick(step(queue, item?.key ?? null, 1)),
    prev: () => pick(step(queue, item?.key ?? null, -1)),
    approve: () => answer('approve', { verb: 'approve' }),
    approve_trust: () => answer('approve_trust', { verb: 'approve_trust' }),
    decline: () => { if (focusItem) { pick(focusItem.key); setNote({ key: focusItem.key, text: '' }) } },
    choice: (key) => answer('choice', { verb: 'choice', n: Number(key) }),
    grant: () => answer('grant', { verb: 'grant' }),
    retry: () => answer('retry', { verb: 'retry' }),
    dismiss: () => answer('dismiss', { verb: 'dismiss' }),
    pause: () => { if (focusRow?.kind === 'task') void act('pause', focusRow.id, focusRow.paused ? 'task.resume' : 'task.pause', { task: focusRow.id }) },
    stop: () => twice('stop', () => { if (exe) void act('stop', `${exe}:stop`, 'execution.stop', { execution_id: exe }) }, 's again stops the running turn'),
    stop_all: () => { if (exe) void act('stop_all', `${exe}:all`, 'execution.stop', { execution_id: exe, tasks: true }, 'Stop its turn, and cancel every task under it?') },
    cancel: () => twice('cancel', () => { if (focusRow?.kind === 'task') void act('cancel', `${focusRow.id}:stop`, 'task.cancel', { task: focusRow.id }) }, 'c again cancels the task'),
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
    fold_dock: () => setFlash('\\ is kept for the dock, which a later row brings'),
    keys: () => setKeysOpen((v) => !v),
    filter_needs: () => bucket('needs'),
    filter_working: () => bucket('working'),
    filter_paused: () => bucket('paused'),
    filter_done: () => bucket('done'),
    filter_all: () => set({ f: {}, q: '' }),
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
    return c
  }, [focusItem, focusRow, slug, living])

  // One handler for the page, ahead of the shell's: a `g` sequence the page claims is the page's; one it does not
  // (`g f`) goes on to the shell. Keys never fire while typing.
  const live = useRef({ handlers, ctxs, focusItem, keysOpen })
  useLayoutEffect(() => { live.current = { handlers, ctxs, focusItem, keysOpen } })
  useEffect(() => {
    let g = 0
    const k = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if (typingIn(e.target as HTMLElement | null)) return
      const { handlers: h, ctxs: c, focusItem: fi, keysOpen: ko } = live.current
      if (ko && e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); h.keys('?'); return }
      let name = keyName(e)
      if (g && Date.now() - g < G_MS && SEQUENCES.includes(`g ${name}`)) name = `g ${name}`
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
  const choices = useMemo(() => ({ ...chipChoices(rows), project: records.filter((t) => t.kind === 'project').map((t) => t.id) }), [rows, records])
  const n = a.lens === 'threads' ? threadList.length : vis.rows.length
  const of = a.lens === 'threads' ? grouped.threads.length : vis.of

  const queuePanel = (
    <Queue items={queue} cursor={item?.key ?? null} onCursor={(k) => pick(k)} open={open} note={note} onNote={setNote} past={past} now={now}
      coming={world ? [] : comingUp(Object.values(board.views), tick)} arrivedKeys={fresh} />
  )
  const canvas = (
    <div className="h-full min-h-0">
      {a.lens === 'board' && <BoardLens cols={cols} older={older} onMore={() => setOlder((x) => x + DONE_PAGE)} focus={row?.id ?? null} onFocus={pick} confirms={confirms} now={now} />}
      {a.lens === 'threads' && <ThreadsLens views={views} groups={groups} ready={ready || !!world} error={error} now={now} past={past} focus={slug} onFocus={pick}
        matches={(t) => threadList.includes(t)} />}
      {a.lens === 'timeline' && (
        <div className="h-full overflow-y-auto pr-1">
          {world && <p className="mb-2 text-[11.5px] text-wait">The agenda shows the present: its marks are the daemon’s now, not the log’s moment.</p>}
          <Agenda />
        </div>
      )}
      {a.lens === 'graph' && <TaskGraphView records={records} past={past ?? undefined} shown />}
    </div>
  )
  return (
    <div ref={frame} className="flex h-full min-h-[560px] flex-col gap-2.5" data-testid="ops-page">
      {world && (
        <div className="flex items-center gap-2 rounded-lg bg-wait/[0.06] px-3 py-2 text-[12px] text-wait ring-1 ring-wait/30">
          <History size={14} /> As of {stamp(world.t)}, folded from the ledger. Acting needs the present: return to LIVE in the ship&rsquo;s log below.
        </div>
      )}
      <CommandBar counts={counts} f={a.f} onFilter={filter} lens={a.lens} onLens={lens} choices={choices} q={a.q} onQ={(q) => set({ q })} n={n} of={of}
        onNew={() => handlers.new_task('N')} searchRef={searchRef} />
      {newTask && <NewTask home={newTask.home} onClose={() => setNewTask(null)} onStarted={(id) => { setNewTask(null); set({ lens: 'board', sel: id }) }} />}
      <div className="relative flex min-h-0 flex-1 gap-2.5">
        {reg.queue === 'side' && (
          <aside aria-label="Needs you" className="flex w-[300px] shrink-0 flex-col overflow-hidden rounded-xl border border-line bg-hull/60">
            <RegionHead title="Needs you" count={queue.length} glow={queue.length > 0} onFold={() => setQFold('folded')} fold="[" />
            {queuePanel}
          </aside>
        )}
        {reg.queue === 'folded' && <Tab side="left" label={`Needs you · ${queue.length}`} onClick={() => setQFold('open')} glow={queue.length > 0} />}
        {reg.queue === 'page' && <section aria-label="Needs you" className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-hull/60"><RegionHead title="Needs you" count={queue.length} glow={queue.length > 0} />{queuePanel}</section>}
        {reg.canvas && <main aria-label={`${a.lens} lens`} className="min-h-0 min-w-0 flex-1">{canvas}</main>}
        {reg.inspector === 'side' && (
          <aside aria-label="Inspector" className="flex w-[360px] shrink-0 flex-col overflow-hidden rounded-xl border border-line bg-hull/60">
            <RegionHead title="Inspector" onFold={() => setIFold('folded')} fold="]" />
            <div className="min-h-0 flex-1 overflow-y-auto"><Inspector sel={selection} confirms={confirms} past={past} now={now} /></div>
          </aside>
        )}
        {reg.inspector === 'over' && (
          <aside aria-label="Inspector" className="absolute inset-y-0 right-0 z-20 flex w-[min(380px,92%)] flex-col overflow-hidden rounded-xl border border-line-strong bg-hull/95 shadow-2xl backdrop-blur">
            <RegionHead title="Inspector" onFold={() => pick(null)} fold="esc" />
            <div className="min-h-0 flex-1 overflow-y-auto"><Inspector sel={selection} confirms={confirms} past={past} now={now} /></div>
          </aside>
        )}
        {reg.inspector === 'folded' && <Tab side="right" label="Inspector" onClick={() => setIFold('open')} />}
      </div>
      {flash && <div role="status" className="pointer-events-none fixed bottom-24 left-1/2 z-30 -translate-x-1/2 rounded-lg bg-hull/95 px-3 py-1.5 text-[12px] text-ink ring-1 ring-line-strong shadow-xl">{flash}</div>}
      {keysOpen && <KeysOverlay onClose={() => setKeysOpen(false)} />}
    </div>
  )
}

function RegionHead({ title, count, glow, onFold, fold }: { title: 'Needs you' | 'Inspector'; count?: number; glow?: boolean; onFold?: () => void; fold?: string }) {
  return (
    <header className="flex items-center gap-2 border-b border-line px-3 py-2">
      {title === 'Needs you' && <Inbox size={13} className={glow ? 'text-wait' : 'text-ink-faint'} />}
      <h2 className="font-display text-[11.5px] font-bold uppercase tracking-[0.08em] text-gold">{title}</h2>
      {count !== undefined && <span className={cn('num rounded-full px-1.5 text-[11px]', glow ? 'bg-wait/15 text-wait' : 'text-ink-faint')}>{count}</span>}
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
