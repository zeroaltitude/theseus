// A reply's citations (`src/lib/cites.ts`), run by `npm test`: the marks, their order, a handle with no source, and the
// hover card's words.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Citation } from '@protocol'
import { CITE_HREF, cardLines, citeOf, handlesIn, markCites, sessionHref } from '../src/lib/cites.ts'

const cite = (handle: string): Citation => ({
  handle, node_id: 'msg_1', session_id: 'ses_harbor', position: 7, at_unix_ms: 0,
  header: 'a message from ada in #harbor, 2026-09-30 14:34 UTC (as of @7)', excerpt: 'We chose oak.',
})

test('the handles in a reply, each once, in order', () => {
  assert.deepEqual(handlesIn('Oak [m:a1b2c3], pine [m:d4e5f6], oak [m:a1b2c3]; [m:abc] [x] [m:a1b2c3'), ['m:a1b2c3', 'm:d4e5f6'])
})

test('a cited handle is a numbered link, and one with no source stays text', () => {
  const { text, order } = markCites('Pine [m:d4e5f6], oak [m:a1b2c3], pine [m:d4e5f6], and [m:ffffff].', [cite('m:a1b2c3'), cite('m:d4e5f6')])
  assert.equal(text, `Pine [[1]](${CITE_HREF}m:d4e5f6), oak [[2]](${CITE_HREF}m:a1b2c3), pine [[1]](${CITE_HREF}m:d4e5f6), and [m:ffffff].`)
  assert.deepEqual(order.map((c) => c.handle), ['m:d4e5f6', 'm:a1b2c3'])
  assert.equal(markCites('no cites [m:a1b2c3]', undefined).text, 'no cites [m:a1b2c3]')
  assert.equal(markCites('no cites [m:a1b2c3]', []).order.length, 0)
})

test("a link says whether it is a citation's, and the card says where to go", () => {
  assert.equal(citeOf(`${CITE_HREF}m:a1b2c3`), 'm:a1b2c3')
  assert.equal(citeOf('https://example.org'), null)
  assert.equal(citeOf(undefined), null)
  assert.equal(sessionHref(cite('m:a1b2c3')), '/session/ses_harbor')
  assert.deepEqual(cardLines(cite('m:a1b2c3')), {
    header: 'a message from ada in #harbor, 2026-09-30 14:34 UTC (as of @7)', excerpt: 'We chose oak.', command: 'theseus history m:a1b2c3',
  })
})
