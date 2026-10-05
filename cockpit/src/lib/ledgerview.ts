// The Ledger view's pure parts (M7 42b): the filter that picks the rows shown (so the export is exactly them), the
// filters kept by name in the browser (the address's query, saved and read back), and the follow mode's count of rows
// that landed while paused. `npm test` runs it.
import type { LedgerEntry } from '@protocol'
import { summarize } from './summary.ts'
import { ledgerKind } from './taxonomy.ts'

/** A filter, as the address holds it (`?q=&kind=a,b&family=&session=&from=&to=`). */
export interface LedgerFilter {
  q: string
  kinds: ReadonlySet<string>
  family: string | null
  session: string | null
  /** The time brush, in ms. */
  range: readonly [number, number] | null
}

/** The address's keys a saved filter holds: the filter, never the view (`view`, `follow`, `nkind`, `row`). */
export const FILTER_KEYS = ['q', 'kind', 'family', 'session', 'from', 'to'] as const

export function filterOf(p: URLSearchParams): LedgerFilter {
  const from = Number(p.get('from')), to = Number(p.get('to'))
  return {
    q: p.get('q') ?? '',
    kinds: new Set((p.get('kind') ?? '').split(',').filter(Boolean)),
    family: p.get('family'),
    session: p.get('session'),
    range: p.has('from') && p.has('to') && Number.isFinite(from) && Number.isFinite(to) ? [from, to] : null,
  }
}

/** The rows a filter shows, newest first: what the list draws, and what the export holds. */
export function filterRows(rows: readonly LedgerEntry[], f: LedgerFilter): LedgerEntry[] {
  const needle = f.q.toLowerCase()
  const out: LedgerEntry[] = []
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    if (f.session && r.session_id !== f.session) continue
    if (f.kinds.size && !f.kinds.has(r.kind)) continue
    if (f.family && ledgerKind(r.kind).family !== f.family) continue
    if (f.range && (r.at_unix_ms < f.range[0] || r.at_unix_ms > f.range[1])) continue
    if (needle && !(r.kind.includes(needle) || (r.session_id ?? '').includes(needle) || summarize(r).toLowerCase().includes(needle) || JSON.stringify(r.data).toLowerCase().includes(needle))) continue
    out.push(r)
  }
  return out
}

/** The export: the rows the filter shows, as the JSON the browser downloads. */
export function exportOf(rows: readonly LedgerEntry[], f: LedgerFilter): string {
  return JSON.stringify(filterRows(rows, f), null, 2)
}

/** The file's name: what it holds and when (`ledger-12-rows-2026-10-05T03-30-00.json`). */
export function exportName(n: number, at: number): string {
  return `ledger-${n}-rows-${new Date(at).toISOString().slice(0, 19).replace(/:/g, '-')}.json`
}

// ---------------------------------------------------------------- saved filters

export interface Saved { name: string; query: string }

/** The filter in the address, as a query string with the keys in a fixed order. */
export function queryOf(p: URLSearchParams): string {
  const out = new URLSearchParams()
  for (const k of FILTER_KEYS) { const v = p.get(k); if (v) out.set(k, v) }
  return out.toString()
}

/** The address with a saved filter's query in place of its filter (its view, follow, and the rest stay). */
export function applySaved(p: URLSearchParams, s: Saved): URLSearchParams {
  const next = new URLSearchParams(p)
  for (const k of FILTER_KEYS) next.delete(k)
  for (const [k, v] of new URLSearchParams(s.query)) if ((FILTER_KEYS as readonly string[]).includes(k)) next.set(k, v)
  return next
}

/** A name replaces the filter of the same name, else it is added last. */
export function withSaved(list: readonly Saved[], name: string, query: string): Saved[] {
  const n = name.trim()
  if (!n) return [...list]
  const at = list.findIndex((s) => s.name === n)
  return at < 0 ? [...list, { name: n, query }] : list.map((s, i) => (i === at ? { name: n, query } : s))
}

export const withoutSaved = (list: readonly Saved[], name: string): Saved[] => list.filter((s) => s.name !== name)

export const SAVED_KEY = 'theseus.ledger.filters'

export const writeSaved = (list: readonly Saved[]): string => JSON.stringify(list)

/** What the browser held, read back: anything that is not a list of named queries is dropped, never thrown. */
export function readSaved(text: string | null): Saved[] {
  if (!text) return []
  try {
    const v: unknown = JSON.parse(text)
    if (!Array.isArray(v)) return []
    return v.flatMap((e) => (e && typeof e === 'object' && typeof (e as Saved).name === 'string' && typeof (e as Saved).query === 'string' ? [{ name: (e as Saved).name, query: (e as Saved).query }] : []))
  } catch {
    return []
  }
}

// ---------------------------------------------------------------- follow

/** Rows after `position` (the newest the list showed when it paused), in a list newest first, as the list is. */
export function newerThan(rows: readonly LedgerEntry[], position: number): number {
  let n = 0
  while (n < rows.length && rows[n].position > position) n++
  return n
}
