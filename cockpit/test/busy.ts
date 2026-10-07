// Ledgers for the watch's tests of its kept reads (theseus-qilc): a seeded random one, its rows' times jittered within
// the skew of their order, and a busy day's, 50,000 rows of turns and calls. Not a test file: `npm test` runs only
// `*.test.ts`.

export const HOUR = 3_600_000
export const MIN = 60_000

/** A seeded generator, so a failure replays. */
export function seeded(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

export interface Row { position: number; at_unix_ms: number; kind: string; session_id: string | null; turn_id: string | null; data: Record<string, unknown> }

/** `n` rows from `start`, about a minute apart (2,400 make a day and more), as the ledger writes them: turns in a
 *  handful of sessions, model calls, tool calls (a job's argv, some running for hours, some failing, some outcomes
 *  unknown and then resolved), questions asked and answered or expired, tasks opened and ended. Each row's time is
 *  jittered by up to `jitter` ms from its place, so some rows land after rows that follow them: never by more than the
 *  skew (`SKEW_MS`), as the ledger's rows never are. */
export function randomLedger(seed: number, n: number, start: number, jitter = 2.4 * MIN): Row[] {
  const r = seeded(seed)
  const pick = <T>(xs: readonly T[]) => xs[Math.floor(r() * xs.length)]
  const rows: Row[] = []
  let t = start
  let k = 0
  const sessions = ['ses_a', 'ses_b', 'ses_c', 'ses_d', 'ses_e']
  const open: { cid: string; sid: string; tool: string; at: number }[] = []
  const add = (kind: string, sid: string | null, data: Record<string, unknown> = {}, turn: string | null = null) =>
    rows.push({ position: rows.length + 1, at_unix_ms: Math.round(t + (r() - 0.5) * 2 * jitter), kind, session_id: sid, turn_id: turn, data })
  while (rows.length < n) {
    t += Math.floor(r() * 120_000)
    const sid = pick(sessions)
    const turn = `turn_${k}`
    const x = r()
    if (x < 0.15) {
      add('turn.started', sid, {}, turn)
      add('provider.call', sid, { cost_usd: Math.round(r() * 1e6) / 1e8, node_id: `nod_${k}` }, turn)
      add(r() < 0.05 ? 'turn.failed' : 'turn.ended', sid, { elapsed_ms: Math.floor(r() * 60_000), reason: 'it failed' }, turn)
    } else if (x < 0.45) {
      const cid = `act_${k}`
      const tool = pick(['proc.run', 'proc.run', 'fs.read', 'fs.write', 'provider.anthropic'])
      add('action.planned', sid, { correlation_id: cid, tool })
      if (tool === 'proc.run') add('tool.job_started', sid, { correlation_id: cid, argv: pick([['sh', '-c', `sleep ${k % 9}`], ['cargo', 'build'], ['sh', '-c', 'echo hi; make']]), class: pick(['l1', 'l0']) })
      open.push({ cid, sid, tool, at: t })
    } else if (x < 0.7 && open.length) {
      // A call settles, sometimes long after it began.
      const c = open.splice(Math.floor(r() * open.length), 1)[0]
      const y = r()
      if (y < 0.08) {
        add('action.outcome_unknown', c.sid, { correlation_id: c.cid, producer: 'reconciler:overdue_no_evidence' })
        if (r() < 0.5) add('action.resolved', c.sid, { correlation_id: c.cid, outcome: 'succeeded' })
      } else {
        add(y < 0.2 ? 'action.failed' : 'action.succeeded', c.sid, { correlation_id: c.cid, duration_ms: Math.max(1, Math.round(t - c.at)), producer: 'inproc:x' })
      }
    } else if (x < 0.8) {
      const cid = `ask_${k}`
      add(r() < 0.3 ? 'budget.asked' : 'tool.confirm_requested', sid, { correlation_id: cid, tool: 'fs.write', reason: 'create notes.md: fs.write — approve', needed_usd: 0.5, limit_usd: 1 })
      if (r() < 0.7) add(pick(['action.confirm_answered', 'action.declined', 'action.expired']), sid, { correlation_id: cid, approved: r() < 0.5, by: pick(['cli', 'discord:owner', undefined]), via: 'cli' })
    } else if (x < 0.86) {
      add('execution.opened', sid, { execution_id: `exe_${k}`, kind: pick(['conversation', 'task']) })
      if (r() < 0.5) add('task.ended', sid, { task: `exe_${k}`, state: pick(['complete', 'failed']), reason: 'its budget ran out' })
    } else {
      add('provider.call', sid, { cost_usd: Math.round(r() * 1e6) / 1e8, node_id: `nod_${k}` })
    }
    k++
  }
  return rows.slice(0, n)
}

/** A busy day's ledger: `n` rows over 26 hours ending at `end`, in the ledger's order, as turns with zero to three
 *  tool calls each write it (50,000 rows a day is the busy day the watch is held to). */
export function busyDay(n: number, end: number): Row[] {
  const rows: Row[] = []
  let c = 0
  const add = (kind: string, sid: string, turn: string | null, data: Record<string, unknown>) =>
    rows.push({ position: rows.length + 1, at_unix_ms: 0, kind, session_id: sid, turn_id: turn, data })
  for (let t = 0; rows.length < n; t++) {
    const sid = `ses_${t % 40}`
    const turn = `turn_${t}`
    if (t % 97 === 0) add('execution.opened', sid, null, { execution_id: `exe_${t}`, kind: t % 3 ? 'conversation' : 'task' })
    add('turn.started', sid, turn, {})
    add('provider.call', sid, turn, { cost_usd: 0.0004 + (t % 13) * 0.0003, node_id: `nod_${t}a` })
    for (let j = 0; j < t % 4; j++) {
      const cid = `act_${c++}`
      const job = c % 3 === 0
      add('action.planned', sid, turn, { correlation_id: cid, tool: job ? 'proc.run' : c % 3 === 1 ? 'fs.read' : 'fs.write' })
      if (c % 41 === 0) {
        add('tool.confirm_requested', sid, turn, { correlation_id: cid, tool: 'fs.write', reason: 'create notes.md: fs.write — approve' })
        add('action.confirm_answered', sid, turn, { correlation_id: cid, approved: true, by: 'cli', via: 'cli' })
      }
      if (job) add('tool.job_started', sid, turn, { correlation_id: cid, argv: ['sh', '-c', c % 2 ? `cargo build -p crate${c % 5}` : `sleep ${c % 9}`], class: c % 4 ? 'l1' : 'l0' })
      add('action.dispatched', sid, turn, { correlation_id: cid })
      add(c % 50 ? 'action.succeeded' : 'action.failed', sid, turn, { correlation_id: cid, duration_ms: job ? 2000 + (c * 7919) % 60000 : 5 + (c * 31) % 200 })
    }
    add('provider.call', sid, turn, { cost_usd: 0.0002 + (t % 7) * 0.0001, node_id: `nod_${t}b` })
    add(t % 300 === 7 ? 'turn.failed' : 'turn.ended', sid, turn, { elapsed_ms: 1500 + (t * 104729) % 40000, reason: 'unpriced' })
  }
  rows.length = n
  rows.forEach((x, i) => { x.at_unix_ms = end - 26 * HOUR + Math.floor((i / n) * 26 * HOUR) })
  return rows
}

/** A value with its floats to nine figures and its maps and sets as lists: two reads compare by what they say, not by
 *  the order their cents were added in. */
export const said = (x: unknown): unknown => JSON.parse(JSON.stringify(x, (_k, v) => (
  typeof v === 'number' && !Number.isInteger(v) ? Number(v.toPrecision(9))
    : v instanceof Map ? [...v] : v instanceof Set ? [...v] : typeof v === 'function' ? undefined : v)))
