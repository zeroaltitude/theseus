// The simple Memories page's pure parts (`src/simple/memories/find.ts`), run by `npm test` with node's own runner, over
// invented people and topics; and where the page is wired (its tabs, and a session's own pages under the simple bar).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { OntologyCategory, OntologyMembership } from '@protocol'
import {
  FOUND, childrenOf, episodeHere, excerpt, exploreHref, exploreTopic, foundKinds, holdersOf, labelOf, lineOf, listAll, listHref, pageHref, pathOf,
  personHref, searchHref, sessionHref, topicHref, topicsOf, turnHere, wordsOf,
} from '../src/simple/memories/find.ts'

const cat = (id: string, name: string, over: Partial<OntologyCategory> = {}): OntologyCategory => ({
  id, kind: id.split(':')[0], name, depth: 1, description: '', added_by: 'owner', members: 0, ...over,
})

const CATS: OntologyCategory[] = [
  cat('topic:gardens', 'gardens', { members: 4 }),
  cat('topic:gardens/roses', 'roses', { parent: 'topic:gardens', depth: 2, members: 2 }),
  cat('topic:gardens/soil', 'soil', { parent: 'topic:gardens', depth: 2 }),
  cat('topic:boats', 'boats', { members: 9 }),
  cat('person:marlo', 'Marlo Quill', { handles: ['slack:U01', 'name:M. Quill'], members: 3, description: 'neighbour' }),
  cat('person:ines', 'Ines Vale', { handles: ['email:ines@example.test'], members: 7 }),
]

const mem = (session_id: string, category: string, as_of_ms: number): OntologyMembership => ({ session_id, kind: category.split(':')[0], category, origin: 'import', as_of_ms })

test('a search is its words, trimmed and on one line; an empty one has none', () => {
  assert.equal(wordsOf('  roses \n in   bloom '), 'roses in bloom')
  assert.equal(wordsOf(null), '')
  assert.equal(wordsOf('   '), '')
})

test('a search names the people and the topics that carry its words, by handle as well as name', () => {
  const f = foundKinds(CATS, 'quill')
  assert.deepEqual(f.people.map((c) => c.id), ['person:marlo'])
  assert.deepEqual(f.topics, [])
  assert.deepEqual(foundKinds(CATS, 'slack:u01').people.map((c) => c.id), ['person:marlo'])
  assert.deepEqual(foundKinds(CATS, 'ines.*example').people, [], 'a handle is matched by its words, not as a pattern')
  assert.deepEqual(foundKinds(CATS, 'roses').topics.map((c) => c.id), ['topic:gardens/roses'])
  assert.deepEqual(foundKinds(CATS, '  '), { people: [], topics: [], morePeople: 0, moreTopics: 0 }, 'no words name nothing')
})

test('a search that names more than it lists says how many more', () => {
  const many = Array.from({ length: FOUND + 3 }, (_, i) => cat(`topic:t${i}`, `tide ${i}`))
  const f = foundKinds(many, 'tide')
  assert.equal(f.topics.length, FOUND)
  assert.equal(f.moreTopics, 3)
  assert.equal(f.morePeople, 0)
})

test('topics list roots first, then the larger, then by name', () => {
  assert.deepEqual(topicsOf(CATS).map((c) => c.id), ['topic:boats', 'topic:gardens', 'topic:gardens/roses', 'topic:gardens/soil'])
  assert.deepEqual(topicsOf(CATS, 'garden').map((c) => c.id), ['topic:gardens', 'topic:gardens/roses', 'topic:gardens/soil'])
})

test('a topic knows where it sits: its chain from the root, its label, its children', () => {
  const roses = CATS[1]
  assert.deepEqual(pathOf(CATS, roses).map((c) => c.name), ['gardens', 'roses'])
  assert.equal(labelOf(CATS, roses), 'gardens/roses', 'the label `import.sessions` filters by')
  assert.deepEqual(childrenOf(CATS, 'topic:gardens').map((c) => c.name), ['roses', 'soil'])
  assert.deepEqual(childrenOf(CATS, 'topic:boats'), [])
  // A parent that is not in the list stops the chain instead of looping, and a cycle does too.
  const orphan = cat('topic:x', 'x', { parent: 'topic:missing' })
  assert.deepEqual(pathOf([orphan], orphan).map((c) => c.id), ['topic:x'])
  const a = cat('topic:a', 'a', { parent: 'topic:b' })
  const b = cat('topic:b', 'b', { parent: 'topic:a' })
  assert.ok(pathOf([a, b], a).length <= 32)
})

test('a list row says how a person is reached and where a topic sits', () => {
  assert.equal(lineOf(CATS, CATS[4]), 'slack:U01 · aka M. Quill')
  assert.equal(lineOf(CATS, CATS[1]), 'under gardens · 2 conversations')
  assert.equal(lineOf(CATS, CATS[0]), 'a main topic · 4 conversations')
  assert.equal(lineOf(CATS, CATS[2]), 'under gardens')
  assert.equal(lineOf(CATS, cat('person:solo', 'Solo', { description: 'a friend' })), 'a friend')
})

test('the sessions under a category split into imported and Theseus’s own, newest first, each once', () => {
  const ms: OntologyMembership[] = [
    mem('ses_ep1', 'topic:boats', 100), mem('ses_ep2', 'topic:boats', 300), mem('ses_own', 'topic:boats', 200),
    mem('ses_ep2', 'topic:boats', 50), mem('ses_ep9', 'topic:gardens', 999),
  ]
  const h = holdersOf(ms, 'topic:boats')
  assert.deepEqual(h.imported, ['ses_ep2', 'ses_ep1'])
  assert.deepEqual(h.native, ['ses_own'])
  assert.equal(h.total, 3, 'a session in a category twice is counted once')
  assert.deepEqual(holdersOf(ms, 'person:nobody'), { imported: [], native: [], total: 0 })
})

test('an excerpt is cut at a word and says it was cut', () => {
  assert.equal(excerpt('one  two\nthree'), 'one two three')
  const long = 'word '.repeat(100)
  const x = excerpt(long, 50)
  assert.ok(x.endsWith('…') && x.length <= 51, x)
  assert.ok(!x.slice(0, -1).endsWith(' w'), 'not mid-word')
})

test('every address the page writes stays on the simple pages, except a session’s, which is theirs too', () => {
  assert.equal(searchHref('  rose  garden '), '/memories?q=rose+garden')
  assert.equal(personHref('person:marlo'), '/memories?v=people&sel=person%3Amarlo')
  assert.equal(topicHref('topic:gardens/roses'), '/memories?v=topics&sel=topic%3Agardens%2Froses')
  assert.equal(pageHref(CATS[4]), personHref('person:marlo'))
  assert.equal(pageHref(CATS[0]), topicHref('topic:gardens'))
  assert.equal(listHref('topic', 'topic:boats', 'bo'), '/memories?v=topics&sel=topic%3Aboats&f=bo')
  assert.equal(listHref('person', 'person:ines', ''), '/memories?v=people&sel=person%3Aines', 'no words, no key')
  assert.equal(listAll('person', 'quill'), '/memories?v=people&f=quill')
  assert.equal(episodeHere('ses_ep1'), '/memories?v=context&episode=ses_ep1')
  assert.equal(exploreHref('roses'), '/memories?v=context&q=roses')
  assert.equal(exploreTopic('gardens/roses'), '/memories?v=context&topic=gardens%2Froses')
  assert.equal(turnHere('ses_1'), '/memories?v=turn&session=ses_1')
  assert.equal(turnHere('ses_1', 'trn 2'), '/memories?v=turn&session=ses_1&turn=trn+2')
  assert.equal(sessionHref('ses_1'), '/session/ses_1')
})

const read = (p: string) => readFileSync(new URL(`../src/${p}`, import.meta.url), 'utf8')

test('Memories opens on the search, then people, topics and a turn, and keeps the explorer and the shelves behind them', () => {
  const m = read('simple/Memories.tsx')
  const ids = [...m.matchAll(/\{ id: '(\w+)'/g)].map((x) => x[1])
  assert.deepEqual(ids, ['search', 'people', 'topics', 'turn', 'context', 'books'], 'the first tab is the address with no `?v=`; `context` and `books` keep the ids old links name')
})
