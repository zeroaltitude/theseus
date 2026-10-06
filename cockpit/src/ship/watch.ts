// The watch (theseus-hnof): five questions the owner asks of the fleet, answered at a glance on the Ship's right, each
// on a brass plate: what is working now, what waits for you, what is slow, what today cost, and what went wrong. For
// each: the number, what it counts, up to three lines (newest or worst first), and what its overlay lights on the
// chart. This is the pure half; `Watch.tsx` draws it.
//
// In: the Ship's model, the calls (action.list, or the fold's at the time machine's moment), the questions
// (confirm.list, or the fold's), the shared ledger history's rows, the moment, and the local day's start. A row after
// the moment is never read, so the time machine's watch shows its moment, never today. Pure: `npm test` runs it.
import type { ActionInfo, ConfirmRequest, LedgerEntry } from '@protocol'
import type { Light, ShipModel, Vessel } from './model.ts'
import { ago, ms, short, usd } from '../lib/figures.ts'

export const HOUR_MS = 3_600_000
export const DAY_MS = 24 * HOUR_MS
/** The spend's pace: its dollars over the 15 minutes before the moment, as an hourly rate (the money river's). */
export const PACE_MS = 15 * 60_000
/** The lines a plate shows; the rest are counted. */
export const LINES = 3
/** A row's time can trail its place in the ledger a little: the walk back reads this far past its windows. */
const SKEW_MS = 5 * 60_000

/** What "went wrong" counts, and what its ledger link filters by. */
export const WRONG_KINDS = ['action.failed', 'turn.failed', 'budget.asked', 'execution.budget_exhausted'] as const

export type WatchKey = 'working' | 'waiting' | 'slow' | 'spent' | 'wrong'
export const WATCH_KEYS: readonly WatchKey[] = ['working', 'waiting', 'slow', 'spent', 'wrong']

/** A state, drawn as a colour, and always with a word or an icon beside it. */
export type Tone = 'live' | 'wait' | 'fault' | 'ok' | 'money' | 'idle'

/** Where an item points: a session, or a node (a call, a message) in it. */
export interface WatchTarget { session?: string; node?: string }

/** What a plate's overlay lights on the chart: its vessels (session ids) and its lights (node ids). */
export interface WatchSet { vessels: string[]; lights: string[] }

export interface WatchLine {
  /** Stable while the thing it names lasts. */
  id: string
  /** What it is, a short word before the text: a tool's name, "turn", "budget". */
  tag: string
  text: string
  /** A second, quieter row: a question's session, its wait, and its time left. */
  detail?: string
  /** The figure on the right: an elapsed time, dollars, how long ago. */
  figure?: string
  /** A flag, as a word: "L1" or "L0" for a job (the sandbox or the host), "expired" for a question. */
  flag?: string
  tone: Tone
  /** All of it in words, for the tooltip. */
  title: string
  /** Where the line flies; none for a line that is only a figure. */
  to?: WatchTarget
}

export interface Plate {
  key: WatchKey
  /** The question: the plate's engraved title. */
  question: string
  /** The big number, as words ("3", "$0.42", "12m"); "…" while its data is still being read. */
  value: string
  /** The word beside the number. */
  unit: string
  /** What the number counts, in one line. */
  caption: string
  tone: Tone
  /** Changes when the number does, never with the clock alone: the number glows once when it changes. */
  pulse: string
  lines: WatchLine[]
  /** Lines past those shown. */
  more: number
  focus: WatchSet
  /** The data page that explains it. */
  link: { to: string; label: string }
}

export interface Working extends Plate { turns: number; jobs: number; queued: number }
export interface Waiting extends Plate { approvals: number; budgets: number; others: number }
export interface Slow extends Plate {
  /** The longest of what runs now, in ms; null when nothing runs. */
  longestMs: number | null
  /** The turns that ended in the hour before the moment: how many, the slowest, and the median, in ms. */
  ended: number
  slowestMs: number | null
  medianMs: number | null
}
export interface Spent extends Plate {
  usd: number
  calls: number
  /** Dollars by hour of the local day (0 is the hour after midnight), and the moment's hour. */
  hours: number[]
  hour: number
  /** Dollars an hour, over the 15 minutes before the moment. */
  pace: number
  top: { session: string; usd: number } | null
}
export interface Wrong extends Plate { count: number; calls: number; turns: number; budgets: number }

export interface WatchPlates { working: Working; waiting: Waiting; slow: Slow; spent: Spent; wrong: Wrong }

export interface WatchInput {
  model: ShipModel | null
  /** The calls (action.list, or the fold's); undefined until read. */
  actions?: readonly ActionInfo[]
  /** The questions waiting (confirm.list, or the fold's); undefined until read. */
  confirms?: readonly ConfirmRequest[]
  /** The ledger history's rows, oldest first. */
  rows: readonly LedgerEntry[]
  /** The history's walk has reached the ledger's end: the day's sums are whole. */
  rowsReady: boolean
  /** The moment: now, or the time machine's. */
  now: number
  /** The local midnight that starts the moment's day. */
  dayStart: number
}

// ---------------------------------------------------------------- words

/** A span of time as the plates say it: seconds under a minute, then minutes, hours and minutes, days and hours. */
export function span(t: number): string {
  const s = Math.max(0, Math.floor(t / 1000))
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, '0')}m`
  return `${Math.floor(h / 24)}d ${h % 24}h`
}

/** Dollars, with a tiny limit kept legible ($0.00001, not $0.0000). */
export function dollars(n: number): string {
  return n !== 0 && Math.abs(n) < 0.0001 ? `$${Number(n.toPrecision(2))}` : usd(n)
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

/** A list in words: "a", "a and b", "a, b and c". */
const andList = (xs: string[]) => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} and ${xs[xs.length - 1]}`)

/** Long absolute paths (four parts or more) cut to their last two: `/tmp/x/projects/harbour/log.md` → `…/harbour/log.md`. */
export function shortPaths(s: string): string {
  return s.replace(/(?<![\w:/.~-])(?:\/[^\s/'"`:;,()]+){4,}\/?/g, (p) => `…/${p.split('/').filter(Boolean).slice(-2).join('/')}`)
}

const SHELL = /^(?:\S*\/)?(?:ba|z|da|k)?sh$/

/** A job's command line in brief: a shell's `-c` dropped and long paths cut. */
export function argvWords(argv: readonly string[]): string {
  const shell = argv.length >= 3 && SHELL.test(argv[0]) && /^-\w*c$/.test(argv[1])
  return shortPaths((shell ? argv.slice(2) : argv).join(' ')).trim()
}

/** A call's command from its plan's summary (``run `sh -c …` in /dir``), when no job row says its argv. */
function commandOf(preview: string): string {
  const m = /`([^`]+)`/.exec(preview)
  return shortPaths((m ? m[1] : preview).replace(/^(?:\S*\/)?(?:ba|z|da|k)?sh -\w*c /, '')).trim()
}

const oneLine = (s: string) => s.replace(/\s+/g, ' ').trim()

/** A result's words without its leading notes (`[exit code 3]`, `[ran in L1, …]`), and its exit code apart. */
export function resultWords(preview: string): { words: string; exit?: string } {
  let s = oneLine(preview)
  let exit: string | undefined
  for (let m = /^\[([^\]]*)\]\s*/.exec(s); m; m = /^\[([^\]]*)\]\s*/.exec(s)) {
    const code = /^exit code (-?\d+)$/.exec(m[1])
    if (code) exit = code[1]
    s = s.slice(m[0].length)
  }
  return { words: s || oneLine(preview), exit }
}

// ---------------------------------------------------------------- reads

type D = Record<string, unknown>
const str = (v: unknown): string | undefined => (typeof v === 'string' && v ? v : undefined)
const num = (v: unknown): number | undefined => (typeof v === 'number' && Number.isFinite(v) ? v : undefined)

/** The model's lookups: a session's vessel and words, and a call's lights by correlation id. */
interface Look {
  vessel: (id: string | null | undefined) => Vessel | undefined
  title: (id: string | null | undefined) => string
  call: Map<string, Light>
  result: Map<string, Light>
}

function lookOf(model: ShipModel | null): Look {
  const call = new Map<string, Light>()
  const result = new Map<string, Light>()
  for (const l of model?.lights ?? []) {
    if (!l.correlationId) continue
    if (l.kind === 'call') call.set(l.correlationId, l)
    else if (l.kind === 'result') result.set(l.correlationId, l)
  }
  const vessel = (id: string | null | undefined): Vessel | undefined => {
    if (!model || !id) return undefined
    const i = model.byId.get(id)
    return i === undefined ? undefined : model.vessels[i]
  }
  return { vessel, call, result, title: (id) => vessel(id)?.title ?? (id ? short(id) : 'no session') }
}

/** The lights of some turns, oldest first. */
function lightsOfTurns(model: ShipModel | null, turns: ReadonlySet<string>): Map<string, Light[]> {
  const m = new Map<string, Light[]>()
  if (!turns.size) return m
  for (const l of model?.lights ?? []) {
    if (!l.turnId || !turns.has(l.turnId)) continue
    const a = m.get(l.turnId)
    if (a) a.push(l)
    else m.set(l.turnId, [l])
  }
  for (const a of m.values()) a.sort((x, y) => x.at - y.at)
  return m
}

interface Failure {
  kind: 'call' | 'turn' | 'budget'
  at: number
  session: string | null
  turn?: string
  cid?: string
  words: string
  producer?: string
}

/** What the rows say, read once backwards from the moment, down to the longest window. */
interface Scan {
  spent: number
  calls: number
  hours: number[]
  bySession: Map<string, number>
  /** Today's model calls' nodes (a provider.call row's `node_id`). */
  spentNodes: Set<string>
  last15: number
  /** The turns that ended in the hour before the moment. */
  ended: { ms: number; turn: string; session: string | null }[]
  /** Each session's newest turn.started by the moment, and whether it is still open then (no end by the moment). */
  newest: Map<string, { turn: string; at: number; open: boolean }>
  /** The last day's failures, newest first. */
  failures: Failure[]
  /** A call's tool (action.planned), its job's argv (tool.job_started), and the jobs that ran in L1. */
  tools: Map<string, string>
  argv: Map<string, string[]>
  l1: Set<string>
}

function budgetWords(d: D): string {
  const need = num(d.needed_usd)
  const limit = num(d.limit_usd)
  if (need === undefined || limit === undefined) return 'it reached its spend limit'
  return d.exceeds_limit === true
    ? `a call needed ${dollars(need)}, more than its whole ${dollars(limit)} limit`
    : `a call needed ${dollars(need)}: its ${dollars(limit)} limit is reached`
}

function scan(rows: readonly LedgerEntry[], now: number, dayStart: number): Scan {
  const s: Scan = {
    spent: 0, calls: 0, hours: new Array<number>(24).fill(0), bySession: new Map(), spentNodes: new Set(), last15: 0,
    ended: [], newest: new Map(), failures: [], tools: new Map(), argv: new Map(), l1: new Set(),
  }
  const from = Math.min(dayStart, now - DAY_MS) - SKEW_MS
  // Going back, a turn's end comes before its start: a start whose end is not seen by then is open at the moment.
  const done = new Set<string>()
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    const at = r.at_unix_ms
    if (at > now) continue
    if (at < from) break
    const d = (r.data ?? {}) as D
    const sid = r.session_id
    const lastDay = at > now - DAY_MS
    switch (r.kind) {
      case 'provider.call': {
        if (at < dayStart) break
        // Spend means what it means in Money and Economics: each model call's recorded cost.
        const cost = Number(d.cost_usd ?? 0) || 0
        s.spent += cost
        s.calls++
        s.hours[Math.min(23, Math.floor((at - dayStart) / HOUR_MS))] += cost
        if (at > now - PACE_MS) s.last15 += cost
        if (sid) s.bySession.set(sid, (s.bySession.get(sid) ?? 0) + cost)
        const node = str(d.node_id)
        if (node) s.spentNodes.add(node)
        break
      }
      case 'turn.ended': {
        const t = r.turn_id
        if (t) done.add(t)
        const e = num(d.elapsed_ms)
        if (t && e !== undefined && at > now - HOUR_MS) s.ended.push({ ms: e, turn: t, session: sid })
        break
      }
      case 'turn.failed':
        if (r.turn_id) done.add(r.turn_id)
        if (lastDay) s.failures.push({ kind: 'turn', at, session: sid, turn: r.turn_id ?? undefined, words: str(d.reason) ?? 'the turn failed' })
        break
      case 'turn.started':
        if (sid && r.turn_id && !s.newest.has(sid)) s.newest.set(sid, { turn: r.turn_id, at, open: !done.has(r.turn_id) })
        break
      case 'action.failed': {
        const cid = str(d.correlation_id)
        if (lastDay && cid) s.failures.push({ kind: 'call', at, session: sid, cid, words: '', producer: str(d.producer) })
        break
      }
      case 'action.planned': {
        const cid = str(d.correlation_id)
        const tool = str(d.tool)
        if (cid && tool) s.tools.set(cid, tool)
        break
      }
      case 'tool.job_started': {
        const cid = str(d.correlation_id)
        if (!cid) break
        if (Array.isArray(d.argv) && d.argv.every((a) => typeof a === 'string')) s.argv.set(cid, d.argv as string[])
        if (d.class === 'l1') s.l1.add(cid)
        break
      }
      case 'sandbox.started': {
        const cid = str(d.correlation_id)
        if (cid && d.class === 'l1') s.l1.add(cid)
        break
      }
      case 'budget.asked':
        if (lastDay) s.failures.push({ kind: 'budget', at, session: sid, words: budgetWords(d) })
        break
      case 'execution.budget_exhausted':
        if (lastDay) s.failures.push({ kind: 'budget', at, session: sid, words: str(d.reason) ?? 'its budget ran out' })
        break
    }
  }
  return s
}

/** A set's ids, once each and in order, so two sets compare by their words. */
const ids = (xs: Iterable<string | undefined>) => [...new Set([...xs].filter((x): x is string => !!x))].sort()

const setOf = (vessels: Iterable<string | undefined | null>, lights: Iterable<string | undefined>): WatchSet => ({
  vessels: ids([...vessels].map((v) => v ?? undefined)),
  lights: ids(lights),
})

/** The median of some numbers (the mean of the middle two for an even count). */
export function median(xs: readonly number[]): number | null {
  if (!xs.length) return null
  const a = [...xs].sort((x, y) => x - y)
  const m = a.length >> 1
  return a.length % 2 ? a[m] : (a[m - 1] + a[m]) / 2
}

// ---------------------------------------------------------------- what runs

/** A turn or a job running at the moment. */
interface Run {
  id: string
  kind: 'turn' | 'job'
  session: string | null
  /** When it began; null when nothing says (a turn whose start row is not read). */
  since: number | null
  turn?: string
  cid?: string
  tool?: string
  /** A job's command in brief, and in full. */
  words?: string
  full?: string
  l1?: boolean
  call?: Light
}

function runs(input: WatchInput, look: Look, sc: Scan): { turns: Run[]; jobs: Run[]; queued: number } {
  const { model } = input
  const turns: Run[] = []
  let queued = 0
  if (model) {
    // A turn runs while its session's execution does (the daemon's state, or the fold's): its start is the session's
    // newest turn.started, unless that one has ended (then nothing says when it began but its attention).
    for (const v of model.vessels) {
      if (v.state === 'queued') queued++
      if (v.state !== 'running') continue
      const t = sc.newest.get(v.id)
      const open = t?.open ? t : undefined
      turns.push({
        id: `turn:${open?.turn ?? v.id}`, kind: 'turn', session: v.id, turn: open?.turn,
        since: open?.at ?? (v.attention?.level === 'working' ? v.attention.since_ms : null),
      })
    }
  } else {
    for (const [sid, t] of sc.newest) if (t.open) turns.push({ id: `turn:${t.turn}`, kind: 'turn', session: sid, turn: t.turn, since: t.at })
  }
  // A job runs from its dispatch to its settling: a proc.run in flight, or any call the model marks running.
  const jobs: Run[] = []
  const seen = new Set<string>()
  const job = (cid: string, tool: string, session: string | null, since: number) => {
    const call = look.call.get(cid)
    const argv = sc.argv.get(cid)
    seen.add(cid)
    jobs.push({
      id: `job:${cid}`, kind: 'job', session, since, cid, tool, call,
      words: argv ? argvWords(argv) : call ? commandOf(call.preview) : tool,
      full: argv ? argv.join(' ') : call?.preview ?? tool,
      l1: !!call?.l1 || sc.l1.has(cid),
    })
  }
  for (const a of input.actions ?? []) {
    if (!a.dispatched_at_ms || a.settled_at_ms) continue
    if (a.tool !== 'proc.run' && !look.call.get(a.correlation_id)?.running) continue
    job(a.correlation_id, a.tool, a.session_id || null, a.dispatched_at_ms)
  }
  for (const l of model?.lights ?? []) {
    if (l.kind === 'call' && l.running && l.correlationId && !seen.has(l.correlationId)) job(l.correlationId, l.tool ?? 'job', l.sessionId, l.at)
  }
  return { turns, jobs, queued }
}

function runLine(r: Run, look: Look, now: number, turnLights: Map<string, Light[]>): WatchLine {
  const elapsed = r.since === null ? null : now - r.since
  const title = look.title(r.session)
  if (r.kind === 'turn') {
    const ls = r.turn ? turnLights.get(r.turn) : undefined
    return {
      id: r.id, tag: 'turn', text: title, figure: elapsed === null ? '—' : span(elapsed), tone: 'live',
      title: `A turn running in "${title}"${elapsed === null ? '' : ` for ${span(elapsed)}`}.`,
      to: { session: r.session ?? undefined, node: ls?.[ls.length - 1]?.id },
    }
  }
  return {
    id: r.id, tag: r.tool ?? 'job', text: r.words || (r.tool ?? 'job'), flag: r.l1 ? 'L1' : 'L0',
    figure: elapsed === null ? '—' : span(elapsed), tone: 'live',
    title: `${r.tool}: ${oneLine(r.full ?? '')} · ${r.l1 ? 'in the sandbox (L1)' : 'on the host (L0)'} · running ${elapsed === null ? '' : `for ${span(elapsed)} `}in "${title}".`,
    to: { session: r.session ?? undefined, node: r.call?.id },
  }
}

/** A run's lights on the chart: a job's call and result; a turn's every light. */
function runLights(r: Run, look: Look, turnLights: Map<string, Light[]>): string[] {
  if (r.kind === 'job') return [r.call?.id, r.cid ? look.result.get(r.cid)?.id : undefined].filter((x): x is string => !!x)
  return (r.turn ? turnLights.get(r.turn) ?? [] : []).map((l) => l.id)
}

// ---------------------------------------------------------------- the plates

export function watchOf(input: WatchInput): WatchPlates {
  const { model, now, dayStart } = input
  const look = lookOf(model)
  const sc = scan(input.rows, now, dayStart)
  const { turns, jobs, queued } = runs(input, look, sc)
  const running = [...turns, ...jobs]

  // The turns whose lights some plate needs: those running, the hour's slowest, and those that failed.
  const ended = sc.ended
  const slowest = ended.reduce<Scan['ended'][number] | null>((a, e) => (!a || e.ms > a.ms ? e : a), null)
  const failures = sc.failures
  const turnLights = lightsOfTurns(model, new Set(ids([
    ...running.map((r) => r.turn), slowest?.turn, ...failures.map((f) => (f.kind === 'turn' ? f.turn : undefined)),
  ])))

  return {
    working: working(input, look, turns, jobs, queued, turnLights),
    waiting: waiting(input, look),
    slow: slow(input, look, running, slowest, turnLights, sc),
    spent: spent(input, look, sc),
    wrong: wrong(input, look, sc, turnLights),
  }
}

/** The plates in their order down the column. */
export const platesOf = (w: WatchPlates): Plate[] => WATCH_KEYS.map((k) => w[k])

function working(input: WatchInput, look: Look, turns: Run[], jobs: Run[], queued: number, turnLights: Map<string, Light[]>): Working {
  const { now } = input
  const ready = input.model !== null && input.actions !== undefined
  const all = [...turns, ...jobs].sort((a, b) => (b.since ?? -Infinity) - (a.since ?? -Infinity))
  const n = all.length
  const what = andList([turns.length ? count(turns.length, 'turn', 'turns') : '', jobs.length ? count(jobs.length, 'job', 'jobs') : ''].filter(Boolean))
  return {
    key: 'working', question: 'Working now', turns: turns.length, jobs: jobs.length, queued,
    value: ready ? String(n) : '…', unit: n ? 'running' : 'idle',
    caption: !ready ? 'reading the fleet…' : `${n ? `running now: ${what}` : 'nothing is running now'}${queued ? `; ${queued} queued` : ''}`,
    tone: n ? 'live' : 'idle', pulse: `${turns.length}/${jobs.length}`,
    lines: all.slice(0, LINES).map((r) => runLine(r, look, now, turnLights)), more: Math.max(0, n - LINES),
    focus: setOf(all.map((r) => r.session), all.flatMap((r) => runLights(r, look, turnLights))),
    link: { to: '/actions', label: 'Actions' },
  }
}

function waiting(input: WatchInput, look: Look): Waiting {
  const { now, model } = input
  const qs = [...(input.confirms ?? [])]
  const asking = new Set(qs.map((c) => c.session_id))
  // Others that need you: a session the daemon says needs you (blocked, failed, at its limit) with no question listed.
  const others = (model?.vessels ?? []).filter((v) => v.attention?.level === 'needs_you' && !asking.has(v.id))
  const approvals = qs.filter((c) => !c.budget).length
  const budgets = qs.length - approvals
  // Worst first: the soonest to expire, then the budget questions (they hold until answered), the longest waiting first.
  qs.sort((a, b) => (a.expires_at_ms || Infinity) - (b.expires_at_ms || Infinity) || a.requested_at_ms - b.requested_at_ms)
  const lines: WatchLine[] = qs.map((c) => {
    const call = look.call.get(c.correlation_id)
    const who = c.task?.title ?? look.title(c.session_id)
    const waited = `waited ${span(now - c.requested_at_ms)}`
    const left = c.expires_at_ms > 0 ? (c.expires_at_ms > now ? `${span(c.expires_at_ms - now)} left` : 'expired') : 'holds until answered'
    const expired = c.expires_at_ms > 0 && c.expires_at_ms <= now
    if (c.budget) {
      const b = c.budget
      return {
        id: `ask:${c.correlation_id}`, tag: 'budget', text: `needs ${dollars(b.needed_usd)} · limit ${dollars(b.limit_usd)}`,
        detail: `${waited} · ${left} · ${who}`, tone: 'wait',
        title: `A budget question from "${who}": ${oneLine(c.reason)} It has waited ${span(now - c.requested_at_ms)} and holds until you answer.`,
        to: { session: c.session_id, node: call?.id },
      }
    }
    // What it would do: the call's plan in words (its light), else the policy's reason up to its rule.
    const plan = call?.preview || c.reason.split(`: ${c.tool} `)[0] || c.resource || ''
    return {
      id: `ask:${c.correlation_id}`, tag: c.tool, text: shortPaths(oneLine(plan)), detail: `${waited} · ${left} · ${who}`,
      flag: expired ? 'expired' : undefined, tone: expired ? 'fault' : 'wait',
      title: `${c.tool} waits for your answer in "${who}": ${oneLine(c.reason)}${c.floor ? ' (the floor asks: it touches the harness itself)' : ''}. It has waited ${span(now - c.requested_at_ms)}; ${c.expires_at_ms > 0 ? (expired ? 'it has expired' : `it expires in ${span(c.expires_at_ms - now)}`) : 'it holds until answered'}.`,
      to: { session: c.session_id, node: call?.id },
    }
  })
  for (const v of others) {
    const since = v.attention?.since_ms
    lines.push({
      id: `needs:${v.id}`, tag: 'needs you', text: v.attention?.label ?? 'needs you', detail: `${since ? `since ${span(now - since)} · ` : ''}${v.title}`,
      tone: 'wait', title: `"${v.title}" needs you: ${v.attention?.label ?? ''}.`, to: { session: v.id },
    })
  }
  const n = qs.length + others.length
  const ready = input.confirms !== undefined
  const parts = [
    approvals ? count(approvals, 'approval', 'approvals') : '', budgets ? count(budgets, 'budget question', 'budget questions') : '',
    others.length ? count(others.length, 'session that needs you', 'sessions that need you') : '',
  ].filter(Boolean)
  return {
    key: 'waiting', question: 'Waiting for you', approvals, budgets, others: others.length,
    value: ready ? String(n) : '…', unit: n ? 'waiting' : 'none',
    caption: !ready ? 'reading the questions…' : n ? `waiting for your answer: ${andList(parts)}` : 'nothing waits for your answer',
    tone: n ? 'wait' : 'idle', pulse: String(n),
    lines: lines.slice(0, LINES), more: Math.max(0, lines.length - LINES),
    focus: setOf([...qs.map((c) => c.session_id), ...others.map((v) => v.id)], qs.map((c) => look.call.get(c.correlation_id)?.id)),
    link: { to: '/actions', label: 'Actions' },
  }
}

function slow(input: WatchInput, look: Look, running: Run[], slowest: Scan['ended'][number] | null, turnLights: Map<string, Light[]>, sc: Scan): Slow {
  const { now } = input
  const timed = running.filter((r) => r.since !== null).sort((a, b) => a.since! - b.since!)
  const longest = timed[0]
  const longestMs = longest ? now - longest.since! : null
  const med = median(sc.ended.map((e) => e.ms))
  const shown = timed.slice(0, slowest ? LINES - 1 : LINES)
  const lines = shown.map((r) => runLine(r, look, now, turnLights))
  if (slowest) {
    const title = look.title(slowest.session)
    const ls = turnLights.get(slowest.turn)
    lines.push({
      id: `slowest:${slowest.turn}`, tag: 'turn', text: `slowest this hour: ${title}`, figure: ms(slowest.ms),
      detail: `the median of ${count(sc.ended.length, 'turn', 'turns')} this hour: ${ms(med)}`, tone: 'idle',
      title: `The slowest of the ${count(sc.ended.length, 'turn', 'turns')} that ended in the hour before the moment took ${ms(slowest.ms)}, in "${title}"; their median took ${ms(med)}.`,
      to: { session: slowest.session ?? undefined, node: ls?.[ls.length - 1]?.id },
    })
  }
  const n = timed.length
  return {
    key: 'slow', question: 'Slow', longestMs, ended: sc.ended.length, slowestMs: slowest?.ms ?? null, medianMs: med,
    value: input.model === null || input.actions === undefined ? '…' : longestMs === null ? '—' : span(longestMs),
    unit: longestMs === null ? 'nothing running' : 'longest',
    caption: longestMs === null ? 'nothing is running now'
      : n === 1 ? `the one ${longest.kind} running now` : `the longest of ${n} things running now (a ${longest.kind})`,
    tone: longestMs === null ? 'idle' : 'live', pulse: `${longest?.id ?? ''}|${slowest?.turn ?? ''}`,
    lines, more: Math.max(0, n - shown.length),
    focus: setOf(
      [...shown.map((r) => r.session), slowest?.session],
      [...shown.flatMap((r) => runLights(r, look, turnLights)), ...(slowest ? turnLights.get(slowest.turn) ?? [] : []).map((l) => l.id)],
    ),
    link: { to: '/actions', label: 'Actions' },
  }
}

function spent(input: WatchInput, look: Look, sc: Scan): Spent {
  const ready = input.rowsReady
  const hour = Math.max(0, Math.min(23, Math.floor((input.now - input.dayStart) / HOUR_MS)))
  let top: Spent['top'] = null
  for (const [session, v] of sc.bySession) if (v > 0 && (!top || v > top.usd)) top = { session, usd: v }
  const pace = sc.last15 * (HOUR_MS / PACE_MS)
  const lines: WatchLine[] = [{
    id: 'pace', tag: 'pace', text: `${dollars(pace)} an hour`, figure: 'last 15 min', tone: 'idle',
    title: `The pace: ${dollars(sc.last15)} in the 15 minutes before the moment, ${dollars(pace)} an hour.`,
  }]
  if (top) {
    const title = look.title(top.session)
    lines.push({
      id: `top:${top.session}`, tag: 'top', text: title, figure: dollars(top.usd), tone: 'idle',
      title: `The session that spent the most today: "${title}", ${dollars(top.usd)} of ${dollars(sc.spent)}.`, to: { session: top.session },
    })
  }
  // The model calls' lights, those the model draws (it reads the newest nodes first).
  const drawn = (id: string) => !input.model || input.model.lightById.has(id)
  return {
    key: 'spent', question: 'Spent today', usd: sc.spent, calls: sc.calls, hours: sc.hours, hour, pace, top,
    value: ready ? dollars(sc.spent) : '…', unit: 'today',
    caption: !ready ? 'reading the ledger…' : sc.calls ? `dollars since local midnight, on ${count(sc.calls, 'model call', 'model calls')}` : 'no model call since local midnight',
    tone: sc.spent > 0 ? 'money' : 'idle', pulse: sc.spent.toFixed(6),
    lines: ready ? lines : [], more: 0,
    focus: setOf(sc.bySession.keys(), [...sc.spentNodes].filter(drawn)),
    link: { to: '/money', label: 'Money' },
  }
}

function wrong(input: WatchInput, look: Look, sc: Scan, turnLights: Map<string, Light[]>): Wrong {
  const { now } = input
  const ready = input.rowsReady
  const toolOf = (cid: string) => sc.tools.get(cid) ?? input.actions?.find((a) => a.correlation_id === cid)?.tool ?? look.call.get(cid)?.tool
  // A model call that failed is its turn's failure, or a retry that went on: the plate counts the tools'.
  const failures = sc.failures.filter((f) => f.kind !== 'call' || !(toolOf(f.cid!)?.startsWith('provider.') || f.producer?.startsWith('provider:')))
  const calls = failures.filter((f) => f.kind === 'call').length
  const turns = failures.filter((f) => f.kind === 'turn').length
  const budgets = failures.length - calls - turns
  const lines: WatchLine[] = failures.slice(0, LINES).map((f) => {
    const title = look.title(f.session)
    const when = ago(f.at, now)
    if (f.kind === 'call') {
      const tool = toolOf(f.cid!) ?? 'a call'
      const res = look.result.get(f.cid!)
      const { words: what, exit } = resultWords(res?.preview || (f.producer ? `failed (${f.producer})` : 'failed'))
      return {
        id: `fail:${f.cid}:${f.at}`, tag: tool, text: shortPaths(what), figure: when, tone: 'fault', flag: exit === undefined ? undefined : `exit ${exit}`,
        title: `${tool} failed in "${title}", ${when}: ${what}`, to: { session: f.session ?? undefined, node: res?.id ?? look.call.get(f.cid!)?.id },
      }
    }
    if (f.kind === 'turn') {
      const ls = f.turn ? turnLights.get(f.turn) : undefined
      return {
        id: `turnfail:${f.turn ?? ''}:${f.at}`, tag: 'turn', text: oneLine(f.words), figure: when, tone: 'fault',
        title: `A turn failed in "${title}", ${when}: ${oneLine(f.words)}`, to: { session: f.session ?? undefined, node: ls?.[ls.length - 1]?.id },
      }
    }
    return {
      id: `budget:${f.session}:${f.at}`, tag: 'budget', text: oneLine(f.words), figure: when, tone: 'fault',
      title: `"${title}" reached its spend limit, ${when}: ${oneLine(f.words)}`, to: { session: f.session ?? undefined },
    }
  })
  const n = failures.length
  const parts = [
    calls ? count(calls, 'call', 'calls') : '', turns ? count(turns, 'turn', 'turns') : '', budgets ? count(budgets, 'spend limit', 'spend limits') : '',
  ].filter(Boolean)
  return {
    key: 'wrong', question: 'Went wrong', count: n, calls, turns, budgets,
    value: ready ? String(n) : '…', unit: n ? 'in a day' : 'all clear',
    caption: !ready ? 'reading the ledger…' : n ? `failures in the last 24 hours: ${andList(parts)}` : 'nothing failed in the last 24 hours',
    tone: n ? 'fault' : 'ok', pulse: String(n),
    lines: ready ? lines : [], more: ready ? Math.max(0, n - LINES) : 0,
    focus: setOf(
      failures.map((f) => f.session),
      failures.flatMap((f) => f.kind === 'call'
        ? [look.call.get(f.cid!)?.id, look.result.get(f.cid!)?.id]
        : f.kind === 'turn' && f.turn ? (turnLights.get(f.turn) ?? []).map((l) => l.id) : []),
    ),
    link: { to: `/ledger?kind=${WRONG_KINDS.join(',')}&from=${now - DAY_MS}&to=${now}`, label: 'the ledger' },
  }
}
