// Series and summaries derived from the ledger: the cockpit's instruments read the same append-only record the
// daemon writes, so every number on screen can be traced back to rows.
import { useMemo } from 'react'
import type { LedgerEntry, Usage } from '@protocol'
import { useRpc } from './rpc'
import { ledgerKind, type Tone } from './taxonomy'

export interface LedgerTail { rows: LedgerEntry[]; total: number }

/** The newest `n` ledger rows, polled. `enabled: false` reads nothing (an inspector that isn't open). */
export function useLedger(n = 2000, interval = 3000, kind?: string, sessionId?: string, enabled = true) {
  const params: Record<string, unknown> = { n }
  if (kind) params.kind = kind
  if (sessionId) params.session_id = sessionId
  return useRpc<LedgerTail>('ledger.tail', params, interval, { enabled })
}

export interface RateLimit {
  input_tokens_remaining?: number; output_tokens_remaining?: number
  requests_limit?: number; requests_remaining?: number; requests_reset?: string
  tokens_limit?: number; tokens_remaining?: number; tokens_reset?: string; retry_after_secs?: number | null
}

export interface ProviderCall {
  at: number; position: number; session_id: string | null; turn_id: string | null
  provider: string; model: string; cost: number; loop: number; stop: string
  usage: Usage; first_byte_ms?: number; first_token_ms?: number; total_ms?: number
  rate?: RateLimit; request_id?: string
}

export function providerCalls(rows: LedgerEntry[] | undefined): ProviderCall[] {
  if (!rows) return []
  const out: ProviderCall[] = []
  for (const r of rows) {
    if (r.kind !== 'provider.call') continue
    const d = (r.data ?? {}) as Record<string, any>
    out.push({
      at: r.at_unix_ms, position: r.position, session_id: r.session_id, turn_id: r.turn_id,
      provider: d.provider ?? '?', model: d.model ?? '?', cost: Number(d.cost_usd ?? 0), loop: Number(d.loop ?? 0),
      stop: d.stop_reason ?? '', usage: d.usage ?? { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
      first_byte_ms: d.timing?.first_byte_ms, first_token_ms: d.timing?.first_token_ms, total_ms: d.timing?.total_ms,
      rate: d.rate_limit ?? undefined, request_id: d.request_id,
    })
  }
  return out
}

export const totalIn = (u: Usage) => u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens

/** Events per bucket, split by tone, for the last `spanMs`. */
export function pulse(rows: LedgerEntry[] | undefined, spanMs: number, buckets: number, now = Date.now()) {
  const size = spanMs / buckets
  const start = now - spanMs
  const tones: Tone[] = ['live', 'model', 'tool', 'think', 'wait', 'ok', 'money', 'fault', 'idle']
  const series: Record<Tone, number[]> = Object.fromEntries(tones.map((t) => [t, new Array(buckets).fill(0)])) as Record<Tone, number[]>
  for (const r of rows ?? []) {
    if (r.at_unix_ms < start) continue
    const i = Math.min(buckets - 1, Math.floor((r.at_unix_ms - start) / size))
    series[ledgerKind(r.kind).tone][i] += 1
  }
  const times = Array.from({ length: buckets }, (_, i) => start + i * size)
  return { times, series, tones }
}

/** Cumulative spend over time from provider calls: [time, usd]. */
export function spendCurve(calls: ProviderCall[]): [number, number][] {
  let acc = 0
  return calls.map((c) => { acc += c.cost; return [c.at, Number(acc.toFixed(6))] as [number, number] })
}

export interface TurnRow {
  turn_id: string; session_id: string | null; start: number; end?: number; elapsed_ms?: number; first_token_ms?: number
  loops?: number; tool_calls?: number; cost?: number; model?: string; stop?: string; failed?: boolean
}

/** Turns from turn.started / turn.ended / turn.failed, oldest first. */
export function turnRows(rows: LedgerEntry[] | undefined): TurnRow[] {
  const m = new Map<string, TurnRow>()
  for (const r of rows ?? []) {
    if (!r.turn_id) continue
    const d = (r.data ?? {}) as Record<string, any>
    if (r.kind === 'turn.started') m.set(r.turn_id, { turn_id: r.turn_id, session_id: r.session_id, start: r.at_unix_ms })
    else if (r.kind === 'turn.ended' || r.kind === 'turn.failed') {
      const t = m.get(r.turn_id) ?? { turn_id: r.turn_id, session_id: r.session_id, start: r.at_unix_ms - Number(d.elapsed_ms ?? 0) }
      Object.assign(t, {
        end: r.at_unix_ms, elapsed_ms: d.elapsed_ms ?? r.at_unix_ms - t.start, first_token_ms: d.first_token_ms,
        loops: d.loops, tool_calls: d.tool_calls, cost: d.cost_usd, model: d.model, stop: d.stop_reason, failed: r.kind === 'turn.failed',
      })
      m.set(r.turn_id, t)
    }
  }
  return [...m.values()].sort((a, b) => a.start - b.start)
}

export interface ToolStat { tool: string; calls: number; ok: number; failed: number; denied: number; durations: number[]; producers: Set<string> }

/** Per-tool counts and durations, from the action lifecycle rows. */
export function toolStats(rows: LedgerEntry[] | undefined): ToolStat[] {
  const toolOf = new Map<string, string>()
  const stats = new Map<string, ToolStat>()
  const get = (tool: string) => {
    let s = stats.get(tool)
    if (!s) { s = { tool, calls: 0, ok: 0, failed: 0, denied: 0, durations: [], producers: new Set() }; stats.set(tool, s) }
    return s
  }
  for (const r of rows ?? []) {
    const d = (r.data ?? {}) as Record<string, any>
    if (r.kind === 'action.planned' && d.correlation_id && d.tool) { toolOf.set(d.correlation_id, d.tool); get(d.tool).calls++ }
    else if (r.kind === 'action.succeeded' || r.kind === 'action.failed') {
      const tool = toolOf.get(d.correlation_id) ?? String(d.producer ?? '?').replace(/^[a-z]+:/, '')
      const s = get(tool)
      if (r.kind === 'action.succeeded') s.ok++; else s.failed++
      if (typeof d.duration_ms === 'number') s.durations.push(d.duration_ms)
      if (d.producer) s.producers.add(String(d.producer).split(':')[0])
    } else if (r.kind === 'tool.denied' && d.tool) get(d.tool).denied++
  }
  return [...stats.values()].sort((a, b) => b.calls - a.calls)
}

export function quantile(xs: number[], q: number): number | undefined {
  if (!xs.length) return undefined
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.floor(q * s.length))]
}

/** est_tokens per context compile, per session: [time, tokens]. */
export function contextSeries(rows: LedgerEntry[] | undefined): Map<string, [number, number][]> {
  const m = new Map<string, [number, number][]>()
  for (const r of rows ?? []) {
    if (r.kind !== 'context.compiled' && r.kind !== 'context.recompiled') continue
    const d = (r.data ?? {}) as Record<string, any>
    const sid = d.session_id ?? r.session_id ?? '?'
    if (typeof d.est_tokens !== 'number') continue
    const arr = m.get(sid) ?? []
    arr.push([r.at_unix_ms, d.est_tokens])
    m.set(sid, arr)
  }
  return m
}

export function useDerived(n = 2000) {
  const q = useLedger(n, 3000)
  const rows = q.data?.rows
  const calls = useMemo(() => providerCalls(rows), [rows])
  return { ...q, rows, calls }
}
