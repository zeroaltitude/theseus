// The shared instruments' legends and table views (`instruments.tsx`, theseus-hnof): the same numbers as each chart, as
// rows, for its panel's chart and table toggle.
import type { LedgerEntry, SessionInfo, StartupPhase } from '@protocol'
import { contextSeries, type ProviderCall, type TurnRow } from '@/lib/derive'
import { ms, short, stamp, tokens, usd } from '@/lib/format'
import { CATEGORICAL, OTHER, TOKEN_KINDS, TONE_MARK, kindTokens } from '@/lib/viz'
import type { LegendItem, TableSpec } from './ChartPanel'

/** A single series' colour: the first categorical slot. */
const ACCENT = CATEGORICAL.dark[0]

/** A session's name as the cockpit says it: its title, its label, or its short id. */
export function titleOf(sessions: SessionInfo[]): (sid: string | null | undefined) => string {
  const m = new Map(sessions.map((s) => [s.session_id, s.title || s.label || short(s.session_id)]))
  return (sid) => (sid ? m.get(sid) ?? short(sid) : '—')
}

/** The last start's two kinds of phase, for its legend. */
export const STARTUP_LEGEND: LegendItem[] = [
  { key: 'fg', label: 'on the way to serving', color: ACCENT, mark: 'rect' },
  { key: 'bg', label: 'after serving, in the background', color: OTHER, mark: 'rect' },
]

export const phaseOrder = (ps: StartupPhase[]) => [...ps].sort((a, b) => Number(a.background) - Number(b.background) || a.start_us - b.start_us)

export function startupTable(phases: StartupPhase[]): TableSpec<StartupPhase> {
  type R = StartupPhase
  return {
    caption: 'the last start, phase by phase, from process start', rows: phaseOrder(phases), rowKey: (r: R) => `${r.background}:${r.name}`,
    columns: [
      { key: 'phase', label: 'phase', cell: (r: R) => r.name },
      { key: 'when', label: 'when', cell: (r: R) => (r.background ? 'after serving' : 'to serving') },
      { key: 'from', label: 'from', num: true, cell: (r: R) => ms(r.start_us / 1000) },
      { key: 'to', label: 'to', num: true, cell: (r: R) => (r.end_us === null ? 'running' : ms(r.end_us / 1000)) },
      { key: 'took', label: 'took', num: true, cell: (r: R) => (r.end_us === null ? '—' : ms((r.end_us - r.start_us) / 1000)) },
    ],
  }
}

export const TURNS_LEGEND: LegendItem[] = [
  { key: 'turn', label: 'a turn: its size is its cost', color: ACCENT, mark: 'dot' },
  { key: 'failed', label: 'a turn that failed', color: TONE_MARK.fault, mark: 'triangle' },
]

export function turnsTable(turns: TurnRow[], title: (sid: string | null | undefined) => string): TableSpec<TurnRow> {
  type R = TurnRow
  return {
    caption: 'each finished turn, the newest first', rows: turns.filter((t) => t.elapsed_ms !== undefined).reverse(), rowKey: (r: R) => r.turn_id,
    columns: [
      { key: 'at', label: 'started', cell: (r: R) => stamp(r.start) },
      { key: 'session', label: 'session', cell: (r: R) => title(r.session_id), title: (r: R) => title(r.session_id) },
      { key: 'took', label: 'took', num: true, cell: (r: R) => ms(r.elapsed_ms) },
      { key: 'first', label: 'first token', num: true, cell: (r: R) => ms(r.first_token_ms) },
      { key: 'loops', label: 'loops', num: true, cell: (r: R) => r.loops ?? '—' },
      { key: 'tools', label: 'tool calls', num: true, cell: (r: R) => r.tool_calls ?? 0 },
      { key: 'cost', label: 'cost', num: true, cell: (r: R) => usd(r.cost) },
      { key: 'how', label: 'ended', cell: (r: R) => (r.failed ? 'failed' : r.stop ?? 'ended') },
    ],
  }
}

/** Every compile's estimated prompt size, for a context chart's table view. */
export function contextTable(rows: LedgerEntry[] | undefined, title: (sid: string | null | undefined) => string): TableSpec<{ sid: string; at: number; tokens: number }> {
  type R = { sid: string; at: number; tokens: number }
  const out: R[] = []
  for (const [sid, pts] of contextSeries(rows)) for (const [at, n] of pts) out.push({ sid, at, tokens: n })
  out.sort((a, b) => b.at - a.at)
  return {
    caption: 'each compile\'s estimated prompt size, the newest first', rows: out, rowKey: (r: R) => `${r.sid}:${r.at}:${r.tokens}`,
    columns: [
      { key: 'at', label: 'compiled', cell: (r: R) => stamp(r.at) },
      { key: 'session', label: 'session', cell: (r: R) => title(r.sid), title: (r: R) => title(r.sid) },
      { key: 'tokens', label: 'estimated tokens', num: true, cell: (r: R) => tokens(r.tokens) },
    ],
  }
}

export const TOKEN_LEGEND: LegendItem[] = TOKEN_KINDS.map((k) => ({ key: k.key, label: k.word, color: k.color, mark: 'rect' as const }))

export function tokenMixTable(calls: ProviderCall[], title: (sid: string | null | undefined) => string): TableSpec<ProviderCall> {
  type R = ProviderCall
  const t = (r: R) => kindTokens([r])
  return {
    caption: 'each model call\'s tokens by kind, the newest first', rows: [...calls].reverse(), rowKey: (r: R) => String(r.position),
    columns: [
      { key: 'at', label: 'when', cell: (r: R) => stamp(r.at) },
      { key: 'model', label: 'model', cell: (r: R) => r.model },
      { key: 'session', label: 'session', cell: (r: R) => title(r.session_id), title: (r: R) => title(r.session_id) },
      ...TOKEN_KINDS.map((k) => ({ key: k.key, label: k.word, num: true, cell: (r: R) => tokens(t(r)[k.key]) })),
      { key: 'all', label: 'in all', num: true, cell: (r: R) => tokens(TOKEN_KINDS.reduce((a, k) => a + t(r)[k.key], 0)) },
      { key: 'cost', label: 'cost', num: true, cell: (r: R) => usd(r.cost) },
    ],
  }
}
