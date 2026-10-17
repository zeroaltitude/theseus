// The cockpit's desktop notices (`src/lib/notices.ts`, theseus-go4ig), run by `npm test`: how a notice the daemon
// pushed (`work.notice`) reaches the owner, and a set reminder's notice. The policy itself is the daemon's, tested in
// theseus-protocol.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { WorkNotice } from '../src/protocol.gen/index.ts'
import { deliver, reminderNotice, tagOf } from '../src/lib/notices.ts'

const work = { id: 'exe_beta', kind: 'conversation', session_id: 'ses_beta', title: 'Beta summary' } as const
const notice = (o: Partial<WorkNotice> = {}): WorkNotice => ({
  urgency: 'interrupt', why: 'asked', work, root: work, line: '● Beta summary asks: fs.write notes.txt?', question: 'act_q1',
  position: 7, at_ms: 1_000, ...o,
})
const away = { seen: 0, away_ms: 5, quiet: false, sound: true }

test('a question pings with sound, and its answer is quiet and closes it by its tag', () => {
  assert.deepEqual(deliver(notice(), away), { as: 'ping', sound: true })
  const answered = notice({ urgency: 'quiet', why: 'answered', question: undefined, retracts: 'act_q1' })
  assert.deepEqual(deliver(answered, away), { as: 'nothing' })
  assert.equal(tagOf(notice()), 'act_q1')
  assert.equal(tagOf(notice({ question: undefined, why: 'failed' })), 'exe_beta:failed')
})

test('what the viewer is looking at is no ping while present, and a ping once away', () => {
  assert.deepEqual(deliver(notice(), { ...away, focused: 'ses_beta', away_ms: 0 }), { as: 'nothing' })
  assert.deepEqual(deliver(notice(), { ...away, focused: 'exe_beta', away_ms: 0 }), { as: 'nothing' })
  assert.deepEqual(deliver(notice(), { ...away, focused: 'ses_other', away_ms: 0 }), { as: 'ping', sound: true })
  assert.deepEqual(deliver(notice(), { ...away, focused: 'ses_beta' }), { as: 'ping', sound: true })
})

test('an inform is a badge; a finish and an overdue milestone ping without sound; a quiet viewer has none', () => {
  assert.deepEqual(deliver(notice({ urgency: 'inform', why: 'reported' }), away), { as: 'badge' })
  assert.deepEqual(deliver(notice({ why: 'finished' }), away), { as: 'ping', sound: false })
  assert.deepEqual(deliver(notice({ why: 'overdue' }), away), { as: 'ping', sound: false })
  assert.deepEqual(deliver(notice(), { ...away, quiet: true }), { as: 'ping', sound: false })
})

test('what was seen is not told again, a reminder aside', () => {
  assert.deepEqual(deliver(notice(), { ...away, seen: 7 }), { as: 'nothing' })
  assert.deepEqual(deliver(notice({ why: 'reminder' }), { ...away, seen: 7 }), { as: 'ping', sound: true })
  assert.deepEqual(deliver(notice({ position: 8 }), { ...away, seen: 7 }), { as: 'ping', sound: true })
})

test('a set reminder interrupts; a model\'s check-in does not', () => {
  const wake = { wake_id: 'wak_1', short: '1', session_id: 'ses_beta', execution_id: 'exe_beta', purpose: 'remind', note: 'Water the ferns\nmore', due_at_ms: 10_000, late_ms: 0, answers: [] } as any
  const n = reminderNotice(wake)!
  assert.deepEqual([n.urgency, n.why, n.line, n.work.kind], ['interrupt', 'woke', '⏰ Water the ferns', 'wake'])
  assert.deepEqual(deliver(n, away), { as: 'ping', sound: true })
  assert.equal(reminderNotice({ ...wake, purpose: 'check' }), undefined)
})
