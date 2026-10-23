// The inspector's sections, as facts (theseus-jid7t.6): a task's or conversation's numbers, its Record, what stands
// for it, its last turns, and a job's command and eta. Pure: the components draw these lines and nothing else, and
// `test/ops-sections.test.ts` runs them over invented fixtures.
import type { NodeInfo, StandingInfo, TaskRecord, WorkForecast } from '@protocol'
import { ago, clock, ms, short, usd } from '../lib/figures.ts'
import type { OpsRow } from './board.ts'

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
