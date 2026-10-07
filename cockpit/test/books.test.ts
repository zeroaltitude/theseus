import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { BookEpisode } from '../src/protocol.gen/BookEpisode.ts'
import {
  BOOK_ORDER, BOOK_WORDS, bookTitle, episodeWhen, facetOptions, filtersOf, filterWords, guarded, joined, labelWords,
  ordered, pageParams, shares, spanWords,
} from '../src/lib/books.ts'

const ep = (over: Partial<BookEpisode> = {}): BookEpisode => ({
  session_id: 'ses_ep01', episode_id: 'ep_01', book: 'diary', tag: 'marsh-2026-01', start_ms: Date.UTC(2026, 0, 2, 9),
  end_ms: Date.UTC(2026, 0, 2, 9, 40), source: 'openclaw-store', sensitivity: 'personal', messages: 3,
  credential_redacted: false, ...over,
})

test('the seven books and the unsorted are listed in order, each with its words, an empty one kept', () => {
  assert.equal(BOOK_ORDER.length, 8)
  for (const b of BOOK_ORDER) assert.ok(BOOK_WORDS[b], b)
  const got = ordered([{ book: 'unsorted', episodes: 4 }, { book: 'diary', episodes: 9, first_ms: 1, last_ms: 2 }])
  assert.deepEqual(got.map((b) => b.book), [...BOOK_ORDER])
  assert.equal(got[0].episodes, 9)
  assert.equal(got[1].episodes, 0, 'a book the daemon left out is empty')
  assert.equal(got[7].episodes, 4)
  assert.equal(bookTitle('sop'), 'SOP reference')
  assert.equal(bookTitle('casebook'), 'Casebook')
})

test('the bars are shares of the largest book', () => {
  assert.deepEqual(shares([{ book: 'a', episodes: 5 }, { book: 'b', episodes: 10 }, { book: 'c', episodes: 0 }]), [0.5, 1, 0])
  assert.deepEqual(shares([{ book: 'a', episodes: 0 }]), [0])
})

test('a span and an episode\'s time are said in UTC', () => {
  assert.equal(spanWords({ episodes: 0 }), 'empty')
  assert.equal(spanWords({ episodes: 2, first_ms: Date.UTC(2024, 2, 1), last_ms: Date.UTC(2026, 8, 30, 23) }), '2024-03-01 to 2026-09-30')
  assert.equal(spanWords({ episodes: 1, first_ms: Date.UTC(2025, 0, 1), last_ms: Date.UTC(2025, 0, 1, 5) }), '2025-01-01')
  assert.equal(episodeWhen(ep()), '2026-01-02 09:00 UTC · 40 min')
  assert.equal(episodeWhen(ep({ end_ms: Date.UTC(2026, 0, 2, 12) })), '2026-01-02 09:00 UTC · 3 h')
  assert.equal(episodeWhen(ep({ end_ms: Date.UTC(2026, 0, 5, 9) })), '2026-01-02 09:00 UTC · 3 d')
})

test('an episode\'s labels in a line, and which sensitivities stay private', () => {
  assert.deepEqual(labelWords(ep({ place: { kind: 'dm', name: 'wren' }, partner: 'partner-candidate:osprey', triage: 'decision_or_preference', credential_redacted: true })),
    ['personal', 'openclaw-store', 'dm:wren', 'partner-candidate:osprey', 'decision or preference', 'a credential redacted'])
  assert.deepEqual(labelWords(ep({ sensitivity: 'public' })), ['public', 'openclaw-store'])
  assert.ok(guarded('personal') && guarded('partner-confidential'))
  assert.ok(!guarded('public') && !guarded('company-confidential'))
})

test('the filters come from the address and go to books.page as set', () => {
  const f = filtersOf(new URLSearchParams('book=casebook&topic=heron%2Fcount&place=dm'))
  assert.deepEqual(f, { book: 'casebook', topic: 'heron/count', place: 'dm' })
  assert.deepEqual(pageParams(f, 'c1', 20), { book: 'casebook', limit: 20, topic: 'heron/count', place: 'dm', cursor: 'c1' })
  assert.deepEqual(pageParams({ book: 'diary' }), { book: 'diary', limit: 50 })
  assert.equal(filtersOf(new URLSearchParams('book=almanac')).book, 'diary', 'an unknown book falls back to the diary')
  assert.equal(filterWords(f), 'topic heron/count · place dm')
  assert.equal(filterWords({ book: 'diary' }), 'every episode')
})

test('a chosen facet stays in its list, and pages join with each episode once', () => {
  const list = [{ value: 'dm', episodes: 4 }]
  assert.deepEqual(facetOptions(list, 'cli'), [{ value: 'cli', episodes: 0 }, { value: 'dm', episodes: 4 }])
  assert.deepEqual(facetOptions(list, 'dm'), list)
  assert.deepEqual(facetOptions(undefined), [])
  const a = ep(), b = ep({ session_id: 'ses_ep02' }), c = ep({ session_id: 'ses_ep03' })
  assert.deepEqual(joined([[a, b], [b, c]]).map((e) => e.session_id), ['ses_ep01', 'ses_ep02', 'ses_ep03'])
})
