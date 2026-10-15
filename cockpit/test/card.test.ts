// The card proposals' pure parts (`src/lib/card.ts`), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { answeredLine, asksUntil, declineQuestion, restsOn, summary } from '../src/lib/card.ts'

const DAY = 86_400_000
const prop = (extra: any = {}) =>
  ({
    id: 'cdp_0123456789abcdef', kind: 'preference', scope: { kind: 'everywhere' }, text: 'wants your opinion when you have one',
    question: 'q', options: ['Keep', 'Edit', 'Not this'], support: 14, created_ms: 0, expires_ms: 14 * DAY, state: 'open', run: 'crd_1',
    sources: [{ episode: 'ep_a', node: 'n', kind: 'decision', at_ms: 1 }, { episode: 'ep_b', node: 'n', kind: 'feedback', at_ms: 2 }, { episode: 'ep_c', node: 'n', kind: 'decision', at_ms: 3 }],
    ...extra,
  }) as any

test('a proposal says how many sources and what kind of evidence they are', () => {
  assert.equal(restsOn(prop()), '14 sources · decision, feedback')
  assert.equal(restsOn(prop({ support: 1, sources: [] })), '1 source')
})

test('an open question says how long it asks, and that an overdue one ends at the next read', () => {
  assert.equal(asksUntil(prop(), 0), 'asks for 14 more days')
  assert.equal(asksUntil(prop(), 13.5 * DAY), 'asks until tomorrow')
  assert.equal(asksUntil(prop(), 15 * DAY), 'expires at the next read')
})

test('the header counts what waits and what the job has spent', () => {
  const r: any = { open: [prop()], kept_total: 2, declined_total: 1, spent_today_usd: 0.1234, spent_total_usd: 0.5, day_usd: 2, max_open: 30, done: false, every_days: 7 }
  assert.equal(summary(r), '1 waiting (at most 30) · 2 kept · 1 declined · $0.12 of $2.00 today, $0.50 in all · nightly at most every 7 days')
  assert.match(summary({ ...r, done: true }), /the card is done: no nightly run$/)
  assert.match(summary({ ...r, every_days: 0 }), /no nightly run$/)
})

test('Not this asks first with the words and the number of sources it will not be proposed for again', () => {
  assert.equal(declineQuestion(prop()), 'Not this: “wants your opinion when you have one”? It will not be proposed again for its 3 sources.')
})

test('an answered proposal shows the words kept and how it ended', () => {
  assert.equal(answeredLine(prop({ state: 'kept' })), 'wants your opinion when you have one — kept')
  assert.equal(answeredLine(prop({ state: 'kept', kept_text: 'wants an opinion' })), 'wants an opinion — kept, edited')
  assert.equal(answeredLine(prop({ state: 'declined', answered_by: 'expiry' })), 'wants your opinion when you have one — declined, expired')
  assert.equal(answeredLine(prop({ state: 'declined', answered_by: 'owner' })), 'wants your opinion when you have one — declined')
})
