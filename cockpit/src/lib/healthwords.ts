// The header's health lamps (theseus-hnof.5): every system the bar watches, as a lamp with its state in a word beside
// it, and the sentence that names the ones that need a look. The old bar drew seven coloured dots with their systems'
// names, and the state only in the colour; the link's round trip and the provider's errors were readouts of their own.
// Now each lamp says its state ("65 ms", "open", "1 error", "off", "dev page"), its card says it whole (the old dots'
// tooltips, word for word), and a narrow bar keeps the dots and says the worst of them in words.
//
// Pure, with only types imported, so `node --test` runs it (test/healthwords.test.ts).
import type { BinaryStatus, DiskStatus, Health } from '@protocol'
import type { Tone } from './taxonomy'

/** One lamp: which system, its name, its tone, its state in a word or two, and the whole of it in a sentence. */
export interface Lamp { id: string; name: string; tone: Tone; word: string; detail: string }

/** The link to the daemon, as the page's socket sees it: open with its median round trip, connecting, or down. */
export interface Link { status: string; rtt: number | null }

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`

/** Free space, short: 512 MB, 3.4 GB, 182 GB. Health counts whole MB (MiB). */
export function freeWords(mb: number): string {
  if (mb < 1024) return `${Math.max(0, Math.round(mb))} MB`
  const gb = mb / 1024
  return `${gb < 10 ? gb.toFixed(1) : Math.round(gb).toLocaleString('en-US')} GB`
}

export function linkLamp(l: Link): Lamp {
  if (l.status === 'open') {
    const rtt = l.rtt !== null ? `${Math.round(l.rtt)} ms` : null
    return { id: 'link', name: 'link', tone: 'ok', word: rtt ?? 'open',
      detail: `the link to the daemon: open${rtt ? `, ${rtt} round trip (the median of the last 12 pings)` : ''}` }
  }
  if (l.status === 'connecting') return { id: 'link', name: 'link', tone: 'wait', word: 'connecting', detail: 'the link to the daemon: connecting' }
  return { id: 'link', name: 'link', tone: 'fault', word: 'down', detail: `the link to the daemon: ${l.status || 'down'}; the views show what they last read` }
}

export function kernelLamp(h: Health | undefined): Lamp {
  if (!h) return { id: 'kernel', name: 'kernel', tone: 'idle', word: '—', detail: 'kernel: not read yet' }
  const k = h.kernel
  const running = k.executions_by_state.running ?? 0
  const more = ` · ${running} running of ${k.admission_ceiling} at once · ${plural(k.turns_held, 'turn')} held`
  return k.accepting
    ? { id: 'kernel', name: 'kernel', tone: 'ok', word: 'open', detail: `kernel accepting${more}` }
    : { id: 'kernel', name: 'kernel', tone: 'wait', word: 'holding', detail: `kernel holding new turns${more}` }
}

export function providerLamp(h: Health | undefined): Lamp {
  if (!h) return { id: 'provider', name: 'provider', tone: 'idle', word: '—', detail: 'provider errors: not read yet' }
  const n = h.provider_errors
  const what = "model calls the provider failed since the daemon started; the ledger's provider.error rows say each"
  return n > 0
    ? { id: 'provider', name: 'provider', tone: 'fault', word: plural(n, 'error'), detail: `${plural(n, 'provider error')}: ${what}` }
    : { id: 'provider', name: 'provider', tone: 'ok', word: 'no errors', detail: `no provider error: ${what}` }
}

/** Discord's binding: ready is well; disabled or not set up is off; connecting or resuming waits; down faults. */
export function discordLamp(h: Health | undefined): Lamp {
  const d = h?.bindings?.find((b) => b.kind === 'discord')
  if (!d) return { id: 'discord', name: 'discord', tone: 'idle', word: 'none', detail: 'Discord: no binding' }
  const detail = `Discord ${d.state}${d.latency_ms ? ` · ${d.latency_ms} ms` : ''}${d.detail ? ` · ${d.detail}` : ''}`
  switch (d.state) {
    case 'ready': return { id: 'discord', name: 'discord', tone: 'ok', word: d.latency_ms ? `${d.latency_ms} ms` : 'ready', detail }
    case 'disabled': case 'unconfigured': return { id: 'discord', name: 'discord', tone: 'idle', word: 'off', detail }
    case 'connecting': case 'resuming': return { id: 'discord', name: 'discord', tone: 'wait', word: d.state, detail }
    default: return { id: 'discord', name: 'discord', tone: 'fault', word: d.state, detail }
  }
}

/** The config: confirmed is well; confirming, held, or restarting waits. */
export function configLamp(h: Health | undefined): Lamp {
  const c = h?.config
  if (!c) return { id: 'config', name: 'config', tone: 'idle', word: '—', detail: 'config: not reported' }
  const detail = `config ${c.state} (${c.source})`
  return c.state === 'confirmed'
    ? { id: 'config', name: 'config', tone: 'ok', word: 'ok', detail }
    : { id: 'config', name: 'config', tone: 'wait', word: c.state, detail }
}

/** The vault's secrets: ready, resolving, or failed (naming how many). */
export function secretsLamp(h: Health | undefined): Lamp {
  const s = h?.secrets
  if (!s) return { id: 'secrets', name: 'secrets', tone: 'idle', word: '—', detail: 'secrets: not reported' }
  const detail = `secrets ${s.state} · ${plural(s.ready.length, 'ready', 'ready')}${s.resolving.length ? ` · ${s.resolving.length} resolving` : ''}${s.failed.length ? ` · failed: ${s.failed.map((f) => f.name).join(', ')}` : ''}`
  if (s.state === 'ready') return { id: 'secrets', name: 'secrets', tone: 'ok', word: 'ready', detail }
  if (s.state === 'failed' || s.failed.length) return { id: 'secrets', name: 'secrets', tone: 'fault', word: `${s.failed.length || ''} failed`.trim(), detail }
  return { id: 'secrets', name: 'secrets', tone: 'wait', word: s.state, detail }
}

/** The web UI's door: a fault when any local user is served (no owner check), a wait while a dev page is let in,
 *  after a refusal, or on a build with no owner check; otherwise ok. A build with the check (theseus-3qf) always
 *  reports `refused_peer`. */
export function webTone(web: Health['web'] | undefined): Tone {
  if (!web) return 'idle'
  if (web.peer_unchecked) return 'fault'
  if (web.refused_peer === undefined || web.dev_origin || web.refused_host + web.refused_origin + web.refused_peer > 0) return 'wait'
  return 'ok'
}

export function webTitle(web: Health['web'] | undefined): string {
  if (!web) return 'web UI: not reported'
  const parts = [`web UI refused ${web.refused_host} by address, ${web.refused_origin} by page, ${web.refused_peer ?? 0} by user`]
  if (web.refused_peer === undefined) parts.push('no owner check in this build: any local user is served')
  if (web.peer_unchecked) parts.push(`owner check off: ${web.peer_unchecked}`)
  if (web.dev_origin) parts.push(`dev origin open: ${web.dev_origin} (${web.dev_origin_served ?? 0} served)`)
  return parts.join(' · ')
}

export function webLamp(h: Health | undefined): Lamp {
  const w = h?.web
  const tone = webTone(w)
  const refused = w ? w.refused_host + w.refused_origin + (w.refused_peer ?? 0) : 0
  const word = !w ? '—'
    : w.peer_unchecked ? 'unguarded'
    : w.refused_peer === undefined ? 'no owner check'
    : w.dev_origin ? 'dev page'
    : refused > 0 ? `${refused} refused`
    : 'guarded'
  return { id: 'web', name: 'web', tone, word, detail: webTitle(w) }
}

/** The binary this daemon runs: protected (jobs cannot write it), writable by jobs, or unknown. */
export function binaryLamp(b: BinaryStatus | undefined, line: string): Lamp {
  switch (b?.state) {
    case 'ok': return { id: 'binary', name: 'binary', tone: 'ok', word: 'safe', detail: line }
    case 'jobs_can_write': return { id: 'binary', name: 'binary', tone: 'fault', word: 'jobs can write', detail: line }
    case 'unknown': return { id: 'binary', name: 'binary', tone: 'wait', word: 'unknown', detail: line }
    default: return { id: 'binary', name: 'binary', tone: 'idle', word: '—', detail: line }
  }
}

/** The disk under the state dir: its free space, low, below the floor (new jobs refused), or unknown. */
export function diskLamp(d: DiskStatus | undefined, summary: string): Lamp {
  switch (d?.state) {
    case 'ok': return { id: 'disk', name: 'disk', tone: 'ok', word: `${freeWords(d.free_mb)} free`, detail: summary }
    case 'low': return { id: 'disk', name: 'disk', tone: 'wait', word: `${freeWords(d.free_mb)} left`, detail: summary }
    case 'below_floor': return { id: 'disk', name: 'disk', tone: 'fault', word: 'jobs refused', detail: summary }
    case 'unknown': return { id: 'disk', name: 'disk', tone: 'wait', word: 'unknown', detail: summary }
    default: return { id: 'disk', name: 'disk', tone: 'idle', word: '—', detail: summary }
  }
}

const SEVERITY: Partial<Record<Tone, number>> = { fault: 2, wait: 1 }

/** The lamps that need a look, the worst first (faults, then waits; in the bar's order within each), and the tone of
 *  the worst; "all well" when none does. Off and unreported lamps are no trouble. */
export function healthSummary(lamps: readonly Lamp[]): { tone: Tone; trouble: Lamp[]; words: string } {
  const trouble = lamps.filter((l) => SEVERITY[l.tone]).sort((a, b) => (SEVERITY[b.tone] ?? 0) - (SEVERITY[a.tone] ?? 0))
  if (!trouble.length) return { tone: 'ok', trouble, words: `all ${lamps.length} well` }
  return { tone: trouble[0].tone, trouble, words: trouble.map((l) => `${l.name} ${l.word}`).join(' · ') }
}
