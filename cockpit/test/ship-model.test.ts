// The Ship's model as benches and the key's lines (`src/ship/model.ts`, `src/ship/keyset.ts`, theseus-hnof), run by
// `npm test`: each turn a bench stern to bow, each tool call an oar of its bench, and every key line naming its shapes.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { buildModel } from '../src/ship/model.ts'
import { keyLines } from '../src/ship/keyset.ts'

const NOW = 1_800_000_000_000
const session = (id: string, o: Record<string, unknown> = {}) => ({
  session_id: id, kind: 'conversation', label: 'DM @owner', title: `title ${id}`, created_at_unix_ms: NOW - 60_000, last_active_ms: NOW - 1000,
  turns: 2, tool_calls: 3, cost_usd: 0.01, pending_confirms: 0, execution_id: `exe_${id}`, execution_state: 'waiting',
  usage: { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }, ...o,
}) as any
let pos = 0
const node = (sid: string, turn: string, kind: string, detail: Record<string, unknown> = {}, text = '') => ({
  node_id: `n${pos}`, kind, session_id: sid, position: pos, at_unix_ms: NOW - 50_000 + 1000 * pos++, turn_id: turn, text, detail, bytes: 0,
}) as any
const input = (o: Record<string, unknown>) => ({
  sessions: [], executions: [], tasks: [], nodes: [], confirms: [], l1: new Set<string>(), jobsRunning: new Set<string>(),
  streaming: new Map(), failedAt: new Map(), reports: new Map(), born: new Map(), bornSessions: new Map(), reserved: new Map(),
  reach: [], now: NOW, ...o,
}) as any

pos = 0
const nodes = [
  node('s1', 't1', 'user_message', {}, 'What do the tide tables say?'),
  node('s1', 't1', 'assistant_message', { model: 'claude-sonnet-5-5', cost_usd: 0.002 }),
  node('s1', 't1', 'tool_call', { tool: 'fs.read', tool_use_id: 'u1', correlation_id: 'act_1' }),
  node('s1', 't1', 'tool_result', { tool: 'fs.read', tool_use_id: 'u1', status: 'ok' }),
  node('s1', 't1', 'assistant_message', { model: 'claude-sonnet-5-5', cost_usd: 0.001 }),
  node('s1', 't2', 'user_message', {}, 'Is the tide gauge working?'),
  node('s1', 't2', 'assistant_message', { model: 'claude-sonnet-5-5', cost_usd: 0.002 }),
  node('s1', 't2', 'tool_call', { tool: 'proc.run', tool_use_id: 'u2', correlation_id: 'act_2' }),
  node('s1', 't2', 'tool_result', { tool: 'proc.run', tool_use_id: 'u2', status: 'error', is_error: true }),
  node('s1', 't2', 'tool_call', { tool: 'fs.write', tool_use_id: 'u3', correlation_id: 'act_3' }),
]

test('each turn is a bench, stern to bow, with its calls, failures and cost', () => {
  const m = buildModel(input({ sessions: [session('s1')], nodes }))
  assert.equal(m.benches.length, 2)
  const [a, b] = m.benches
  assert.deepEqual([a.n, a.turnId, a.models, a.calls, a.failed], [1, 't1', 2, 1, 0])
  assert.deepEqual([b.n, b.turnId, b.models, b.calls, b.failed], [2, 't2', 1, 2, 1])
  assert.ok(Math.abs(a.cost - 0.003) < 1e-9)
  assert.equal(a.preview, 'What do the tide tables say?')
  // The older turn sits aft of the newer, and they do not overlap.
  assert.ok(a.x + a.half <= b.x - b.half + 1e-9, `${a.x}+${a.half} vs ${b.x}-${b.half}`)
  // Every light knows its bench; a result rides its call's oar.
  for (const l of m.lights) assert.equal(m.benches[l.bench].turnId, l.turnId)
  const r = m.lights.find((l) => l.kind === 'result' && l.toolUseId === 'u1')!
  assert.ok(r.ox !== undefined)
  assert.equal(m.vessels[0].activeBench, -1)
})

test('the running turn is the one the pushes name, or the newest of a vessel that works', () => {
  const named = buildModel(input({ sessions: [session('s1')], nodes, active: new Map([['s1', 't1']]) }))
  assert.equal(named.benches[named.vessels[0].activeBench].turnId, 't1')
  assert.equal(named.benches[0].running, true)
  const working = buildModel(input({ sessions: [session('s1', { attention: { level: 'working', label: 'working', since_ms: NOW } })], nodes }))
  assert.equal(working.benches[working.vessels[0].activeBench].turnId, 't2')
})

test('a call that waits for the operator marks its oar and its bench', () => {
  const confirms = [{ correlation_id: 'act_3', session_id: 's1', execution_id: 'exe_s1', tool: 'fs.write', input: {}, reason: '', by: '', requested_at_ms: NOW, expires_at_ms: NOW + 1, floor: false }]
  const m = buildModel(input({ sessions: [session('s1')], nodes, confirms }))
  const w = m.lights.find((l) => l.correlationId === 'act_3')!
  assert.equal(w.waiting, true)
  assert.equal(m.benches[w.bench].waiting, true)
  assert.equal(m.vessels[0].rig, 'lantern')
})

test('every key line names the shapes it lights', () => {
  const confirms = [{ correlation_id: 'act_3', session_id: 's1', execution_id: 'exe_s1', tool: 'fs.write', input: {}, reason: '', by: '', requested_at_ms: NOW, expires_at_ms: NOW + 1, floor: false }]
  const m = buildModel(input({ sessions: [session('s1'), session('s2', { label: null, kind: 'task', parent_session_id: 's1' })], nodes, confirms, jobsRunning: new Set(['act_2']) }))
  const by = new Map(keyLines(m).map((k) => [k.id, k]))
  assert.equal(by.get('session')!.count, 1)
  assert.deepEqual(by.get('task')!.vessels, ['s2'])
  assert.equal(by.get('turn')!.count, 2)
  assert.equal(by.get('call')!.count, 3)
  // The failed result lights its call, the oar it is drawn from.
  assert.deepEqual(by.get('failed')!.lights, [m.lights.find((l) => l.kind === 'call' && l.toolUseId === 'u2')!.id])
  assert.equal(by.get('waiting')!.count, 1)
  assert.equal(by.get('job')!.count, 1)
  assert.deepEqual(by.get('needs')!.vessels, ['s1'])
  assert.equal(by.get('place')!.lights.length + by.get('place')!.vessels.length, 0)
})

test('a harbour keeps its place as its ships grow, and moves only when a growing neighbour would overlap it', () => {
  const ss = [
    session('a', { label: 'DM @owner', created_at_unix_ms: NOW - 60_000 }),
    session('b', { label: 'discord #ops', created_at_unix_ms: NOW - 50_000 }),
    session('c', { label: 'web', created_at_unix_ms: NOW - 40_000 }),
    session('d', { label: 'tui', created_at_unix_ms: NOW - 30_000 }),
  ]
  const turn = (sid: string, t: string) => [
    node(sid, t, 'user_message', {}, 'Check the tides.'),
    node(sid, t, 'assistant_message', { model: 'claude-sonnet-5-5', cost_usd: 0.001 }),
    node(sid, t, 'tool_call', { tool: 'fs.read', tool_use_id: `u_${sid}${t}`, correlation_id: `act_${sid}${t}` }),
    node(sid, t, 'tool_result', { tool: 'fs.read', tool_use_id: `u_${sid}${t}`, status: 'ok' }),
  ]
  // The first harbour grows a turn at a time. A harbour whose old place is still clear of those placed before it must
  // stay there; none may overlap another. Returns how often a harbour moved though its place was clear, and how often
  // one was pushed.
  const grow = (seen?: Map<string, { x: number; z: number }>) => {
    pos = 0
    let nodes = [...turn('a', 'a1'), ...turn('b', 'b1'), ...turn('c', 'c1'), ...turn('d', 'd1')]
    let prev = buildModel(input({ sessions: ss, nodes, placesSeen: seen })).formations
    let strayed = 0
    let pushed = 0
    for (let k = 2; k < 40; k++) {
      nodes = [...nodes, ...turn('a', `a${k}`)]
      const fs = buildModel(input({ sessions: ss, nodes, placesSeen: seen })).formations
      fs.forEach((f, i) => {
        const was = prev.find((p) => p.key === f.key)!
        const blocked = fs.slice(0, i).some((p) => Math.hypot(p.x - was.x, p.z - was.z) <= p.radius + f.radius + 3.5 * 1.6)
        if (f.x !== was.x || f.z !== was.z) { if (blocked) pushed++; else strayed++ }
        for (const g of fs.slice(0, i)) assert.ok(Math.hypot(f.x - g.x, f.z - g.z) > f.radius + g.radius, `${f.key} overlaps ${g.key}`)
      })
      assert.deepEqual([fs[0].x, fs[0].z], [0, 0], 'the first harbour stays at the centre')
      prev = fs
    }
    return { strayed, pushed }
  }
  const kept = grow(new Map())
  assert.equal(kept.strayed, 0)
  assert.ok(kept.pushed > 0, 'the growing harbour pushed a neighbour at least once')
  // Without the memory, the spiral re-places harbours whose places were still clear: the jump this fixes.
  assert.ok(grow().strayed > 0)
})
