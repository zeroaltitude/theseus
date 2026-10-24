// Drag on the Timeline (theseus-jid7t.5): each row of the drag table, each refusal, the 5-minute snap, the `when` sent as
// the exact instant of the local time dropped on (DST included), and the Undo that puts back exactly the time before.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { Mark } from '../src/lib/agenda.ts'
import { dropAt, dropOf, PAST_REFUSAL, snap } from '../src/ops/drag.ts'
import { wordsOf } from '../src/ops/refusal.ts'
import { local } from './ops-calendar-fixtures.ts'

const withZone = <T>(zone: string, f: () => T): T => {
  const was = process.env.TZ
  process.env.TZ = zone
  try { return f() } finally { if (was === undefined) delete process.env.TZ; else process.env.TZ = was }
}
const mark = (kind: Mark['kind'], at_ms: number): Mark => ({ key: `${kind}:1`, kind, at_ms, title: `a ${kind}`, id: kind === 'start' ? 'tsk_kelp' : 'wak_kelp' })

test('a reminder or check-in moves its wake, with the time before as its Undo', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 23, 9)
    for (const kind of ['reminder', 'check'] as const) {
      const m = mark(kind, local(2026, 10, 23, 14))
      const d = dropOf(m, local(2026, 10, 24, 10, 30), false, now)
      assert.ok(!('refused' in d), kind)
      if ('refused' in d) continue
      assert.equal(d.method, 'wake.snooze')
      assert.deepEqual(d.params, { wake: 'wak_kelp', when: '2026-10-24T17:30:00.000Z' }, 'Phoenix is UTC−7 all year')
      assert.deepEqual(d.undo(undefined), { method: 'wake.snooze', params: { wake: 'wak_kelp', when: '2026-10-23T21:00:00.000Z' } })
      assert.match(d.words, /Sat 10:30/)
    }
  })
})

test('a deferred start moves by task.reschedule, and its Undo is the daemon’s `previous`', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 23, 9)
    const m = mark('start', local(2026, 10, 23, 15))
    const d = dropOf(m, local(2026, 10, 23, 16, 5), false, now)
    assert.ok(!('refused' in d))
    if ('refused' in d) return
    assert.equal(d.method, 'task.reschedule')
    assert.deepEqual(d.params, { task: 'tsk_kelp', start_at_ms: local(2026, 10, 23, 16, 5) })
    assert.deepEqual(d.undo({ task: 'tsk_kelp', starts_at_ms: local(2026, 10, 23, 16, 5), previous: local(2026, 10, 23, 14, 55) }),
      { method: 'task.reschedule', params: { task: 'tsk_kelp', start_at_ms: local(2026, 10, 23, 14, 55) } }, 'the daemon’s word for the time before')
    assert.deepEqual(d.undo(undefined).params, { task: 'tsk_kelp', start_at_ms: m.at_ms }, 'without it, the mark’s own')
  })
})

test('everything else is refused, with the reason on the cursor', () => {
  const now = local(2026, 10, 23, 9)
  const later = local(2026, 10, 23, 12)
  const refused = (what: Parameters<typeof dropOf>[0], at = later, past = false) => { const d = dropOf(what, at, past, now); return 'refused' in d ? d.refused : null }
  assert.match(refused(mark('run', later))!, /routine’s schedule is edited in its form \(routine\.update\)/)
  assert.match(refused(mark('past', local(2026, 10, 22)))!, /ended work/)
  assert.match(refused(mark('watch', later))!, /watch keeps its window/)
  assert.match(refused(mark('milestone', later))!, /milestone moves with its project/)
  assert.match(refused({ event: true })!, /never writes to Google Calendar/)
  assert.equal(refused(mark('reminder', later), local(2026, 10, 23, 13), true), PAST_REFUSAL, 'the time machine')
  assert.equal(refused({ event: true }, later, true), PAST_REFUSAL)
  assert.match(refused(mark('start', local(2026, 10, 23, 8)))!, /it has started/)
  assert.match(refused(mark('reminder', later), local(2026, 10, 23, 8))!, /that time has passed/)
  assert.match(refused(mark('reminder', later), later)!, /already then/)
})

test('a drop snaps to 5 minutes of the local clock, and a DST day lands each wall-clock time exactly', () => {
  withZone('America/New_York', () => {
    assert.equal(snap(local(2026, 10, 23, 9, 2)), local(2026, 10, 23, 9, 0))
    assert.equal(snap(local(2026, 10, 23, 9, 3)), local(2026, 10, 23, 9, 5))
    assert.equal(snap(local(2026, 10, 23, 23, 58)), local(2026, 10, 24, 0, 0), 'over midnight')
    // 2026-11-01 is the fall-back Sunday (25 hours): 09:00 that day is 14:00 UTC, the day before 13:00 UTC.
    assert.equal(new Date(dropAt(local(2026, 11, 1), 9)).toISOString(), '2026-11-01T14:00:00.000Z')
    assert.equal(new Date(dropAt(local(2026, 10, 31), 9)).toISOString(), '2026-10-31T13:00:00.000Z')
    // 2026-03-08 springs forward (23 hours): 09:00 is 13:00 UTC; 02:30 does not exist, and lands on 03:30.
    assert.equal(new Date(dropAt(local(2026, 3, 8), 9)).toISOString(), '2026-03-08T13:00:00.000Z')
    assert.equal(new Date(dropAt(local(2026, 3, 8), 2.5)).getHours(), 3)
    assert.equal(dropAt(local(2026, 10, 23), 14.04), local(2026, 10, 23, 14, 0), '14:02 snaps to 14:00')
    assert.equal(dropAt(local(2026, 10, 23), 14.05), local(2026, 10, 23, 14, 5), '14:03 snaps to 14:05')
  })
})

test('a refusal is said in the daemon’s words, whatever shape the client rejects with', () => {
  // The protocol client rejects with the daemon's `{ code, message }`, not an Error: an "unknown method" from a daemon
  // without task.reschedule must reach the toast as its words, never as [object Object].
  assert.equal(wordsOf({ code: -32601, message: 'unknown method: task.reschedule' }), 'unknown method: task.reschedule')
  assert.equal(wordsOf(new Error('not connected')), 'not connected')
  assert.equal(wordsOf('plain'), 'plain')
})
