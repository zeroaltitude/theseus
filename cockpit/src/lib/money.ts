// Money from the record: the catalog's per-million rates, and a provider call's cost split by token kind. Shared by
// Economics and the money river, so "saved by caching" means one thing everywhere: what the cache reads would have
// cost as plain input, less what they cost.
import type { CatalogList } from '@protocol'
import type { ProviderCall } from './derive'

export interface Price { input: number; output: number; cacheRead: number; cacheWrite: number; cacheWrite1h: number }

export function pricing(cat?: CatalogList): Map<string, Price> {
  const m = new Map<string, Price>()
  for (const x of cat?.models ?? []) {
    const e = x.entry as Record<string, number>
    const input = e.input_per_mtok ?? 0
    // A catalog from before 13c has no 1-hour write price: Anthropic's is 2 × input (theseus-ev1).
    m.set(x.model, { input, output: e.output_per_mtok ?? 0, cacheRead: e.cache_read_per_mtok ?? 0, cacheWrite: e.cache_write_per_mtok ?? 0, cacheWrite1h: e.cache_write_1h_per_mtok ?? 2 * input })
  }
  return m
}

export interface Split { input: number; cacheRead: number; cacheWrite: number; cacheWrite1h: number; output: number; saved: number }

/** A call's cost split by token kind, from the catalog's rates: 5-minute and 1-hour cache writes each at their own. */
export function split(c: ProviderCall, p?: Price): Split {
  if (!p) return { input: 0, cacheRead: 0, cacheWrite: 0, cacheWrite1h: 0, output: 0, saved: 0 }
  const u = c.usage
  const w1h = Math.min(u.cache_creation_1h_input_tokens ?? 0, u.cache_creation_input_tokens)
  return {
    input: (u.input_tokens * p.input) / 1e6,
    cacheRead: (u.cache_read_input_tokens * p.cacheRead) / 1e6,
    cacheWrite: ((u.cache_creation_input_tokens - w1h) * p.cacheWrite) / 1e6,
    cacheWrite1h: (w1h * p.cacheWrite1h) / 1e6,
    output: (u.output_tokens * p.output) / 1e6,
    saved: (u.cache_read_input_tokens * (p.input - p.cacheRead)) / 1e6,
  }
}

/** What caching saved a call, net: its reads at the cache-read price instead of the input price, less the premium its writes
 *  paid over the input price (the 1-hour writes at their own price). The Observatory's 'saved' (theseus-ev1); it can be
 *  negative while a cache is being built and not yet read. `split().saved` is the gross figure of reads alone. */
export function netSaved(c: ProviderCall, p?: Price): number {
  if (!p) return 0
  const u = c.usage
  const w1h = Math.min(u.cache_creation_1h_input_tokens ?? 0, u.cache_creation_input_tokens)
  const premium = (u.cache_creation_input_tokens - w1h) * (p.cacheWrite - p.input) + w1h * (p.cacheWrite1h - p.input)
  return (u.cache_read_input_tokens * (p.input - p.cacheRead) - premium) / 1e6
}

export interface CacheRow { key: string; sessions: number; input: number; read: number; written: number; saved: number }

/** The calls grouped by a key (a session's profile), each group's input tokens, the share read from cache, the tokens
 *  written to it, and the net dollars saved. Groups with no input are left out; the most input first. */
export function cacheBy(calls: ProviderCall[], keyOf: (c: ProviderCall) => string, prices: Map<string, Price>): CacheRow[] {
  const m = new Map<string, CacheRow & { ids: Set<string> }>()
  for (const c of calls) {
    const u = c.usage
    const input = u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens
    if (input === 0) continue
    const key = keyOf(c)
    const r = m.get(key) ?? { key, sessions: 0, input: 0, read: 0, written: 0, saved: 0, ids: new Set<string>() }
    r.input += input
    r.read += u.cache_read_input_tokens
    r.written += u.cache_creation_input_tokens
    r.saved += netSaved(c, prices.get(c.model))
    if (c.session_id) r.ids.add(c.session_id)
    m.set(key, r)
  }
  return [...m.values()].map(({ ids, ...r }) => ({ ...r, sessions: ids.size })).sort((a, b) => b.input - a.input)
}
