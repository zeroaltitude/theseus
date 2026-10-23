// The Operations page's address (theseus-jid7t.3): every choice on screen is in it, so a link reopens exactly what was
// shown. Pure: `npm test` runs it (test/ops-address.test.ts). The keys:
//   lens    graph · board · timeline · threads (board when absent: the Graph lens proper is a later row's)
//   sel     the selected id: `exe_`, `ses_`, `act_`, `wak_`, `tsk_`, a thread's slug, a mark's key
//   f       the filters, `key:value` pairs joined by commas, each value URI-encoded
//   q       the search
//   w       the week offset (the Agenda's own `?w=`, kept as it was)
//   join    a later row's (the Join pane): parsed and carried
//   redact  `1`: a later row gives it meaning; parsed and carried in every link now
//   do      `answer`: a deep link focuses the selected item's answer in the queue (herdr's `&do=answer`)
// Keys the page does not own (the task graph's `task`, a chart's `table`) are kept as they were.

export type Lens = 'graph' | 'board' | 'timeline' | 'threads'
export const LENSES: readonly Lens[] = ['graph', 'board', 'timeline', 'threads']
export const DEFAULT_LENS: Lens = 'board'

/** The command bar's counters, each also a filter. */
export type Bucket = 'needs' | 'working' | 'paused' | 'still' | 'done' | 'spent'
export const BUCKETS: readonly Bucket[] = ['needs', 'working', 'paused', 'still', 'done', 'spent']

/** The filter chips. `ended` is a span (`1h`, `1d`, `7d`): what ended within it. */
export const FILTER_KEYS = ['bucket', 'thread', 'model', 'place', 'kind', 'state', 'project', 'ended'] as const
export type FilterKey = (typeof FILTER_KEYS)[number]
export type Filters = Partial<Record<FilterKey, string>>

export interface Address {
  lens: Lens
  sel: string | null
  f: Filters
  q: string
  w: number
  join: string | null
  redact: boolean
  do: 'answer' | null
}

export const OWN_KEYS = ['lens', 'sel', 'f', 'q', 'w', 'join', 'redact', 'do'] as const

const isLens = (s: string | null): s is Lens => s != null && (LENSES as readonly string[]).includes(s)

export function parseFilters(raw: string | null): Filters {
  const out: Filters = {}
  for (const part of (raw ?? '').split(',')) {
    const i = part.indexOf(':')
    if (i <= 0) continue
    const k = part.slice(0, i) as FilterKey
    if (!(FILTER_KEYS as readonly string[]).includes(k)) continue
    let v: string
    try { v = decodeURIComponent(part.slice(i + 1)) } catch { continue }
    if (v) out[k] = v
  }
  return out
}

/** The filters as the address writes them: in `FILTER_KEYS` order, so one set of filters is one string. */
export function writeFilters(f: Filters): string {
  return FILTER_KEYS.filter((k) => f[k]).map((k) => `${k}:${encodeURIComponent(f[k] as string)}`).join(',')
}

export function parseAddress(search: string | URLSearchParams): Address {
  const p = typeof search === 'string' ? new URLSearchParams(search) : search
  const lens = p.get('lens')
  const w = parseInt(p.get('w') ?? '0', 10)
  return {
    lens: isLens(lens) ? lens : DEFAULT_LENS,
    sel: p.get('sel') || null,
    f: parseFilters(p.get('f')),
    q: p.get('q') ?? '',
    // The Agenda's own bounds on the week.
    w: Number.isFinite(w) ? Math.max(-52, Math.min(52, w)) : 0,
    join: p.get('join') || null,
    redact: p.get('redact') === '1',
    do: p.get('do') === 'answer' ? 'answer' : null,
  }
}

/** `a` written into `into` (a copy): the page's own keys replaced, every other key kept. The lens is always written,
 *  so a link names its lens; the rest only when it differs from the default. */
export function writeAddress(a: Address, into: URLSearchParams = new URLSearchParams()): URLSearchParams {
  const p = new URLSearchParams(into)
  for (const k of OWN_KEYS) p.delete(k)
  p.set('lens', a.lens)
  if (a.sel) p.set('sel', a.sel)
  const f = writeFilters(a.f)
  if (f) p.set('f', f)
  if (a.q) p.set('q', a.q)
  if (a.w) p.set('w', String(a.w))
  if (a.join) p.set('join', a.join)
  if (a.redact) p.set('redact', '1')
  if (a.do) p.set('do', a.do)
  return p
}

/** The address of a link to `/ops`. */
export const opsHref = (a: Partial<Address>): string => `/ops?${writeAddress({ ...parseAddress(''), ...a }).toString()}`

/** The lens each old page's address now opens (the owner's decision 1). `/threads/:slug` stays a page of its own. */
export const REDIRECTS: Readonly<Record<string, Lens>> = { '/agenda': 'timeline', '/tasks': 'board', '/threads': 'threads' }

/** Where an old address goes: the matching lens, its query kept whole (`?w=`, `?sel=` and the rest); null for any
 *  other path. */
export function redirectOf(path: string, search: string): string | null {
  const lens = REDIRECTS[path.replace(/\/+$/, '') || '/']
  if (!lens) return null
  // The lens first, then the old query in its own order.
  const rest = [...new URLSearchParams(search).entries()].filter(([k]) => k !== 'lens')
  return `/ops?${new URLSearchParams([['lens', lens], ...rest]).toString()}`
}
