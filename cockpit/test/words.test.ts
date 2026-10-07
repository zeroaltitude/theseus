// The Ship's plain words (`src/ship/words.ts`, theseus-hnof), run by `npm test`: every shape says what it is.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  authorWord, benchLabel, benchLine, benchState, count, depthOf, harbourLine, keelTags, oarTag, outcome, plateLine, SHAPES, span, stateWord, usdShort,
  vesselNoun, vesselSea,
} from '../src/ship/words.ts'

const v = (o: Record<string, unknown>) => ({ kind: 'conversation', rig: 'anchor', state: 'waiting', ...o }) as any

test('a vessel says its state in plain words, with the sea word beside it', () => {
  assert.deepEqual(stateWord(v({ rig: 'sail' })), { word: 'working', tone: 'live', sea: 'under sail' })
  assert.equal(stateWord(v({ rig: 'sail', attention: { label: 'waiting on 1 call' } })).word, 'working · running 1 call')
  assert.equal(stateWord(v({ rig: 'lantern', attention: { label: 'confirm fs.write: create /tmp/x' } })).word, 'waiting for you · approve fs.write')
  assert.equal(stateWord(v({ rig: 'lantern', attention: { label: 'budget: $0 of $0.0000' } })).word, 'waiting for you · raise its budget')
  assert.equal(stateWord(v({ rig: 'flare', state: 'budget_exhausted' })).word, 'over its budget')
  assert.equal(stateWord(v({ rig: 'flare', state: 'failed' })).tone, 'fault')
  assert.equal(stateWord(v({ kind: 'task', state: 'complete' })).word, 'done')
  assert.equal(stateWord(v({})).word, 'idle')
  assert.equal(vesselNoun(v({ kind: 'task' })), 'task')
  assert.equal(vesselNoun(v({})), 'session')
})

test('a tool call says its outcome as its blade shows it', () => {
  const call = (o: Record<string, unknown>) => ({ kind: 'call', ...o }) as any
  assert.deepEqual(outcome(call({}), true), { word: 'ok', tone: 'ok' })
  assert.deepEqual(outcome(call({}), false), { word: 'pending', tone: 'idle' })
  assert.equal(outcome(call({ failed: true }), true).word, 'failed')
  assert.equal(outcome(call({ waiting: true }), false).word, 'waiting for you')
  assert.equal(outcome(call({ running: true }), false).word, 'job running')
  assert.equal(outcome(call({ collapsedAt: 5 }), true).word, 'stopped, verified')
  assert.equal(outcome(call({ external: true }), true).word, 'ok · text from the web')
})

test('counts, spans, and a turn line read as people say them', () => {
  assert.equal(count(1, 'turn'), '1 turn')
  assert.equal(count(17, 'turn'), '17 turns')
  assert.equal(count(2, 'model call'), '2 model calls')
  assert.equal(span(340), '340 ms')
  assert.equal(span(4400), '4.4 s')
  assert.equal(span(192_000), '3 m 12 s')
  assert.equal(span(7_500_000), '2 h 5 m')
  assert.equal(span(-1), '—')
  const usd = (n: number) => `$${n.toFixed(4)}`
  assert.equal(benchLine({ models: 2, calls: 3, failed: 1, cost: 0.0004 }, usd), '2 model calls · 3 tool calls, 1 failed · $0.0004')
  assert.equal(benchLine({ models: 1, calls: 0, failed: 0, cost: 0 }, usd), '1 model call · no tool calls')
})

test('a message says who wrote it', () => {
  assert.equal(authorWord(undefined), 'you')
  assert.equal(authorWord('operator (sock#19)'), 'you')
  assert.equal(authorWord('discord:pilot'), '@pilot')
  assert.equal(authorWord('task 1c8fbe'), 'a task')
  assert.equal(authorWord('sock#4'), 'you, from the CLI')
  assert.equal(authorWord('web#2'), 'you, from the web')
  // A task's report names its task; a task's brief comes from the session that started it.
  assert.equal(authorWord('task:e19294'), 'task e19294')
  assert.equal(authorWord('session:ses_01a1'), 'the session that started it')
  assert.equal(authorWord('harness'), 'the harness')
})

test('a turn says its state on its bench, and its calls when there is room', () => {
  const b = (o: Record<string, unknown>) => ({ n: 8, models: 2, calls: 3, failed: 0, cost: 0, running: false, waiting: false, ...o }) as any
  assert.deepEqual(benchState(b({})), { word: 'done', tone: 'idle' })
  assert.equal(benchLabel(b({}), false), 'turn 8')
  assert.equal(benchLabel(b({}), true), 'turn 8 · 3 tool calls')
  assert.equal(benchLabel(b({ calls: 0 }), true), 'turn 8')
  assert.equal(benchLabel(b({ failed: 1 }), false), 'turn 8 · 1 failed')
  assert.equal(benchLabel(b({ running: true, failed: 1 }), false), 'turn 8 · working')
  // What waits for the operator comes first: it is what the operator looks for.
  assert.equal(benchLabel(b({ running: true, waiting: true }), true), 'turn 8 · waits for you')
  assert.equal(benchState(b({ waiting: true })).tone, 'wait')
})

test('a nameplate, a harbour and an oar say what they are in one line', () => {
  const v = { kind: 'conversation', rig: 'anchor', state: 'waiting', turns: 17, cost: 0.0069 } as any
  assert.equal(plateLine(v), 'idle · 17 turns · $0.0069')
  assert.equal(plateLine({ ...v, rig: 'sail', turns: 1, cost: 0, hold: {} }), 'working · 1 turn · $0 · holds web text')
  assert.equal(harbourLine(1, 4), ' · 1 session · 4 tasks')
  assert.equal(harbourLine(3, 0), ' · 3 sessions')
  assert.equal(oarTag('fs.read', { word: 'ok' }), 'fs.read')
  assert.equal(oarTag('proc.run', { word: 'failed' }), 'proc.run · failed')
  assert.equal(oarTag(undefined, { word: 'pending' }), 'tool · pending')
  assert.equal(usdShort(0), '$0')
  assert.equal(usdShort(0.00042), '$0.0004')
  assert.equal(usdShort(0.015), '$0.015')
  assert.equal(usdShort(3.2), '$3.20')
})

test('every shape has a plain word, a noun and a sea word, and a vessel says which it is', () => {
  for (const [id, w] of Object.entries(SHAPES)) {
    assert.ok(w.word && w.noun && w.sea, id)
    assert.ok(!/\blights?\b/.test(`${w.word} ${w.sea}`), `"light" is gone from the chart's words (${id})`)
  }
  assert.equal(vesselSea({ kind: 'task' } as any), 'boat in tow')
  assert.equal(vesselSea({ kind: 'conversation' } as any), 'ship')
})

test('the depth gauge reads the camera as fleet, ship, turn, or call', () => {
  assert.equal(depthOf(120, false), 'fleet')
  assert.equal(depthOf(600, false), 'ship')
  assert.equal(depthOf(2400, false), 'turn')
  assert.equal(depthOf(2400, true), 'call')
})

test("a ship's keel says an author or a model only where it changes", () => {
  const L = [
    { vessel: 0, kind: 'user', author: 'sock#4' }, { vessel: 0, kind: 'model', model: 'claude-sonnet-5-5' }, { vessel: 0, kind: 'call' },
    { vessel: 0, kind: 'user', author: 'sock#9' }, { vessel: 0, kind: 'model', model: 'claude-sonnet-5-5' },
    { vessel: 0, kind: 'user', author: 'task:e19294' }, { vessel: 0, kind: 'model', model: 'claude-opus-5-5' },
    { vessel: 1, kind: 'user', author: 'sock#4' }, { vessel: 1, kind: 'model', model: 'claude-opus-5-5' },
  ]
  assert.deepEqual([...keelTags(L)], [
    [0, 'you, from the CLI'], [1, 'sonnet-5-5'], [5, 'task e19294'], [6, 'opus-5-5'], [7, 'you, from the CLI'], [8, 'opus-5-5'],
  ])
})
