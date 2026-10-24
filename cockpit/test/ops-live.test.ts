// Live and the dock (theseus-jid7t.6): the model's words from the push, a job's chunks, the three streams merged, and the
// dock's stream with its pause and its follow.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { NarrativeLine } from '../src/protocol.gen/index.ts'
import { appendChunk, dockStream, EMPTY_OUT, headroom, liveOf, mergeLive, modelsStrip, narrativeOf, peekOut, whoOf } from '../src/ops/live.ts'

const ev = (method: string, params: Record<string, unknown>, at = 0) => ({ method, at, params })
const line = (seq: number, session_id: string | undefined, text: string, at = seq * 1000): NarrativeLine => ({ seq, at_unix_ms: at, part: 'turn', session_id, text } as NarrativeLine)

test('liveOf: the loop’s words and the tools running; a turn’s end clears it; another session’s events do not count', () => {
  const evs = [ev('turn.started', { session_id: 'ses_a', turn_id: 't1' }), ev('model.delta', { turn_id: 't1', text: 'Tracing the ' }), ev('model.delta', { turn_id: 't1', text: 'quay.' }), ev('tool.started', { turn_id: 't1', tool_use_id: 'u1', tool: 'fs.read' }, 5),
    ev('model.delta', { turn_id: 'other', text: 'NOT' })]
  const l = liveOf(evs, 'ses_a')!
  assert.equal(l.text, 'Tracing the quay.')
  assert.deepEqual(l.running.map((r) => r.tool), ['fs.read'])
  assert.equal(liveOf(evs, 'ses_b'), null)
  assert.equal(liveOf([...evs, ev('turn.ended', { turn_id: 't1' })], 'ses_a'), null)
  assert.equal(liveOf([...evs, ev('loop.started', { turn_id: 't1' })], 'ses_a')!.text, '')
})

test('job chunks: whole lines kept, the unfinished one held, a repeated chunk adds nothing', () => {
  let o = peekOut(['one', 'two'], 8)
  o = appendChunk(o, { offset: 8, text: 'thr' })
  assert.deepEqual([o.lines, o.partial], [['one', 'two'], 'thr'])
  o = appendChunk(o, { offset: 11, text: 'ee\nfou' })
  assert.deepEqual([o.lines, o.partial, o.offset], [['one', 'two', 'three'], 'fou', 17])
  assert.equal(appendChunk(o, { offset: 8, text: 'thr' }), o, 'a repeat is nothing')
  const over = appendChunk(o, { offset: 11, text: 'ee\nfour\n' })
  assert.deepEqual(over.lines, ['one', 'two', 'three', 'four'])
  assert.equal(appendChunk(EMPTY_OUT, { offset: 0, text: '' }), EMPTY_OUT)
})

test('the three streams merge by time, the newest kept', () => {
  const narr = [line(1, 'ses_a', 'a opened a file', 1000), line(2, 'ses_a', 'a ran tests', 3000)]
  const model = { turn_id: 't', text: 'Checking', running: [{ id: 'u', tool: 'proc.run', startedAt: 2000 }] }
  const out = mergeLive(narr, model, { lines: ['build ok'], partial: 'warn', offset: 0 }, 9000)
  assert.deepEqual(out.map((x) => `${x.src}:${x.text}`), ['narrative:a opened a file', 'model:running proc.run', 'narrative:a ran tests', 'model:Checking', 'job:build ok', 'job:warn'])
  assert.equal(mergeLive(narr, model, null, 9000, 2).length, 2)
  assert.deepEqual(narrativeOf([...narr, line(3, 'ses_b', 'b')], 'ses_a').length, 2)
})

test('the dock: newest first, a pause holds its line, a follow shows one session', () => {
  const all = [line(1, 'ses_a', 'a1'), line(2, 'ses_b', 'b1'), line(3, 'ses_a', 'a2'), line(4, undefined, 'd')]
  assert.deepEqual(dockStream(all, { follow: null, pausedAt: null }).map((l) => l.text), ['d', 'a2', 'b1', 'a1'])
  assert.deepEqual(dockStream(all, { follow: null, pausedAt: 2 }).map((l) => l.text), ['b1', 'a1'], 'paused: later lines wait')
  assert.deepEqual(dockStream(all, { follow: 'ses_a', pausedAt: null }).map((l) => l.text), ['a2', 'a1'])
  assert.equal(whoOf(all[3]), 'daemon')
})

test('the models strip: sessions on each profile and the newest call’s headroom, the tightest gauge', () => {
  assert.equal(headroom({ requests_remaining: 900, requests_limit: 1000, tokens_remaining: 20_000, tokens_limit: 100_000 }), '20% of tokens left')
  assert.equal(headroom({}), null)
  const profiles = [{ name: 'quick', model: 'm-s', live: true, origin: 'us' }, { name: 'kimi', model: 'm-cn', live: false, origin: 'cn' }, { name: 'idle', model: 'm-i', live: false }]
  const calls = [{ model: 'm-s', rate: { tokens_remaining: 10, tokens_limit: 100 } }, { model: 'm-s', rate: { tokens_remaining: 50, tokens_limit: 100 } }, { model: 'm-cn' }]
  const strip = modelsStrip(profiles, [{ profile: 'quick' }, { profile: 'quick' }, { profile: 'quick', retired: {} }, { profile: 'kimi' }], calls)
  assert.deepEqual(strip.map((r) => [r.name, r.sessions, r.headroom, r.origin]), [['quick', 2, '50% of tokens left', 'us'], ['kimi', 1, null, 'cn'], ['idle', 0, null, 'unknown']])
})
