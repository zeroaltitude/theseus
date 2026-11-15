// The Books view's words and its address (theseus-civ0): the seven shelves and the unsorted, what each holds, a shelf's
// span, an episode's labels in a line, the filters as `books.page` takes them from the address, and the bars' shares.
// A shelf is one of the sorted sets of imported conversations, and exists today; a book is what Theseus will write from a
// shelf, and none exists (planned for 3.0.0). The wire, the route and these identifiers keep the word "book"; what a
// person reads says "shelf". Pure, with no import but the protocol's types, so `node --test` runs its test (test/books.test.ts).
import type { BookEpisode, BookFacet, BookInfo, BooksPageParams } from '@protocol'
import { span } from './figures.ts'

/** The shelves in the order every surface lists them, then the episodes the import put on no shelf. */
export const BOOK_ORDER = ['diary', 'encyclopedia', 'cookbook', 'sop', 'casebook', 'register', 'dictionary', 'unsorted'] as const

/** What each shelf holds, in a line: the owner's design for organizing memory. */
export const BOOK_WORDS: Record<string, string> = {
  diary: 'what happened, day by day',
  encyclopedia: 'what is known about things',
  cookbook: 'how to make things work',
  sop: 'the standing procedures',
  casebook: 'cases worked, and what they taught',
  register: 'who and what is on record',
  dictionary: 'what words and names mean',
  unsorted: 'episodes the import put on no shelf',
}

/** What this view shows, and what it does not, said once at the top: shelves exist, books do not yet. */
export const FIRST_CUT =
  'This page shows shelves, not books. A shelf is one of the sorted sets of your imported conversations: the import put ' +
  'each conversation on the shelf it guessed it belongs on. A book is something Theseus will write from a shelf, a real ' +
  'dictionary or diary. None is written yet; written books are planned for Theseus 3.0.0. Nothing here can be changed.'

/** The line a note says to the reader about what is and is not built, in the same words wherever it is shown. */
export const SHELVES_TODAY = 'Shelves exist today. Written books are planned for Theseus 3.0.0 and are not built.'

/** A shelf's name as the view shows it: the sop is the SOP reference. */
export function bookTitle(book: string): string {
  if (book === 'sop') return 'SOP reference'
  return book.charAt(0).toUpperCase() + book.slice(1)
}

/** A shelf's title in a heading: the diary is the Diary shelf; the unsorted are on no shelf. */
export function shelfTitle(book: string): string {
  return book === 'unsorted' ? 'Unsorted: on no shelf' : `${bookTitle(book)} shelf`
}

/** The shelves in `BOOK_ORDER`, those the daemon did not name as empty, and any it named beyond them after. */
export function ordered(books: BookInfo[]): BookInfo[] {
  const by = new Map(books.map((b) => [b.book, b]))
  const known = BOOK_ORDER.map((book) => by.get(book) ?? { book, episodes: 0 })
  return [...known, ...books.filter((b) => !(BOOK_ORDER as readonly string[]).includes(b.book))]
}

/** Each shelf's bar as a share of the largest, from 0 to 1; an empty one 0. */
export function shares(books: BookInfo[]): number[] {
  const most = Math.max(0, ...books.map((b) => b.episodes))
  return books.map((b) => (most > 0 ? b.episodes / most : 0))
}

const day = (ms: number): string => new Date(ms).toISOString().slice(0, 10)

/** A shelf's span: `2024-03-01 to 2026-09-30`, one day once, or `empty`. */
export function spanWords(b: Pick<BookInfo, 'episodes' | 'first_ms' | 'last_ms'>): string {
  if (!b.episodes || b.first_ms == null || b.last_ms == null) return 'empty'
  const [a, z] = [day(b.first_ms), day(b.last_ms)]
  return a === z ? a : `${a} to ${z}`
}

/** An episode's span: its start's date and time, and how long it ran. */
export function episodeWhen(e: Pick<BookEpisode, 'start_ms' | 'end_ms'>): string {
  const start = new Date(e.start_ms).toISOString().slice(0, 16).replace('T', ' ')
  const mins = Math.max(0, Math.round((e.end_ms - e.start_ms) / 60_000))
  return `${start} UTC · ${mins < 1 ? '<1m' : span(mins * 60_000)}`
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

/** The filters a page takes from the address: a shelf (the diary when none or an unknown one), and a topic, source, and
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
