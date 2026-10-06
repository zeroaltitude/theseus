// The Ship's plain words (`src/ship/words.ts`, theseus-hnof), run by `npm test`: every shape says what it is.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { authorWord, benchLine, count, depthOf, outcome, span, stateWord, vesselNoun } from '../src/ship/words.ts'

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
})

test('the depth gauge reads the camera as fleet, ship, turn, or call', () => {
  assert.equal(depthOf(120, false), 'fleet')
  assert.equal(depthOf(600, false), 'ship')
  assert.equal(depthOf(2400, false), 'turn')
  assert.equal(depthOf(2400, true), 'call')
})
