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
