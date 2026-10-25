// The inspector's sections, as facts (theseus-jid7t.6): a task's or conversation's numbers, its Record, what stands
// for it, its last turns, and a job's command and eta. Pure: the components draw these lines and nothing else, and
// `test/ops-sections.test.ts` runs them over invented fixtures.
import type { NodeInfo, StandingInfo, TaskRecord, WorkForecast } from '@protocol'
import { ago, clock, ms, short, usd } from '../lib/figures.ts'
import { alive, counterLabel, selects, type AllCounters, type OpsRow } from './board.ts'
import type { Bucket } from './address.ts'
import { bucketGlyph } from './state.ts'

export interface FactLine { label: string; value: string; tone?: 'money' | 'wait' | 'fault' | 'ok' | 'think' | 'dim' }

/** The forecast as the work board words it: dollars and minutes at the median and the 90th percentile. */
export function forecastWords(f: WorkForecast): string {
  const past = f.past ? ' · past its p90' : ''
  return `${usd(f.usd_p50)}–${usd(f.usd_p90)} · ${Math.round(f.min_p50)}–${Math.round(f.min_p90)} min · from ${f.n} finished${past}${f.read ? ` · Jev reads it as ${f.read}` : ''}`
}

/** The prompt cache lives five minutes past the last call: a session quiet longer is cold. Said as an estimate. */
export const CACHE_WARM_MS = 5 * 60_000
export const cacheWords = (lastActiveMs: number, now: number): string => (now - lastActiveMs <= CACHE_WARM_MS ? 'warm (active in the last 5 min)' : 'cold (quiet for over 5 min)')

/** A task's or conversation's facts: spend of limit, turns, the forecast, the cache, who it reports to, whether it
 *  wakes its parent, the pieces it was given, and its check. A line with no data is absent. */
export function taskFacts(r: OpsRow, now: number): FactLine[] {
  const out: FactLine[] = []
  const info = r.task?.info
  const cost = r.view?.cost ?? (info ? { spent_usd: info.spent_usd, limit_usd: info.limit_usd } : undefined)
  if (cost) out.push({ label: 'spend', value: `${usd(cost.spent_usd)} of ${usd(cost.limit_usd)}`, tone: 'money' })
  const turns = info?.turns ?? r.session?.turns
  if (turns != null) out.push({ label: 'turns', value: `${turns}${r.view?.progress ? ` · step ${r.view.progress.steps_done} of ${r.view.progress.steps_total}` : ''}` })
  if (r.view?.forecast) out.push({ label: 'forecast', value: forecastWords(r.view.forecast), tone: r.view.forecast.past ? 'wait' : 'dim' })
  if (r.session) out.push({ label: 'cache', value: cacheWords(r.session.last_active_ms, now), tone: 'dim' })
  if (info?.parent_session_id) out.push({ label: 'reports to', value: short(info.parent_session_id) })
  if (info?.wake_parent) out.push({ label: 'wakes parent', value: 'when it reports' })
  const pieces = info?.arrangement?.pieces.length
  if (pieces) out.push({ label: '📎 pieces', value: `${pieces} quoted into its brief` })
  if (info?.check) out.push({ label: 'check', value: `of ${info.check.checked_short} · ${info.check.admitted.length} pieces admitted · on ${info.check.profile}`, tone: 'think' })
  if (r.session?.data && r.session.data !== 'open') out.push({ label: 'data', value: r.session.data, tone: 'wait' })
  return out
}

export interface RecordFacts {
  lines: FactLine[]
  objective: string
  acceptance: string[]
  /** The change a waiting proposal would make, for its card. */
  waiting?: { by: string; card: string }
}

/** A task's Record (the three layers): claim, version, owner, deps, evidence, objective, acceptance, a waiting change. */
export function recordFacts(rec: TaskRecord | undefined, now: number): RecordFacts | null {
  if (!rec) return null
  const lines: FactLine[] = [
    { label: 'record', value: `${rec.id} · v${rec.version} · ${rec.state}` },
    { label: 'owner', value: rec.owner },
  ]
  if (rec.claim) lines.push({ label: 'claim', value: `${rec.claim.by} until ${clock(rec.claim.until_ms)}${rec.claim.until_ms < now ? ' (lapsed)' : ''}`, tone: rec.claim.until_ms < now ? 'wait' : undefined })
  if (rec.deps.length) lines.push({ label: 'waits on', value: rec.deps.join(', ') })
  if (rec.evidence.length) lines.push({ label: 'evidence', value: `${rec.evidence.length} · the last ${ago(rec.evidence[rec.evidence.length - 1].at_ms, now)} by ${rec.evidence[rec.evidence.length - 1].by}` })
  return { lines, objective: rec.objective, acceptance: rec.acceptance, waiting: rec.proposal ? { by: rec.proposal.by, card: rec.proposal.card } : undefined }
}

/** The standing records a session carries, live ones first: the kind, its words and its scope's. */
export function standingLines(records: readonly StandingInfo[]): Array<{ id: string; kind: string; text: string; scope: string }> {
  return records.filter((r) => r.state === 'live' || r.state === 'active').map((r) => ({ id: r.id, kind: r.kind, text: r.text, scope: r.scope.kind === 'everywhere' ? 'everywhere' : `${r.scope.kind} ${r.scope.of}` }))
}

/** The last `n` turns' messages, oldest first: who said it and the start of what was said. */
export function transcriptTail(nodes: readonly NodeInfo[], n = 6): Array<{ id: string; who: 'you' | 'theseus'; text: string; at: number }> {
  return nodes.filter((x) => (x.kind === 'user_message' || x.kind === 'assistant_message') && x.text.trim())
    .slice(-n).map((x) => ({ id: x.node_id, who: x.kind === 'user_message' ? 'you' as const : 'theseus' as const, text: x.text.trim().slice(0, 600), at: x.at_unix_ms }))
}

/** A job's command and its time against its usual (the work board's `eta`). */
export function jobFacts(r: OpsRow, now: number): FactLine[] {
  const v = r.view
  const out: FactLine[] = [{ label: 'command', value: r.title }]
  if (v) {
    const ran = now - v.started_at_ms
    out.push({ label: 'running', value: v.ended_at_ms ? `ran ${ms(v.ended_at_ms - v.started_at_ms)}` : `${ms(ran)}${v.eta ? ` of a usual ${ms(v.eta.usual_ms)} (${v.eta.runs} runs)` : ''}`, tone: v.eta && !v.ended_at_ms && ran > v.eta.usual_ms * 2 ? 'wait' : undefined })
    if (r.sessionId) out.push({ label: 'session', value: short(r.sessionId) })
  }
  return out
}

/** The job to follow for a session: its first running job on the board. */
export function runningJob(rows: readonly OpsRow[], sessionId: string | undefined): OpsRow | undefined {
  return sessionId ? rows.find((r) => r.kind === 'job' && r.sessionId === sessionId && alive(r)) : undefined
}

/** The newest turn that ended in a session, from the ledger's rows: what `I` inspects. */
export function lastTurnOf(rows: ReadonlyArray<{ kind: string; session_id?: string | null; turn_id?: string | null }>, sessionId: string): string | undefined {
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    if (r.session_id === sessionId && r.turn_id && (r.kind === 'turn.ended' || r.kind === 'turn.failed')) return r.turn_id
  }
  return undefined
}

/** Which body the inspector draws for a row: a job's, a step's or plan's record, else the card's whole. */
export const bodyOf = (r: OpsRow): 'job' | 'step' | 'row' => (r.kind === 'job' ? 'job' : r.kind === 'step' ? 'step' : 'row')

/** The state glyph of a bucket: the fallback when `bucketGlyph` has no opinion. */
export const GLYPH: Readonly<Record<OpsRow['bucket'], string>> = { needs: '●', working: '◐', scheduled: '◷', paused: '⏸', todo: '○', rest: '·', ended: '✓' }

export interface StepView {
  noun: 'plan' | 'step'
  glyph: string
  stateWord: string
  short: string
  /** thread › plan › … above the row, nearest last; each a row the breadcrumb selects, or only words. */
  crumbs: Array<{ id?: string; title: string }>
  /** A plan's closed children against all of them. */
  progress?: { n: number; m: number }
  children: Array<{ id: string; title: string; glyph: string; stateWord: string; closed: boolean }>
  /** Created, became a plan, closed. */
  history: FactLine[]
  /** The ledger rows of its record. */
  ledger?: string
  /** The row that carries it when it has no task of its own: its conversation (or the task above). */
  carrier?: { id: string; title: string }
  /** The task row attached to it, when there is one: its sections, verbs and Live are the task's. */
  task?: OpsRow
}

const stateWords = (s: string | undefined): string => (s ?? 'unknown').replace(/_/g, ' ')

/** A step's or plan's facts: noun, state, breadcrumb, its children as a checklist, history, and who carries it. */
export function stepView(r: OpsRow, rows: readonly OpsRow[], now: number): StepView {
  const rowOf = (id: string | undefined): OpsRow | undefined => (id ? rows.find((x) => selects(x, id)) : undefined)
  const crumbs: StepView['crumbs'] = []
  const seen = new Set<string>([r.id])
  let up = rowOf(r.parent)
  for (let i = 0; up && !seen.has(up.id) && i < 16; i++) { seen.add(up.id); crumbs.unshift({ id: up.id, title: up.title }); up = rowOf(up.parent) }
  if (r.thread) crumbs.unshift({ title: r.thread })
  const kids = rows.filter((c) => c.id !== r.id && (c.parent === r.id || (r.recordId != null && c.record?.parent === r.recordId)))
  const closed = (c: OpsRow) => c.bucket === 'ended'
  const rec = r.record
  const history: FactLine[] = []
  if (rec) history.push({ label: 'created', value: ago(rec.created_at_ms, now) })
  const firstKid = Math.min(...kids.map((c) => c.record?.created_at_ms ?? Infinity))
  if (r.plan && kids.length) history.push({ label: 'became a plan', value: Number.isFinite(firstKid) ? `its first sub-step was added ${ago(firstKid, now)}` : `${kids.length} sub-steps` })
  if (r.bucket === 'ended') history.push({ label: 'closed', value: ago(r.endedMs ?? rec?.updated_at_ms ?? r.activityMs, now), tone: 'ok' })
  const sid = r.sessionId ?? r.record?.session
  const task = r.task ? r : sid ? rows.find((x) => x.kind === 'task' && x.sessionId === sid) : undefined
  const carrier = r.carrier ? rowOf(r.carrier) : undefined
  return {
    noun: r.plan ? 'plan' : 'step', glyph: bucketGlyph(r.bucket, r.state ?? '') ?? GLYPH[r.bucket], stateWord: stateWords(r.state), short: r.short, crumbs,
    progress: r.plan ? { n: kids.filter(closed).length, m: kids.length } : undefined,
    children: kids.map((c) => ({ id: c.id, title: c.title, glyph: bucketGlyph(c.bucket, c.state ?? '') ?? GLYPH[c.bucket], stateWord: stateWords(c.state), closed: closed(c) })),
    history, ledger: rec ? `/ledger?q=${rec.id}` : undefined, carrier: carrier ? { id: carrier.id, title: carrier.title } : undefined, task,
  }
}

/** A taskless step is carried by a conversation: "carried by ‹title›". */
export const carriedWords = (v: StepView): string => (v.carrier ? `carried by ${v.carrier.title}` : 'carried by no one yet')

/** The empty inspector's words: what to do, the counts each a filter, and the keys. */
export const NONE_WORDS = 'Nothing is selected. Click a card, or press j to take the first.'
export const NONE_KEYS: ReadonlyArray<{ keys: string; words: string }> = [
  { keys: 'j k', words: 'down and up the queue' }, { keys: 'h l', words: 'across the graph or board' }, { keys: 'esc ]', words: 'fold this panel' }, { keys: '?', words: 'every key' },
]

/** The empty inspector's counts, one a link that sets that filter: the bar's counters but the dollars. */
export function noneCounts(c: AllCounters): Array<{ b: Bucket; label: string; n: number }> {
  return (['needs', 'working', 'paused', 'still', 'done'] as const).map((b) => ({ b, label: counterLabel(b), n: c[b] }))
}

/** The row an address's `sel` (a graph node's id, a Board card's, a breadcrumb's) names, in the page's whole work set:
 *  never a filtered one, so a card the counter hides is still selectable and clearing the filter drops nothing. */
export const rowFor = (rows: readonly OpsRow[], sel: string | null): OpsRow | undefined => rows.find((r) => selects(r, sel))
