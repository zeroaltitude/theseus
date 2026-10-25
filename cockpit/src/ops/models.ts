// The dock's `Models · profiles` strip grouped by model (theseus-yxkww), pure: `sonnet` and `sonnethi` are one model,
// Sonnet 5.5, at two efforts, so the strip shows the model once, its effort as a sub-line, and its profiles under it.
// The effort is what the turns really ran at: `route.decided` carries the effort each routed turn's request had. Before
// any turn of the model is read, the sub-line is the efforts its profiles are configured at, and says so.
import type { LedgerEntry } from '@protocol'

/** The efforts, lowest first: the order of a tie and of the configured line. */
const LADDER = ['low', 'medium', 'high', 'xhigh', 'max']
const rank = (e: string): number => {
  const i = LADDER.indexOf(e)
  return i < 0 ? LADDER.length : i
}

/** A profile as `modelsStrip` rows it: `effort` is the one it is configured at, when it has one. */
export interface StripProfile { name: string; model: string; origin: string; live: boolean; sessions: number; headroom: string | null; effort?: string | null }

/** One routed turn: the model it ran on and the effort its request carried. */
export interface EffortCall { model: string; effort: string }

export interface ModelProfile { name: string; live: boolean; sessions: number; effort: string | null }
export interface ModelEffort {
  /** `usage`: the turns' own; `configured`: no turn of this model read, so what its profiles are set to. */
  basis: 'usage' | 'configured'
  text: string
  /** The turns the shares are of; 0 for `configured`. */
  turns: number
}
export interface ModelGroup {
  model: string; label: string; origin: string; live: boolean; sessions: number; headroom: string | null
  effort: ModelEffort | null; profiles: ModelProfile[]
}

/** A model id in words: `claude-sonnet-5-5` is `Sonnet 5.5`, `glm-5.3-flash` is `GLM 5.3 Flash`; a date is dropped. */
export function modelLabel(id: string): string {
  const parts = id.replace(/^claude-/, '').split('-').filter((t) => t !== '' && !/^\d{8}$/.test(t))
  const out: string[] = []
  for (const [i, t] of parts.entries()) {
    if (/^\d+$/.test(t) && /^\d+(\.\d+)*$/.test(out[out.length - 1] ?? '')) out[out.length - 1] += `.${t}`
    else if (i === 0 && (t === 'glm' || t === 'gpt')) out.push(t.toUpperCase())
    else out.push(t.charAt(0).toUpperCase() + t.slice(1))
  }
  return out.join(' ') || id
}

/** The effort each routed turn's request carried, newest `max` of them, oldest first: `route.decided`'s `effort_sent`
 *  when a hold kept the previous level on the wire, else its `effort_ran`. A turn that carried none is not counted. */
export function routeEfforts(rows: readonly LedgerEntry[], max = 400): EffortCall[] {
  const out: EffortCall[] = []
  for (const r of rows) {
    if (r.kind !== 'route.decided') continue
    const d = (r.data ?? {}) as Record<string, unknown>
    const effort = typeof d.effort_sent === 'string' && d.effort_sent !== '' ? d.effort_sent : d.effort_ran
    if (typeof d.model === 'string' && d.model !== '' && typeof effort === 'string' && effort !== '') out.push({ model: d.model, effort })
  }
  return out.slice(-max)
}

function usageOf(turns: readonly string[]): ModelEffort {
  const n = new Map<string, number>()
  for (const e of turns) n.set(e, (n.get(e) ?? 0) + 1)
  const text = [...n].sort((a, b) => b[1] - a[1] || rank(a[0]) - rank(b[0])).map(([e, c]) => `${e} ${Math.round((c / turns.length) * 100)}%`).join(' / ')
  return { basis: 'usage', text, turns: turns.length }
}

function configuredOf(profiles: readonly ModelProfile[]): ModelEffort {
  const levels = [...new Set(profiles.map((p) => p.effort ?? 'default'))].sort((a, b) => (a === 'default' ? -1 : b === 'default' ? 1 : rank(a) - rank(b)))
  return { basis: 'configured', text: `configured: ${levels.join(' / ')}`, turns: 0 }
}

/** The strip's profiles grouped by model, in the order the profile list first names each model. */
export function groupByModel(strip: readonly StripProfile[], routes: readonly EffortCall[]): ModelGroup[] {
  const groups = new Map<string, ModelGroup>()
  for (const p of strip) {
    let g = groups.get(p.model)
    if (!g) {
      g = { model: p.model, label: modelLabel(p.model), origin: p.origin, live: false, sessions: 0, headroom: null, effort: null, profiles: [] }
      groups.set(p.model, g)
    }
    if (g.origin === 'unknown') g.origin = p.origin
    g.live ||= p.live
    g.sessions += p.sessions
    g.headroom ??= p.headroom
    g.profiles.push({ name: p.name, live: p.live, sessions: p.sessions, effort: p.effort ?? null })
  }
  for (const g of groups.values()) {
    const turns = routes.filter((r) => r.model === g.model).map((r) => r.effort)
    g.effort = turns.length ? usageOf(turns) : configuredOf(g.profiles)
  }
  return [...groups.values()]
}
