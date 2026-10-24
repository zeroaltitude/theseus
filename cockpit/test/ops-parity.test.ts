// The data-parity test (theseus-jid7t.3; the owner's rule: no data lost). Every datum and act of the Agenda, Tasks,
// Threads and Actions pages and of the TUI's board has a home (`src/ops/parity.ts`): the table names every id once,
// every row of 1a-1d renders now (on Operations, or where it stays), each home's source says so, and each card's fact
// function makes every field the old component showed, and the card draws it. The pattern is `wakeFacts` against the
// old Wakes panel (test/agenda.test.ts).
import test from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import type { ThreadRow } from '../src/protocol.gen/index.ts'
import { INVENTORY, PARITY, type FactFn } from '../src/ops/parity.ts'
import { boardRows } from '../src/ops/board.ts'
import { queueOf } from '../src/ops/queue.ts'
import { cardFacts, itemFacts, threadFacts, threadPanelFacts } from '../src/ops/facts.ts'
import { groupThreads } from '../src/lib/threadboard.ts'
import { board, NOW, view } from './ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../${f}`, import.meta.url), 'utf8')

test('every row of the inventory is in the table, once, and no other', () => {
  const ids = PARITY.map((r) => r.id)
  assert.equal(new Set(ids).size, ids.length, 'an id twice')
  assert.deepEqual([...ids].sort(), [...INVENTORY].sort())
  assert.equal(INVENTORY.length, 59)
  for (const r of PARITY) assert.ok(r.datum.length > 3, r.id)
})

test('every row of 1a-1d has a home that renders it now; 1e and herdr may wait on a later row, named', () => {
  for (const r of PARITY) {
    if (['1a', '1b', '1c', '1d'].includes(r.group)) assert.ok(r.home === 'here' || r.home === 'stays', `${r.id} has no home: ${r.home}`)
    else assert.match(r.home, /^(here|stays|pending: ops-(act|graph|timeline))$/, r.id)
    if (r.home === 'here' || (r.home === 'stays' && r.group !== 'herdr')) assert.ok(r.at.length > 0, `${r.id}: where?`)
  }
})

test('each home’s source renders what it is the home of', () => {
  for (const r of PARITY) {
    for (const [file, needle] of r.at) {
      assert.ok(existsSync(new URL(`../${file}`, import.meta.url)), `${r.id}: ${file} is gone`)
      assert.ok(src(file).includes(needle), `${r.id}: ${file} no longer has ${needle}`)
    }
  }
})

/** Each fact function over the invented board: the fields it makes, across every row, thread and item. */
function made(fn: FactFn): Set<string> {
  const b = board()
  const out = new Set<string>()
  const add = (o: object) => { for (const [k, v] of Object.entries(o)) if (v !== undefined && !(Array.isArray(v) && v.length === 0)) out.add(k) }
  const grouped = groupThreads([...b.views, view('act_pq', 'question', 'needs_you', { thread: 'paused-one' }, 'needs_you')],
    [...b.groups, { slug: 'paused-one', title: 'Paused one', state: 'paused', sealed: true, last_active_ms: NOW - 3600_000 }])
  const row: ThreadRow = {
    thread: {
      slug: 'harbour-survey', title: 'Harbour survey', goal: 'chart the harbour', topics: ['top_quay'], working_set: { repos: ['harbour'] }, card: ['walls mapped'], state: 'active',
      sealed: { by: 'the owner', reason: 'private charts', at_ms: 1 } as never, seal_pending: 'it read a private chart', parent: 'charts', links: ['tide-tables'], session_id: 'ses_home', last_active_ms: NOW, created_at_ms: 1,
    },
    context_tokens: 41_000, today_usd: 1.2,
  }
  if (fn === 'cardFacts') for (const r of boardRows({ ...b, now: NOW })) add(cardFacts(r, NOW))
  if (fn === 'threadFacts') for (const t of grouped.threads) add(threadFacts(t, NOW))
  if (fn === 'threadPanelFacts') add(threadPanelFacts(row, grouped.threads[0], NOW))
  if (fn === 'itemFacts') for (const i of queueOf({ confirms: b.confirms, views: b.views, groups: b.groups })) add(itemFacts(i, NOW))
  return out
}

test('every field the old component showed is made by its new home’s fact function, and drawn by its card', () => {
  for (const r of PARITY) {
    if (!r.facts) continue
    const got = made(r.facts.fn)
    const drawn = src(r.facts.drawn)
    for (const field of r.facts.fields) {
      assert.ok(got.has(field), `${r.id}: ${r.facts.fn} never makes ${field}`)
      assert.ok(drawn.includes(`f.${field}`), `${r.id}: ${r.facts.drawn} does not draw ${field}`)
    }
  }
  assert.ok(PARITY.filter((r) => r.facts).length >= 4, 'the task card, the thread, the thread panel and the queue card')
})

test('the Timeline lens hosts the Agenda unchanged, and the Graph lens the task graph', () => {
  const page = src('src/views/Ops.tsx')
  // The Timeline lens is its own now (ops-timeline, theseus-jid7t.5): the Agenda's page moved into it, whole.
  assert.match(page, /import \{ TimelineLens, type TimelineReport \} from '@\/ops\/TimelineLens'/)
  assert.ok(!existsSync(new URL('../src/views/Agenda.tsx', import.meta.url)), 'the Agenda moved, not copied')
  assert.match(page, /a\.lens === 'graph' && <GraphLens views=\{world \? graphViews : views\} records=\{records\}/)
  assert.match(src('src/components/TaskGraph.tsx'), /<Link to=\{opsHref\(\{ lens: 'graph', sel: pick \}\)\}[^>]*>open in Operations<\/Link>/)
})

test('ops-act left nothing pending: B5, B10, B11, B13, B14, B15 and R6 are here, R3 stays with its reason', () => {
  assert.deepEqual(PARITY.filter((r) => r.home === 'pending: ops-act').map((r) => r.id), [])
  const by = Object.fromEntries(PARITY.map((r) => [r.id, r]))
  for (const id of ['B5', 'B10', 'B11', 'B13', 'B14', 'B15', 'R6']) assert.equal(by[id].home, 'here', id)
  assert.equal(by.R3.home, 'stays')
  assert.match(by.R3.datum, /no honest way/)
})
