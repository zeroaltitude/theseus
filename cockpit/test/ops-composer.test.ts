// The Join pane's composer (theseus-jid7t.6): ⏎ steers a running turn, ⌘⏎ holds a message until the turn ends, esc drops it.
import test from 'node:test'
import assert from 'node:assert/strict'
import { composerHint, composerStep, type ComposerState } from '../src/ops/composer.ts'

const idle: ComposerState = { kind: 'idle' }
const running: ComposerState = { kind: 'running' }

test('⏎ with nothing running sends; while a turn runs it steers', () => {
  assert.deepEqual(composerStep(idle, { kind: 'enter', text: ' hello ' }).effect, { send: 'hello', steer: false })
  assert.deepEqual(composerStep(running, { kind: 'enter', text: 'go left' }).effect, { send: 'go left', steer: true })
  assert.equal(composerStep(running, { kind: 'enter', text: '  ' }).effect, null, 'nothing typed, nothing sent')
})

test('⌘⏎ while running holds the message until the turn ends, then sends it once', () => {
  const held = composerStep(running, { kind: 'mod_enter', text: 'then this' })
  assert.deepEqual(held.state, { kind: 'queued', text: 'then this' })
  assert.equal(held.effect, null, 'nothing is sent yet')
  assert.deepEqual(composerStep(held.state, { kind: 'turn_started' }).state, held.state, 'a start does not release it')
  const end = composerStep(held.state, { kind: 'turn_ended' })
  assert.deepEqual(end.effect, { send: 'then this', steer: false })
  assert.deepEqual(end.state, { kind: 'idle' })
  assert.equal(composerStep(end.state, { kind: 'turn_ended' }).effect, null, 'never twice')
})

test('⌘⏎ with nothing running is a plain send, and esc drops a held message', () => {
  assert.deepEqual(composerStep(idle, { kind: 'mod_enter', text: 'hi' }).effect, { send: 'hi', steer: false })
  const dropped = composerStep({ kind: 'queued', text: 'x' }, { kind: 'escape' })
  assert.deepEqual(dropped.state, running)
  assert.equal(composerStep(dropped.state, { kind: 'turn_ended' }).effect, null, 'dropped: nothing goes at the end')
  assert.deepEqual(composerStep(idle, { kind: 'escape' }).state, idle)
})

test('the composer says what ⏎ and ⌘⏎ do', () => {
  assert.match(composerHint(running), /⏎ steers.*⌘⏎ queues/)
  assert.match(composerHint({ kind: 'queued', text: 'x' }), /esc drops/)
  assert.match(composerHint(idle), /⏎ sends/)
})
