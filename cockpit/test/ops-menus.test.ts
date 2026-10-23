// The model and mode menus (theseus-jid7t.6): rows with their origin, cost hint and the sessions on them; the floor's
// greying; the sticky choice read from the ledger; the toast and its Undo.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ProfileInfo } from '../src/protocol.gen/index.ts'
import { costHint, MODES, modeAsk, profileRows, profileToast, stickyOf } from '../src/ops/menus.ts'
import { session } from './ops-fixtures.ts'

const prof = (name: string, model: string, origin: string, live = false): ProfileInfo => ({ name, provider: 'p', model, max_output_tokens: 1, has_system: false, live, origin })
const profiles = [prof('quick', 'm-small', 'us', true), prof('deep', 'm-big', 'us'), prof('kimi', 'm-cn', 'cn'), prof('glm', 'm-cn2', 'cn')]
const price = (input: number, output: number) => ({ input, output, cacheRead: 0, cacheWrite: 0, cacheWrite1h: 0 })
const prices = new Map([['m-small', price(1, 5)], ['m-big', price(4, 20)], ['m-cn', price(0.5, 2)]])

test('rows: origin, sessions on it, cost hint, and the floor’s grey for a sensitive session', () => {
  const sessions = [session('ses_a', { profile: 'quick' }), session('ses_b', { profile: 'quick' }), session('ses_c', { profile: 'deep' })]
  const rows = profileRows(profiles, sessions, 'sensitive', 'quick', prices)
  const by = Object.fromEntries(rows.map((r) => [r.name, r]))
  assert.equal(by.quick.onIt, 2)
  assert.equal(by.quick.current, true)
  assert.equal(by.deep.hint, '~4× the cost')
  assert.equal(by.kimi.hint, '~2× cheaper')
  assert.equal(by.quick.grey, null)
  assert.equal(by.deep.grey, null)
  assert.match(by.kimi.grey!, /holds sensitive content, and `kimi` is not a model of US origin/)
  assert.match(by.glm.grey!, /US origin/)
  assert.deepEqual(profileRows(profiles, sessions, 'open', 'quick', prices).map((r) => r.grey), [null, null, null, null], 'an open session may use any')
})

test('a cost hint only when both are priced and the gap is real', () => {
  assert.equal(costHint(undefined, price(1, 1)), '')
  assert.equal(costHint(price(1.1, 1), price(1, 1)), '')
  assert.equal(costHint(price(9, 9), price(1, 1)), '~9× the cost')
})

test('the sticky profile is the newest session.profile_set row’s to; a let-go reads null', () => {
  const row = (session_id: string, to: string | null) => ({ kind: 'session.profile_set', session_id, data: { to } })
  const rows = [row('ses_a', 'deep'), row('ses_b', 'quick'), row('ses_a', 'quick'), { kind: 'turn.ended', session_id: 'ses_a', data: null }]
  assert.equal(stickyOf(rows, 'ses_a'), 'quick')
  assert.equal(stickyOf([...rows, row('ses_a', null)], 'ses_a'), null)
  assert.equal(stickyOf(rows, 'ses_zzz'), null)
})

test('the toast: the floor’s refusal changes nothing; a set offers Undo with the previous; a note rides along', () => {
  assert.deepEqual(profileToast({ refused: 'no', profile: null, previous: null }, 'kimi'), { text: 'no', undo: undefined })
  assert.deepEqual(profileToast({ profile: 'deep', previous: 'quick' }, 'deep'), { text: 'from now on: deep', undo: 'quick' })
  assert.deepEqual(profileToast({ profile: 'deep', previous: null, note: 'floor decides' }, 'deep'), { text: 'from now on: deep · floor decides', undo: null })
  assert.equal(profileToast({ profile: null, previous: 'deep' }, null).text, 'back to the default')
  assert.equal(profileToast({ profile: 'deep', previous: 'deep' }, 'deep').undo, undefined, 'unchanged: nothing to undo')
})

test('four modes, and only Auto asks first', () => {
  assert.deepEqual(MODES.map((m) => m.id), ['ask', 'edits', 'plan', 'auto'])
  assert.ok(modeAsk('auto') && !modeAsk('plan') && !modeAsk('ask'))
})
