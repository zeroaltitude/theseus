// The turn inspector's pure parts (`src/lib/inspect.ts`, theseus-x9swu): lanes, packing, the keyboard's walk, zoom and
// pan, the person's waits, and a loop's request against the one before.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { LANES, blockWords, cacheShare, laneOf, layout, loopDiff, pack, pan, personWaits, signed, step, walk, zoom } from '../src/lib/inspect.ts'

const span = (name: string, kind: string, start_us: number, end_us: number | null, children: unknown[] = []) =>
  ({ name, kind, start_us, end_us, attrs: null, children }) as any

const trace = span('turn', 'turn', 0, 1000, [
  span('admission.wait', 'lock', 0, 100),
  span('loop 0', 'loop', 100, 600, [
    span('compile', 'compile', 100, 150),
    span('provider.messages', 'provider', 150, 600, [span('first token', 'mark', 400, 400)]),
  ]),
  span('loop 1', 'loop', 600, 1000, [
    span('tools', 'tools', 600, 900, [span('tool fs.read', 'tool', 600, 700), span('tool proc.run', 'tool', 600, 900), span('tool fs.list', 'tool', 720, 800)]),
    span('job_wait', 'store', 900, 990),
    span('recall.query', 'store', 610, 640),
    span('judge route', 'store', 20, 40),
  ]),
])

test('a span goes to the lane its kind and name say', () => {
  assert.equal(laneOf({ name: 'loop 2', kind: 'loop' }), 'loops')
  assert.equal(laneOf({ name: 'provider.messages', kind: 'provider' }), 'model')
  assert.equal(laneOf({ name: 'tool fs.read', kind: 'tool' }), 'tools')
  assert.equal(laneOf({ name: 'job_wait', kind: 'store' }), 'waits')
  assert.equal(laneOf({ name: 'admission.wait', kind: 'lock' }), 'waits')
  assert.equal(laneOf({ name: 'first token', kind: 'mark' }), 'marks')
  assert.equal(laneOf({ name: 'context.compact', kind: 'store' }), 'compile')
  assert.equal(laneOf({ name: 'recall.query', kind: 'store' }), 'recall')
  assert.equal(laneOf({ name: 'judge route', kind: 'store' }), 'judges')
  assert.equal(laneOf({ name: 'action.settle', kind: 'store' }), 'store')
})

test('parallel calls stack in rows of their own and calls in turn share one', () => {
  const { bars, rowsIn } = layout(trace)
  const tools = bars.filter((b) => b.lane === 'tools' && b.kind === 'tool')
  const rowOf = (n: string) => tools.find((b) => b.name === n)!.row
  assert.notEqual(rowOf('tool fs.read'), rowOf('tool proc.run'), 'fs.read and proc.run overlap')
  assert.notEqual(rowOf('tool fs.list'), rowOf('tool proc.run'))
  assert.ok(rowsIn.tools >= 2)
  assert.equal(pack([
    { id: 'a', lane: 'tools', start: 0, end: 10, name: 'a', kind: 'tool', attrs: null, loop: null },
    { id: 'b', lane: 'tools', start: 10, end: 20, name: 'b', kind: 'tool', attrs: null, loop: null },
  ]).map((b) => b.row).join(), '0,0', 'calls in turn share a row')
})

test('a span knows its loop, and every lane is one the view draws', () => {
  const { bars } = layout(trace)
  assert.equal(bars.find((b) => b.name === 'tool fs.read')!.loop, 1)
  assert.equal(bars.find((b) => b.name === 'provider.messages')!.loop, 0)
  assert.ok(bars.every((b) => LANES.includes(b.lane)))
})

test('the keyboard walks the bars by start, and stops at the ends', () => {
  const { bars } = layout(trace)
  const first = step(bars, null, 1)!
  assert.equal(first.start, 0)
  const second = step(bars, first.id, 1)!
  assert.ok(second.start >= first.start)
  assert.equal(step(bars, first.id, -1)!.id, first.id)
  const last = walk(bars).at(-1)!
  assert.equal(step(bars, last.id, 1)!.id, last.id)
  assert.equal(step([], null, 1), null)
})

test('zoom keeps the point under the pointer and the window inside the axis; pan stops at the ends', () => {
  const w = zoom([0, 1000], 500, 0.5, 1000)
  assert.deepEqual(w, [250, 750])
  assert.deepEqual(zoom([0, 1000], 0, 0.5, 1000), [0, 500])
  assert.deepEqual(zoom(w, 500, 4, 1000), [0, 1000], 'zooming out is the whole axis')
  assert.deepEqual(pan([250, 750], -900, 1000), [0, 500])
  assert.deepEqual(pan([250, 750], 900, 1000), [500, 1000])
})

test('the time spent waiting on a person runs from the question to its answer', () => {
  const rows = [
    { position: 1, at_ms: 10_000, kind: 'turn.started', data: {} },
    { position: 2, at_ms: 10_500, kind: 'tool.confirm_requested', data: { tool: 'proc.run' } },
    { position: 3, at_ms: 12_500, kind: 'action.confirm_answered', data: { answer: 'approve' } },
  ] as any
  const w = personWaits(rows, 10_000)
  assert.equal(w.length, 1)
  assert.equal(w[0].start, 500_000)
  assert.equal(w[0].end, 2_500_000)
  assert.equal(w[0].lane, 'person')
  const l = layout(trace, rows, null, 10_000)
  assert.ok(l.bars.some((b) => b.lane === 'person'))
  assert.ok(l.end >= 2_500_000, 'the axis reaches the wait')
})

const loop = (i: number, tokens: number, messages: number, digest: string, layers: Record<string, string>, added: string[] = []) =>
  ({ loop: i, added, calls: [], answer: null, compiled: { est_tokens: tokens, messages, tools: 12, digest, decision: i ? 'append' : 'recompile', cache: { breakpoints: ['header', 'conversation'], layers } } }) as any

test('a loop against the one before: tokens, messages, the layers whose digest moved', () => {
  const a = loop(0, 10_000, 4, 'd1', { header: 'h1', project: 'p1' })
  const b = loop(1, 11_240, 7, 'd2', { header: 'h1', project: 'p2' }, ['msg_1', 'tcl_1', 'trs_1'])
  const d = loopDiff(a, b)
  assert.equal(d.tokens, 1240)
  assert.equal(d.messages, 3)
  assert.equal(d.tools, 0)
  assert.deepEqual(d.layers, ['project'])
  assert.ok(d.digestChanged)
  assert.equal(d.addedNodes, 3)
  assert.equal(loopDiff(undefined, a).tokens, null)
  assert.equal(signed(1240), '+1,240')
  assert.equal(signed(-3), '−3')
  assert.equal(signed(0), '±0')
  assert.equal(signed(null), '—')
})

test('a block is worded by its type and size, and the cache share is read over all the input', () => {
  assert.equal(blockWords({ type: 'thinking', thinking: 'abc' }), 'thinking · 3 chars')
  assert.equal(blockWords({ type: 'thinking', chars: 1200 }), 'thinking · 1,200 chars')
  assert.equal(blockWords({ type: 'tool_use', name: 'fs_read' }), 'tool_use · fs_read')
  assert.equal(cacheShare({ input_tokens: 100, cache_read_input_tokens: 800, cache_creation_input_tokens: 100 }), 0.8)
  assert.equal(cacheShare({ input_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }), null)
})
