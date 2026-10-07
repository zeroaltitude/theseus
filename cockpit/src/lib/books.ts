// The Books view's words and its address (theseus-civ0): the seven books and the unsorted, what each is for, a book's
// span, an episode's labels in a line, the filters as `books.page` takes them from the address, and the bars' shares.
// Pure, with no import but the protocol's types, so `node --test` runs its test (test/books.test.ts).
import type { BookEpisode, BookFacet, BookInfo, BooksPageParams } from '@protocol'

/** The books in the order every surface lists them, then the episodes the import gave no book. */
export const BOOK_ORDER = ['diary', 'encyclopedia', 'cookbook', 'sop', 'casebook', 'register', 'dictionary', 'unsorted'] as const

/** What each book holds, in a line: the owner's design for organizing memory. */
export const BOOK_WORDS: Record<string, string> = {
  diary: 'what happened, day by day',
  encyclopedia: 'what is known about things',
  cookbook: 'how to make things work',
  sop: 'the standing procedures',
  casebook: 'cases worked, and what they taught',
  register: 'who and what is on record',
  dictionary: 'what words and names mean',
  unsorted: 'episodes the import gave no book',
}

/** What this first cut is not, said once at the top of the view. */
export const FIRST_CUT =
  'A first cut: these are the imported episodes as the import labelled them, sorted by its book hint. ' +
  'The compiled books, written from them, come next; nothing here is compiled, and nothing here can be changed.'

/** A book's title as the view shows it: the sop is the SOP reference. */
export function bookTitle(book: string): string {
  if (book === 'sop') return 'SOP reference'
  return book.charAt(0).toUpperCase() + book.slice(1)
}

/** The books in `BOOK_ORDER`, those the daemon did not name as empty, and any it named beyond them after. */
export function ordered(books: BookInfo[]): BookInfo[] {
  const by = new Map(books.map((b) => [b.book, b]))
  const known = BOOK_ORDER.map((book) => by.get(book) ?? { book, episodes: 0 })
  return [...known, ...books.filter((b) => !(BOOK_ORDER as readonly string[]).includes(b.book))]
}

/** Each book's bar as a share of the largest, from 0 to 1; an empty one 0. */
export function shares(books: BookInfo[]): number[] {
  const most = Math.max(0, ...books.map((b) => b.episodes))
  return books.map((b) => (most > 0 ? b.episodes / most : 0))
}

const day = (ms: number): string => new Date(ms).toISOString().slice(0, 10)

/** A book's span: `2024-03-01 to 2026-09-30`, one day once, or `empty`. */
export function spanWords(b: Pick<BookInfo, 'episodes' | 'first_ms' | 'last_ms'>): string {
  if (!b.episodes || b.first_ms == null || b.last_ms == null) return 'empty'
  const [a, z] = [day(b.first_ms), day(b.last_ms)]
  return a === z ? a : `${a} to ${z}`
}

/** An episode's span: its start's date and time, and how long it ran. */
export function episodeWhen(e: Pick<BookEpisode, 'start_ms' | 'end_ms'>): string {
  const start = new Date(e.start_ms).toISOString().slice(0, 16).replace('T', ' ')
  const mins = Math.max(0, Math.round((e.end_ms - e.start_ms) / 60_000))
  const long = mins < 60 ? `${mins} min` : mins < 48 * 60 ? `${Math.round(mins / 6) / 10} h` : `${Math.round(mins / 144) / 10} d`
  return `${start} UTC · ${long}`
}

/** A place as the facets name it: `dm`, or `dm:wren`. */
export function placeWords(p: BookEpisode['place']): string {
  if (!p) return '—'
  return p.name ? `${p.kind}:${p.name}` : p.kind
}

/** The labels' line on an episode's card: its sensitivity, source, place, and what the pipeline noted. */
export function labelWords(e: BookEpisode): string[] {
  const out = [e.sensitivity, e.source]
  if (e.place) out.push(placeWords(e.place))
  if (e.partner) out.push(e.partner)
  if (e.triage) out.push(e.triage.replace(/_/g, ' '))
  if (e.credential_redacted) out.push('a credential redacted')
  return out
}

/** The sensitivities whose text stays in a private place, as the pipeline labels them. */
export function guarded(sensitivity: string): boolean {
  return sensitivity === 'personal' || sensitivity === 'partner-confidential'
}

/** The filters a page takes from the address: a book (the diary when none or an unknown one), and a topic, source, and
 *  place when set. */
export interface Filters { book: string; topic?: string; source?: string; place?: string }

export function filtersOf(params: URLSearchParams): Filters {
  const book = params.get('book') ?? ''
  const f: Filters = { book: (BOOK_ORDER as readonly string[]).includes(book) ? book : 'diary' }
  for (const k of ['topic', 'source', 'place'] as const) {
    const v = params.get(k)
    if (v) f[k] = v
  }
  return f
}

/** `books.page`'s params for `f`, after `cursor`, `limit` at a time. */
export function pageParams(f: Filters, cursor?: string, limit = 50): BooksPageParams {
  return { book: f.book, limit, ...(f.topic ? { topic: f.topic } : {}), ...(f.source ? { source: f.source } : {}),
    ...(f.place ? { place: f.place } : {}), ...(cursor ? { cursor } : {}) }
}

/** The filters in words: `topic heron/count · place dm`, or `every episode`. */
export function filterWords(f: Filters): string {
  const parts = (['topic', 'source', 'place'] as const).filter((k) => f[k]).map((k) => `${k} ${f[k]}`)
  return parts.length ? parts.join(' · ') : 'every episode'
}

/** A facet's options: each value with its count, and the one chosen kept first even when the cut list lost it. */
export function facetOptions(list: BookFacet[] | undefined, chosen?: string): BookFacet[] {
  const out = [...(list ?? [])]
  if (chosen && !out.some((f) => f.value === chosen)) out.unshift({ value: chosen, episodes: 0 })
  return out
}

/** Pages put together in order, each episode once: a page read again after the one after it never repeats a row. */
export function joined(pages: BookEpisode[][]): BookEpisode[] {
  const seen = new Set<string>()
  const out: BookEpisode[] = []
  for (const p of pages) for (const e of p) if (!seen.has(e.session_id)) { seen.add(e.session_id); out.push(e) }
  return out
}
