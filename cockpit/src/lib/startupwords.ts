// The last start and the store, in words (theseus-vm3n.6): what each startup phase found, how much of the start path no
// phase names, and what `theseus health` says loudly about a corrupt history, the records a corrupt frame made list reads
// skip, and the newest crash. Pure, with no import but the protocol's types, so `node --test` runs its test.
import type { CrashStatus, StartupPhase, StoreStatus } from '@protocol'
import { us } from './figures.ts'

const fmt = (n: number): string => n.toLocaleString()

/** What a phase found, from its detail: the outcome, the state, the method, the position reached, what failed. */
export function phaseOutcome(p: StartupPhase): string {
  const d = (p.detail ?? {}) as Record<string, unknown>
  const parts: string[] = []
  if (typeof d.outcome === 'string') parts.push(d.outcome)
  if (typeof d.state === 'string') parts.push(d.state)
  if (typeof d.method === 'string') parts.push(d.method)
  if (typeof d.secret === 'string') parts.push(`secret ${d.secret}`)
  if (typeof d.waited_ms === 'number') parts.push(`waited ${fmt(d.waited_ms)} ms`)
  if (typeof d.source === 'string') parts.push(`from ${d.source}`)
  if (typeof d.last_position === 'number') parts.push(`${fmt(d.last_position)} positions`)
  if (typeof d.replayed_into_index === 'number' && d.replayed_into_index > 0) parts.push(`${fmt(d.replayed_into_index)} replayed`)
  if (typeof d.login === 'string') parts.push(d.login)
  if (typeof d.error === 'string') parts.push(d.error)
  if (Array.isArray(d.failed) && d.failed.length > 0) parts.push(`failed: ${d.failed.join(', ')}`)
  if (Array.isArray(d.steps)) parts.push((d.steps as { name: string; us: number }[]).map((s) => `${s.name} ${us(s.us)}`).join(' · '))
  return parts.join(' · ')
}

/** When the start path ended (serving), and the part of it no phase names: a slow start that shows an unnamed gap has an
 *  unnamed cause. */
export function startFigures(phases: StartupPhase[]): { serving: number; between: number; slow: boolean } {
  const path = phases.filter((p) => !p.background && p.end_us != null)
  const serving = path.length ? Math.max(...path.map((p) => p.end_us as number)) : 0
  const named = path.reduce((t, p) => t + ((p.end_us as number) - p.start_us), 0)
  const between = Math.max(0, serving - named)
  return { serving, between, slow: between > serving / 4 }
}

export interface StoreLine { tone: 'fault' | 'idle'; text: string }

/** The loud lines about the store and the last crash: a corrupt history check, the records skipped and what repairs them,
 *  and the newest crash a start found (a fault when it ended the last run). */
export function storeLines(phases: StartupPhase[], store?: StoreStatus, crash?: CrashStatus): StoreLine[] {
  const out: StoreLine[] = []
  const verify = phases.find((p) => p.name === 'store.verify')
  if (verify?.detail?.outcome === 'corrupt') {
    out.push({ tone: 'fault', text: `store: CORRUPT history, ${String(verify.detail.error ?? '?')}; reads from there are refused` })
  }
  if (store && store.refused_records > 0) {
    const more = store.refused_positions.length < store.refused_records ? ', …' : ''
    out.push({
      tone: 'fault',
      text: `store: ${fmt(store.refused_records)} record${store.refused_records === 1 ? '' : 's'} skipped by list reads, their frame corrupt (positions ${store.refused_positions.join(', ')}${more})${store.repair ? ` · ${store.repair}` : ''}`,
    })
  }
  if (crash) {
    out.push({
      tone: crash.this_start ? 'fault' : 'idle',
      text: `crash: ${new Date(crash.at_unix_ms).toISOString()} at ${crash.location} (thread ${crash.thread}, pid ${crash.pid}, ${crash.version})${crash.this_start ? ', which ended the last run' : ''} · ${crash.file}`,
    })
  }
  return out
}
