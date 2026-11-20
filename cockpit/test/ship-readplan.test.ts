// What the Ship reads (`src/ship/readplan.ts`, GitHub issue 37), run by `npm test`: with 300 conversations of which 5
// are live, the page's open reads those 5 and nothing else; a state at rest reads its newest twenty; "Load all" reads
// the rest; a ship that turns live is read; the page opens on Live whatever the address says. The calls are counted
// at a stand-in `call`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { countsOf } from '../src/lib/sessionState.ts'
import { createReader, drawnIds, openAddress, readLine, readPlan, READ_PARALLEL, STATE_READ_N } from '../src/ship/readplan.ts'

const NOW = 1_800_000_000_000
const session = (id: string, state: string, o: Record<string, unknown> = {}) => ({
  session_id: id, kind: 'conversation', state, last_active_ms: NOW, pending_confirms: 0, ...o,
}) as any

/** 300 conversations: 5 live (l0…l4), 100 quiet (q0 the newest), 195 retired (r0 the newest). */
function fleet() {
  const out = []
  for (let i = 0; i < 5; i++) out.push(session(`l${i}`, 'live', { last_active_ms: NOW - i }))
  for (let i = 0; i < 100; i++) out.push(session(`q${i}`, 'quiet', { last_active_ms: NOW - 1_000_000 - i * 1000 }))
  for (let i = 0; i < 195; i++) out.push(session(`r${i}`, 'retired', { last_active_ms: NOW - 9_000_000 - i * 1000 }))
  return out
}

/** A reader over a stand-in `call` that counts and records what is asked, and says when it is idle. */
function harness() {
  const calls: { method: string; params: any }[] = []
  let inFlight = 0
  let widest = 0
  const merged: string[] = []
  let idles = 0
  const reader = createReader({
    call: async (method, params) => {
      calls.push({ method, params })
      inFlight++
      widest = Math.max(widest, inFlight)
      await new Promise((r) => setTimeout(r, 1))
      inFlight--
      return { nodes: [] }
    },
    merge: (sid) => { merged.push(sid) },
    progress: () => {},
    idle: () => { idles++ },
  })
  const settled = async () => { for (let i = 0; i < 400 && (inFlight > 0 || idles === 0); i++) await new Promise((r) => setTimeout(r, 1)) }
  return { reader, calls, merged, widest: () => widest, idles: () => idles, settled, sessions: () => calls.map((c) => c.params.session_id as string) }
}

test('the open reads the nodes of the live ships and makes no other per-session read', async () => {
  const sessions = fleet()
  const plan = readPlan(sessions, 'live', false)
  assert.deepEqual(plan.ids, ['l0', 'l1', 'l2', 'l3', 'l4'])
  assert.equal(plan.total, 5)
  assert.equal(plan.rest, 0)
  const h = harness()
  h.reader.enqueue(plan.ids)
  await h.settled()
  assert.equal(h.calls.length, 5, 'one call per live ship, where it was 1 + 1 + 300')
  assert.ok(h.calls.every((c) => c.method === 'node.list' && c.params.session_id), 'every read names its session: no global read, no session.list')
  assert.deepEqual(h.sessions().sort(), ['l0', 'l1', 'l2', 'l3', 'l4'])
  assert.ok(h.widest() <= READ_PARALLEL, 'four at a time')
  assert.equal(h.idles(), 1)
})

test('a conversation that waits on the owner or works stays on the sea whatever its state', () => {
  const sessions = [
    ...fleet(),
    session('qwait', 'quiet', { pending_confirms: 1, last_active_ms: NOW - 99_000_000 }),
    session('qask', 'retired', { attention: { level: 'needs_you' }, last_active_ms: NOW - 99_000_000 }),
    session('qrun', 'quiet', { execution_state: 'running', last_active_ms: NOW - 99_000_000 }),
    session('qidle', 'quiet', { execution_state: 'idle', last_active_ms: NOW - 99_000_000 }),
  ]
  assert.deepEqual(readPlan(sessions, 'live', false).ids.slice(5).sort(), ['qask', 'qrun', 'qwait'], 'the three busy ones are read at open; the idle quiet one is not')
  assert.ok(!readPlan(sessions, 'live', false).ids.includes('qidle'))
})

test('a task is read with the conversation that started it, and a conversation of a state at rest counts alone', () => {
  const sessions = [
    session('l0', 'live'),
    session('t0', 'live', { kind: 'task', parent_session_id: 'l0' }),
    session('q0', 'quiet', { last_active_ms: NOW - 5 }),
    session('t1', 'quiet', { kind: 'task', parent_session_id: 'q0', last_active_ms: NOW - 6 }),
  ]
  assert.deepEqual(readPlan(sessions, 'live', false).ids, ['l0', 't0'])
  const quiet = readPlan(sessions, 'quiet', false)
  assert.deepEqual(quiet.ids, ['q0', 't1'])
  assert.equal(quiet.total, 1)
})

test('choosing quiet reads exactly the newest twenty quiet ships, and no more', async () => {
  const sessions = fleet()
  const plan = readPlan(sessions, 'quiet', false)
  assert.equal(plan.ids.length, STATE_READ_N)
  assert.deepEqual(plan.ids, Array.from({ length: STATE_READ_N }, (_, i) => `q${i}`), 'newest first')
  assert.equal(plan.total, 100)
  assert.equal(plan.rest, 80)
  const h = harness()
  h.reader.enqueue(readPlan(sessions, 'live', false).ids)
  h.reader.enqueue(plan.ids)
  await h.settled()
  assert.equal(h.calls.length, 5 + STATE_READ_N, 'the live five and the quiet twenty')
  assert.ok(!h.sessions().some((id) => id.startsWith('r')), 'no retired ship is read')
  // Retired too: twenty of its 195.
  const retired = readPlan(sessions, 'retired', false)
  assert.equal(retired.ids.length, STATE_READ_N)
  assert.equal(retired.rest, 175)
})

test('"All" reads every live ship and the newest twenty of the rest', () => {
  const plan = readPlan(fleet(), 'all', false)
  assert.equal(plan.ids.length, 5 + STATE_READ_N)
  assert.equal(plan.total, 300)
  assert.equal(plan.rest, 300 - 5 - STATE_READ_N)
})

test('"Load all" reads the rest of the state and nothing twice', async () => {
  const sessions = fleet()
  const h = harness()
  h.reader.enqueue(readPlan(sessions, 'live', false).ids)
  h.reader.enqueue(readPlan(sessions, 'quiet', false).ids)
  const all = readPlan(sessions, 'quiet', true)
  assert.equal(all.ids.length, 100)
  assert.equal(all.rest, 0, 'the control is gone for the state')
  h.reader.enqueue(all.ids)
  await h.settled()
  await new Promise((r) => setTimeout(r, 20))
  const asked = h.sessions()
  assert.equal(asked.length, 5 + 100)
  assert.equal(new Set(asked).size, asked.length, 'no session read twice')
  assert.ok(!asked.some((id) => id.startsWith('r')))
})

test('nothing loads the rest without the press: the plan for a state is the same on every poll', () => {
  const sessions = fleet()
  assert.deepEqual(readPlan(sessions, 'quiet', false), readPlan([...sessions].reverse(), 'quiet', false))
})

test('a conversation that turns live after the open is read when it does', async () => {
  const sessions = fleet()
  const h = harness()
  h.reader.enqueue(readPlan(sessions, 'live', false).ids)
  await h.settled()
  assert.equal(h.calls.length, 5)
  // The next poll says q7 is live: the plan names it, and only it is read.
  const later = sessions.map((s: any) => (s.session_id === 'q7' ? { ...s, state: 'live', last_active_ms: NOW + 5 } : s))
  const plan = readPlan(later, 'live', false)
  assert.equal(plan.ids.length, 6)
  assert.equal(plan.ids[0], 'q7')
  h.reader.enqueue(plan.ids)
  await new Promise((r) => setTimeout(r, 20))
  assert.deepEqual(h.sessions().slice(5), ['q7'])
})

test('the same poll twice reads nothing more, and a read that failed is asked again', async () => {
  const calls: string[] = []
  let fail = true
  const reader = createReader({
    call: async (_m, p) => { calls.push(p.session_id as string); if (fail) throw new Error('gone'); return { nodes: [] } },
    merge: () => {}, progress: () => {}, idle: () => {},
  })
  reader.enqueue(['a', 'b'])
  await new Promise((r) => setTimeout(r, 10))
  fail = false
  reader.enqueue(['a', 'b'])
  await new Promise((r) => setTimeout(r, 10))
  reader.enqueue(['a', 'b'])
  await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(calls, ['a', 'b', 'a', 'b'])
})

test('leaving the page stops the reads: nothing more is started or merged', async () => {
  const sessions = fleet()
  const h = harness()
  h.reader.enqueue(readPlan(sessions, 'quiet', true).ids)
  h.reader.stop()
  await new Promise((r) => setTimeout(r, 30))
  assert.ok(h.calls.length <= READ_PARALLEL, 'only the reads already under way')
  assert.equal(h.merged.length, 0)
  h.reader.enqueue(['x'])
  assert.ok(h.calls.length <= READ_PARALLEL)
})

test('the page opens on Live whatever state the address carries, and drops it', () => {
  const stored = openAddress(new URLSearchParams('st=quiet&s=ses_a'))
  assert.equal(stored.filter, 'live')
  assert.equal(stored.dropped, true)
  assert.equal(stored.search.get('st'), null)
  assert.equal(stored.search.get('s'), 'ses_a', 'the rest of the address stays')
  for (const st of ['retired', 'all', 'nonsense']) assert.equal(openAddress(new URLSearchParams(`st=${st}`)).filter, 'live')
  const plain = openAddress(new URLSearchParams('s=ses_a'))
  assert.equal(plain.dropped, false)
  assert.equal(plain.filter, 'live')
})

test('the count line and the state counts stay true while a state is part read', () => {
  const sessions = fleet()
  assert.deepEqual(countsOf(sessions), { live: 5, quiet: 100, retired: 195, all: 300 })
  const plan = readPlan(sessions, 'quiet', false)
  assert.equal(readLine('quiet', plan.total, plan.rest), '20 of 100 quiet ships shown')
  assert.equal(readLine('', 300, 275), '25 of 300 ships shown')
  assert.equal(readLine('quiet', 100, 0), null, 'nothing waits: no line')
})

test('what is drawn is what was read, and the visitors show whether read or not', () => {
  const sessions = fleet()
  const read = new Set(readPlan(sessions, 'quiet', false).ids)
  const drawn = drawnIds(sessions, 'quiet', [], read)
  assert.equal(drawn.size, STATE_READ_N, 'the 80 not read are not drawn as empty hulls')
  assert.ok(!drawn.has('q50'))
  const withVisitor = drawnIds(sessions, 'quiet', ['l2', 'q50'], read)
  assert.ok(withVisitor.has('l2') && withVisitor.has('q50'), 'the selected session shows whatever the filter and the read')
  assert.equal(withVisitor.size, STATE_READ_N + 2)
})

test('the hook reads no session-less node.list and nothing outside the plan; the page opens through openAddress', () => {
  const hook = readFileSync(new URL('../src/ship/useShipData.ts', import.meta.url), 'utf8')
  assert.ok(!/'node\.list',\s*\{\s*session_id:\s*null/.test(hook), 'no read of every session at once')
  assert.ok(!/call<[^>]*>\('session\.list'\)/.test(hook), 'no session.list of its own: the page polls it')
  assert.match(hook, /readPlan\(planned, filter, all\)/, 'the reads are the plan')
  const page = readFileSync(new URL('../src/views/Ship.tsx', import.meta.url), 'utf8')
  assert.match(page, /openAddress\(params\)[\s\S]{0,400}?opened \? filterOf\(params\.get\('st'\)\) : 'live'/, 'the Ship opens on Live')
})
