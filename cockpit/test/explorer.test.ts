// The context explorer's pure parts (`src/lib/explorer.ts`, theseus-7n3e), run by `npm test`: its state in the address
// and the query it makes, the topic tree from the facets (and the ontology's topics beside them), the veil over
// sensitive text, a turn's anatomy by block, and the as-of months.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  BLOCKS, DATED_FLOOR, PAGE, anatomy, barHeight, rangeOf, scaleOf, timelineView, anatomyNote, hasBytes, ancestors, contextHref, filtersOf, monthBins, monthSpan, monthsSpan, namedParams, ontologyPaths, paramsOf, tabOf, thenWords,
  shortIds, topicTree, veiled, when,
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
  // Named sessions: each once, the erased too (a recalled note's may be), no summaries.
  assert.deepEqual(namedParams(['b', 'a', 'b']), { ids: ['a', 'b'], limit: 500, summaries: false, erased: true })
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

test('a moment says its year, and a note’s long ids read as people name them', () => {
  assert.equal(when(Date.UTC(2025, 5, 26, 22, 34, 44)), '2025-06-26 22:34 UTC')
  assert.equal(when(0), '—')
  const id = 'a'.repeat(58) + 'b1c2d3'
  assert.equal(shortIds(`this turn's recall rcl_${id}: node imp_${id}_6 of session ses_ep${id}`),
    "this turn's recall rcl·b1c2d3: node imp·b1c2d3 of session ses·b1c2d3")
  assert.equal(shortIds('turn trn_short stays'), 'turn trn_short stays')
})

test('a detour’s parts table has no bytes column and says its parts are token counts; a compiled turn’s keeps both', () => {
  const detour = { detour: { kind: 'quick', profile: 'haiku', model: 'm' }, compiled: null }
  assert.equal(hasBytes(detour), false)
  assert.match(anatomyNote(detour)!, /detour’s row’s token counts/)
  const compiled = { compiled: { est_tokens: 1234, estimate: { method: 'counted' } } }
  assert.equal(hasBytes(compiled), true)
  assert.equal(
    anatomyNote(compiled),
    'Every part is counted from its bytes at the model’s figures, the conversation as the turn’s request less the parts above. The turn’s last loop itself estimated 1,234 tokens, part of it the provider’s own count.',
  )
})

const owners = [facet('1970-01', 5), facet('1973-11', 5), facet('2006-07', 323), facet('2026-08', 900), facet('2026-09', 4100), facet('2026-10', 19778)]

test('the owner’s shape: old months are undated, the window ends at the newest month, no bar before the floor', () => {
  const v = timelineView(owners, '2y')
  assert.equal(v.undatedCount, 10)
  assert.deepEqual(v.undated.map((m) => m.value), ['1970-01', '1973-11'])
  assert.equal(v.undatedPick, '1970-01..1999-12')
  assert.equal(v.last, '2026-10')
  assert.equal(v.first, '2024-11')
  assert.equal(v.earlier, 323)
  const drawn = v.columns.flatMap((c) => (c.kind === 'month' ? [c.value] : []))
  assert.ok(drawn.every((m) => m >= DATED_FLOOR))
  const all = timelineView(owners, 'all')
  assert.equal(all.columns[0].kind === 'month' && all.columns[0].value, '2006-07')
  assert.equal(all.earlier, 0)
  assert.ok(timelineView(owners, '5y').columns.length <= 60)
  assert.ok(all.columns.length <= 60)
  const sum = (v2: typeof v) => v2.undatedCount + v2.earlier + v2.columns.reduce((a, c) => a + (c.kind === 'month' ? c.count : 0), 0)
  assert.equal(sum(v), 5 + 5 + 323 + 900 + 4100 + 19778)
})

test('a long gap is one mark, a short one stays empty months', () => {
  const v = timelineView([facet('2020-01', 2), facet('2023-06', 3), facet('2023-10', 1)], 'all')
  const gaps = v.columns.filter((c) => c.kind === 'gap')
  assert.equal(gaps.length, 1)
  assert.deepEqual(gaps[0], { kind: 'gap', from: '2020-02', to: '2023-05', months: 40 })
  const empties = v.columns.filter((c) => c.kind === 'month' && c.count === 0)
  assert.equal(empties.length, 3)
})

test('sqrt heights show a small month beside a huge one; linear leaves it tiny; the default is sqrt', () => {
  assert.ok(barHeight(323, 19778, 'sqrt') > 10)
  assert.ok(barHeight(323, 19778, 'linear') < 3.01)
  assert.equal(barHeight(0, 10, 'sqrt'), 0)
  assert.equal(barHeight(1, 1e9, 'linear'), 3)
  assert.equal(scaleOf(null), 'sqrt')
  assert.ok(barHeight(323, 19778, scaleOf(null)) > 10)
  assert.equal(rangeOf('zzz'), '2y')
  assert.equal(rangeOf('5y'), '5y')
})
