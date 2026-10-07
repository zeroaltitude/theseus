// What's new on the Ship (`src/ship/news.ts`, theseus-hnof.2, the owner's C6), run by `npm test`: the tour on a first
// visit, the new things once after an update, and nothing twice.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { COCKPIT_VERSION, LEGACY_SEEN, NEWS, tourPlan, type NewsItem } from '../src/ship/news.ts'

const item = (id: string, since: string): NewsItem => ({ id, since, title: id, body: id, anchor: { kind: 'fleet' } })
const news = [item('a', '2026-10-07'), item('b', '2026-10-07'), item('c', '2026-11-02')]

test('a first visit gets the whole tour', () => {
  assert.deepEqual(tourPlan(null, null, '2026-11-02', news), { kind: 'tour' })
})

test('after an update, only what this browser has not seen, in order', () => {
  const p = tourPlan('done', '2026-10-07', '2026-11-02', news)
  assert.deepEqual(p.kind === 'news' && p.items.map((n) => n.id), ['c'])
  const q = tourPlan('done', '2026-10-06', '2026-11-02', news)
  assert.deepEqual(q.kind === 'news' && q.items.map((n) => n.id), ['a', 'b', 'c'])
})

test('once seen, nothing opens by itself again', () => {
  assert.deepEqual(tourPlan('done', '2026-11-02', '2026-11-02', news), { kind: 'none' })
  // A browser that already knows a newer cockpit (another install, an older build after a rollback) is not shown the past.
  assert.deepEqual(tourPlan('done', '2026-12-01', '2026-11-02', news), { kind: 'none' })
})

test("a browser that finished the prototype's tour, before versions were kept, gets what came after it", () => {
  const p = tourPlan('done', null, '2026-11-02', news)
  assert.deepEqual(p.kind === 'news' && p.items.map((n) => n.id), ['a', 'b', 'c'])
  assert.equal(LEGACY_SEEN, '2026-10-06')
})

test("the cockpit's version is its newest stop's, and every stop says where it points", () => {
  assert.equal(COCKPIT_VERSION, NEWS.reduce((v, n) => (n.since > v ? n.since : v), ''))
  for (const n of NEWS) {
    assert.match(n.since, /^\d{4}-\d{2}-\d{2}$/)
    assert.ok(n.title && n.body.length > 40, n.id)
    if (n.anchor.kind === 'dom') assert.match(n.anchor.selector, /^\.ship-/)
  }
  assert.equal(new Set(NEWS.map((n) => n.id)).size, NEWS.length)
})
