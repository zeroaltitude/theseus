// The turn inspector's pure parts (`src/lib/inspect.ts`, theseus-x9swu): lanes, packing, the keyboard's walk, zoom and
// pan, the person's waits, and a loop's request against the one before.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { LANES, blockWords, breakpointParts, cacheShare, callTiming, laneOf, layout, loopDiff, pack, pan, personWaits, redactionMarks, rowsOfCall, signed, step, walk, zoom } from '../src/lib/inspect.ts'

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
    { position: 2, at_ms: 10_500, kind: 'tool.confirm_requested', data: { tool: 'proc.run', correlation_id: 'act_1' } },
    { position: 3, at_ms: 12_500, kind: 'action.confirm_answered', data: { answer: 'approve', correlation_id: 'act_1' } },
  ] as any
  const w = personWaits(rows, 10_000)
  assert.equal(w.length, 1)
  assert.equal(w[0].start, 500_000)
  assert.equal(w[0].end, 2_500_000)
  assert.equal(w[0].lane, 'person')
  const big = span('turn', 'turn', 0, 5_000_000, [span('loop 0', 'loop', 0, 5_000_000)])
  const l = layout(big, rows, null, 10_000)
  assert.ok(l.bars.some((b) => b.lane === 'person'))
  // A wait that outlasts the turn is drawn to the turn's end, and says how long it was.
  const long = [
    { position: 2, at_ms: 10_500, kind: 'tool.confirm_requested', data: { correlation_id: 'act_9' } },
    { position: 9, at_ms: 10_000 + 3_600_000, kind: 'action.confirm_answered', data: { correlation_id: 'act_9' } },
  ] as any
  const w2 = personWaits(long, 10_000)
  assert.equal(w2[0].name, 'waiting on you (confirm_answered, 60 min)')
  const l2 = layout(big, long, null, 10_000)
  assert.equal(l2.end, 5_000_000, 'the axis stays the turn\'s')
  assert.equal(l2.bars.find((b) => b.lane === 'person')!.end, 5_000_000)
  // The answer is the row with the question\'s correlation id, not the next answer row.
  const two = [
    { position: 1, at_ms: 10_000, kind: 'tool.confirm_requested', data: { correlation_id: 'a' } },
    { position: 2, at_ms: 11_000, kind: 'action.confirm_answered', data: { correlation_id: 'b' } },
    { position: 3, at_ms: 12_000, kind: 'action.confirm_answered', data: { correlation_id: 'a' } },
  ] as any
  assert.equal(personWaits(two, 10_000)[0].end, 2_000_000)
  assert.equal(personWaits(rows.slice(0, 2), 10_000)[0].name, 'waiting on you (unanswered)')
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

test('a loop\'s model call is timed from the trace: first byte, first token, the streaming after', () => {
  const t = span('turn', 'turn', 0, 1000, [
    span('loop 0', 'loop', 0, 1000, [
      span('provider.call', 'provider', 100, 900, [span('first_byte', 'mark', 250, 250), span('first_token', 'mark', 300, 300)]),
    ]),
  ])
  const { bars } = layout(t)
  assert.deepEqual(callTiming(bars, 0), { call: 800, firstByte: 150, firstToken: 200, streaming: 600 })
  assert.equal(callTiming(bars, 1), null)
})

test('the ledger rows of a call are those that carry its tool_use_id or its action\'s correlation id, and no others', () => {
  const rows = [
    { kind: 'action.planned', data: { correlation_id: 'act_1' } },
    { kind: 'tool.confirm_requested', data: { tool_use_id: 'tu_1' } },
    { kind: 'action.planned', data: { correlation_id: 'act_2' } },
    { kind: 'turn.ended', data: {} },
    { kind: 'x', data: null },
  ]
  assert.deepEqual(rowsOfCall(rows, 'tu_1', 'act_1').map((r) => r.kind), ['action.planned', 'tool.confirm_requested'])
  assert.deepEqual(rowsOfCall(rows, 'tu_9', null), [])
})

test('a cache breakpoint falls after the last part of its block', () => {
  const parts = [
    { block: 'header', name: 'persona' }, { block: 'header', name: 'assembly' },
    { block: 'tools', name: '33 tools' }, { block: 'conversation', name: '1 nodes' },
  ]
  assert.deepEqual([...breakpointParts(parts, ['header', 'conversation'])], [1, 3])
  assert.deepEqual([...breakpointParts(parts, ['persona'])], [0], 'a layer named by a part')
  assert.deepEqual([...breakpointParts(parts, ['nothing'])], [])
})

test('a continuation or an advancer is no tool call', () => {
  assert.equal(laneOf({ name: 'continuation', kind: 'tool' }), 'store')
  assert.equal(laneOf({ name: 'advancer', kind: 'advancer' }), 'store')
})

test('a request that carried a vault secret is a mark beside its loop\'s provider call', () => {
  const rows = [
    { position: 4, at_ms: 10_200, kind: 'request.redacted', data: { op: 'stream', loop: 0, names: ['anthropic_key'], count: 2, substituted: true } },
    { position: 5, at_ms: 10_300, kind: 'request.redacted', data: { op: 'count_tokens', loop: null, names: ['x'], count: 1, substituted: true } },
    { position: 6, at_ms: 10_400, kind: 'request.redacted', data: { op: 'stream', loop: 1, names: ['db_password'], count: 1, substituted: false } },
    { position: 7, at_ms: 10_450, kind: 'request.redacted', data: { op: 'compaction', provider: 'openai', model: 'gpt-summary', loop: 1, names: ['db_password'], count: 2, substituted: true } },
  ] as any
  const m = redactionMarks(rows, 10_000)
  assert.equal(m.length, 3, 'a token count is no loop\'s request, a compaction call is')
  assert.deepEqual(m.map((b) => [b.lane, b.loop, b.start]), [['marks', 0, 200_000], ['marks', 1, 400_000], ['marks', 1, 450_000]])
  assert.equal(m[2].name, 'request.redacted: db_password (substituted) in a compaction call to openai/gpt-summary')
  assert.equal(m[0].name, 'request.redacted: anthropic_key (substituted)')
  assert.equal(m[1].name, 'request.redacted: db_password (sent as written)')
  const big = span('turn', 'turn', 0, 5_000_000, [span('loop 0', 'loop', 0, 5_000_000)])
  assert.equal(layout(big, rows, null, 10_000).bars.filter((b) => b.name.startsWith('request.redacted')).length, 3)
})
