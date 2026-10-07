// The Ship's sound (`src/ship/sound.ts`, theseus-hnof.2, the owner's C4), run by `npm test`: three cues on the daemon's
// own events, each once a change, a burst of oars rowing a few strokes, and off until the operator turns it on.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CUES, cueOf, newEar, SPACING, soundOn } from '../src/ship/sound.ts'

const view = (sid: string, state: string, level: string, previous?: string) =>
  ({ session_id: sid, state, attention: { level, label: '', since_ms: 0 }, ...(previous ? { previous } : {}) })

test('off by default: only the operator turns it on', () => {
  assert.equal(soundOn(null), false)
  assert.equal(soundOn('off'), false)
  assert.equal(soundOn('on'), true)
})

test('an oar going out splashes, and a burst of them rows a few strokes, not one each', () => {
  const ear = newEar()
  assert.equal(cueOf(ear, 'tool.started', { session_id: 's1', correlation_id: 'a1' }, 1000), 'oar')
  let n = 0
  for (let k = 1; k <= 10; k++) if (cueOf(ear, 'tool.started', { session_id: 's1' }, 1000 + k * 50)) n++
  assert.ok(n >= 1 && n <= 3, `ten calls in half a second: ${n} splashes`)
  assert.equal(cueOf(ear, 'tool.started', {}, 1000 + 10 * 50 + SPACING.oar), 'oar')
  // Other pushes are silent: a result, a node, a model's stream, a turn that ends well.
  for (const m of ['tool.ended', 'node.written', 'model.delta', 'turn.started', 'turn.ended', 'task.changed']) assert.equal(cueOf(ear, m, { session_id: 's1' }, 9e9), null, m)
})

test('something waiting for you rings the bell once: the question and its execution are one event', () => {
  const ear = newEar()
  assert.equal(cueOf(ear, 'execution.changed', view('s1', 'running', 'working'), 0), null, 'the first view is not news')
  assert.equal(cueOf(ear, 'confirm.requested', { session_id: 's1', correlation_id: 'act_1' }, 1000), 'bell')
  assert.equal(cueOf(ear, 'execution.changed', view('s1', 'waiting', 'needs_you', 'running'), 1200), null, 'one ring for one question')
  // A budget question in another session, a little later, rings again.
  assert.equal(cueOf(ear, 'execution.changed', view('s2', 'running', 'working'), 2000), null)
  assert.equal(cueOf(ear, 'execution.changed', view('s2', 'waiting', 'needs_you', 'running'), 6000), 'bell')
  // Still waiting, the same level again: no news.
  assert.equal(cueOf(ear, 'execution.changed', view('s2', 'waiting', 'needs_you', 'waiting'), 20_000), null)
})

test('a failure sounds the low horn once, whether the turn or its execution says it first', () => {
  const ear = newEar()
  assert.equal(cueOf(ear, 'turn.failed', { session_id: 's1', turn_id: 't1' }, 1000), 'horn')
  assert.equal(cueOf(ear, 'execution.changed', view('s1', 'failed', 'needs_you', 'running'), 1100), null)
  // Over its budget is a failure too, and needs you: the horn, not the bell.
  assert.equal(cueOf(ear, 'execution.changed', view('s2', 'budget_exhausted', 'needs_you', 'running'), 9000), 'horn')
  // A view of an execution already failed when the page opened is not news.
  assert.equal(cueOf(newEar(), 'execution.changed', view('s3', 'failed', 'needs_you'), 0), null)
})

test('each cue names what sounds it and where its sound comes from', () => {
  assert.deepEqual(CUES.map((c) => c.cue), ['oar', 'bell', 'horn'])
  for (const c of CUES) assert.match(c.source, /synthesized in the browser/)
})
