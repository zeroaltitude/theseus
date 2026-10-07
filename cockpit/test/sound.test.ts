// The Ship's sound (`src/ship/sound.ts`, theseus-hnof.2, the owner's C4), run by `npm test`: three cues on the daemon's
// own events, each once a change, a burst of oars rowing a few strokes, and off until the operator turns it on.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CUES, cueOf, cueOfRow, newEar, SPACING, soundOn } from '../src/ship/sound.ts'

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

test("a failure or a question only the ledger tells (a session the Ship does not watch) sounds from its row, once", () => {
  const ear = newEar()
  const row = (kind: string, sid: string, at: number) => ({ kind, session_id: sid, at_unix_ms: at })
  // A model the daemon cannot price fails before the turn runs: no push reaches the page, its row does.
  assert.equal(cueOfRow(ear, row('turn.failed', 's9', 5000), 6000, 1000), 'horn')
  // The push said it first: its row, a few seconds later, is the same event.
  assert.equal(cueOf(ear, 'confirm.requested', { session_id: 's8' }, 7000), 'bell')
  assert.equal(cueOfRow(ear, row('tool.confirm_requested', 's8', 7000), 9500, 1000), null)
  assert.equal(cueOfRow(ear, row('budget.asked', 's7', 20_000), 20_100, 1000), 'bell')
  // Rows from before sound was on are history, not news; other kinds are silent.
  assert.equal(cueOfRow(newEar(), row('turn.failed', 's9', 500), 6000, 1000), null)
  assert.equal(cueOfRow(newEar(), row('action.failed', 's9', 5000), 6000, 1000), null)
  assert.equal(cueOfRow(newEar(), row('turn.ended', 's9', 5000), 6000, 1000), null)
})

test('a question rings once even when its row comes more than the bell’s spacing after its push (theseus-n7ra, A7)', () => {
  const ear = newEar()
  assert.equal(cueOf(ear, 'confirm.requested', { session_id: 's1', correlation_id: 'act_1' }, 1000), 'bell')
  // Its ledger row, read 5 s later (past the bell's 4 s spacing): the same question, no second bell.
  assert.ok(5000 > SPACING.bell)
  assert.equal(cueOfRow(ear, { kind: 'tool.confirm_requested', session_id: 's1', at_unix_ms: 1000 }, 6000, 0), null)
  assert.equal(cueOfRow(ear, { kind: 'budget.asked', session_id: 's1', at_unix_ms: 1000 }, 9000, 0), null)
  // Another session's question rings.
  assert.equal(cueOfRow(ear, { kind: 'tool.confirm_requested', session_id: 's2', at_unix_ms: 6500 }, 6500, 0), 'bell')
  // And so does the same session's next question, once the change's window has passed.
  assert.equal(cueOf(ear, 'confirm.requested', { session_id: 's1', correlation_id: 'act_2' }, 12_000), 'bell')
  // A failure is the same: its push, then its row 5 s on, one horn.
  assert.equal(cueOf(ear, 'turn.failed', { session_id: 's3', turn_id: 't1' }, 20_000), 'horn')
  assert.equal(cueOfRow(ear, { kind: 'turn.failed', session_id: 's3', at_unix_ms: 20_000 }, 25_000, 0), null)
})

test('a failed tool call sounds no horn: its rose blade and pennant show it (theseus-n7ra, A11)', () => {
  const ear = newEar()
  for (const status of ['error', 'failed', 'denied', 'timeout', 'cancelled', 'ok']) {
    assert.equal(cueOf(ear, 'tool.ended', { session_id: 's1', correlation_id: 'a1', status }, 1000), null, status)
  }
  // Nor its ledger rows.
  for (const kind of ['tool.ended', 'tool.failed', 'action.failed', 'tool.result']) {
    assert.equal(cueOfRow(ear, { kind, session_id: 's1', at_unix_ms: 2000 }, 2000, 0), null, kind)
  }
  // The horn is for a turn: still there for one.
  assert.equal(cueOf(ear, 'turn.failed', { session_id: 's1' }, 3000), 'horn')
})
