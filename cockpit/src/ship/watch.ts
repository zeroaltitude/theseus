// The watch (theseus-hnof): the questions the owner asks of the fleet, answered at a glance on the Ship's right, each
// on a brass plate: what is working now, what waits for you, what is slow, what today cost, what went wrong, and (the
// sixth, `since.ts`) what happened since you last looked. For each: the number, what it counts, up to three lines
// (newest or worst first), and what its overlay lights on the chart. This is the pure half; `Watch.tsx` draws it.
//
// In: the Ship's model, the calls (action.list's newest and every one not settled, or the fold's at the time machine's
// moment), the questions (confirm.list, or the fold's), the shared ledger history's rows, the moment, and the local
// day's start and end. A row after the moment is never read, so the time machine's watch shows its moment, never
// today. Pure: `npm test` runs it.
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
export const SKEW_MS = 5 * 60_000

/** Something running past this many times its usual is slow. */
export const SLOW_TIMES = 3
/** Nothing that has run less than this is slow, whatever its usual. */
export const SLOW_FLOOR_MS = 5_000
/** A usual needs this many of its kind, settled in the day before the moment. */
export const USUAL_MIN = 3

/** What "went wrong" counts, and what its ledger link filters by: a call that failed, a job the harness lost, refused,
 *  stopped below the disk's floor or never started, a call whose outcome is unknown, a failed turn, a spend limit
 *  reached, and a crash of the daemon. */
export const WRONG_KINDS = [
  'action.failed', 'action.outcome_unknown', 'job.wrapper_lost', 'job.refused', 'job.stopped_below_floor',
  'job.not_started', 'turn.failed', 'budget.asked', 'execution.budget_exhausted', 'server.crashed',
] as const

/** The plates, in their order down the column; the key that toggles each one's overlay is its place, 1 to 6. */
export type WatchKey = 'working' | 'waiting' | 'slow' | 'spent' | 'wrong' | 'since'
export const WATCH_KEYS: readonly WatchKey[] = ['working', 'waiting', 'slow', 'spent', 'wrong', 'since']
/** The digit that toggles a plate's overlay. */
export const keyOf = (k: WatchKey): string => String(WATCH_KEYS.indexOf(k) + 1)

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
  /** A flag, as a word: "L1" or "L0" for a job (the sandbox or the host), "expired" for a question, "7×" for
   *  something slow, "exit 3", "unknown", "disk" or "not run" for what went wrong. */
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
  /** The question in the compact strip, when the whole one does not fit there; the whole is its tooltip. */
  short?: string
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
export interface Waiting extends Plate {
  approvals: number
  budgets: number
  others: number
  /** Sessions that hold text from outside: their next call that acts waits for your approval (the old gate's second
   *  needle). Said in the caption and the lines, not counted in the number: nothing is asked yet. */
  holds: number
}
export interface Slow extends Plate {
  /** The longest of what runs now, in ms; null when nothing runs. */
  longestMs: number | null
  /** What runs now past `SLOW_TIMES` its usual (and past `SLOW_FLOOR_MS`). */
  slow: number
  /** The most times its usual that anything running now has run; null when nothing running has a usual. */
  worst: number | null
  /** The turns that ended in the hour before the moment: how many, the slowest, and the median, in ms. */
  ended: number
  slowestMs: number | null
  medianMs: number | null
}
export interface Spent extends Plate {
  usd: number
  calls: number
  /** Dollars by hour of the local day (0 is the hour after midnight): 24 bars, or 23 or 25 on a day the clocks
   *  change; and the moment's hour. */
  hours: number[]
  hour: number
  /** Dollars an hour, over the 15 minutes before the moment. */
  pace: number
  top: { session: string; usd: number } | null
}
export interface Wrong extends Plate { count: number; calls: number; turns: number; budgets: number; crashes: number }

export interface WatchPlates { working: Working; waiting: Waiting; slow: Slow; spent: Spent; wrong: Wrong }

export interface WatchInput {
  model: ShipModel | null
  /** The calls (action.list's newest and every one not settled, or the fold's); undefined until read. */
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
  /** The local midnight that ends it: 24 hours after its start, or 23 or 25 on a day the clocks change. */
  dayEnd?: number
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

export const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

/** A list in words: "a", "a and b", "a, b and c". */
export const andList = (xs: string[]) => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} and ${xs[xs.length - 1]}`)

/** How many times its usual: "7×", "2.5×", "120×". */
export const times = (x: number) => `${x >= 10 ? Math.round(x) : Number(x.toFixed(1))}×`

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

/** Words that run another command, and the assignments and options before it: `nice -n 19 timeout 60 cargo build`
 *  runs cargo. */
const WRAPPERS = new Set(['env', 'exec', 'time', 'nohup', 'nice', 'timeout', 'command', 'stdbuf', 'ionice'])
/** A shell's own words, which run no program of their own: `cd harbour && make` runs make. */
const BUILTINS = new Set(['cd', 'export', 'set', 'unset', 'source', '.', 'true', ':', 'pushd', 'popd', 'umask', 'ulimit', 'trap'])
/** A shell's words that only say something, which do none of the job's work: `echo "building"; cargo build` runs cargo
 *  (theseus-cov9). */
const SAYS = new Set(['echo', 'printf'])

/** The program a job runs: its command's first program (a shell's `-c` looked through, its builtins, its `echo` and
 *  `printf`, wrappers, assignments and options passed over): `cargo`, `sleep`, `make`. A job is held against its
 *  program's runs when its own command has too few. */
export function programOf(argv: readonly string[]): string | undefined {
  const shell = argv.length >= 3 && SHELL.test(argv[0]) && /^-\w*c$/.test(argv[1])
  const commands = shell ? argv[2].replace(/[(){}]/g, ' ').split(/;|&&|\|\||\||\n/) : [argv.join(' ')]
  for (const c of commands) {
    const words = c.trim().split(/\s+/).filter(Boolean)
    let i = 0
    while (i < words.length && (words[i].includes('=') || words[i].startsWith('-') || /^\d+[smhd]?$/.test(words[i]) || WRAPPERS.has(words[i].split('/').pop()!))) i++
    const name = (words[i] ?? '').split('/').pop() ?? ''
    if (!name || BUILTINS.has(name) || SAYS.has(name)) continue
    return /^[\w.+-]+$/.test(name) ? name : undefined
  }
  return undefined
}

/** A job's command with its numbers taken out, so `sleep 8` and `sleep 9` are runs of one command, and `cargo build
 *  --release` is not `cargo build`: a job's usual is first its own command's. */
export const commandKey = (argv: readonly string[]) => argvWords(argv).replace(/\d+(?:\.\d+)?/g, 'N').replace(/\s+/g, ' ').trim()

/** A call's command from its plan's summary (``run `sh -c …` in /dir``), when no job row says its argv. */
function commandOf(preview: string): string {
  const m = /`([^`]+)`/.exec(preview)
  return shortPaths((m ? m[1] : preview).replace(/^(?:\S*\/)?(?:ba|z|da|k)?sh -\w*c /, '')).trim()
}

export const oneLine = (s: string) => s.replace(/\s+/g, ' ').trim()

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
const mb = (v: unknown) => (num(v) === undefined ? 'too little' : `${num(v)!.toLocaleString('en-US')} MB`)

/** The model's lookups: a session's vessel and words, and a call's lights by correlation id. */
export interface Look {
  vessel: (id: string | null | undefined) => Vessel | undefined
  title: (id: string | null | undefined) => string
  call: Map<string, Light>
  result: Map<string, Light>
}

/** Each model's lookups, made once: the five plates and the sixth read the same model on every recompute. */
const looks = new WeakMap<ShipModel, Look>()

export function lookOf(model: ShipModel | null): Look {
  const kept = model ? looks.get(model) : undefined
  if (kept) return kept
  const look = lookOfModel(model)
  if (model) looks.set(model, look)
  return look
}

function lookOfModel(model: ShipModel | null): Look {
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
export function lightsOfTurns(model: ShipModel | null, turns: ReadonlySet<string>): Map<string, Light[]> {
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

// ---------------------------------------------------------------- what went wrong

/** Something that went wrong, from its row: a call (and its job), a turn, a spend limit, or the daemon itself. */
export interface Failure {
  kind: 'call' | 'turn' | 'budget' | 'crash'
  /** When: the newest row that says it (a crash's own time). */
  at: number
  session: string | null
  turn?: string
  cid?: string
  /** The row's kind; for a call, the most precise of its rows (`CALL_WHY`). */
  why: string
  /** What happened, in words; empty for a failed call, whose result says it. */
  words: string
  producer?: string
  /** The tool, when the row names it. */
  tool?: string
}

/** A call's rows, the most precise first: a job's own word on why it ended before the call's outcome. */
const CALL_WHY = ['job.wrapper_lost', 'job.stopped_below_floor', 'job.refused', 'job.not_started', 'action.outcome_unknown', 'action.failed']

function budgetWords(d: D): string {
  const need = num(d.needed_usd)
  const limit = num(d.limit_usd)
  if (need === undefined || limit === undefined) return 'it reached its spend limit'
  return d.exceeds_limit === true
    ? `a call needed ${dollars(need)}, more than its whole ${dollars(limit)} limit`
    : `a call needed ${dollars(need)}: its ${dollars(limit)} limit is reached`
}

/** Why the reconciler could not say: `reconciler:overdue_no_evidence` → "overdue, with no evidence". */
function unknownWords(producer: string | undefined): string {
  const why = producer?.startsWith('reconciler:') ? producer.slice('reconciler:'.length).replace(/_/g, ' ').replace(/ no /, ', with no ') : ''
  return `its outcome is unknown${why ? `: ${why}` : ''}`
}

const FAILURE_KINDS: ReadonlySet<string> = new Set(WRONG_KINDS)

/** A row that says something went wrong, as a failure; null for any other row. */
export function failureOf(r: LedgerEntry): Failure | null {
  if (!FAILURE_KINDS.has(r.kind)) return null
  const d = (r.data ?? {}) as D
  const at = r.at_unix_ms
  const session = r.session_id
  const cid = str(d.correlation_id)
  const call = (words: string): Failure | null => (cid ? { kind: 'call', at, session, cid, why: r.kind, words, tool: str(d.tool), producer: str(d.producer) } : null)
  switch (r.kind) {
    case 'action.failed': return call('')
    case 'action.outcome_unknown': return call(unknownWords(str(d.producer)))
    case 'job.wrapper_lost':
      return call(`its job's wrapper was killed${num(d.signal) !== undefined ? ` by signal ${d.signal}` : ''} before it reported: its outcome is unknown`)
    case 'job.refused': return call(`not started: the disk had ${mb(d.free_mb)} free, under its floor of ${mb(d.floor_mb)}`)
    case 'job.stopped_below_floor': return call(`stopped: the disk fell to ${mb(d.free_mb)} free, under its floor of ${mb(d.floor_mb)}`)
    case 'job.not_started': return call(`not started: ${str(d.resolution) ?? 'a stop reached it before its launch'}`)
    case 'turn.failed': return { kind: 'turn', at, session, turn: r.turn_id ?? undefined, why: r.kind, words: str(d.reason) ?? 'the turn failed' }
    case 'budget.asked': return { kind: 'budget', at, session, cid, why: r.kind, words: budgetWords(d) }
    case 'execution.budget_exhausted': return { kind: 'budget', at, session, why: r.kind, words: str(d.reason) ?? 'its budget ran out' }
    case 'server.crashed': {
      const where = [str(d.thread) ? `on thread ${d.thread}` : '', str(d.location) ? `at ${d.location}` : ''].filter(Boolean).join(' ')
      return { kind: 'crash', at: num(d.at_unix_ms) ?? at, session: null, why: r.kind, words: `the daemon crashed${where ? `: a panic ${where}` : ''}; it started again` }
    }
  }
  return null
}

/** The failures of a stretch, read newest first: each call once, however many of its rows say it went wrong (a job's
 *  lost wrapper and its unknown outcome are one failure, said by the more precise row); an unknown outcome a later
 *  result settled as succeeded is none. */
export class Failures {
  private calls = new Map<string, Failure>()
  private others: Failure[] = []
  /** Calls whose unknown outcome a later row resolved: none of their unknown rows count. */
  private resolved = new Set<string>()

  /** Read a row; rows come newest first. */
  add(r: LedgerEntry): void {
    if (r.kind !== 'action.resolved' && !FAILURE_KINDS.has(r.kind)) return
    if (r.kind === 'action.resolved') {
      const d = (r.data ?? {}) as D
      const cid = str(d.correlation_id)
      if (cid && d.outcome === 'succeeded') this.resolved.add(cid)
      return
    }
    const f = failureOf(r)
    if (!f) return
    if (f.kind !== 'call') { this.others.push(f); return }
    const cid = f.cid!
    if (this.resolved.has(cid) && (f.why === 'action.outcome_unknown' || f.why === 'job.wrapper_lost')) return
    const was = this.calls.get(cid)
    if (!was) { this.calls.set(cid, f); return }
    const precise = CALL_WHY.indexOf(f.why) < CALL_WHY.indexOf(was.why) ? f : was
    this.calls.set(cid, { ...precise, at: Math.max(f.at, was.at), tool: was.tool ?? f.tool, producer: was.producer ?? f.producer })
  }

  /** Every failure, newest first, without a model call's (`isModel`): a model call that failed is its turn's failure,
   *  or a retry that went on. */
  list(isModel: (f: Failure) => boolean): Failure[] {
    return [...this.calls.values(), ...this.others].filter((f) => f.kind !== 'call' || !isModel(f)).sort((a, b) => b.at - a.at)
  }
}

/** A call's tool, from its row, its plan's row, the calls, or its light. */
export type ToolOf = (cid: string) => string | undefined

export const isModelFailure = (toolOf: ToolOf) => (f: Failure) =>
  !!((f.tool ?? toolOf(f.cid!))?.startsWith('provider.') || f.producer?.startsWith('provider:'))

const FLAG_OF: Record<string, string> = {
  'action.outcome_unknown': 'unknown', 'job.wrapper_lost': 'unknown', 'job.refused': 'disk', 'job.stopped_below_floor': 'disk',
  'job.not_started': 'not run',
}

/** A failure's line on a plate: what failed, how, and when. */
export function failureLine(f: Failure, look: Look, now: number, turnLights: Map<string, Light[]>, toolOf: ToolOf): WatchLine {
  const title = look.title(f.session)
  const when = ago(f.at, now)
  if (f.kind === 'call') {
    const tool = f.tool ?? toolOf(f.cid!) ?? 'a call'
    const res = look.result.get(f.cid!)
    const to = { session: f.session ?? undefined, node: res?.id ?? look.call.get(f.cid!)?.id }
    if (f.why !== 'action.failed') {
      return {
        id: `fail:${f.cid}:${f.at}`, tag: tool, text: shortPaths(f.words), figure: when, tone: 'fault', flag: FLAG_OF[f.why],
        title: `${tool} in "${title}", ${when}: ${f.words}.`, to,
      }
    }
    const { words: what, exit } = resultWords(res?.preview || (f.producer ? `failed (${f.producer})` : 'failed'))
    return {
      id: `fail:${f.cid}:${f.at}`, tag: tool, text: shortPaths(what), figure: when, tone: 'fault', flag: exit === undefined ? undefined : `exit ${exit}`,
      title: `${tool} failed in "${title}", ${when}: ${what}`, to,
    }
  }
  if (f.kind === 'turn') {
    const ls = f.turn ? turnLights.get(f.turn) : undefined
    return {
      id: `turnfail:${f.turn ?? ''}:${f.at}`, tag: 'turn', text: oneLine(f.words), figure: when, tone: 'fault',
      title: `A turn failed in "${title}", ${when}: ${oneLine(f.words)}`, to: { session: f.session ?? undefined, node: ls?.[ls.length - 1]?.id },
    }
  }
  if (f.kind === 'crash') {
    // The daemon's own failure: no session to fly to; its line is only words.
    return { id: `crash:${f.at}`, tag: 'daemon', text: oneLine(f.words), figure: when, tone: 'fault', title: `${oneLine(f.words)}, ${when}. Its crash file names where.` }
  }
  return {
    id: `budget:${f.session}:${f.at}`, tag: 'budget', text: oneLine(f.words), figure: when, tone: 'fault',
    title: `"${title}" reached its spend limit, ${when}: ${oneLine(f.words)}`, to: { session: f.session ?? undefined },
  }
}

/** The lights a failure lights: a call's call and result; a failed turn's every light. */
export function failureLights(f: Failure, look: Look, turnLights: Map<string, Light[]>): (string | undefined)[] {
  if (f.kind === 'call') return [look.call.get(f.cid!)?.id, look.result.get(f.cid!)?.id]
  if (f.kind === 'turn' && f.turn) return (turnLights.get(f.turn) ?? []).map((l) => l.id)
  return []
}

// ---------------------------------------------------------------- the scan

/** A turn that ended: how long it ran, and when. */
interface Ended { ms: number; turn: string; session: string | null; at: number }

/** What the rows say, read from the moment back to the longest window (the local day, or the 24 hours before the
 *  moment). */
interface Scan {
  spent: number
  calls: number
  hours: number[]
  bySession: Map<string, number>
  /** Today's model calls' nodes (a provider.call row's `node_id`). */
  spentNodes: Set<string>
  last15: number
  /** The turns that ended after `t` (in the day before the moment), newest first. */
  endedAfter: (t: number) => Ended[]
  /** Each session's newest turn.started by the moment, and whether it is still open then (no end by the moment). */
  newest: Map<string, { turn: string; at: number; open: boolean }>
  /** The last day's failures. */
  failures: Failures
  /** The calls that settled after `t` (in the day before the moment), with how long each ran (action.succeeded and
   *  action.failed's `duration_ms`), newest first. */
  settledAfter: (t: number) => { cid: string; ms: number; at: number }[]
  /** A call's tool (action.planned), its job's argv (tool.job_started), and whether its job ran in L1. */
  tool: (cid: string) => string | undefined
  argv: (cid: string) => string[] | undefined
  l1: (cid: string) => boolean
  /** Each kind's usual at the moment. */
  usuals: Usuals
}

/** Where a walk back from `t` starts: past the last row at or before it, and the skew after (rows are in the ledger's
 *  order, their times within `SKEW_MS` of it). So a scrub, a replay or a stretch that ended hours ago reads back from
 *  its moment, never from the ledger's end. */
export function endOf(rows: readonly LedgerEntry[], t: number): number {
  let lo = 0
  let hi = rows.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (rows[mid].at_unix_ms <= t + SKEW_MS) lo = mid + 1
    else hi = mid
  }
  return lo
}

/** Where a window from `t` starts: the first row whose time is `t` or later, less the skew (rows in the ledger's order,
 *  their times within `SKEW_MS` of it). */
export function startOf(rows: readonly LedgerEntry[], t: number): number {
  let lo = 0
  let hi = rows.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (rows[mid].at_unix_ms < t - SKEW_MS) lo = mid + 1
    else hi = mid
  }
  return lo
}

/** Something a row said, with its time and its place in the ledger: kept until the window passes it. */
type Kept<T> = T & { at: number; i: number }

/** Numbers kept in order, so a median is a look, not a sort: the usuals' times (theseus-qilc). What joins is put in
 *  its place when next asked, all at once, so a day read afresh sorts once. */
class Bag {
  private xs: number[] = []
  private joining: number[] = []
  get size(): number { return this.xs.length + this.joining.length }
  add(x: number) { this.joining.push(x) }
  remove(x: number) {
    this.settle()
    const i = this.place(x)
    if (this.xs[i] === x) this.xs.splice(i, 1)
  }
  /** The median (the mean of the middle two for an even count), as `median` says it. */
  median(): number | null {
    this.settle()
    const a = this.xs
    if (!a.length) return null
    const m = a.length >> 1
    return a.length % 2 ? a[m] : (a[m - 1] + a[m]) / 2
  }
  private settle() {
    if (!this.joining.length) return
    const add = this.joining.sort((p, q) => p - q)
    this.joining = []
    if (add.length > 8) {
      // Many at once: merge the two runs.
      const a = this.xs
      const out = new Array<number>(a.length + add.length)
      let i = 0
      let j = 0
      for (let k = 0; k < out.length; k++) out[k] = j >= add.length || (i < a.length && a[i] <= add[j]) ? a[i++] : add[j++]
      this.xs = out
    } else {
      for (const x of add) this.xs.splice(this.place(x), 0, x)
    }
  }
  private place(x: number): number {
    let lo = 0
    let hi = this.xs.length
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (this.xs[mid] < x) lo = mid + 1
      else hi = mid
    }
    return lo
  }
}

/** A call that settled, kept: the usuals' keys it is counted under now (none while it is out of the day before the
 *  moment, or its tool is a model's or unknown), and whether its tool came from a row or from the calls and the chart. */
type Settled = Kept<{ cid: string; ms: number }> & { keys: string[] | null; fromRow: boolean; expires?: number }

/** The usuals' keys of a call whose tool is `tool` and whose job ran `argv`: its tool's, and a job's command's and
 *  program's. */
function usualKeys(tool: string, argv: readonly string[] | undefined): string[] {
  if (tool !== 'proc.run' || !argv) return [tool]
  const [command, prog] = keysOf(argv)
  return prog ? [tool, `proc.run $ ${command}`, `proc.run ${prog}`] : [tool, `proc.run $ ${command}`]
}

/** The day's scan, kept between recomputes (theseus-qilc). The ledger's rows only ever join its end, and the live
 *  moment only moves on, so a recompute reads just the rows that came since the last (and those the moment has now
 *  reached), not the day again; what falls out of the window is passed over as it is read, and let go now and then.
 *  The usuals are kept too: each call that settles joins its kinds' times as it comes, and leaves them as the day
 *  passes it, so a median is a look. A moment moved back (a scrub), a new local day, or another ledger reads afresh.
 *  Each watch keeps one; `watchOf` without one reads afresh. Fresh or kept, a moment reads the same. */
export class DayScan {
  /** Rows read in all: a test's measure of the work. */
  read = 0
  private rows: readonly LedgerEntry[] = []
  /** The first and last rows of the ledger last read, and its length: a ledger that only grew starts with them. */
  private first: LedgerEntry | undefined
  private last: LedgerEntry | undefined
  private len = 0
  /** Rows before this are read (or wait in `pending`). */
  private hi = 0
  private now = -Infinity
  private dayStart = NaN
  private nHours = 0
  /** Rows read before their time: each is taken once the moment reaches it. */
  private pending: number[] = []
  private spent = 0
  private calls = 0
  private hours: number[] = []
  /** Each session's dollars, and the place of its newest model call: the session that spent most, on a tie, is the
   *  one that spent last. */
  private bySession = new Map<string, { usd: number; i: number }>()
  private spentNodes = new Set<string>()
  private ended: Kept<Ended & { inDay: boolean }>[] = []
  private started = new Map<string, Kept<{ turn: string }>>()
  private done = new Map<string, number>()
  private failing: Kept<{ r: LedgerEntry }>[] = []
  private settled: Settled[] = []
  private tools = new Map<string, Kept<{ tool: string }>>()
  private argvs = new Map<string, Kept<{ argv: string[] }>>()
  private l1s = new Map<string, number>()
  /** Some row was taken out of the ledger's order: the kept lists are put back in it before they are read. */
  private late = false
  private letGo = -Infinity
  // The usuals, kept: each kind's times, the turns', and what changed since they were last brought up to the moment.
  private bags = new Map<string, Bag>()
  private turns = new Bag()
  /** Settled calls counted with no plan row for their tool, or a job with no argv: a row read late may say it. */
  private waiting = new Map<string, Settled[]>()
  /** Calls settled since, not yet counted; calls whose tool came from the calls or the chart (asked again each time);
   *  calls whose plan or job row came, or left the window. */
  private unbagged: Settled[] = []
  private unturned: Kept<Ended & { inDay: boolean }>[] = []
  private fallbacks = new Set<Settled>()
  private touched = new Set<Settled>()
  /** Calls whose plan or job row the window lets go before the day lets the call go (a call that ran longer than the
   *  skew): each is counted again when it does. */
  private expiring = new Set<Settled>()
  /** The window's start at the moment. */
  private from = -Infinity
  /** How far the day before the moment has passed the settled calls and the ended turns. */
  private settledPast = 0
  private endedPast = 0

  /** What `toolOf` was last read from. */
  private sources: readonly unknown[] = []

  /** The scan of `rows` at the moment `now`, in the local day from `dayStart` (`nHours` long). `toolOf` says a call's
   *  tool when no row in the window does, from `sources` (the calls, the chart): while they are the same objects, what
   *  it said stands. */
  of(rows: readonly LedgerEntry[], now: number, dayStart: number, nHours: number, toolOf: ToolOf = () => undefined, sources: readonly unknown[] = []): Scan {
    const grew = rows.length >= this.len && (this.len === 0 || (rows[0] === this.first && rows[this.len - 1] === this.last))
    if (!grew || now < this.now || dayStart !== this.dayStart || nHours !== this.nHours) this.reset(rows, now, dayStart, nHours)
    this.rows = rows
    if (now !== this.now || this.pending.length) {
      this.now = now
      this.from = windowFrom(now, dayStart)
      const waiting = this.pending
      this.pending = []
      for (const i of waiting) this.take(rows[i], i)
    }
    const to = endOf(rows, now)
    for (let i = this.hi; i < to; i++) this.take(rows[i], i)
    this.hi = Math.max(this.hi, to)
    this.len = rows.length
    this.first = rows[0]
    this.last = rows[rows.length - 1]
    const asked = sources.length !== this.sources.length || sources.some((x, k) => x !== this.sources[k])
    this.sources = sources
    return this.result(toolOf, asked)
  }

  private reset(rows: readonly LedgerEntry[], now: number, dayStart: number, nHours: number) {
    this.now = now
    this.dayStart = dayStart
    this.nHours = nHours
    this.hi = startOf(rows, windowFrom(now, dayStart))
    this.pending = []
    this.spent = 0
    this.calls = 0
    this.hours = new Array<number>(nHours).fill(0)
    this.bySession = new Map()
    this.spentNodes = new Set()
    this.ended = []
    this.started = new Map()
    this.done = new Map()
    this.failing = []
    this.settled = []
    this.tools = new Map()
    this.argvs = new Map()
    this.l1s = new Map()
    this.late = false
    this.letGo = windowFrom(now, dayStart)
    this.bags = new Map()
    this.turns = new Bag()
    this.waiting = new Map()
    this.unbagged = []
    this.unturned = []
    this.fallbacks = new Set()
    this.touched = new Set()
    this.expiring = new Set()
    this.from = windowFrom(now, dayStart)
    this.settledPast = 0
    this.endedPast = 0
  }

  /** Keep `x` at the end of `xs`; a row out of the ledger's order (read late: its time had not come) is noted. */
  private keep<T extends { i: number }>(xs: T[], x: T) {
    if (xs.length && x.i < xs[xs.length - 1].i) this.late = true
    xs.push(x)
  }

  /** Take one row: one whose time is after the moment waits for it; one before the window says nothing. */
  private take(r: LedgerEntry, i: number) {
    this.read++
    const at = r.at_unix_ms
    if (at > this.now) { this.pending.push(i); return }
    if (at < this.from) return
    const d = (r.data ?? {}) as D
    const sid = r.session_id
    if (r.kind === 'action.resolved' || FAILURE_KINDS.has(r.kind)) this.keep(this.failing, { r, at, i })
    switch (r.kind) {
      case 'provider.call': {
        if (at < this.dayStart) break
        // Spend means what it means in Money and Economics: each model call's recorded cost.
        const cost = Number(d.cost_usd ?? 0) || 0
        this.spent += cost
        this.calls++
        this.hours[Math.min(this.nHours - 1, Math.floor((at - this.dayStart) / HOUR_MS))] += cost
        if (sid) {
          const was = this.bySession.get(sid)
          if (!was) this.bySession.set(sid, { usd: cost, i })
          else {
            was.usd += cost
            was.i = Math.max(was.i, i)
          }
        }
        const node = str(d.node_id)
        if (node) this.spentNodes.add(node)
        break
      }
      case 'turn.ended': {
        const t = r.turn_id
        if (t) this.done.set(t, at)
        const e = num(d.elapsed_ms)
        if (t && e !== undefined) {
          const x = { ms: e, turn: t, session: sid, at, i, inDay: false }
          this.keep(this.ended, x)
          this.unturned.push(x)
        }
        break
      }
      case 'turn.failed':
        if (r.turn_id) this.done.set(r.turn_id, at)
        break
      case 'turn.started': {
        // A session's newest start, by its place in the ledger.
        const was = sid ? this.started.get(sid) : undefined
        if (sid && r.turn_id && (!was || i > was.i)) this.started.set(sid, { turn: r.turn_id, at, i })
        break
      }
      case 'action.succeeded':
      case 'action.failed': {
        const cid = str(d.correlation_id)
        const took = num(d.duration_ms)
        if (!cid || took === undefined) break
        const c: Settled = { cid, ms: took, at, i, keys: null, fromRow: true }
        this.keep(this.settled, c)
        this.unbagged.push(c)
        break
      }
      case 'action.planned': {
        // A call's first plan says its tool.
        const cid = str(d.correlation_id)
        const tool = str(d.tool)
        const was = cid ? this.tools.get(cid) : undefined
        if (cid && tool && (!was || i < was.i)) {
          this.tools.set(cid, { tool, at, i })
          this.came(cid)
        }
        break
      }
      case 'tool.job_started': {
        const cid = str(d.correlation_id)
        if (!cid) break
        const was = this.argvs.get(cid)
        if (Array.isArray(d.argv) && d.argv.every((a) => typeof a === 'string') && (!was || i < was.i)) {
          this.argvs.set(cid, { argv: d.argv as string[], at, i })
          this.came(cid)
        }
        if (d.class === 'l1') this.l1s.set(cid, at)
        break
      }
      case 'sandbox.started': {
        const cid = str(d.correlation_id)
        if (cid && d.class === 'l1') this.l1s.set(cid, at)
        break
      }
    }
  }

  /** A call's plan or job row came after it settled (read late): it is counted again. One that settles later is
   *  counted as it does. */
  private came(cid: string) {
    const cs = this.waiting.get(cid)
    if (!cs) return
    this.waiting.delete(cid)
    for (const c of cs) this.touched.add(c)
  }

  private tool(cid: string, from: number): string | undefined {
    const t = this.tools.get(cid)
    return t && t.at >= from ? t.tool : undefined
  }

  private argv(cid: string, from: number): string[] | undefined {
    const a = this.argvs.get(cid)
    return a && a.at >= from ? a.argv : undefined
  }

  /** Count a settled call under its kinds' usuals as the moment sees it: none outside the day before the moment, or
   *  for a model's call or an unknown tool; else its tool's, and a job's command's and program's. */
  private bag(c: Settled, from: number, day: number, toolOf: ToolOf) {
    if (c.keys) for (const k of c.keys) this.bags.get(k)?.remove(c.ms)
    c.keys = null
    if (!c.fromRow) this.fallbacks.delete(c)
    if (c.expires !== undefined) this.expiring.delete(c)
    c.fromRow = true
    c.expires = undefined
    if (c.at <= day) return
    const t = this.tools.get(c.cid)
    const a = this.argvs.get(c.cid)
    const row = t && t.at >= from ? t.tool : undefined
    const tool = row ?? toolOf(c.cid)
    if (row === undefined) {
      c.fromRow = false
      this.fallbacks.add(c)
    }
    // The rows it is counted by: when the window lets one go before the day lets the call go, it is counted again.
    const rowAt = Math.min(t && t.at >= from ? t.at : Infinity, a && a.at >= from ? a.at : Infinity)
    if (rowAt !== Infinity && rowAt + SKEW_MS <= c.at) {
      c.expires = rowAt
      this.expiring.add(c)
    }
    const argv = a && a.at >= from ? a.argv : undefined
    if (row === undefined || (tool === 'proc.run' && !argv)) {
      const cs = this.waiting.get(c.cid)
      if (!cs) this.waiting.set(c.cid, [c])
      else if (!cs.includes(c)) cs.push(c)
    }
    if (!tool || tool.startsWith('provider.')) return
    const ks = usualKeys(tool, argv)
    for (const k of ks) {
      let b = this.bags.get(k)
      if (!b) this.bags.set(k, (b = new Bag()))
      b.add(c.ms)
    }
    c.keys = ks
  }

  /** Bring the usuals up to the moment: what the day passed leaves, what settled since joins, and what a row changed (or
   *  the calls and the chart might have) is counted again. */
  private bagsAt(from: number, day: number, toolOf: ToolOf, asked: boolean) {
    // Passed by the day: the lists are in the ledger's order, their times within the skew of it.
    for (let k = this.settledPast; k < this.settled.length && this.settled[k].at <= day + SKEW_MS; k++) {
      const c = this.settled[k]
      if (c.at <= day && c.keys) this.bag(c, from, day, toolOf)
      if (k === this.settledPast && c.at <= day) this.settledPast++
    }
    for (let k = this.endedPast; k < this.ended.length && this.ended[k].at <= day + SKEW_MS; k++) {
      const e = this.ended[k]
      if (e.at <= day && e.inDay) { this.turns.remove(e.ms); e.inDay = false }
      if (k === this.endedPast && e.at <= day) this.endedPast++
    }
    for (const e of this.unturned) if (e.at > day) { this.turns.add(e.ms); e.inDay = true }
    this.unturned = []
    // Plan and job rows the window let go.
    for (const c of this.expiring) if (c.expires! < from) this.touched.add(c)
    for (const c of this.touched) this.bag(c, from, day, toolOf)
    this.touched.clear()
    for (const c of this.unbagged) if (!c.keys) this.bag(c, from, day, toolOf)
    this.unbagged = []
    if (asked) for (const c of [...this.fallbacks]) this.bag(c, from, day, toolOf)
  }

  /** What the rows say at the moment: the window's part of what was read, newest first. */
  private result(toolOf: ToolOf, asked: boolean): Scan {
    const { now, rows } = this
    const from = windowFrom(now, this.dayStart)
    const day = now - DAY_MS
    if (this.late) {
      const byPlace = (a: { i: number }, b: { i: number }) => a.i - b.i
      this.ended.sort(byPlace)
      this.failing.sort(byPlace)
      this.settled.sort(byPlace)
      this.settledPast = this.endedPast = 0
      this.late = false
    }
    this.bagsAt(from, day, toolOf, asked)
    // Let go of what the window has passed, now and then: the moment only moves on, so it never comes back.
    if (from - this.letGo > 10 * 60_000) {
      this.letGo = from
      const keep = <T extends { at: number }>(xs: T[]) => xs.filter((x) => x.at >= from)
      this.ended = keep(this.ended)
      this.failing = keep(this.failing)
      this.settled = keep(this.settled)
      for (const [cid, cs] of this.waiting) if (cs.every((c) => c.at <= day)) this.waiting.delete(cid)
      this.settledPast = this.endedPast = 0
      for (const m of [this.started, this.tools, this.argvs]) for (const [k, v] of m) if (v.at < from) m.delete(k)
      for (const m of [this.done, this.l1s]) for (const [k, at] of m) if (at < from) m.delete(k)
    }
    // The pace's 15 minutes, read back from the moment.
    let last15 = 0
    for (let i = this.hi - 1; i >= 0; i--) {
      const r = rows[i]
      const at = r.at_unix_ms
      if (at < now - PACE_MS - SKEW_MS || at < from) break
      if (r.kind === 'provider.call' && at <= now && at > now - PACE_MS && at >= this.dayStart) last15 += Number((r.data as D | null)?.cost_usd ?? 0) || 0
    }
    // A list's part after `t`, read back from its end: it is in the ledger's order, its times within the skew of it.
    const after = <T extends { at: number }>(xs: T[], t: number): T[] => {
      const out: T[] = []
      for (let k = xs.length - 1; k >= 0 && xs[k].at >= Math.max(t, day) - SKEW_MS; k--) if (xs[k].at > t && xs[k].at > day) out.push(xs[k])
      return out
    }
    const { ended, settled } = this
    const failures = new Failures()
    for (let k = this.failing.length - 1; k >= 0; k--) if (this.failing[k].at > day) failures.add(this.failing[k].r)
    const newest: Scan['newest'] = new Map()
    for (const [sid, s] of [...this.started].filter(([, s]) => s.at >= from).sort((a, b) => b[1].i - a[1].i)) {
      newest.set(sid, { turn: s.turn, at: s.at, open: !this.done.has(s.turn) })
    }
    const { bags, l1s } = this
    const usual = (k: string, of: string): Usual | null => {
      const b = bags.get(k)
      return b && b.size >= USUAL_MIN ? { ms: b.median()!, n: b.size, of } : null
    }
    const turn = this.turns.size >= USUAL_MIN ? { ms: this.turns.median()!, n: this.turns.size, of: 'turns' } : null
    return {
      spent: this.spent, calls: this.calls, hours: [...this.hours], spentNodes: this.spentNodes, last15,
      bySession: new Map([...this.bySession].sort((a, b) => b[1].i - a[1].i).map(([sid, s]) => [sid, s.usd])),
      endedAfter: (t) => after(ended, t), settledAfter: (t) => after(settled, t), newest, failures,
      tool: (cid) => this.tool(cid, from),
      argv: (cid) => this.argv(cid, from),
      l1: (cid) => (l1s.get(cid) ?? -Infinity) >= from,
      usuals: {
        of: (tool, argv) => {
          const [, command, prog] = usualKeys(tool, argv)
          return (command !== undefined ? usual(command, 'runs of this command') : null)
            ?? (prog !== undefined ? usual(prog, `\`${prog.slice('proc.run '.length)}\` jobs`) : null) ?? usual(tool, `${tool} calls`)
        },
        turn,
      },
    }
  }
}

/** The scan's window starts at the local day's start, or 24 hours before the moment if that is earlier, less the skew. */
const windowFrom = (now: number, dayStart: number) => Math.min(dayStart, now - DAY_MS) - SKEW_MS

/** A set's ids, once each and in order, so two sets compare by their words. */
const ids = (xs: Iterable<string | undefined>) => [...new Set([...xs].filter((x): x is string => !!x))].sort()

export const setOf = (vessels: Iterable<string | undefined | null>, lights: Iterable<string | undefined>): WatchSet => ({
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

/** The number of hours in the local day from `dayStart` to `dayEnd`: 24, or 23 or 25 on a day the clocks change. */
export const hoursOf = (dayStart: number, dayEnd?: number) =>
  Math.max(1, Math.min(26, Math.round(((dayEnd ?? dayStart + DAY_MS) - dayStart) / HOUR_MS)))

// ---------------------------------------------------------------- usuals

/** What a call's or a turn's time is held against: the median of its own kind's in the day before the moment. */
export interface Usual {
  ms: number
  /** How many it is the median of, and of what, in words: "proc.run calls", "`cargo` jobs", "turns". */
  n: number
  of: string
}

/** Each kind's times in the day before the moment: a tool's calls, a job's program's runs, and the turns. */
export interface Usuals { of(tool: string, argv?: readonly string[]): Usual | null; turn: Usual | null }

/** A command's two keys (its command, its program), worked out once for each command line however many times it ran,
 *  and kept between recomputes: a day has a few hundred lines at most, and a full cache starts over. */
const keys = new Map<string, [string, string | undefined]>()
function keysOf(argv: readonly string[]): [string, string | undefined] {
  const line = argv.join('\u0000')
  let k = keys.get(line)
  if (!k) {
    if (keys.size >= 5000) keys.clear()
    keys.set(line, (k = [commandKey(argv), programOf(argv)]))
  }
  return k
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
  argv?: string[]
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
    const argv = sc.argv(cid)
    seen.add(cid)
    jobs.push({
      id: `job:${cid}`, kind: 'job', session, since, cid, tool, call, argv,
      words: argv ? argvWords(argv) : call ? commandOf(call.preview) : tool,
      full: argv ? argv.join(' ') : call?.preview ?? tool,
      l1: !!call?.l1 || sc.l1(cid),
    })
  }
  for (const a of input.actions ?? []) {
    if (!a.dispatched_at_ms || a.settled_at_ms || seen.has(a.correlation_id)) continue
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

/** The five plates at the moment. `day`, a watch's own scan kept between recomputes, reads only the rows since its
 *  last; without one the day is read afresh, to the same plates. */
export function watchOf(input: WatchInput, day: DayScan = new DayScan()): WatchPlates {
  const { model, now, dayStart } = input
  const look = lookOf(model)
  const calls = new Map((input.actions ?? []).map((a) => [a.correlation_id, a]))
  // A call's tool when no row in the window says it: the calls, then the chart.
  const elsewhere: ToolOf = (cid) => calls.get(cid)?.tool ?? look.call.get(cid)?.tool
  const sc = day.of(input.rows, now, dayStart, hoursOf(dayStart, input.dayEnd), elsewhere, [input.actions, model])
  const toolOf: ToolOf = (cid) => sc.tool(cid) ?? elsewhere(cid)
  const failures = sc.failures.list(isModelFailure(toolOf))
  const usuals = sc.usuals
  const { turns, jobs, queued } = runs(input, look, sc)
  const running = [...turns, ...jobs]

  // The turns whose lights some plate needs: those running, the hour's slowest, and those that failed.
  const hour = sc.endedAfter(now - HOUR_MS)
  const slowest = hour.reduce<Ended | null>((a, e) => (!a || e.ms > a.ms ? e : a), null)
  const turnLights = lightsOfTurns(model, new Set(ids([
    ...running.map((r) => r.turn), slowest?.turn, ...failures.map((f) => (f.kind === 'turn' ? f.turn : undefined)),
  ])))

  return {
    working: working(input, look, turns, jobs, queued, turnLights),
    waiting: waiting(input, look),
    slow: slow(input, look, running, hour, slowest, turnLights, sc, usuals, toolOf, calls),
    spent: spent(input, look, sc),
    wrong: wrong(input, look, failures, turnLights, toolOf),
  }
}

/** The five plates in their order down the column (the sixth, since you last looked, is `since.ts`'s). */
export const platesOf = (w: WatchPlates): Plate[] => WATCH_KEYS.filter((k): k is keyof WatchPlates => k !== 'since').map((k) => w[k])

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
  const holds = (model?.vessels ?? []).filter((v) => v.hold)
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
  for (const v of holds) {
    const h = v.hold!
    const what = h.query ? `"${h.query}"` : h.url.replace(/^\w+:\/\//, '')
    lines.push({
      id: `hold:${v.id}`, tag: 'holds', text: `read ${h.tool} ${what}`, detail: `since ${span(now - h.since_ms)} · ${v.title}`, tone: 'idle',
      title: `"${v.title}" holds text from outside: it read ${h.tool} ${what} ${span(now - h.since_ms)} ago. Its next call that acts waits for your approval until you trust it again.`,
      to: { session: v.id, node: h.node_id || undefined },
    })
  }
  const n = qs.length + others.length
  const ready = input.confirms !== undefined
  const held = holds.length ? `; ${count(holds.length, 'session holds', 'sessions hold')} web text` : ''
  const parts = [
    approvals ? count(approvals, 'approval', 'approvals') : '', budgets ? count(budgets, 'budget question', 'budget questions') : '',
    others.length ? count(others.length, 'session that needs you', 'sessions that need you') : '',
  ].filter(Boolean)
  return {
    key: 'waiting', question: 'Waiting for you', approvals, budgets, others: others.length, holds: holds.length,
    value: ready ? String(n) : '…', unit: n ? 'waiting' : 'none',
    caption: !ready ? 'reading the questions…' : `${n ? `waiting for your answer: ${andList(parts)}` : 'nothing waits for your answer'}${held}`,
    tone: n ? 'wait' : 'idle', pulse: `${n}|${holds.length}`,
    lines: lines.slice(0, LINES), more: Math.max(0, lines.length - LINES),
    focus: setOf([...qs.map((c) => c.session_id), ...others.map((v) => v.id), ...holds.map((v) => v.id)], qs.map((c) => look.call.get(c.correlation_id)?.id)),
    link: { to: '/actions', label: 'Actions' },
  }
}

/** A thing running now against its usual. */
interface Timed { r: Run; elapsed: number; usual: Usual | null; x: number | null; slow: boolean }

function slow(
  input: WatchInput, look: Look, running: Run[], hour: Ended[], slowest: Ended | null,
  turnLights: Map<string, Light[]>, sc: Scan, usuals: Usuals, toolOf: ToolOf, calls: Map<string, ActionInfo>,
): Slow {
  const { now } = input
  // Each thing running against its own usual: a job against its program's (or its tool's), a turn against the day's.
  const timed: Timed[] = running.filter((r) => r.since !== null).map((r) => {
    const elapsed = now - r.since!
    const usual = r.kind === 'turn' ? usuals.turn : usuals.of(r.tool ?? 'job', r.argv)
    const x = usual && usual.ms > 0 ? elapsed / usual.ms : null
    return { r, elapsed, usual, x, slow: x !== null && x >= SLOW_TIMES && elapsed >= SLOW_FLOOR_MS }
  })
  // The slowest against their usuals first; then those with no usual yet, the longest first.
  timed.sort((a, b) => (b.x ?? -1) - (a.x ?? -1) || b.elapsed - a.elapsed)
  const longest = timed.reduce<Timed | null>((a, t) => (!a || t.elapsed > a.elapsed ? t : a), null)
  const nSlow = timed.filter((t) => t.slow).length
  const worst = timed.reduce<number | null>((a, t) => (t.x === null ? a : Math.max(a ?? 0, t.x)), null)

  // The hour's worst against its usual: a call that settled, or a turn that ended, in the hour before the moment.
  let hourWorst: { line: WatchLine; session: string | null; lights: (string | undefined)[] } | null = null
  let best = 1
  for (const c of sc.settledAfter(now - HOUR_MS)) {
    const tool = toolOf(c.cid)
    if (!tool || tool.startsWith('provider.')) continue
    const u = usuals.of(tool, sc.argv(c.cid))
    if (!u || u.ms <= 0 || c.ms / u.ms <= best || c.ms < SLOW_FLOOR_MS) continue
    best = c.ms / u.ms
    const call = look.call.get(c.cid)
    const session = call?.sessionId ?? calls.get(c.cid)?.session_id ?? null
    hourWorst = {
      session, lights: [call?.id, look.result.get(c.cid)?.id],
      line: {
        id: `slowcall:${c.cid}`, tag: tool, text: `slowest this hour: ${look.title(session)}`, figure: ms(c.ms), flag: times(best),
        detail: `usually ${ms(u.ms)} (${u.n} ${u.of} today)`, tone: best >= SLOW_TIMES ? 'wait' : 'idle',
        title: `The slowest call of the hour against its usual: ${tool} took ${ms(c.ms)} in "${look.title(session)}", ${times(best)} its usual ${ms(u.ms)}, the median of ${u.n} ${u.of} in the day before the moment.`,
        to: { session: session ?? undefined, node: call?.id },
      },
    }
  }
  const tu = usuals.turn
  for (const e of hour) {
    if (!tu || tu.ms <= 0 || e.ms / tu.ms <= best || e.ms < SLOW_FLOOR_MS) continue
    best = e.ms / tu.ms
    const ls = turnLights.get(e.turn) ?? lightsOfTurns(input.model, new Set([e.turn])).get(e.turn)
    hourWorst = {
      session: e.session, lights: (ls ?? []).map((l) => l.id),
      line: {
        id: `slowturn:${e.turn}`, tag: 'turn', text: `slowest this hour: ${look.title(e.session)}`, figure: ms(e.ms), flag: times(best),
        detail: `usually ${ms(tu.ms)} (${tu.n} turns today)`, tone: best >= SLOW_TIMES ? 'wait' : 'idle',
        title: `The slowest turn of the hour against its usual took ${ms(e.ms)} in "${look.title(e.session)}", ${times(best)} the median turn of the day, ${ms(tu.ms)} (${tu.n} turns).`,
        to: { session: e.session ?? undefined, node: ls?.[ls.length - 1]?.id },
      },
    }
  }
  // With no usual yet (a fresh store), the hour's slowest turn against the hour's median, as before.
  const med = median(hour.map((e) => e.ms))
  if (!hourWorst && slowest) {
    const title = look.title(slowest.session)
    const ls = turnLights.get(slowest.turn)
    hourWorst = {
      session: slowest.session, lights: (ls ?? []).map((l) => l.id),
      line: {
        id: `slowest:${slowest.turn}`, tag: 'turn', text: `slowest this hour: ${title}`, figure: ms(slowest.ms),
        detail: `the median of ${count(hour.length, 'turn', 'turns')} this hour: ${ms(med)}`, tone: 'idle',
        title: `The slowest of the ${count(hour.length, 'turn', 'turns')} that ended in the hour before the moment took ${ms(slowest.ms)}, in "${title}"; their median took ${ms(med)}.`,
        to: { session: slowest.session ?? undefined, node: ls?.[ls.length - 1]?.id },
      },
    }
  }

  const shown = timed.slice(0, hourWorst ? LINES - 1 : LINES)
  const lines = shown.map((t): WatchLine => {
    const line = runLine(t.r, look, now, turnLights)
    const u = t.usual
    return {
      ...line,
      flag: t.slow ? times(t.x!) : line.flag, tone: t.slow ? 'wait' : 'live',
      detail: u ? `usually ${ms(u.ms)} (${u.n} ${u.of} today)` : `no usual yet: under ${USUAL_MIN} like it today`,
      title: `${line.title}${u ? ` Its usual is ${ms(u.ms)}, the median of ${u.n} ${u.of} in the day before the moment: it has run ${times(t.x!)} that.` : ''}`,
    }
  })
  if (hourWorst) lines.push(hourWorst.line)
  const n = timed.length
  const ready = input.model !== null && input.actions !== undefined
  const kind = longest ? (longest.r.kind === 'turn' ? 'turn' : 'job') : ''
  return {
    key: 'slow', question: 'Slow', longestMs: longest?.elapsed ?? null, slow: nSlow, worst,
    ended: hour.length, slowestMs: slowest?.ms ?? null, medianMs: med,
    value: !ready ? '…' : longest ? span(longest.elapsed) : '—',
    unit: longest ? 'longest' : 'nothing running',
    caption: !ready ? 'reading the fleet…'
      : !longest ? 'nothing is running now'
        : nSlow ? (n === 1 ? `the one ${kind} running is ${times(timed[0].x!)} its usual`
          : nSlow === n ? `all ${n} things running are past ${SLOW_TIMES}× their usual`
            : `${nSlow} of the ${n} things running ${nSlow === 1 ? 'is' : 'are'} past ${SLOW_TIMES}× ${nSlow === 1 ? 'its' : 'their'} usual`)
          : n === 1 ? `the one ${kind} running now${longest.usual ? ', within its usual' : ''}`
            : `the longest of ${n} things running now (a ${kind})${worst !== null ? `; none past ${SLOW_TIMES}× its usual` : ''}`,
    tone: nSlow ? 'wait' : longest ? 'live' : 'idle',
    pulse: `${nSlow}|${longest?.r.id ?? ''}|${hourWorst?.line.id ?? ''}`,
    lines, more: Math.max(0, n - shown.length),
    focus: setOf(
      [...shown.map((t) => t.r.session), hourWorst?.session],
      [...shown.flatMap((t) => runLights(t.r, look, turnLights)), ...(hourWorst?.lights ?? [])],
    ),
    link: { to: '/actions', label: 'Actions' },
  }
}

function spent(input: WatchInput, look: Look, sc: Scan): Spent {
  const ready = input.rowsReady
  const hour = Math.max(0, Math.min(sc.hours.length - 1, Math.floor((input.now - input.dayStart) / HOUR_MS)))
  let top: Spent['top'] = null
  // A tie (to a billionth of a cent, whatever order the cents were added in) goes to the session that spent last.
  for (const [session, v] of sc.bySession) if (v > 0 && (!top || v > top.usd + 1e-11)) top = { session, usd: v }
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

function wrong(input: WatchInput, look: Look, failures: Failure[], turnLights: Map<string, Light[]>, toolOf: ToolOf): Wrong {
  const { now } = input
  const ready = input.rowsReady
  const calls = failures.filter((f) => f.kind === 'call').length
  const turns = failures.filter((f) => f.kind === 'turn').length
  const crashes = failures.filter((f) => f.kind === 'crash').length
  const budgets = failures.length - calls - turns - crashes
  const lines = failures.slice(0, LINES).map((f) => failureLine(f, look, now, turnLights, toolOf))
  const n = failures.length
  const parts = [
    calls ? count(calls, 'call', 'calls') : '', turns ? count(turns, 'turn', 'turns') : '', budgets ? count(budgets, 'spend limit', 'spend limits') : '',
    crashes ? count(crashes, 'crash', 'crashes') : '',
  ].filter(Boolean)
  return {
    key: 'wrong', question: 'Went wrong', count: n, calls, turns, budgets, crashes,
    value: ready ? String(n) : '…', unit: n ? 'in a day' : 'all clear',
    caption: !ready ? 'reading the ledger…' : n ? `failures in the last 24 hours: ${andList(parts)}` : 'nothing failed in the last 24 hours',
    tone: n ? 'fault' : 'ok', pulse: String(n),
    lines: ready ? lines : [], more: ready ? Math.max(0, n - LINES) : 0,
    focus: setOf(failures.map((f) => f.session), failures.flatMap((f) => failureLights(f, look, turnLights))),
    link: { to: `/ledger?kind=${WRONG_KINDS.join(',')}&from=${now - DAY_MS}&to=${now}`, label: 'the ledger' },
  }
}

// ---------------------------------------------------------------- the calls the Ship reads

const SETTLED_STATES = new Set(['succeeded', 'failed', 'cancelled'])

/** The calls the Ship and the watch read: action.list's newest page and every call not settled, however old (its
 *  `unsettled` read), each once, newest first. A call in both is taken from the read that says it settled, since a
 *  call never comes unsettled again. A daemon from before the option answers the newest page instead: its settled
 *  calls are left out of the unsettled read. */
export function mergeActions(newest?: readonly ActionInfo[], unsettled?: readonly ActionInfo[]): ActionInfo[] | undefined {
  if (!newest && !unsettled) return undefined
  const by = new Map<string, ActionInfo>()
  for (const a of newest ?? []) by.set(a.correlation_id, a)
  for (const a of unsettled ?? []) {
    if (SETTLED_STATES.has(a.state)) continue
    const was = by.get(a.correlation_id)
    if (!was || (!was.settled_at_ms && !SETTLED_STATES.has(was.state))) by.set(a.correlation_id, a)
  }
  return [...by.values()].sort((a, b) => b.planned_at_ms - a.planned_at_ms)
}
