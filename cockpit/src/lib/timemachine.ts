// The time machine (theseus-logs, round two): the state at any moment, folded from the ledger. Theseus's store only
// grows, and every fact since C2 is one typed row with a time, so the sessions, executions, calls, questions, holds,
// and gauges as they stood at 15:12:07 are the rows up to 15:12:07, folded in order.
//
// - The fold is pure: a step replaces what it changes and never mutates it, so a checkpoint is a set of shallow map
//   copies that share their entries.
// - A checkpoint every few hundred rows: a moment folds from the nearest one before it, so a scrub folds at most
//   that many rows, never the whole ledger again.
// - The views get what they always read (SessionInfo, ExecutionInfo, ActionInfo, ConfirmRequest, TaskInfo), as of
//   the moment, with the live lists' fixed fields (titles, labels, kinds, parents) kept. Attention follows the
//   server's own rules (`theseus_protocol::attention`) on what the ledger knows.
import { create } from 'zustand'
import type {
  ActionInfo, Attention, CancelVerdict, ConfirmRequest, ExecutionInfo, ExternalText, ExternalTextInfo, LedgerEntry,
  SessionInfo, TaskInfo, Tightening, Usage,
} from '@protocol'

import { busyOf, DEFAULT_RULE, deriveState, foldLife, type Life, type StateRule } from './sessionState'

type D = Record<string, any>

// ---------------------------------------------------------------- the moment

/** The moment every view shows: null is live. The bar keeps `?t=` in the address in step with it. */
export const useAsOf = create<{ t: number | null; set: (t: number | null) => void }>((set) => ({
  t: (() => {
    const v = Number(new URLSearchParams(window.location.search).get('t'))
    return Number.isFinite(v) && v > 0 ? v : null
  })(),
  set: (t) => set({ t }),
}))

// ---------------------------------------------------------------- the fold's state

interface SessAt {
  created: number
  turns: number
  usage: Usage
  cost: number
  toolCalls: number
  lastActive: number
  execId?: string
  hold?: ExternalText
  profile?: string
  model?: string
}

interface ExecAt {
  session: string
  kind: string
  state: string
  turns: number
  limit: number
  spent: number
  resets: number
  created: number
  updated: number
  interrupted: number
  wakeOn?: string
  why?: string
  endedReason?: string
  parent?: string
}

interface ActAt {
  exec: string
  session: string
  tool: string
  state: string
  retry: string
  planned: number
  authorized?: number
  dispatched?: number
  settled?: number
  deadline: number
  reserved: number
  confirmed: boolean
  cancel?: string
  verdict?: CancelVerdict
}

export interface Snap {
  /** Rows folded: `rows[0 .. index)`. */
  index: number
  sessions: Map<string, SessAt>
  execs: Map<string, ExecAt>
  acts: Map<string, ActAt>
  asks: Map<string, ConfirmRequest>
  tight: Map<string, Tightening>
  /** Jobs started and not settled. */
  jobs: Set<string>
  l1: Set<string>
  startedAt: number | null
  stoppedAt: number | null
  servingUs?: number
  profile?: string
  costTotal: number
  usageTotal: Usage
  /** Jev's judgments (M5 23b): replaced, never mutated, by each judge row. */
  judge: JudgeAt
  /** Each session's state rows (theseus-emqx): retired, superseded, reopened. */
  lives: Map<string, Life>
}

/** The judge as of a moment: its judgments, their cost, the shadow budget's pause, and the breaker. */
export interface JudgeAt {
  calls: number
  failed: number
  skipped: number
  costMicros: number
  paused: boolean
  /** `closed`, or `open` after a `judge.circuit` that opened it. */
  breaker: string
}

const zeroJudge: JudgeAt = { calls: 0, failed: 0, skipped: 0, costMicros: 0, paused: false, breaker: 'closed' }

const zeroUsage = (): Usage => ({ input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 })

function addUsage(a: Usage, u: D | undefined): Usage {
  if (!u) return a
  const w1h = Number(u.cache_creation_1h_input_tokens ?? 0)
  return {
    input_tokens: a.input_tokens + Number(u.input_tokens ?? 0),
    output_tokens: a.output_tokens + Number(u.output_tokens ?? 0),
    cache_read_input_tokens: a.cache_read_input_tokens + Number(u.cache_read_input_tokens ?? 0),
    cache_creation_input_tokens: a.cache_creation_input_tokens + Number(u.cache_creation_input_tokens ?? 0),
    ...(w1h || a.cache_creation_1h_input_tokens ? { cache_creation_1h_input_tokens: (a.cache_creation_1h_input_tokens ?? 0) + w1h } : {}),
  }
}

const empty = (): Snap => ({
  index: 0, lives: new Map(), sessions: new Map(), execs: new Map(), acts: new Map(), asks: new Map(), tight: new Map(), jobs: new Set(),
  l1: new Set(), startedAt: null, stoppedAt: null, costTotal: 0, usageTotal: zeroUsage(), judge: zeroJudge,
})

const copy = (s: Snap): Snap => ({
  ...s, lives: new Map(s.lives), sessions: new Map(s.sessions), execs: new Map(s.execs), acts: new Map(s.acts), asks: new Map(s.asks),
  tight: new Map(s.tight), jobs: new Set(s.jobs), l1: new Set(s.l1),
})

const EXEC_STATES: Record<string, string> = {
  'execution.queued': 'queued', 'execution.running': 'running', 'execution.waiting': 'waiting',
  'execution.blocked': 'blocked', 'execution.complete': 'complete', 'execution.failed': 'failed',
  'execution.cancelled': 'cancelled', 'execution.interrupted': 'interrupted',
  // A unit budget's end, from before dollar budgets (a store keeps the row).
  'execution.budget_exhausted': 'budget_exhausted',
  // W1's /stop parks the execution on input.
  'execution.stopped': 'waiting',
}

const SETTLED: Record<string, string> = {
  'action.succeeded': 'succeeded', 'action.failed': 'failed', 'action.cancelled': 'cancelled',
  'action.declined': 'declined', 'action.denied': 'denied', 'action.expired': 'expired',
  'action.outcome_unknown': 'outcome_unknown', 'action.resolved': 'resolved',
}

const VERDICTS: Record<string, string> = {
  'action.cancel_verified': 'termination_verified', 'action.cancel_uncertain': 'outcome_uncertain',
  'action.cancel_unsupported': 'unsupported',
}

const sessionOf = (s: Snap, sid: string, at: number): SessAt =>
  s.sessions.get(sid) ?? { created: at, turns: 0, usage: zeroUsage(), cost: 0, toolCalls: 0, lastActive: at }

/** One row into the state: what it changes is replaced, never mutated. */
function step(s: Snap, r: LedgerEntry): void {
  const d = (r.data ?? {}) as D
  const at = r.at_unix_ms
  const sid = r.session_id ?? undefined
  const k = r.kind
  const exec = EXEC_STATES[k]
  if (exec) {
    const id = String(d.execution_id ?? '')
    const e = s.execs.get(id)
    if (!e) return
    const next: ExecAt = { ...e, state: exec, updated: at }
    if (k === 'execution.running' && typeof d.turn === 'number') next.turns = d.turn
    if (k === 'execution.waiting') next.wakeOn = typeof d.wake?.on === 'string' ? d.wake.on : 'input'
    if (k === 'execution.queued') next.why = typeof d.why === 'string' ? d.why : undefined
    if (k === 'execution.interrupted') next.interrupted = e.interrupted + 1
    if (typeof d.reason === 'string' && d.reason) next.endedReason = d.reason
    s.execs.set(id, next)
    return
  }
  const settled = SETTLED[k]
  if (settled) {
    const id = String(d.correlation_id ?? '')
    const a = s.acts.get(id)
    if (a) s.acts.set(id, { ...a, state: settled, settled: at })
    s.jobs.delete(id)
    s.asks.delete(id)
    return
  }
  const verdict = VERDICTS[k]
  if (verdict) {
    const id = String(d.correlation_id ?? '')
    const a = s.acts.get(id)
    const v: CancelVerdict = {
      correlation_id: id, tool: String(d.tool ?? a?.tool ?? ''), state: verdict, verified_by: String(d.verified_by ?? 'none'),
      ms: Number(d.ms ?? 0),
      ...(typeof d.killed === 'number' ? { killed: d.killed } : {}),
      ...(typeof d.survivors === 'number' ? { survivors: d.survivors } : {}),
      ...(typeof d.scope === 'string' ? { scope: d.scope } : {}),
      ...(typeof d.why === 'string' ? { why: d.why } : {}),
    }
    if (a) s.acts.set(id, { ...a, verdict: v })
    return
  }
  switch (k) {
    case 'session.superseded':
    case 'session.retired':
    case 'session.reopened':
      foldLife(s.lives, r)
      break
    case 'session.opened':
      if (sid && !s.sessions.has(sid)) s.sessions.set(sid, { ...sessionOf(s, sid, at), execId: typeof d.execution_id === 'string' ? d.execution_id : undefined })
      return
    case 'execution.opened': {
      const id = String(d.execution_id ?? '')
      if (!id || !sid) return
      // A record from before dollar budgets carried units: a million to the dollar.
      const limit = typeof d.limit_usd === 'number' ? d.limit_usd : typeof d.budget === 'number' ? d.budget / 1e6 : 0
      s.execs.set(id, {
        session: sid, kind: String(d.kind ?? 'conversation'), state: 'waiting', wakeOn: 'input', turns: 0, limit, spent: 0,
        resets: 0, created: at, updated: at, interrupted: 0, parent: typeof d.parent === 'string' ? d.parent : undefined,
      })
      const ss = sessionOf(s, sid, at)
      s.sessions.set(sid, { ...ss, execId: id })
      return
    }
    case 'turn.started':
      if (sid) {
        const ss = sessionOf(s, sid, at)
        s.sessions.set(sid, { ...ss, lastActive: at, profile: d.profile ?? ss.profile, model: d.model ?? ss.model })
      }
      return
    // The session's record counts the turns that ended or failed; one a crash cut is not counted.
    case 'turn.ended':
    case 'turn.failed':
      if (sid) {
        const ss = sessionOf(s, sid, at)
        s.sessions.set(sid, { ...ss, turns: ss.turns + 1, lastActive: at, toolCalls: ss.toolCalls + Number(d.tool_calls ?? 0) })
      }
      return
    case 'provider.call': {
      const cost = Number(d.cost_usd ?? 0)
      s.costTotal += cost
      s.usageTotal = addUsage(s.usageTotal, d.usage)
      if (!sid) return
      const ss = sessionOf(s, sid, at)
      s.sessions.set(sid, { ...ss, cost: ss.cost + cost, usage: addUsage(ss.usage, d.usage), lastActive: at })
      const e = ss.execId ? s.execs.get(ss.execId) : undefined
      if (e && ss.execId) s.execs.set(ss.execId, { ...e, spent: e.spent + cost })
      return
    }
    case 'action.planned': {
      const id = String(d.correlation_id ?? '')
      const eid = String(d.execution_id ?? '')
      if (!id) return
      // The row's retry class is tagged ({"class": "non_repeatable"}); the action list says its name.
      const retry = typeof d.retry_class === 'string' ? d.retry_class : String(d.retry_class?.class ?? '')
      s.acts.set(id, {
        exec: eid, session: sid ?? s.execs.get(eid)?.session ?? '', tool: String(d.tool ?? '?'), state: 'planned',
        retry, planned: at, deadline: Number(d.deadline_at_ms ?? 0), reserved: Number(d.reserved_usd ?? 0),
        confirmed: false,
      })
      return
    }
    case 'action.authorized': {
      const id = String(d.correlation_id ?? '')
      const a = s.acts.get(id)
      if (a) s.acts.set(id, { ...a, state: 'authorized', authorized: at, confirmed: !!d.confirmed })
      return
    }
    case 'action.dispatched': {
      const id = String(d.correlation_id ?? '')
      const a = s.acts.get(id)
      if (a) s.acts.set(id, { ...a, state: 'dispatched', dispatched: at, deadline: Number(d.deadline_at_ms ?? a.deadline) })
      return
    }
    case 'action.cancel': {
      const id = String(d.correlation_id ?? '')
      const a = s.acts.get(id)
      if (a) s.acts.set(id, { ...a, cancel: typeof d.state === 'string' ? d.state : 'requested' })
      return
    }
    case 'tool.confirm_requested': {
      const id = String(d.correlation_id ?? '')
      if (!id) return
      s.asks.set(id, {
        correlation_id: id, session_id: String(d.session_id ?? sid ?? ''), execution_id: String(d.execution_id ?? ''),
        tool: String(d.tool ?? '?'), input: d.input ?? null, reason: String(d.reason ?? ''), by: String(d.by ?? ''),
        requested_at_ms: Number(d.requested_at_ms ?? at), expires_at_ms: Number(d.expires_at_ms ?? 0), floor: !!d.floor,
        ...(typeof d.resource === 'string' ? { resource: d.resource } : {}),
        ...(d.task ? { task: d.task } : {}),
        ...(d.external_text ? { external_text: d.external_text } : {}),
      })
      return
    }
    case 'budget.asked': {
      const id = String(d.correlation_id ?? '')
      const eid = String(d.execution_id ?? '')
      if (!id) return
      s.asks.set(id, {
        correlation_id: id, session_id: sid ?? s.execs.get(eid)?.session ?? '', execution_id: eid, tool: 'budget.reset', input: null,
        reason: `the spend limit: $${Number(d.spent_usd ?? 0).toFixed(4)} of $${Number(d.limit_usd ?? 0).toFixed(2)}, $${Number(d.needed_usd ?? 0).toFixed(4)} more needed`,
        by: 'the kernel', requested_at_ms: at, expires_at_ms: 0, floor: false,
        budget: { spent_usd: Number(d.spent_usd ?? 0), limit_usd: Number(d.limit_usd ?? 0), needed_usd: Number(d.needed_usd ?? 0), lifetime_usd: 0 },
      })
      return
    }
    case 'action.confirm_answered':
    case 'action.confirmed':
      s.asks.delete(String(d.correlation_id ?? ''))
      return
    case 'session.external_read':
      if (sid) {
        const ss = sessionOf(s, sid, at)
        const hold: ExternalText = {
          since_ms: Number(d.since_ms ?? at), tool: String(d.tool ?? ''), url: String(d.url ?? ''), node_id: String(d.node_id ?? ''),
          ...(typeof d.from_session === 'string' ? { from_session: d.from_session } : {}),
          ...(typeof d.via === 'string' ? { via: d.via } : {}),
          ...(typeof d.query === 'string' ? { query: d.query } : {}),
        }
        s.sessions.set(sid, { ...ss, hold })
      }
      return
    case 'session.trusted':
      if (sid) {
        const ss = s.sessions.get(sid)
        if (ss) s.sessions.set(sid, { ...ss, hold: undefined })
      }
      return
    case 'policy.tightened':
      if (typeof d.tool === 'string') {
        s.tight.set(d.tool, {
          tool: d.tool, posture: String(d.posture ?? 'approve'), by: String(d.by ?? ''), who: String(d.who ?? ''), via: String(d.via ?? ''),
          at_ms: at, ...(typeof d.correlation_id === 'string' ? { correlation_id: d.correlation_id } : {}), ...(sid ? { session_id: sid } : {}),
          ...(typeof d.digest === 'string' ? { digest: d.digest } : {}),
        })
      }
      return
    case 'policy.untightened':
      if (typeof d.tool === 'string') s.tight.delete(d.tool)
      return
    case 'budget.reset': {
      const id = String(d.execution_id ?? '')
      const e = s.execs.get(id)
      if (e) s.execs.set(id, { ...e, spent: 0, resets: e.resets + 1 })
      return
    }
    case 'budget.limit_changed':
    case 'budget.reopened':
    case 'budget.migrated': {
      // A new limit; the move to dollar budgets (migrated) and a reopening also say the execution's state.
      const id = String(d.execution_id ?? '')
      const e = s.execs.get(id)
      if (!e) return
      const limit = Number(d.limit_usd ?? d.to_usd ?? NaN)
      const next: ExecAt = { ...e, updated: at }
      if (Number.isFinite(limit)) next.limit = limit
      if (typeof d.spent_usd === 'number' && k === 'budget.migrated') next.spent = d.spent_usd
      if (typeof d.state === 'string' && k !== 'budget.limit_changed') {
        next.state = d.state
        if (d.state === 'waiting') next.wakeOn = typeof d.wake?.on === 'string' ? d.wake.on : 'input'
      }
      if (k === 'budget.reopened') next.endedReason = undefined
      s.execs.set(id, next)
      return
    }
    case 'tool.job_started': {
      const id = String(d.correlation_id ?? '')
      s.jobs.add(id)
      if (d.class === 'l1') s.l1.add(id)
      return
    }
    case 'sandbox.started':
      if (d.class === 'l1' && typeof d.correlation_id === 'string') s.l1.add(d.correlation_id)
      return
    case 'server.started':
      s.startedAt = at
      s.stoppedAt = null
      return
    case 'server.serving':
      if (typeof d.serving_us === 'number') s.servingUs = d.serving_us
      return
    case 'server.stopping':
    case 'server.crashed':
      s.stoppedAt = at
      return
    case 'profile.changed':
      if (typeof d.live === 'string') s.profile = d.live
      return
    // Jev's judgments (M5 23b): the Judgment view's counts, cost, pause, and breaker as of the moment.
    case 'judge.call': {
      const o = String((d.outcome as D | undefined)?.outcome ?? '')
      const j = s.judge
      s.judge = {
        ...j, calls: j.calls + (o === 'skipped' ? 0 : 1), failed: j.failed + (o === 'failed' ? 1 : 0),
        skipped: j.skipped + (o === 'skipped' ? 1 : 0), costMicros: j.costMicros + Number(d.cost_micros ?? 0),
      }
      return
    }
    case 'judge.paused':
      // The notices' pause (step 24's notices) is not the shadow budget's.
      if (d.what !== 'notices') s.judge = { ...s.judge, paused: true }
      return
    case 'judge.resumed':
      s.judge = { ...s.judge, paused: false }
      return
    case 'judge.circuit': {
      // A breaker of its own (rerank's, 32d) is not the shared one.
      if (d.breaker) return
      const c = String((d.transition as D | undefined)?.circuit ?? '')
      s.judge = { ...s.judge, breaker: c === 'closed' ? 'closed' : 'open' }
      return
    }
  }
}

// ---------------------------------------------------------------- folding, with checkpoints

const EVERY = 300

export class Folder {
  private rows: LedgerEntry[] = []
  private times: number[] = []
  private marks: Snap[] = [empty()]
  /** The profile live before the first `profile.changed` row: that row's `previous`. */
  private firstProfile: string | undefined

  /** The ledger grew (or is new): keep the checkpoints that still hold. */
  update(rows: LedgerEntry[]): void {
    const same = rows.length >= this.rows.length && (this.rows.length === 0 || rows[this.rows.length - 1] === this.rows[this.rows.length - 1])
    if (!same) {
      this.marks = [empty()]
      this.firstProfile = undefined
    }
    this.rows = rows
    // Times rise with positions, but a row's clock may step back: keep a running maximum, so a search by time is
    // safe and a moment includes every row written before it.
    this.times = new Array(rows.length)
    let m = 0
    for (let i = 0; i < rows.length; i++) { m = Math.max(m, rows[i].at_unix_ms); this.times[i] = m }
    if (this.firstProfile === undefined) {
      const pc = rows.find((r) => r.kind === 'profile.changed')
      this.firstProfile = pc ? String(((pc.data ?? {}) as D).previous ?? '') || undefined : undefined
    }
  }

  get length() { return this.rows.length }
  get first() { return this.rows.length ? this.rows[0].at_unix_ms : null }

  /** How many rows lie at or before `t`. */
  count(t: number): number {
    let lo = 0
    let hi = this.times.length
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (this.times[mid] <= t) lo = mid + 1
      else hi = mid
    }
    return lo
  }

  /** The state after the rows at or before `t`. */
  at(t: number): Snap {
    const n = this.count(t)
    // The nearest checkpoint at or before n; fold forward, leaving new checkpoints on the way.
    let base = this.marks[Math.min(this.marks.length - 1, Math.floor(n / EVERY))]
    while (base.index + EVERY <= n && this.marks.length * EVERY <= n) {
      const s = copy(base)
      for (let i = s.index; i < s.index + EVERY; i++) step(s, this.rows[i])
      s.index += EVERY
      this.marks.push(s)
      base = s
    }
    const s = copy(base)
    for (let i = s.index; i < n; i++) step(s, this.rows[i])
    s.index = n
    if (s.profile === undefined) s.profile = this.firstProfile
    return s
  }
}

// ---------------------------------------------------------------- what the views read, as of a moment

export interface World {
  t: number
  sessions: SessionInfo[]
  executions: ExecutionInfo[]
  actions: ActionInfo[]
  confirms: ConfirmRequest[]
  tasks: TaskInfo[]
  holds: ExternalTextInfo[]
  tightenings: Tightening[]
  jobsRunning: Set<string>
  l1: Set<string>
  /** Reserved by each execution's calls in flight. */
  reserved: Map<string, number>
  /** Jev's judgments as of the moment (M5 23b). */
  judge: JudgeAt
  gauges: {
    /** Up since the last start, or null while the daemon was down. */
    uptimeSecs: number | null
    accepting: boolean
    running: number
    costTotal: number
    usageTotal: Usage
    profile?: string
    /** Tokens of the model calls in the minute before the moment. */
    tpm: number
  }
}

const TERMINAL = new Set(['complete', 'cancelled', 'failed', 'budget_exhausted'])

/** Each fold entry's ActionInfo, made once: an entry is replaced when its action changes, so an action that did not
 *  change between two moments is the same object in both, and a memoized row need not draw again. */
const actionInfo = new WeakMap<ActAt, ActionInfo>()

function infoOf(id: string, a: ActAt): ActionInfo {
  let info = actionInfo.get(a)
  if (!info) {
    info = {
      correlation_id: id, execution_id: a.exec, session_id: a.session, tool: a.tool, state: a.state, retry_class: a.retry,
      planned_at_ms: a.planned, deadline_at_ms: a.deadline, reserved_usd: a.settled ? 0 : a.reserved, confirmed: a.confirmed,
      completions_seen: a.settled ? 1 : 0,
      ...(a.authorized ? { authorized_at_ms: a.authorized } : {}),
      ...(a.dispatched ? { dispatched_at_ms: a.dispatched } : {}),
      ...(a.settled ? { settled_at_ms: a.settled } : {}),
      ...(a.cancel ? { cancel: a.cancel } : {}),
      ...(a.verdict ? { verdict: a.verdict } : {}),
    }
    actionInfo.set(a, info)
  }
  return info
}
const clip = (s: string, n = 60) => (s.length > n ? `${s.slice(0, n - 1)}…` : s)
const usd = (n: number) => `$${n.toFixed(n < 1 ? 4 : 2)}`

/** The server's attention rules (`theseus_protocol::attention`), on what the ledger knows. */
function attentionOf(e: ExecAt | undefined, asks: ConfirmRequest[], outstanding: number, since: number): Attention | undefined {
  if (!e) return undefined
  const budget = asks.find((a) => a.budget)
  if (budget) return { level: 'needs_you', label: `budget: ${usd(e.spent)} of ${usd(e.limit)}`, since_ms: since }
  if (asks.length) return { level: 'needs_you', label: clip(`confirm ${asks[0].tool}: ${asks[0].reason}`), since_ms: since }
  const reason = (w: string) => (e.endedReason ? clip(`${w}: ${e.endedReason}`) : w)
  switch (e.state) {
    case 'blocked': return { level: 'needs_you', label: reason('blocked'), since_ms: since }
    case 'failed': return { level: 'needs_you', label: reason('failed'), since_ms: since }
    case 'budget_exhausted': return { level: 'needs_you', label: 'budget exhausted', since_ms: since }
    case 'running': return { level: 'working', label: `turn ${Math.max(1, e.turns)}`, since_ms: since }
    case 'queued': return { level: 'working', label: e.why && e.why !== 'input' ? `queued · ${e.why}` : 'queued', since_ms: since }
    case 'waiting':
      if (e.wakeOn === 'input' && outstanding > 0) return { level: 'working', label: `waiting on ${outstanding} call${outstanding === 1 ? '' : 's'}`, since_ms: since }
      if (e.wakeOn === 'input') return { level: 'ready', label: 'ready', since_ms: since }
      if (e.wakeOn === 'confirm' || e.wakeOn === 'budget') return { level: 'needs_you', label: 'waiting on you', since_ms: since }
      return { level: 'working', label: `waiting on ${e.wakeOn ?? 'something'}`, since_ms: since }
    case 'complete':
    case 'cancelled': return { level: 'idle', label: e.state, since_ms: since }
    default: return { level: 'working', label: e.state, since_ms: since }
  }
}

/** The lists the views read, as they stood at `t`, from the fold and the live lists' fixed fields. */
export function worldAt(f: Folder, t: number, live: { sessions: SessionInfo[]; tasks: TaskInfo[]; rule?: StateRule }, calls: LedgerEntry[]): World {
  const s = f.at(t)
  const asksBySession = new Map<string, ConfirmRequest[]>()
  for (const a of s.asks.values()) {
    const l = asksBySession.get(a.session_id) ?? []
    l.push(a)
    asksBySession.set(a.session_id, l)
  }
  const outstanding = new Map<string, number>()
  const reserved = new Map<string, number>()
  const actions: ActionInfo[] = []
  for (const [id, a] of s.acts) {
    if (!a.settled) {
      if (a.dispatched) outstanding.set(a.exec, (outstanding.get(a.exec) ?? 0) + 1)
      if (a.reserved > 0) reserved.set(a.exec, (reserved.get(a.exec) ?? 0) + a.reserved)
    }
    actions.push(infoOf(id, a))
  }
  const executions: ExecutionInfo[] = []
  for (const [id, e] of s.execs) {
    const res = reserved.get(id) ?? 0
    const asks = asksBySession.get(e.session) ?? []
    const attention = attentionOf(e, asks, outstanding.get(id) ?? 0, e.updated)
    executions.push({
      execution_id: id, session_id: e.session, kind: e.kind, state: e.state, turns: e.turns, interrupted: e.interrupted,
      outstanding: outstanding.get(id) ?? 0, queued_results: 0, created_at_ms: e.created, updated_at_ms: e.updated,
      budget: {
        limit_usd: e.limit, spent_usd: e.spent, reserved_usd: res, held_unknown_usd: 0, available_usd: Math.max(0, e.limit - e.spent - res),
        resets: e.resets, ...(asks.find((a) => a.budget) ? { question: asks.find((a) => a.budget)!.correlation_id } : {}),
      },
      ...(e.endedReason && TERMINAL.has(e.state) ? { ended_reason: e.endedReason } : {}),
      ...(attention ? { attention } : {}),
    })
  }
  const execById = new Map(executions.map((e) => [e.execution_id, e]))
  const sessions: SessionInfo[] = []
  const holds: ExternalTextInfo[] = []
  for (const ls of live.sessions) {
    const fs = s.sessions.get(ls.session_id)
    const created = fs?.created ?? ls.created_at_unix_ms
    if (created > t) continue
    const e = fs?.execId ? execById.get(fs.execId) : ls.execution_id ? execById.get(ls.execution_id) : undefined
    const asks = asksBySession.get(ls.session_id) ?? []
    const info: SessionInfo = {
      ...ls,
      created_at_unix_ms: created,
      turns: fs?.turns ?? 0,
      usage: fs?.usage ?? zeroUsage(),
      cost_usd: fs?.cost ?? 0,
      tool_calls: fs?.toolCalls ?? 0,
      last_active_ms: fs?.lastActive ?? created,
      pending_confirms: asks.length,
      execution_state: e?.state ?? 'waiting',
      attention: e?.attention,
      external_text: fs?.hold,
      profile: fs?.profile,
      model: fs?.model,
    }
    if (!fs?.hold) delete info.external_text
    if (!e?.attention) delete info.attention
    // Its state then (theseus-emqx): the fold's rows, derived with the window `session.list` names.
    const life = s.lives.get(ls.session_id) ?? {}
    const st = deriveState({ retired: life.retired, turns: info.turns, created_ms: created, last_active_ms: info.last_active_ms, reopened_ms: life.reopened, busy: busyOf(info) }, live.rule ?? DEFAULT_RULE, t)
    info.state = st.state
    if (st.retired) info.retired = st.retired; else delete info.retired
    if (life.supersededBy) info.superseded_by = life.supersededBy; else delete info.superseded_by
    if (life.supersedes) info.supersedes = life.supersedes; else delete info.supersedes
    sessions.push(info)
    if (fs?.hold) {
      const task = live.tasks.find((x) => x.task_id === ls.session_id)
      holds.push({ session_id: ls.session_id, held: fs.hold, ...(ls.title ? { title: ls.title } : {}), ...(task ? { task: task.short } : {}) })
    }
  }
  holds.sort((a, b) => a.held.since_ms - b.held.since_ms)
  const tasks: TaskInfo[] = []
  for (const lt of live.tasks) {
    const e = execById.get(lt.execution_id)
    if (!e) continue
    const fs = s.sessions.get(lt.task_id)
    tasks.push({
      ...lt, state: e.state, spent_usd: e.budget.spent_usd, cost_usd: fs?.cost ?? 0, turns: fs?.turns ?? 0,
      pending_confirms: (asksBySession.get(lt.task_id) ?? []).length, updated_at_ms: e.updated_at_ms,
      ...(e.attention ? { attention: e.attention } : {}),
    })
  }
  // The gauges: up since the last start unless a stop came after it; tokens a minute from the calls before t.
  const up = s.startedAt !== null && (s.stoppedAt === null || s.stoppedAt < s.startedAt) ? Math.max(0, (t - s.startedAt) / 1000) : null
  let tpm = 0
  for (let i = calls.length - 1; i >= 0; i--) {
    const c = calls[i]
    if (c.at_unix_ms > t) continue
    if (c.at_unix_ms <= t - 60_000) break
    const u = ((c.data ?? {}) as D).usage as D | undefined
    if (u) tpm += Number(u.input_tokens ?? 0) + Number(u.output_tokens ?? 0) + Number(u.cache_read_input_tokens ?? 0) + Number(u.cache_creation_input_tokens ?? 0)
  }
  return {
    t, sessions, executions, actions, confirms: [...s.asks.values()], tasks, holds, tightenings: [...s.tight.values()],
    jobsRunning: s.jobs, l1: s.l1, reserved, judge: s.judge,
    gauges: {
      uptimeSecs: up, accepting: up !== null, running: executions.filter((e) => e.state === 'running').length, costTotal: s.costTotal,
      usageTotal: s.usageTotal, profile: s.profile, tpm,
    },
  }
}

// ---------------------------------------------------------------- the timeline's axis

/** The log's axis: busy stretches at their own pace, and each quiet stretch longer than `gapMs` folded into a short
 *  break of `gapPx`, so a week with one busy evening still gives that evening the track. Monotone both ways, so a
 *  drag through a break still moves the moment, only faster. */
export interface Axis {
  x(t: number): number
  at(px: number): number
  /** The breaks, for drawing: where each sits, and the quiet time it folds. */
  gaps: { x0: number; x1: number; from: number; to: number }[]
}

interface Seg { a: number; b: number; px0: number; w: number; pad: number }

export function axisOf(times: number[], start: number, end: number, width: number, gapMs = 15 * 60_000, gapPx = 18, minSegMs = 90_000): Axis {
  const spans: [number, number][] = []
  let a = start
  let b = start
  for (const t of times) {
    if (t < start || t > end) continue
    if (t - b > gapMs) { spans.push([a, b]); a = t; b = t } else b = t
  }
  if (end - b > gapMs) { spans.push([a, b]); a = end; b = end } else b = end
  spans.push([a, b])
  const lens = spans.map(([s, e]) => Math.max(e - s, minSegMs))
  const n = spans.length - 1
  const gp = n > 0 ? Math.min(gapPx, (width * 0.35) / n) : 0
  const k = Math.max(1e-9, (width - n * gp) / lens.reduce((x, y) => x + y, 0))
  const segs: Seg[] = []
  let px = 0
  spans.forEach(([s, e], i) => {
    const w = lens[i] * k
    segs.push({ a: s, b: e, px0: px, w, pad: (w - (e - s) * k) / 2 })
    px += w + gp
  })
  const gaps = segs.slice(0, -1).map((s, i) => ({ x0: s.px0 + s.w, x1: segs[i + 1].px0, from: s.b, to: segs[i + 1].a }))
  const x = (t: number): number => {
    if (t <= segs[0].a) return segs[0].px0 + segs[0].pad
    for (let i = 0; i < segs.length; i++) {
      const s = segs[i]
      if (t <= s.b) return s.px0 + s.pad + (t - s.a) * k
      const nx = segs[i + 1]
      if (nx && t < nx.a) {
        const g = gaps[i]
        return g.x0 + ((t - s.b) / Math.max(1, nx.a - s.b)) * (g.x1 - g.x0)
      }
    }
    const l = segs[segs.length - 1]
    return l.px0 + l.pad + (l.b - l.a) * k
  }
  const at = (p: number): number => {
    for (let i = 0; i < segs.length; i++) {
      const s = segs[i]
      if (p <= s.px0 + s.w) return Math.min(s.b, Math.max(s.a, s.a + (p - s.px0 - s.pad) / k))
      const g = gaps[i]
      if (g && p < g.x1) return g.from + ((p - g.x0) / Math.max(1e-9, g.x1 - g.x0)) * (g.to - g.from)
    }
    return segs[segs.length - 1].b
  }
  return { x, at, gaps }
}
