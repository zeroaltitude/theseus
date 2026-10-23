// Following a job (theseus-jid7t.6): peek then follow, and unfollow on leaving, never twice.
import test from 'node:test'
import assert from 'node:assert/strict'
import { followOf, type Follow, type FollowEffect, type FollowEvent } from '../src/ops/follow.ts'

function run(events: FollowEvent[]): { state: Follow; log: FollowEffect[] } {
  let state: Follow = { kind: 'off' }
  const log: FollowEffect[] = []
  for (const e of events) { const r = followOf(state, e); state = r.state; log.push(...r.effects) }
  return { state, log }
}

test('selecting a job peeks it, and the peek starts the follow from its bytes', () => {
  const r = run([{ kind: 'select', job: 'act_1' }, { kind: 'peeked', job: 'act_1', bytes: 120 }])
  assert.deepEqual(r.log, [{ op: 'peek', job: 'act_1' }, { op: 'follow', job: 'act_1', from: 120 }])
  assert.deepEqual(r.state, { kind: 'following', job: 'act_1', from: 120 })
})

test('following unsubscribes on leave, and never twice', () => {
  const r = run([{ kind: 'select', job: 'act_1' }, { kind: 'peeked', job: 'act_1', bytes: 0 }, { kind: 'leave' }, { kind: 'leave' }, { kind: 'select', job: null }])
  assert.equal(r.log.filter((e) => e.op === 'unfollow').length, 1)
  assert.deepEqual(r.state, { kind: 'off' })
})

test('moving to another job unfollows the first before peeking the second; the same job again does nothing', () => {
  const r = run([{ kind: 'select', job: 'act_1' }, { kind: 'peeked', job: 'act_1', bytes: 5 }, { kind: 'select', job: 'act_1' }, { kind: 'select', job: 'act_2' }])
  assert.deepEqual(r.log.slice(2), [{ op: 'unfollow', job: 'act_1' }, { op: 'peek', job: 'act_2' }])
})

test('a peek that answers after the selection moved on follows nothing', () => {
  const r = run([{ kind: 'select', job: 'act_1' }, { kind: 'select', job: 'act_2' }, { kind: 'peeked', job: 'act_1', bytes: 9 }])
  assert.ok(!r.log.some((e) => e.op === 'follow'))
  assert.deepEqual(r.state, { kind: 'peeking', job: 'act_2' })
})

test('leaving before the peek answered leaves nothing to unfollow', () => {
  const r = run([{ kind: 'select', job: 'act_1' }, { kind: 'leave' }])
  assert.deepEqual(r.log, [{ op: 'peek', job: 'act_1' }])
})
