// The session deck's rows (`src/lib/sessionrows.ts`, theseus-kuzw), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { deckRows, wholeHistory } from '../src/lib/sessionrows.ts'

const row = (position: number, session: string | null, kind = 'tool.called') =>
  ({ position, kind, at_unix_ms: 1_000 + position, session_id: session, turn_id: null, data: {} }) as any

// A long session: 1,500 of its rows among 600 of another's and 30 with no session, the order the ledger wrote them.
const rows = Array.from({ length: 2_130 }, (_, i) => row(i + 1, i % 71 === 0 ? null : i % 7 < 2 ? 'ses_b' : 'ses_a',
  i % 50 === 0 ? 'turn.ended' : 'tool.called'))
const mine = rows.filter((r) => r.session_id === 'ses_a')

test('a long session keeps every row: past the 1,000 a tail read gives, and its first turn first', () => {
  assert.ok(mine.length > 1_000, `the fixture is long: ${mine.length}`)
  const tail = mine.slice(-1_000)
  const got = deckRows({ rows, ready: true, partial: false }, 'ses_a', tail)!
  assert.equal(got.length, mine.length)
  assert.deepEqual(got.map((r) => r.position), mine.map((r) => r.position))
  assert.ok(got.every((r) => r.session_id === 'ses_a'))
  // The turns are numbered over all of them: the deck's "turn 1" is the session's first turn, not the tail's first.
  const turns = got.filter((r) => r.kind === 'turn.ended')
  assert.equal(turns[0].position, mine.find((r) => r.kind === 'turn.ended')!.position)
  assert.ok(turns.length > tail.filter((r) => r.kind === 'turn.ended').length)
})

test('the tail read stands in while the history is read, or when the daemon cannot page', () => {
  const tail = mine.slice(-1_000)
  assert.equal(deckRows({ rows: rows.slice(0, 900), ready: false, partial: false }, 'ses_a', tail)!.length, 1_000)
  assert.equal(deckRows({ rows: rows.slice(-1_000), ready: true, partial: true }, 'ses_a', tail)!.length, 1_000)
  assert.equal(deckRows({ rows: [], ready: false, partial: false }, 'ses_a', undefined), undefined)
  assert.equal(wholeHistory({ rows: [], ready: true, partial: false }), true)
  assert.equal(wholeHistory({ rows: [], ready: true, partial: true }), false)
})

test('a session with no rows yet has none, not another session\'s', () => {
  assert.deepEqual(deckRows({ rows, ready: true, partial: false }, 'ses_new', undefined), [])
})
