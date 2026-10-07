// The context explorer's pure parts (`src/lib/explorer.ts`, theseus-7n3e), run by `npm test`: its state in the address
// and the query it makes, the topic tree from the facets (and the ontology's topics beside them), the veil over
// sensitive text, a turn's anatomy by block, and the as-of months.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  BLOCKS, PAGE, anatomy, ancestors, contextHref, filtersOf, monthBins, monthSpan, monthsSpan, ontologyPaths, paramsOf, tabOf, thenWords,
  topicTree, veiled,
} from '../src/lib/explorer.ts'

const facet = (value: string, count: number) => ({ value, count })

test('the address holds the tab and the filters, and the query is what import.sessions takes', () => {
  const p = new URLSearchParams('tab=books&source=wiki&sens=personal&topic=reef%2Fsurvey&q=%20tide%20log%20&month=2026-01..2026-03&zzz=1')
  assert.equal(tabOf(p), 'books')
  assert.equal(tabOf(new URLSearchParams('tab=nowhere')), 'episodes')
  assert.equal(tabOf(new URLSearchParams('')), 'episodes')
  const f = filtersOf(p)
  assert.deepEqual(f, { source: 'wiki', sens: 'personal', topic: 'reef/survey', q: ' tide log ', month: '2026-01..2026-03' })
  const q = paramsOf(f, 'oldest', 3)
  assert.equal(q.sensitivity, 'personal')
  assert.equal(q.q, 'tide log', 'words trimmed')
  assert.equal(q.sort, 'oldest')
  assert.equal(q.offset, 2 * PAGE)
  assert.equal(q.limit, PAGE)
  assert.equal(q.summaries, true)
  assert.equal(q.from_ms, Date.UTC(2026, 0, 1))
  assert.equal(q.to_ms, Date.UTC(2026, 3, 1) - 1, 'through the last ms of March')
  // The default sort is said by leaving it out; an empty word list is none.
  assert.equal(paramsOf({ q: '   ' }, 'newest', 1).sort, undefined)
  assert.equal(paramsOf({ q: '   ' }, 'newest', 1).q, undefined)
  assert.equal(paramsOf({}, null, 0).offset, 0)
})

test('a month is its calendar month in UTC, and a span runs from the first to the last', () => {
  assert.deepEqual(monthSpan('2024-02'), [Date.UTC(2024, 1, 1), Date.UTC(2024, 2, 1) - 1])
  assert.equal(monthSpan('2024-13'), null)
  assert.equal(monthSpan('24-02'), null)
  assert.deepEqual(monthsSpan('2025-12'), monthSpan('2025-12'))
  assert.equal(monthsSpan('2025-12..nonsense'), null)
  assert.equal(monthsSpan(undefined), null)
})

test('the topic tree folds the facet by slash paths, the most first, and opens only what is open', () => {
  const topics = [facet('harbour', 2), facet('harbour/pier', 2), facet('reef', 5), facet('reef/survey', 3), facet('reef/survey/tides', 1), facet('reef/surveyor', 2)]
  const closed = topicTree(topics, new Set())
  assert.deepEqual(closed.map((r) => [r.path, r.depth, r.parent]), [['reef', 0, true], ['harbour', 0, true]])
  const open = topicTree(topics, new Set(['reef', 'reef/survey']))
  assert.deepEqual(open.map((r) => r.path), ['reef', 'reef/survey', 'reef/survey/tides', 'reef/surveyor', 'harbour'])
  assert.deepEqual(open.map((r) => r.name), ['reef', 'survey', 'tides', 'surveyor', 'harbour'])
  assert.equal(open[2].depth, 2)
  // A path whose parent is not in the facet (a filter took it) does not hang off nothing.
  assert.deepEqual(topicTree([facet('a/b', 1)], new Set()).map((r) => r.path), [])
  assert.deepEqual(ancestors('a/b/c'), ['a', 'a/b'])
  assert.deepEqual(ancestors(undefined), [])
})

test('an ontology topic is matched to a label by its path of names, and says so with its members', () => {
  const cat = (id: string, name: string, parent?: string, members?: number) => ({ id, kind: 'topic', name, parent, depth: 1, description: '', added_by: 'import', members })
  const cats = [cat('topic:reef', 'reef', undefined, 5), cat('topic:reef-survey', 'survey', 'topic:reef', 3), { ...cat('channel:9', 'quay'), kind: 'channel' }]
  const paths = ontologyPaths(cats as never)
  assert.deepEqual([...paths.keys()], ['reef', 'reef/survey'], 'only topics, by their names from the root')
  const rows = topicTree([facet('reef', 5), facet('reef/survey', 3), facet('reef/other', 1)], new Set(['reef']), paths)
  assert.deepEqual(rows.map((r) => [r.path, r.ontology, r.members]), [['reef', true, 5], ['reef/survey', true, 3], ['reef/other', false, undefined]])
  // Before the ontology holds a topic (theseus-anh3), every row says it is the labels' alone.
  assert.ok(topicTree([facet('reef', 5)], new Set(), new Map()).every((r) => !r.ontology))
})

test('the veil covers personal and partner-confidential text until opened, and never a label', () => {
  const e = (sensitivity: string) => ({ sensitivity, session_id: `ses_ep${sensitivity}` })
  assert.equal(veiled(e('personal'), true, new Set()), true)
  assert.equal(veiled(e('partner-confidential'), true, new Set()), true)
  assert.equal(veiled(e('company-confidential'), true, new Set()), false)
  assert.equal(veiled(e('public'), true, new Set()), false)
  assert.equal(veiled(e('personal'), false, new Set()), false, 'the veil off')
  assert.equal(veiled(e('personal'), true, new Set(['ses_eppersonal'])), false, 'opened')
})

test('a turn’s anatomy sums each block’s parts in the request’s order and leaves out an empty block', () => {
  const part = (block: string, tokens: number) => ({ block, name: block, bytes: tokens * 3, tokens })
  const a = anatomy([part('tools', 900), part('header', 200), part('header', 100), part('context', 0), part('conversation', 4000)])
  assert.deepEqual(a.map((b) => [b.key, b.tokens, b.parts]), [['header', 300, 2], ['tools', 900, 1], ['conversation', 4000, 1]])
  assert.deepEqual(BLOCKS.map((b) => b.key), ['header', 'context', 'guidance', 'tools', 'recall', 'conversation'])
  assert.equal(thenWords('same'), 'as the turn saw it')
  assert.equal(thenWords('changed'), 'changed since the turn')
  assert.equal(thenWords(undefined), '')
})

test('the months between the first and the last are each a bin, an empty month a zero', () => {
  const bins = monthBins([facet('2025-11', 3), facet('2026-02', 1), facet('bad', 9)])
  assert.deepEqual(bins, [facet('2025-11', 3), facet('2025-12', 0), facet('2026-01', 0), facet('2026-02', 1)])
  assert.deepEqual(monthBins([]), [])
})

test('the Ship and the session deck link to a session’s context and a bench to its turn’s', () => {
  assert.equal(contextHref('ses_1'), '/context?tab=turn&session=ses_1')
  assert.equal(contextHref('ses_1', 'trn 2'), '/context?tab=turn&session=ses_1&turn=trn%202')
  const ship = readFileSync(new URL('../src/views/Ship.tsx', import.meta.url), 'utf8')
  assert.match(ship, /contextHref\(v\.id, bench\?\.turnId\)/)
  const deck = readFileSync(new URL('../src/views/SessionDeck.tsx', import.meta.url), 'utf8')
  assert.match(deck, /contextHref\(session\.session_id\)/)
  const main = readFileSync(new URL('../src/main.tsx', import.meta.url), 'utf8')
  assert.match(main, /path: 'context', lazy: async \(\) => \(\{ Component: \(await import\('\.\/views\/Context'\)\)\.default \}\)/)
})
