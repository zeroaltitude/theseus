// The ontology view's pure parts (`src/lib/ontology.ts`), run by `npm test` with node's own runner: no dependency.
// Invented categories and sessions.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { LedgerEntry, OntologyCategory, OntologyKind, OntologyMembership } from '@protocol'
import { readFileSync } from 'node:fs'
import { ancestorsOf, confidenceWords, GROWTH_DAYS, growthOf, growthWords, groupKeyOf, groupsOf, guidanceInPlay, guidanceWords, judgmentHref, lastDays, mintedOf, nothingPending, parents, pendingOf, recordedOf, seriesOf, shown, treeOrder, undoAsk, undoneWrites, UNKNOWN_GROUP, withinCounts } from '../src/lib/ontology.ts'

const cat = (id: string, name: string, parent?: string, guidance?: [number, string], members = 0): OntologyCategory => ({
  id, kind: id.split(':')[0], name, parent, depth: 1, description: '', added_by: 'the operator', members,
  guidance: guidance ? { category: id, text: 'be brief', version: guidance[0], digest: guidance[1], added_by: 'the operator' } : undefined,
})
const kind = (name: string, rule: string): OntologyKind => ({
  name, basis: name === 'topic' ? 'interpreted' : 'given', assigned_by: [], per_session: 'many', precedence: 1, rule, description: '', version: 1, added_by: 'seed',
})
const kinds = [kind('channel', 'intent_line'), kind('topic', 'chain')]
const member = (category: string, origin = 'operator'): OntologyMembership => ({ session_id: 'ses_a', kind: category.split(':')[0], category, origin, as_of_ms: 1 })

const cats = [
  cat('topic:harbor-tides', 'harbor-tides', 'topic:harbor'),
  cat('topic:zephyr', 'zephyr'),
  cat('channel:123', 'lab'),
  cat('topic:harbor', 'harbor', undefined, [1, 'aaaa']),
  cat('topic:harbor-tides-spring', 'spring', 'topic:harbor-tides'),
]

test('the tree is depth first: roots by name, each followed by its children, with its depth counted from its parents', () => {
  const rows = treeOrder(cats)
  assert.deepEqual(rows.map((r) => [r.category.id, r.depth]), [
    ['topic:harbor', 1],
    ['topic:harbor-tides', 2],
    ['topic:harbor-tides-spring', 3],
    ['channel:123', 1],
    ['topic:zephyr', 1],
  ])
})

test('a category whose parent is not listed stands as a root, and a cycle does not loop', () => {
  const rows = treeOrder([cat('topic:kelp', 'kelp', 'topic:gone'), cat('topic:a', 'a', 'topic:b'), cat('topic:b', 'b', 'topic:a')])
  assert.deepEqual(rows.map((r) => [r.category.id, r.depth]), [['topic:kelp', 1]])
})

const manifest = (memberships: unknown[], guidance: unknown[]) => ({ memberships, guidance })
const used = (category: string, origin = 'operator') => ({ kind: category.split(':')[0], category, origin, as_of_ms: 9 })

test('nothing is pending when the list and the manifest agree, as-of times aside', () => {
  const m = [member('topic:harbor'), member('channel:123', 'transport')]
  const p = pendingOf(m, cats, kinds, manifest([used('topic:harbor'), used('channel:123', 'transport')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.deepEqual(p, { added: [], removed: [], guidance: [] })
  assert.ok(nothingPending(p))
})

test('a membership added since the compile is pending, and so is one taken away', () => {
  const added = pendingOf([member('topic:harbor'), member('topic:zephyr')], cats, kinds, manifest([used('topic:harbor')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.deepEqual(added.added, ['topic:zephyr'])
  assert.deepEqual(added.removed, [])
  const removed = pendingOf([], cats, kinds, manifest([used('topic:zephyr')], []))
  assert.deepEqual(removed.removed, ['topic:zephyr'])
  assert.ok(!nothingPending(removed))
})

test('a guidance version changed since the compile is pending, for the category and for its chain', () => {
  const edited = cats.map((c) => (c.id === 'topic:harbor' ? cat('topic:harbor', 'harbor', undefined, [2, 'bbbb']) : c))
  // The session holds only the nested topic: the parent's guidance reaches it under `chain`.
  const p = pendingOf([member('topic:harbor-tides')], edited, kinds, manifest([used('topic:harbor-tides')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.deepEqual(p.added, [])
  assert.deepEqual(p.removed, [])
  assert.equal(p.guidance.length, 1)
  assert.equal(guidanceWords(p.guidance[0]), 'topic:harbor guidance v1 → v2')
  // The same version with another digest is a change too; guidance taken away is one.
  const swapped = pendingOf([member('topic:harbor-tides')], cats, kinds, manifest([used('topic:harbor-tides')], [{ category: 'topic:harbor', version: 1, digest: 'cccc' }]))
  assert.equal(swapped.guidance.length, 1)
  const gone = pendingOf([member('topic:harbor-tides')], cats.map((c) => (c.id === 'topic:harbor' ? cat('topic:harbor', 'harbor') : c)), kinds, manifest([used('topic:harbor-tides')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.equal(guidanceWords(gone.guidance[0]), 'topic:harbor guidance no longer applies (was v1)')
})

test('an intent_line kind takes no ancestors, and a manifest with neither field recorded none (the daemon leaves empty lists out)', () => {
  const play = guidanceInPlay([member('channel:123', 'transport')], [cat('channel:123', 'lab', undefined, [3, 'dddd'])], kinds)
  assert.deepEqual(play, [{ category: 'channel:123', version: 3, digest: 'dddd' }])
  const none = { added: ['topic:harbor'], removed: [], guidance: [{ category: 'topic:harbor', from: null, to: { category: 'topic:harbor', version: 1, digest: 'aaaa' } }] }
  assert.deepEqual(pendingOf([member('topic:harbor')], cats, kinds, { model: 'x' }), none)
  assert.deepEqual(pendingOf([member('topic:harbor')], cats, kinds, undefined), none)
  assert.deepEqual(pendingOf([], cats, kinds, { model: 'x' }), { added: [], removed: [], guidance: [] })
  assert.equal(recordedOf({ memberships: 'no', guidance: [null, 3, { category: 'topic:a', version: 2 }] }).guidance.length, 1)
})

// The tree at an import's size (theseus-anh3): counts with the descendants', the branches, the folded view, and the
// selected row's ancestors.
const sized = [
  cat('topic:harbor', 'harbor', undefined, undefined, 2),
  cat('topic:harbor-tides', 'tides', 'topic:harbor', undefined, 5),
  cat('topic:harbor-tides-neap', 'neap', 'topic:harbor-tides', undefined, 7),
  cat('topic:harbor-boats', 'boats', 'topic:harbor', undefined, 11),
  cat('topic:zephyr', 'zephyr', undefined, undefined, 1),
]

test('a category counts its sessions with every descendant\'s', () => {
  const within = withinCounts(treeOrder(sized))
  assert.deepEqual([...within.entries()].sort(), [
    ['topic:harbor', 25], ['topic:harbor-boats', 11], ['topic:harbor-tides', 12], ['topic:harbor-tides-neap', 7], ['topic:zephyr', 1],
  ])
})

test('the branches are the rows with children, and a folded branch hides everything below it', () => {
  const rows = treeOrder(sized)
  assert.deepEqual([...parents(rows)].sort(), ['topic:harbor', 'topic:harbor-tides'])
  const ids = (open: string[]) => shown(rows, new Set(open)).map((r) => r.category.id)
  assert.deepEqual(ids([]), ['topic:harbor', 'topic:zephyr'])
  assert.deepEqual(ids(['topic:harbor']), ['topic:harbor', 'topic:harbor-boats', 'topic:harbor-tides', 'topic:zephyr'])
  assert.deepEqual(ids(['topic:harbor-tides']), ['topic:harbor', 'topic:zephyr'], 'a closed root hides an open child')
  assert.deepEqual(ids(['topic:harbor', 'topic:harbor-tides']), rows.map((r) => r.category.id))
})

test('a selected category\'s ancestors are named root first, so it shows', () => {
  const rows = treeOrder(sized)
  assert.deepEqual(ancestorsOf(rows, 'topic:harbor-tides-neap'), ['topic:harbor', 'topic:harbor-tides'])
  assert.deepEqual(ancestorsOf(rows, 'topic:zephyr'), [])
  assert.deepEqual(ancestorsOf(rows, null), [])
})

const kindsAll = [kind('channel', 'intent_line'), kind('person', 'handle'), kind('topic', 'chain')]
const some = [...cats, cat('person:ada', 'Ada'), cat('person:bo', 'Bo'), cat('person:cy', 'Cy')]

test('the groups: one a kind, in the kinds table’s order, each holding its own categories as a tree, with a count', () => {
  const g = groupsOf(some, kindsAll)
  assert.deepEqual(g.map((x) => [x.label, x.count]), [['channel', 1], ['person', 3], ['topic', 4]])
  assert.deepEqual(g[2].rows.map((r) => [r.category.id, r.depth]), [['topic:harbor', 1], ['topic:harbor-tides', 2], ['topic:harbor-tides-spring', 3], ['topic:zephyr', 1]])
  assert.deepEqual(g.map((x) => x.key), ['kind:channel', 'kind:person', 'kind:topic'])
})

test('the people group holds every person, and every category is in exactly one group: nothing is lost', () => {
  const g = groupsOf(some, kindsAll)
  assert.deepEqual(g.find((x) => x.label === 'person')!.rows.map((r) => r.category.id).sort(), ['person:ada', 'person:bo', 'person:cy'])
  const all = g.flatMap((x) => x.rows.map((r) => r.category.id)).sort()
  assert.deepEqual(all, some.map((c) => c.id).sort())
  assert.equal(new Set(all).size, some.length)
  assert.equal(groupKeyOf(g, 'person:bo'), 'kind:person')
  assert.equal(groupKeyOf(g, 'topic:harbor-tides-spring'), 'kind:topic')
  assert.equal(groupKeyOf(g, 'topic:nowhere'), null)
  assert.equal(groupKeyOf(g, null), null)
})

test('a kind the table does not name goes in `<unknown>`, last; the group is left out when it is empty', () => {
  assert.equal(groupsOf(some, kindsAll).some((x) => x.label === UNKNOWN_GROUP), false)
  const odd = [...some, { ...cat('weird:x', 'x'), kind: 'weird' }, { ...cat('none:y', 'y'), kind: '' }]
  const g = groupsOf(odd, kindsAll)
  assert.equal(g[g.length - 1].label, '<unknown>')
  assert.deepEqual(g[g.length - 1].rows.map((r) => r.category.id).sort(), ['none:y', 'weird:x'])
  assert.equal(g[g.length - 1].count, 2)
  assert.deepEqual(groupsOf(odd, []).map((x) => x.label), ['<unknown>'], 'with no kinds table, all are unknown, and still shown')
  assert.deepEqual(groupsOf([], kindsAll), [])
})

test('the view starts with every group closed, opens a selected category’s group, and shows rows only inside an open group', () => {
  const v = readFileSync(new URL('../src/views/Ontology.tsx', import.meta.url), 'utf8')
  assert.match(v, /const \[groupsOpen, setGroupsOpen\] = useState<Set<string>>\(new Set\(\)\)/)
  assert.match(v, /groupKeyOf\(groups, selected\)/)
  assert.match(v, /\{gopen\.has\(g\.key\) && visibleOf\(g\)\.map\(/)
})

// ---------------------------------------------------------------- growth: the chart and the minted list

// A fixture ledger on invented days. `NOW` is noon on the 20th of October; local noon keeps a row on its day in any zone.
const NOW = new Date(2026, 9, 20, 12)
const at = (daysAgo: number, hour = 9) => new Date(2026, 9, 20 - daysAgo, hour).getTime()
let pos = 0
const row = (kind: string, when: number, data: unknown, session: string | null = null): LedgerEntry =>
  ({ position: ++pos, at_unix_ms: when, kind, session_id: session, turn_id: null, data })
const undoRec = (write: string, key: string, wrote: unknown, before: unknown = null) => ({ write, key, wrote, before })
const catRow = (when: number, id: string, origin: string, undo?: ReturnType<typeof undoRec>) =>
  row('ontology.category', when, { id, kind: id.split(':')[0], name: id.split(':')[1], parent: null, origin, who: 'x', via: 'y', ...(undo ? { undo } : {}) })
const memRow = (when: number, session: string, write: string, category: string, confidence: number) =>
  row('ontology.membership', when, {
    kind: 'topic', categories: [category], who: 'x', via: 'machine',
    undo: undoRec(write, `onto:member:${session}:topic`, { session, kind: 'topic', members: [{ category, origin: 'jev', confidence, as_of_ms: 1 }] }),
  }, session)
const kindRow = (when: number, name: string, retired = false) => row('ontology.kind', when, { name, version: 1, retired, assigned_by: ['jev'], who: 'x', via: 'y' })
const undoRow = (when: number, write: string) => row('ontology.undo', when, { write, label: 'wrong', keys: ['k'], who: 'x', via: 'y' })

const ledger: LedgerEntry[] = [
  catRow(at(200), 'topic:old', 'operator'),                                                  // before the window: not counted
  catRow(at(10), 'topic:lab', 'operator'),
  catRow(at(3, 8), 'topic:kelp', 'jev', undoRec('j1', 'onto:cat:topic:kelp', { id: 'topic:kelp' })),
  memRow(at(3, 8), 'ses_a', 'j1', 'topic:kelp', 0.82),
  catRow(at(3, 10), 'topic:tides', 'jev', undoRec('j2', 'onto:cat:topic:tides', { id: 'topic:tides' })),
  catRow(at(3, 10), 'topic:tides', 'jev'),                                                   // a change of the same id: not a new one
  memRow(at(3, 10), 'ses_b', 'j2', 'topic:tides', 0.64),
  undoRow(at(2), 'j2'),
  catRow(at(1), 'topic:moss', 'sweep', undoRec('s1', 'onto:cat:topic:moss', { id: 'topic:moss' })),
  catRow(at(1), 'topic:reed', 'sweep', undoRec('s1', 'onto:cat:topic:reed', { id: 'topic:reed' })),
  catRow(at(0), 'topic:adopted', 'jev', undoRec('j3', 'onto:cat:topic:adopted', { id: 'topic:adopted' }, { id: 'topic:adopted', was: 'earlier' })),
  kindRow(at(5), 'expertise'),
  kindRow(at(4), 'expertise'),                                                               // a second row of the same kind
  kindRow(at(4), 'culture', true),                                                           // a retire is not an addition
  kindRow(at(0), 'resource'),
]

test('the window is ninety local days ending today, by the calendar', () => {
  const days = lastDays(NOW, GROWTH_DAYS)
  assert.equal(days.length, 90)
  assert.equal(days.at(-1), '2026-10-20')
  assert.equal(days.at(-2), '2026-10-19')
  assert.equal(days[0], '2026-07-23')
  assert.equal(new Set(days).size, 90)
})

test('a spring-forward day, 23 hours long, is still one day of the window', () => {
  // A zone that shifts its clocks, set for this test alone: 24-hour steps back from 00:30 on the 9th skip the 8th.
  const was = process.env.TZ
  process.env.TZ = 'America/New_York'
  try {
    assert.deepEqual(lastDays(new Date(2026, 2, 9, 0, 30), 4), ['2026-03-06', '2026-03-07', '2026-03-08', '2026-03-09'])
  } finally {
    if (was === undefined) delete process.env.TZ
    else process.env.TZ = was
  }
})

test('the growth chart stacks categories by origin per day, counts kinds, and marks undone writes', () => {
  const g = growthOf(ledger, NOW)
  assert.equal(g.days.length, 90)
  const on = (daysAgo: number) => g.days.find((d) => d.day === `2026-10-${String(20 - daysAgo).padStart(2, '0')}`)!
  assert.deepEqual(on(10).categories, { operator: 1 })
  assert.deepEqual(on(3).categories, { jev: 2 })
  assert.equal(on(3).undone, 1, 'the write j2 was taken back: its day is marked')
  assert.deepEqual(on(1).categories, { sweep: 2 })
  assert.equal(on(1).undone, 0)
  assert.deepEqual(on(0).categories, { jev: 1 })
  assert.equal(on(5).kinds, 1)
  assert.equal(on(4).kinds, 0, 'a second row of a kind, and a retire, are not additions')
  assert.equal(on(0).kinds, 1)
  assert.deepEqual(g.origins, ['operator', 'jev', 'sweep'], 'the owner first, the machines after')
  assert.equal(g.made, 6)
  assert.equal(g.undone, 1)
  assert.equal(g.kinds, 2)
  assert.equal(growthWords(g), '6 categories made, 1 taken back and 2 kinds added in 90 days')
})

test('a snapshot of the growth panel from the fixture ledger', () => {
  const s = seriesOf(growthOf(ledger, NOW))
  const tail = (xs: number[]) => xs.slice(-11)
  assert.deepEqual(s.days.slice(-11), ['2026-10-10', '2026-10-11', '2026-10-12', '2026-10-13', '2026-10-14', '2026-10-15', '2026-10-16', '2026-10-17', '2026-10-18', '2026-10-19', '2026-10-20'])
  assert.deepEqual(s.stacks.map((x) => [x.origin, tail(x.values)]), [
    ['operator', [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
    ['jev', [0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 1]],
    ['sweep', [0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0]],
  ])
  assert.deepEqual(tail(s.kinds), [0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1])
  assert.deepEqual(tail(s.undone), [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0])
  assert.equal(s.max, 2)
})

test('a ledger with nothing of the ontology grows nothing, and a category outside the window is not counted', () => {
  const g = growthOf([row('turn.start', at(1), {})], NOW)
  assert.equal(g.made, 0)
  assert.deepEqual(g.origins, [])
  assert.ok(g.days.every((d) => d.kinds === 0 && Object.keys(d.categories).length === 0))
  assert.equal(growthWords(g), '0 categories made and 0 kinds added in 90 days')
  assert.equal(growthOf(ledger, NOW).days.some((d) => d.day === '2026-04-02'), false)
})

test('the minted list is the machines new categories, newest first, with session, confidence and undo state', () => {
  const m = mintedOf(ledger)
  assert.deepEqual(m.map((x) => x.category), ['topic:moss', 'topic:reed', 'topic:tides', 'topic:kelp'], 'a re-write (before not null) and an operator row are not mints')
  const [moss, , tides, kelp] = m
  assert.equal(moss.origin, 'sweep')
  assert.equal(moss.siblings, 2, 'one write minted two categories')
  assert.equal(moss.session, null, 'the category row has no session of its own')
  assert.equal(kelp.write, 'j1')
  assert.equal(kelp.session, 'ses_a')
  assert.equal(kelp.confidence, 0.82)
  assert.equal(kelp.undone, false)
  assert.equal(kelp.undoneAt, null)
  assert.equal(tides.confidence, 0.64)
  assert.equal(tides.undone, true)
  assert.equal(tides.undoneAt, at(2))
  assert.equal(tides.siblings, 1)
  assert.equal(moss.confidence, null, 'no membership row for the write: no confidence is invented')
})

test('the minted list can be narrowed to one origin', () => {
  assert.deepEqual(mintedOf(ledger, ['jev']).map((x) => x.category), ['topic:tides', 'topic:kelp'])
})

test('an undo of a write names it, and the judgment link is its id', () => {
  const [moss, , tides] = mintedOf(ledger)
  assert.match(undoAsk(tides), /Undo the write j2 \(ontology\.undo\)\?/)
  assert.match(undoAsk(tides), /the category tides \(topic:tides\)/)
  assert.match(undoAsk(moss), /all 2 categories that write minted/)
  assert.equal(judgmentHref('j2'), '/judgment?id=j2')
  assert.equal(judgmentHref('a b&c'), '/judgment?id=a%20b%26c')
  assert.equal(confidenceWords(0.5), '0.50')
  assert.equal(confidenceWords(null), '—')
})

test('only the first undo of a write counts, and a row with no write is ignored', () => {
  const u = undoneWrites([undoRow(100, 'w'), undoRow(200, 'w'), row('ontology.undo', 300, { label: 'wrong' })])
  assert.deepEqual([...u], [['w', 100]])
})
