// A billed model call as the cockpit reads it from its `provider.call` row, or a keep-warm read's `keep_warm` row
// (theseus-ezeg: the same provider, model, usage and cost, booked to its session): pure, with no import but the protocol's
// types, so the modules that read calls (`viz.ts`, `money.ts`) stay runnable by `node --test` as they are. derive.ts
// re-exports all of it.
import type { LedgerEntry, Usage } from '@protocol'

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
    if (r.kind !== 'provider.call' && r.kind !== 'keep_warm') continue
    const d = (r.data ?? {}) as Record<string, any>
    out.push({
      at: r.at_unix_ms, position: r.position, session_id: r.session_id, turn_id: r.turn_id,
      provider: d.provider ?? '?', model: d.model ?? '?', cost: Number(d.cost_usd ?? 0), loop: Number(d.loop ?? 0),
      stop: d.stop_reason ?? (r.kind === 'keep_warm' ? 'keep_warm' : ''), usage: d.usage ?? { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
      first_byte_ms: d.timing?.first_byte_ms, first_token_ms: d.timing?.first_token_ms, total_ms: d.timing?.total_ms,
      rate: d.rate_limit ?? undefined, request_id: d.request_id,
    })
  }
  return out
}

export const totalIn = (u: Usage) => u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens
