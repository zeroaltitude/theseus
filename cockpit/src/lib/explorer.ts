// The context explorer's pure parts (theseus-7n3e): its state in the address, the imported episodes' filters as
// `import.sessions` takes them, the topic tree folded from the facets, the veil over sensitive text, a turn's anatomy
// by block, and the as-of months. Pure: it imports the protocol's types alone, so node's runner tests it.
import type { ContextPart, ImportFacet, ImportSessionsParams, ImportedEpisode, OntologyCategory } from '@protocol'

/** The explorer's tabs, in the rail's order. */
export const TABS = ['episodes', 'turn', 'search', 'books'] as const
export type Tab = (typeof TABS)[number]

/** A page of episodes. */
export const PAGE = 50

/** The episode filters the address keeps, each by its key. `month` is a month (`2026-03`) or a span of them
 *  (`2026-01..2026-03`). */
export const FILTER_KEYS = ['tag', 'source', 'place', 'sens', 'book', 'topic', 'q', 'month'] as const
export type FilterKey = (typeof FILTER_KEYS)[number]
export type Filters = Partial<Record<FilterKey, string>>

/** The episode filters in the address. */
export function filtersOf(p: URLSearchParams): Filters {
  const out: Filters = {}
  for (const k of FILTER_KEYS) {
    const v = p.get(k)
    if (v) out[k] = v
  }
  return out
}

/** The address's tab, `episodes` when it names none it knows. */
export function tabOf(p: URLSearchParams): Tab {
  const t = p.get('tab')
  return (TABS as readonly string[]).includes(t ?? '') ? (t as Tab) : 'episodes'
}

/** A month key's first and last unix ms (UTC). Null for a key that does not read. */
export function monthSpan(key: string): [number, number] | null {
  const m = /^(\d{4})-(\d{2})$/.exec(key)
  if (!m) return null
  const y = Number(m[1]), mo = Number(m[2])
  if (mo < 1 || mo > 12) return null
  return [Date.UTC(y, mo - 1, 1), Date.UTC(y, mo, 1) - 1]
}

/** `month`'s span: one month, or `a..b` from a's first ms to b's last. */
export function monthsSpan(v: string | undefined): [number, number] | null {
  if (!v) return null
  const [a, b] = v.split('..')
  const s = monthSpan(a), e = monthSpan(b ?? a)
  return s && e ? [s[0], e[1]] : null
}

/** The filters, the sort and the page as `import.sessions` takes them. */
export function paramsOf(f: Filters, sort: string | null, page: number): ImportSessionsParams {
  const span = monthsSpan(f.month)
  return {
    tag: f.tag, source: f.source, place: f.place, sensitivity: f.sens, book: f.book, topic: f.topic,
    q: f.q?.trim() || undefined,
    from_ms: span?.[0], to_ms: span?.[1],
    sort: sort && sort !== 'newest' ? sort : undefined,
    offset: Math.max(0, page - 1) * PAGE, limit: PAGE, summaries: true, erased: false, ids: [],
  }
}

/** One row of the topic tree. */
export interface TopicRow {
  path: string
  name: string
  depth: number
  count: number
  /** It has topics under it. */
  parent: boolean
  /** An ontology topic of the same path (theseus-anh3), and its members when the daemon counts them. */
  ontology: boolean
  members?: number
}

/** Each ontology topic's path of names from its root, `a/b/c`: what an imported topic label is when it is in the
 *  ontology. */
export function ontologyPaths(categories: OntologyCategory[]): Map<string, OntologyCategory> {
  const byId = new Map(categories.map((c) => [c.id, c]))
  const out = new Map<string, OntologyCategory>()
  for (const c of categories) {
    if (c.kind !== 'topic') continue
    const names: string[] = []
    let at: OntologyCategory | undefined = c
    for (let guard = 0; at && guard < 32; guard++) {
      names.unshift(at.name)
      at = at.parent ? byId.get(at.parent) : undefined
    }
    out.set(names.join('/'), c)
  }
  return out
}

/** The topic facet (every path and its ancestors, by path) as a tree, depth first, each branch's children by count,
 *  the most first; only the branches under `open` (and the roots) are listed. */
export function topicTree(facets: ImportFacet[], open: ReadonlySet<string>, ontology?: Map<string, OntologyCategory>): TopicRow[] {
  const count = new Map(facets.map((f) => [f.value, f.count]))
  const kids = new Map<string, string[]>()
  for (const f of facets) {
    const i = f.value.lastIndexOf('/')
    // A path whose parent is not in the facet is never reached from the roots: the walk below leaves it out.
    const up = i > 0 ? f.value.slice(0, i) : ''
    const list = kids.get(up) ?? []
    list.push(f.value)
    kids.set(up, list)
  }
  for (const list of kids.values()) list.sort((a, b) => (count.get(b) ?? 0) - (count.get(a) ?? 0) || a.localeCompare(b))
  const out: TopicRow[] = []
  const walk = (up: string, depth: number) => {
    for (const path of kids.get(up) ?? []) {
      const cat = ontology?.get(path)
      out.push({
        path, name: path.slice(path.lastIndexOf('/') + 1), depth, count: count.get(path) ?? 0, parent: kids.has(path),
        ontology: !!cat, members: (cat as { members?: number } | undefined)?.members,
      })
      if (open.has(path)) walk(path, depth + 1)
    }
  }
  walk('', 0)
  return out
}

/** A topic and its ancestors: what the tree opens to show it. */
export function ancestors(path: string | undefined): string[] {
  if (!path) return []
  const parts = path.split('/')
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join('/'))
}

/** The sensitivities the veil covers: the owner's personal history and a partner's confidence. */
export const VEILED = ['personal', 'partner-confidential'] as const

/** Whether an episode's text is veiled on screen: its label is one the veil covers, the veil is on, and it was not
 *  opened. The labels themselves are always shown. */
export function veiled(e: Pick<ImportedEpisode, 'sensitivity' | 'session_id'>, veilOn: boolean, opened: ReadonlySet<string>): boolean {
  return veilOn && (VEILED as readonly string[]).includes(e.sensitivity) && !opened.has(e.session_id)
}

/** A sensitivity in words, and the tone that carries it beside the words. */
export function sensitivityWords(s: string): { word: string; tone: 'think' | 'wait' | 'fault' | 'ok' | 'idle' } {
  switch (s) {
    case 'personal': return { word: 'personal', tone: 'think' }
    case 'company-confidential': return { word: 'company confidential', tone: 'wait' }
    case 'partner-confidential': return { word: 'partner confidential', tone: 'fault' }
    case 'public': return { word: 'public', tone: 'ok' }
    default: return { word: s || 'unlabelled', tone: 'idle' }
  }
}

/** A request's blocks, in its order, with their words. */
export const BLOCKS = [
  { key: 'header', word: 'header', what: 'the persona, the assembly and precedence notes, the tools note, the profile’s own text' },
  { key: 'context', word: 'context files', what: 'each context file under its header' },
  { key: 'guidance', word: 'guidance', what: 'each category’s guidance, after the files' },
  { key: 'tools', word: 'tools', what: 'the tools’ definitions' },
  { key: 'recall', word: 'recall', what: 'what recall put in front of the model' },
  { key: 'conversation', word: 'conversation', what: 'the messages, tool calls and results: the rest of the estimate' },
] as const
export type BlockKey = (typeof BLOCKS)[number]['key']

/** A turn's tokens by block, in the request's order, blocks with none left out. */
export function anatomy(parts: ContextPart[]): { key: BlockKey; word: string; what: string; tokens: number; parts: number }[] {
  return BLOCKS.map((b) => {
    const mine = parts.filter((p) => p.block === b.key)
    return { key: b.key, word: b.word, what: b.what, tokens: mine.reduce((a, p) => a + p.tokens, 0), parts: mine.length }
  }).filter((b) => b.parts > 0 && b.tokens > 0)
}

/** A part's state against the turn's compilation, in words. */
export function thenWords(then: string | undefined): string {
  switch (then) {
    case 'same': return 'as the turn saw it'
    case 'changed': return 'changed since the turn'
    case 'new': return 'new since the turn'
    case 'gone': return 'the turn carried it; gone now'
    default: return ''
  }
}

/** The months between the first and the last of `months` (`2026-03`, oldest first), each with its count, gaps as 0. */
export function monthBins(months: ImportFacet[]): ImportFacet[] {
  const keyed = months.filter((m) => monthSpan(m.value))
  if (!keyed.length) return []
  const have = new Map(keyed.map((m) => [m.value, m.count]))
  const [y0, m0] = keyed[0].value.split('-').map(Number)
  const [y1, m1] = keyed[keyed.length - 1].value.split('-').map(Number)
  const out: ImportFacet[] = []
  for (let y = y0, m = m0; y < y1 || (y === y1 && m <= m1); m === 12 ? (y++, m = 1) : m++) {
    const key = `${String(y).padStart(4, '0')}-${String(m).padStart(2, '0')}`
    out.push({ value: key, count: have.get(key) ?? 0 })
    if (out.length > 1200) break
  }
  return out
}

/** A count with its thousands marked. */
export function count(n: number): string {
  return n.toLocaleString('en-US')
}

/** The explorer's address for a session's context, or one turn's: from the Ship and the session deck. */
export function contextHref(session: string, turn?: string | null): string {
  return `/context?tab=turn&session=${encodeURIComponent(session)}${turn ? `&turn=${encodeURIComponent(turn)}` : ''}`
}

/** The explorer's address for an imported episode. */
export function episodeHref(session: string): string {
  return `/context?episode=${encodeURIComponent(session)}`
}

/** A moment with its year, in UTC: an episode's span can be years back (`2025-06-26 22:34 UTC`). */
export function when(ms: number): string {
  return Number.isFinite(ms) && ms > 0 ? `${new Date(ms).toISOString().slice(0, 16).replace('T', ' ')} UTC` : '—'
}

/** Long ids in a line, each as people name it (`imp_…` to `imp·a1b2c3`), so a note reads. */
export function shortIds(line: string): string {
  return line.replace(/\b([a-z]{3})_([0-9a-z_]{20,})\b/g, (_, p: string, rest: string) => `${p}·${rest.replace(/_\d+$|_summary$/, '').slice(-6)}`)
}

/** `import.sessions`' read of named sessions' rows alone: a recalled note's or a deep link's labels. */
export function namedParams(ids: string[]): ImportSessionsParams {
  return { ids: [...new Set(ids)].sort(), limit: 500, summaries: false, erased: true }
}
